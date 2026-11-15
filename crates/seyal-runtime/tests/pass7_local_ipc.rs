#![cfg(target_os = "macos")]

use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};

#[cfg(feature = "test-fault-injection")]
use seyal_exec::test_fault::{self, FaultPoint};
use seyal_exec::LineId;
use seyal_exec::{CommandSpec, WindowSize};
use seyal_runtime::{
    display::{decode_chunk, empty_cache, DecodedDisplayChunk, DisplayCache},
    local_ipc::framing::{
        encode_frame, Attach, Attached, ClientHello, ErrorMessage, FrameHeader,
        HistoryRangeRequest, HistoryRangeSnapshot, HistoryRangeStatus, InputRef, MessageType,
        ResizeRequest, ResizeResult, ResizeResultCode, Role, ServerHello, TerminalKey,
        TerminalKeyKind, TerminalKeyModifiers, TerminalKeyV2, TerminalKeyV2Event,
        TerminalKeyV2Kind, TerminalKeyV2Modifiers, TerminalMouse, TerminalMouseKind,
        CAP_CORRELATED_RESIZE, CAP_EXTENDED_TERMINAL_KEY, CAP_SEMANTIC_TERMINAL_KEY, HEADER_LEN,
        HISTORY_CELL_SIDECAR_FLAG,
    },
    AttachmentId, LocalIpcMode, Runtime, RuntimeConfig,
};

fn config() -> RuntimeConfig {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let suffix = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let process = std::process::id();
    let mut config = RuntimeConfig::m001().expect("config");
    config.singleton_path = std::env::temp_dir().join(format!("s7-{process}-{suffix:x}.lock"));
    config.local_ipc = LocalIpcMode::Enabled {
        runtime_dir_override: Some(std::env::temp_dir().join(format!("s7d-{process}-{suffix:x}"))),
    };
    config
}

struct Harness {
    runtime: Runtime,
    stream: UnixStream,
    buffered: Vec<u8>,
}

impl Drop for Harness {
    fn drop(&mut self) {
        // This harness owns disposable test executions. Production Runtime
        // deliberately does not terminate TerminalExecution on ordinary Drop,
        // so tests must close their own children explicitly or a full cargo
        // suite can accumulate PTYs/processes and fail later spawns.
        if self.runtime.begin_shutdown().is_ok() {
            let _ = self
                .runtime
                .run_until_empty(Instant::now() + Duration::from_secs(3));
        }
    }
}

impl Harness {
    fn new(command: CommandSpec) -> (Self, seyal_runtime::ExecutionId) {
        Self::new_with(command, config(), WindowSize::cells(80, 24).expect("size"))
    }

    fn new_with(
        command: CommandSpec,
        config: RuntimeConfig,
        size: WindowSize,
    ) -> (Self, seyal_runtime::ExecutionId) {
        let mut runtime = Runtime::new(config).expect("Runtime");
        let execution_id = runtime.create_execution(command, size).expect("execution");
        let socket = runtime
            .local_ipc_socket_path()
            .expect("socket")
            .to_path_buf();
        let deadline = Instant::now() + Duration::from_secs(2);
        let stream = loop {
            match UnixStream::connect(&socket) {
                Ok(stream) => break stream,
                Err(_) => {
                    assert!(Instant::now() < deadline, "connect timeout");
                    runtime.poll_once(Some(Duration::from_millis(5))).unwrap();
                }
            }
        };
        stream.set_nonblocking(true).unwrap();
        runtime.poll_once(Some(Duration::from_millis(5))).unwrap();
        (
            Self {
                runtime,
                stream,
                buffered: Vec::new(),
            },
            execution_id,
        )
    }

    fn pump(&mut self) {
        self.runtime
            .poll_once(Some(Duration::from_millis(5)))
            .expect("poll");
    }

    /// Read and discard every complete frame until the socket is quiet and the
    /// local buffer is empty. Keeps stream framing aligned for the next request.
    fn quiesce(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut idle_rounds = 0u8;
        while Instant::now() < deadline {
            self.pump();
            let mut made_progress = false;
            let mut buf = [0u8; 64 * 1024];
            match self.stream.read(&mut buf) {
                Ok(0) => panic!("socket closed during quiesce"),
                Ok(count) => {
                    self.buffered.extend_from_slice(&buf[..count]);
                    made_progress = true;
                    idle_rounds = 0;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => panic!("read: {error}"),
            }
            while self.buffered.len() >= HEADER_LEN {
                let header = FrameHeader::decode(&self.buffered[..HEADER_LEN]).unwrap();
                let total = HEADER_LEN + header.payload_len as usize;
                if self.buffered.len() < total {
                    break;
                }
                let _ = self.buffered.drain(..total);
                made_progress = true;
                idle_rounds = 0;
            }
            if !made_progress && self.buffered.is_empty() {
                idle_rounds += 1;
                if idle_rounds >= 3 {
                    return;
                }
            }
        }
        panic!("quiesce timeout (buffered={} bytes)", self.buffered.len());
    }

    fn send(&mut self, kind: MessageType, payload: &[u8]) {
        let bytes = encode_frame(kind, payload);
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut sent = 0;
        while sent < bytes.len() {
            match self.stream.write(&bytes[sent..]) {
                Ok(0) => panic!("zero write"),
                Ok(count) => sent += count,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => self.pump(),
                Err(error) => panic!("write: {error}"),
            }
            assert!(Instant::now() < deadline, "send timeout");
        }
        self.pump();
    }

    fn frame(&mut self) -> (u16, Vec<u8>) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if self.buffered.len() >= HEADER_LEN {
                let header = FrameHeader::decode(&self.buffered[..HEADER_LEN]).unwrap();
                let total = HEADER_LEN + header.payload_len as usize;
                if self.buffered.len() >= total {
                    let bytes = self.buffered.drain(..total).collect::<Vec<_>>();
                    return (header.message_type, bytes[HEADER_LEN..].to_vec());
                }
            }
            let mut buf = [0u8; 16 * 1024];
            match self.stream.read(&mut buf) {
                Ok(0) => panic!("closed"),
                Ok(count) => self.buffered.extend_from_slice(&buf[..count]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => self.pump(),
                Err(error) => panic!("read: {error}"),
            }
            assert!(Instant::now() < deadline, "frame timeout");
        }
    }

    fn hello(&mut self) -> ServerHello {
        self.hello_with_capabilities(0)
    }

    fn hello_with_capabilities(&mut self, client_capabilities: u32) -> ServerHello {
        self.send(
            MessageType::ClientHello,
            &ClientHello {
                client_capabilities,
            }
            .encode(),
        );
        let (kind, payload) = self.frame();
        assert_eq!(kind, MessageType::ServerHello as u16);
        ServerHello::decode(&payload).unwrap()
    }

    fn attach(
        &mut self,
        execution_id: seyal_runtime::ExecutionId,
        role: Role,
    ) -> (Attached, DisplayCache) {
        self.send(
            MessageType::Attach,
            &Attach {
                execution_id,
                requested_role: role,
            }
            .encode(),
        );
        let (kind, payload) = self.frame();
        assert_eq!(kind, MessageType::Attached as u16);
        let attached = Attached::decode(&payload).unwrap();
        let chunks = self.display_batch(MessageType::DisplaySnapshot);
        let mut cache = empty_cache();
        cache.apply_chunks(&chunks).unwrap();
        (attached, cache)
    }

    fn display_batch(&mut self, expected: MessageType) -> Vec<DecodedDisplayChunk> {
        let (kind, payload) = self.frame();
        assert_eq!(kind, expected as u16);
        let first = decode_chunk(&encode_frame(expected, &payload)).unwrap();
        let count = first.chunk_count;
        let mut chunks = vec![first];
        for _ in 1..count {
            let (kind, payload) = self.frame();
            assert_eq!(kind, expected as u16);
            chunks.push(decode_chunk(&encode_frame(expected, &payload)).unwrap());
        }
        chunks
    }

    fn next_display(&mut self, cache: &mut DisplayCache) {
        let (kind, payload) = self.frame();
        let kind = MessageType::from_u16(kind).expect("display message");
        assert!(matches!(
            kind,
            MessageType::DisplaySnapshot | MessageType::DisplayDelta
        ));
        let first = decode_chunk(&encode_frame(kind, &payload)).unwrap();
        let count = first.chunk_count;
        let mut chunks = vec![first];
        for _ in 1..count {
            let (next_kind, next_payload) = self.frame();
            assert_eq!(next_kind, kind as u16);
            chunks.push(decode_chunk(&encode_frame(kind, &next_payload)).unwrap());
        }
        cache.apply_chunks(&chunks).unwrap();
    }

    fn wait_for_close(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut buf = [0u8; 1024];
        loop {
            self.pump();
            match self.stream.read(&mut buf) {
                Ok(0) => return,
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => panic!("read after fatal protocol error: {error}"),
            }
            assert!(
                Instant::now() < deadline,
                "fatal V2 connection remained open"
            );
        }
    }
}

#[test]
fn history_range_combining_grapheme_round_trips_over_runtime_wire() {
    let (mut harness, execution_id) = Harness::new_with(
        CommandSpec::new("/bin/cat"),
        config(),
        WindowSize::cells(80, 1).expect("size"),
    );
    harness.hello();
    let (attached, _cache) = harness.attach(execution_id, Role::Controller);
    harness.send(
        MessageType::Input,
        &InputRef {
            attachment_id: attached.attachment_id,
            bytes: "e\u{301}\r\n".as_bytes(),
        }
        .encode(),
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    while !harness
        .runtime
        .execution(execution_id)
        .expect("execution")
        .terminal()
        .primary_history_units_range(LineId(1), LineId(u64::MAX), 32)
        .iter()
        .any(|unit| unit.text == "e\u{301}")
    {
        assert!(Instant::now() < deadline, "canonical grapheme not retained");
        harness.pump();
    }
    harness.quiesce();
    harness.send(
        MessageType::HistoryRangeRequest,
        &HistoryRangeRequest {
            attachment_id: attached.attachment_id,
            request_id: 41,
            block_id: 7,
            start_line: 1,
            end_line: u64::MAX,
            max_lines: 32,
            max_cells: 2_560,
            start_unit: 0,
        }
        .encode(),
    );

    loop {
        let (kind, payload) = harness.frame();
        match MessageType::from_u16(kind) {
            Some(MessageType::HistoryRangeSnapshot) => {
                let snapshot = HistoryRangeSnapshot::decode(&payload).expect("snapshot");
                assert_eq!(snapshot.request_id, 41);
                assert_eq!(snapshot.block_id, 7);
                let cell = snapshot
                    .rows
                    .iter()
                    .flat_map(|row| &row.cells)
                    .find(|cell| {
                        cell.sidecar_utf8(&snapshot.sidecar)
                            .ok()
                            .flatten()
                            .is_some_and(|text| text == "e\u{301}".as_bytes())
                    })
                    .expect("combining grapheme on history wire");
                assert_eq!(
                    cell.flags & HISTORY_CELL_SIDECAR_FLAG,
                    HISTORY_CELL_SIDECAR_FLAG
                );
                break;
            }
            Some(MessageType::DisplaySnapshot | MessageType::DisplayDelta | MessageType::Error) => {
                if MessageType::from_u16(kind) == Some(MessageType::Error) {
                    panic!("history combining grapheme became DisplayUnavailable");
                }
                continue;
            }
            other => panic!("unexpected history response: {other:?}"),
        }
    }
}

#[test]
fn evicted_history_range_reports_stale_over_runtime_wire() {
    let mut runtime_config = config();
    runtime_config.history_aggregate_bytes = 24 * 1024;
    let (mut harness, execution_id) = Harness::new_with(
        CommandSpec::new("/bin/cat"),
        runtime_config,
        WindowSize::cells(512, 1).expect("size"),
    );
    harness.hello();
    let (attached, _cache) = harness.attach(execution_id, Role::Controller);
    let body = format!("{}\r\n", "a".repeat(512)).repeat(64);
    harness.send(
        MessageType::Input,
        &InputRef {
            attachment_id: attached.attachment_id,
            bytes: body.as_bytes(),
        }
        .encode(),
    );

    let deadline = Instant::now() + Duration::from_secs(10);
    while harness
        .runtime
        .execution(execution_id)
        .expect("execution")
        .terminal()
        .primary_history_eviction_generation()
        == 0
    {
        assert!(Instant::now() < deadline, "history was not evicted");
        harness.pump();
    }
    harness.quiesce();
    harness.send(
        MessageType::HistoryRangeRequest,
        &HistoryRangeRequest {
            attachment_id: attached.attachment_id,
            request_id: 42,
            block_id: 7,
            start_line: 1,
            end_line: 1,
            max_lines: 1,
            max_cells: 512,
            start_unit: 0,
        }
        .encode(),
    );

    loop {
        let (kind, payload) = harness.frame();
        match MessageType::from_u16(kind) {
            Some(MessageType::HistoryRangeSnapshot) => {
                let snapshot = HistoryRangeSnapshot::decode(&payload).expect("snapshot");
                assert_eq!(snapshot.request_id, 42);
                assert_eq!(snapshot.status, HistoryRangeStatus::Stale);
                assert!(snapshot.rows.is_empty());
                break;
            }
            Some(MessageType::DisplaySnapshot | MessageType::DisplayDelta) => continue,
            other => panic!("unexpected history response: {other:?}"),
        }
    }
}

#[test]
fn server_advertises_both_pass7_capabilities() {
    let (mut harness, _execution_id) = Harness::new(CommandSpec::new("/bin/cat"));
    let hello = harness.hello();
    assert_ne!(hello.server_capabilities & CAP_SEMANTIC_TERMINAL_KEY, 0);
    assert_ne!(hello.server_capabilities & CAP_CORRELATED_RESIZE, 0);
    assert_ne!(hello.server_capabilities & CAP_EXTENDED_TERMINAL_KEY, 0);
}

#[test]
fn controller_terminal_key_is_encoded_by_runtime_and_reaches_pty() {
    let command = CommandSpec::new("/bin/sh").args([
        "-c",
        "stty raw -echo; od -An -tu1 -N3 | tr -s ' ' | sed 's/^ //'",
    ]);
    let (mut harness, execution_id) = Harness::new(command);
    harness.hello();
    let (attached, mut cache) = harness.attach(execution_id, Role::Controller);
    harness.send(
        MessageType::TerminalKey,
        &TerminalKey {
            attachment_id: attached.attachment_id,
            kind: TerminalKeyKind::ArrowUp,
            modifiers: TerminalKeyModifiers::NONE,
            scalar: 0,
        }
        .encode(),
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let text: String = cache.cells.iter().map(|cell| cell.scalar).collect();
        if text.contains("27 91 65") {
            break;
        }
        assert!(Instant::now() < deadline, "semantic key bytes not observed");
        harness.next_display(&mut cache);
    }
}

#[test]
fn malformed_v2_key_closes_connection_after_bounded_error() {
    let (mut harness, execution_id) = Harness::new(CommandSpec::new("/bin/cat"));
    harness.hello_with_capabilities(CAP_EXTENDED_TERMINAL_KEY);
    let (attached, _cache) = harness.attach(execution_id, Role::Controller);
    let mut malformed = vec![0u8; TerminalKeyV2::WIRE_LEN - 1];
    malformed[..16].copy_from_slice(&attached.attachment_id.to_bytes());
    harness.send(MessageType::TerminalKeyV2, &malformed);
    harness.wait_for_close();
}

#[test]
fn unnegotiated_v2_key_closes_connection_after_bounded_error() {
    let (mut harness, execution_id) = Harness::new(CommandSpec::new("/bin/cat"));
    harness.hello();
    let (attached, _cache) = harness.attach(execution_id, Role::Controller);
    let key = TerminalKeyV2 {
        attachment_id: attached.attachment_id,
        kind: TerminalKeyV2Kind::ArrowUp,
        modifiers: TerminalKeyV2Modifiers::NONE,
        value: 0,
        event: TerminalKeyV2Event::Press,
        shifted_ascii: 0,
        action_id: 1,
    };
    harness.send(MessageType::TerminalKeyV2, &key.encode());
    harness.wait_for_close();
}

fn sample_v2_key() -> TerminalKeyV2 {
    TerminalKeyV2 {
        attachment_id: AttachmentId::from_bytes([0; 16]),
        kind: TerminalKeyV2Kind::ArrowUp,
        modifiers: TerminalKeyV2Modifiers::NONE,
        value: 0,
        event: TerminalKeyV2Event::Press,
        shifted_ascii: 0,
        action_id: 1,
    }
}

#[test]
fn type_29_before_hello_closes_connection_after_bounded_error() {
    let (mut harness, _execution_id) = Harness::new(CommandSpec::new("/bin/cat"));
    harness.send(MessageType::TerminalKeyV2, &sample_v2_key().encode());
    harness.wait_for_close();
}

#[test]
fn type_29_after_hello_before_attach_closes_connection_after_bounded_error() {
    let (mut harness, _execution_id) = Harness::new(CommandSpec::new("/bin/cat"));
    harness.hello_with_capabilities(CAP_EXTENDED_TERMINAL_KEY);
    harness.send(MessageType::TerminalKeyV2, &sample_v2_key().encode());
    harness.wait_for_close();
}

#[test]
fn v2_rejection_echoes_action_id_and_duplicate_id_is_fatal() {
    let (mut harness, execution_id) = Harness::new(CommandSpec::new("/bin/cat"));
    harness.hello_with_capabilities(CAP_EXTENDED_TERMINAL_KEY);
    let (attached, _cache) = harness.attach(execution_id, Role::Controller);
    let rejected = TerminalKeyV2 {
        attachment_id: attached.attachment_id,
        kind: TerminalKeyV2Kind::Keypad,
        modifiers: TerminalKeyV2Modifiers::SHIFT,
        value: 1,
        event: TerminalKeyV2Event::Press,
        shifted_ascii: 0,
        action_id: 1,
    };
    harness.send(MessageType::TerminalKeyV2, &rejected.encode());
    let (kind, payload) = harness.frame();
    assert_eq!(kind, MessageType::Error as u16);
    let error = ErrorMessage::decode(&payload).unwrap();
    assert_eq!(error.detail_code, rejected.action_id);

    // A reused action ID is a fatal protocol violation after the ordinary
    // rejection above, proving both correlation and monotonic admission.
    harness.send(MessageType::TerminalKeyV2, &rejected.encode());
    harness.wait_for_close();
}

#[test]
fn correlated_resize_returns_applied_generation_before_projection() {
    let (mut harness, execution_id) = Harness::new(CommandSpec::new("/bin/cat"));
    harness.hello();
    let (attached, mut cache) = harness.attach(execution_id, Role::Controller);
    harness.send(
        MessageType::ResizeRequest,
        &ResizeRequest {
            attachment_id: attached.attachment_id,
            request_id: 1,
            rows: 40,
            columns: 120,
        }
        .encode(),
    );

    let (kind, payload) = harness.frame();
    assert_eq!(kind, MessageType::ResizeResult as u16);
    let result = ResizeResult::decode(&payload).unwrap();
    assert_eq!(result.request_id, 1);
    assert_eq!(result.result_code, ResizeResultCode::Applied);
    assert!(result.applied_generation > cache.generation);

    while cache.generation < result.applied_generation {
        harness.next_display(&mut cache);
    }
    assert_eq!((cache.rows, cache.columns), (40, 120));
}

#[test]
fn duplicate_resize_request_id_is_correlated_malformed_failure() {
    let (mut harness, execution_id) = Harness::new(CommandSpec::new("/bin/cat"));
    harness.hello();
    let (attached, _cache) = harness.attach(execution_id, Role::Controller);
    let request = ResizeRequest {
        attachment_id: attached.attachment_id,
        request_id: 7,
        rows: 30,
        columns: 100,
    };
    harness.send(MessageType::ResizeRequest, &request.encode());
    let (kind, payload) = harness.frame();
    assert_eq!(kind, MessageType::ResizeResult as u16);
    assert_eq!(
        ResizeResult::decode(&payload).unwrap().result_code,
        ResizeResultCode::Applied
    );

    // Consume the projection from the first successful resize before testing
    // the duplicate result, so mandatory-vs-presentation ordering is explicit.
    let _ = harness.display_batch(MessageType::DisplaySnapshot);
    harness.send(MessageType::ResizeRequest, &request.encode());
    let (kind, payload) = harness.frame();
    assert_eq!(kind, MessageType::ResizeResult as u16);
    assert_eq!(
        ResizeResult::decode(&payload).unwrap().result_code,
        ResizeResultCode::Error(seyal_runtime::local_ipc::framing::ErrorCode::MalformedPayload)
    );
}

#[cfg(feature = "test-fault-injection")]
#[test]
fn persistent_winsize_failure_is_bounded_before_canonical_commit() {
    let (mut harness, execution_id) = Harness::new(CommandSpec::new("/bin/cat"));
    harness.hello();
    let (attached, cache) = harness.attach(execution_id, Role::Controller);
    let initial_generation = cache.generation;

    // Fail exactly the permitted attempts. The endpoint fault is injected
    // before TerminalState::resize, so every rejected request must leave the
    // canonical geometry and generation untouched.
    test_fault::fail_times(FaultPoint::ResizeWinsize, 3);
    for request_id in 1..=3 {
        harness.send(
            MessageType::ResizeRequest,
            &ResizeRequest {
                attachment_id: attached.attachment_id,
                request_id,
                rows: 40,
                columns: 120,
            }
            .encode(),
        );
        let (kind, payload) = harness.frame();
        assert_eq!(kind, MessageType::ResizeResult as u16);
        let result = ResizeResult::decode(&payload).unwrap();
        assert_eq!(result.request_id, request_id);
        assert_eq!(
            result.result_code,
            ResizeResultCode::Error(seyal_runtime::local_ipc::framing::ErrorCode::InternalFailure)
        );
        assert_eq!(result.applied_generation, 0);
    }
    assert_eq!(test_fault::remaining(FaultPoint::ResizeWinsize), 0);
    let execution = harness.runtime.execution(execution_id).unwrap();
    assert_eq!(execution.terminal().damage_generation(), initial_generation);
    assert_eq!(execution.terminal().rows(), 24);
    assert_eq!(execution.terminal().cols(), 80);

    // Recovery is explicit: once the fault epoch is cleared, a new request
    // may apply and publish one new canonical generation.
    harness.send(
        MessageType::ResizeRequest,
        &ResizeRequest {
            attachment_id: attached.attachment_id,
            request_id: 4,
            rows: 40,
            columns: 120,
        }
        .encode(),
    );
    let (kind, payload) = harness.frame();
    assert_eq!(kind, MessageType::ResizeResult as u16);
    let result = ResizeResult::decode(&payload).unwrap();
    assert_eq!(result.result_code, ResizeResultCode::Applied);
    assert_eq!(result.applied_generation, initial_generation + 1);
}

#[test]
fn unauthorized_resize_cannot_poison_request_id_sequence() {
    let (mut harness, execution_id) = Harness::new(CommandSpec::new("/bin/cat"));
    harness.hello();
    let (attached, _cache) = harness.attach(execution_id, Role::Controller);

    // This frame is structurally valid but carries a stale attachment. Its
    // large request ID must not advance the live connection's ordering state.
    harness.send(
        MessageType::ResizeRequest,
        &ResizeRequest {
            attachment_id: AttachmentId::from_bytes(999u128.to_le_bytes()),
            request_id: 99,
            rows: 30,
            columns: 100,
        }
        .encode(),
    );
    let (kind, payload) = harness.frame();
    assert_eq!(kind, MessageType::ResizeResult as u16);
    assert_eq!(
        ResizeResult::decode(&payload).unwrap().result_code,
        ResizeResultCode::Error(seyal_runtime::local_ipc::framing::ErrorCode::StaleIdentity)
    );

    // The first valid request ID remains usable after the rejected request.
    harness.send(
        MessageType::ResizeRequest,
        &ResizeRequest {
            attachment_id: attached.attachment_id,
            request_id: 1,
            rows: 30,
            columns: 100,
        }
        .encode(),
    );
    let (kind, payload) = harness.frame();
    assert_eq!(kind, MessageType::ResizeResult as u16);
    assert_eq!(
        ResizeResult::decode(&payload).unwrap().result_code,
        ResizeResultCode::Applied
    );
}

#[test]
fn history_range_over_wire_budget_returns_truncated_not_capacity_error() {
    let (mut harness, execution_id) = Harness::new(CommandSpec::new("/bin/cat"));
    harness.hello();
    let (attached, mut cache) = harness.attach(execution_id, Role::Controller);

    // Fill primary history past the ~151-row wire budget at 80 columns while
    // staying inside production max_lines/max_cells (512 / 131072).
    let mut body = String::new();
    for i in 0..230 {
        body.push_str(&format!("{i:080}\n"));
    }
    harness.send(
        MessageType::Input,
        &InputRef {
            attachment_id: attached.attachment_id,
            bytes: body.as_bytes(),
        }
        .encode(),
    );

    let fill_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let rows = harness
            .runtime
            .execution(execution_id)
            .expect("execution")
            .terminal()
            .primary_history_range(LineId(1), LineId(u64::MAX), 512)
            .expect("scalar history remains representable");
        if rows.len() >= 200 {
            break;
        }
        assert!(
            Instant::now() < fill_deadline,
            "timed out waiting for ≥200 retained history rows (have {})",
            rows.len()
        );
        harness.pump();
        let mut buf = [0u8; 64 * 1024];
        match harness.stream.read(&mut buf) {
            Ok(0) => panic!("socket closed while filling history"),
            Ok(count) => harness.buffered.extend_from_slice(&buf[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("read: {error}"),
        }
        while harness.buffered.len() >= HEADER_LEN {
            let header = FrameHeader::decode(&harness.buffered[..HEADER_LEN]).unwrap();
            let total = HEADER_LEN + header.payload_len as usize;
            if harness.buffered.len() < total {
                break;
            }
            let _ = harness.buffered.drain(..total);
        }
    }
    harness.quiesce();
    let _ = &mut cache;

    harness.send(
        MessageType::HistoryRangeRequest,
        &HistoryRangeRequest {
            attachment_id: attached.attachment_id,
            request_id: 1,
            block_id: 7,
            start_line: 1,
            end_line: u64::MAX,
            max_lines: 512,
            max_cells: 131_072,
            start_unit: 0,
        }
        .encode(),
    );

    let reply_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(Instant::now() < reply_deadline, "history snapshot timeout");
        let (kind, payload) = harness.frame();
        match MessageType::from_u16(kind) {
            Some(MessageType::HistoryRangeSnapshot) => {
                let snapshot = HistoryRangeSnapshot::decode(&payload).expect("snapshot");
                assert_eq!(snapshot.status, HistoryRangeStatus::Truncated);
                assert!(!snapshot.rows.is_empty());
                assert!(
                    snapshot.rows.len() < 200,
                    "wire admission must stop before 200 full-width rows, got {}",
                    snapshot.rows.len()
                );
                return;
            }
            Some(MessageType::Error) => {
                panic!("history wire overflow must not send Error/CapacityExceeded");
            }
            Some(MessageType::DisplaySnapshot | MessageType::DisplayDelta) => continue,
            other => panic!("unexpected reply while waiting for history: {other:?}"),
        }
    }
}

fn wait_for_text(harness: &mut Harness, cache: &mut DisplayCache, needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let text: String = cache.cells.iter().map(|cell| cell.scalar).collect();
        if text.contains(needle) {
            return;
        }
        assert!(Instant::now() < deadline, "missing {needle:?} on display");
        harness.next_display(cache);
    }
}

#[test]
fn host_mouse_drag_selection_is_published_as_display_damage() {
    let command = CommandSpec::new("/bin/sh").args(["-c", "printf 'selectable'; sleep 30"]);
    let (mut harness, execution_id) = Harness::new(command);
    harness.hello();
    let (attached, mut cache) = harness.attach(execution_id, Role::Controller);
    wait_for_text(&mut harness, &mut cache, "selectable");

    for (action_id, kind, col) in [
        (1, TerminalMouseKind::Press, 0),
        (2, TerminalMouseKind::Move, 5),
        (3, TerminalMouseKind::Release, 5),
    ] {
        harness.send(
            MessageType::TerminalMouse,
            &TerminalMouse {
                attachment_id: attached.attachment_id,
                action_id,
                kind,
                button: 0,
                modifiers: TerminalKeyV2Modifiers::NONE,
                col,
                row: 0,
                click_count: if kind == TerminalMouseKind::Press {
                    1
                } else {
                    0
                },
            }
            .encode(),
        );
        harness.next_display(&mut cache);
    }

    assert!(cache.cells[..6].iter().all(|cell| cell.attributes.inverse));
    assert!(!cache.cells[6].attributes.inverse);
}

#[test]
fn host_double_click_selects_word_without_writing_to_pty() {
    let command = CommandSpec::new("/bin/sh").args([
        "-c",
        "printf 'one two'; stty raw -echo; od -An -tu1 -N1 | tr -s ' ' | sed 's/^ //'",
    ]);
    let (mut harness, execution_id) = Harness::new(command);
    harness.hello();
    let (attached, mut cache) = harness.attach(execution_id, Role::Controller);
    wait_for_text(&mut harness, &mut cache, "one two");

    harness.send(
        MessageType::TerminalMouse,
        &TerminalMouse {
            attachment_id: attached.attachment_id,
            action_id: 1,
            kind: TerminalMouseKind::Press,
            button: 0,
            modifiers: TerminalKeyV2Modifiers::NONE,
            col: 5,
            row: 0,
            click_count: 2,
        }
        .encode(),
    );
    harness.next_display(&mut cache);

    assert!(cache.cells[4..7].iter().all(|cell| cell.attributes.inverse));
    assert!(!cache.cells[..4].iter().any(|cell| cell.attributes.inverse));
    assert!(!cache.cells[7].attributes.inverse);

    harness.send(
        MessageType::Input,
        &InputRef {
            attachment_id: attached.attachment_id,
            bytes: b"x",
        }
        .encode(),
    );
    wait_for_text(&mut harness, &mut cache, "120");
}

#[test]
fn controller_sgr_mouse_reaches_pty_after_canonical_mode() {
    let command = CommandSpec::new("/bin/sh").args([
        "-c",
        "printf '\\033[?1000h\\033[?1006hREADY\\n'; stty raw -echo; od -An -tu1 -N9 | tr -s ' ' | sed 's/^ //'",
    ]);
    let (mut harness, execution_id) = Harness::new(command);
    harness.hello();
    let (attached, mut cache) = harness.attach(execution_id, Role::Controller);
    wait_for_text(&mut harness, &mut cache, "READY");
    harness.send(
        MessageType::TerminalMouse,
        &TerminalMouse {
            attachment_id: attached.attachment_id,
            action_id: 1,
            kind: TerminalMouseKind::Press,
            button: 0,
            modifiers: TerminalKeyV2Modifiers::NONE,
            col: 0,
            row: 0,
            click_count: 2,
        }
        .encode(),
    );
    wait_for_text(&mut harness, &mut cache, "27 91 60");
}

#[test]
fn shift_override_does_not_write_application_mouse() {
    let command = CommandSpec::new("/bin/sh").args([
        "-c",
        "printf '\\033[?1000h\\033[?1006hREADY\\n'; stty raw -echo; od -An -tu1 -N1 | tr -s ' ' | sed 's/^ //'",
    ]);
    let (mut harness, execution_id) = Harness::new(command);
    harness.hello();
    let (attached, mut cache) = harness.attach(execution_id, Role::Controller);
    wait_for_text(&mut harness, &mut cache, "READY");
    harness.send(
        MessageType::TerminalMouse,
        &TerminalMouse {
            attachment_id: attached.attachment_id,
            action_id: 1,
            kind: TerminalMouseKind::Press,
            button: 0,
            modifiers: TerminalKeyV2Modifiers::SHIFT,
            col: 1,
            row: 1,
            click_count: 1,
        }
        .encode(),
    );
    harness.send(
        MessageType::Input,
        &InputRef {
            attachment_id: attached.attachment_id,
            bytes: b"x",
        }
        .encode(),
    );
    wait_for_text(&mut harness, &mut cache, "120");
}

#[test]
fn shift_press_latches_host_override_through_unshifted_release() {
    let command = CommandSpec::new("/bin/sh").args([
        "-c",
        "printf '\\033[?1000h\\033[?1006hREADY\\n'; stty raw -echo; od -An -tu1 -N1 | tr -s ' ' | sed 's/^ //'",
    ]);
    let (mut harness, execution_id) = Harness::new(command);
    harness.hello();
    let (attached, mut cache) = harness.attach(execution_id, Role::Controller);
    wait_for_text(&mut harness, &mut cache, "READY");
    harness.send(
        MessageType::TerminalMouse,
        &TerminalMouse {
            attachment_id: attached.attachment_id,
            action_id: 1,
            kind: TerminalMouseKind::Press,
            button: 0,
            modifiers: TerminalKeyV2Modifiers::SHIFT,
            col: 1,
            row: 1,
            click_count: 1,
        }
        .encode(),
    );
    harness.send(
        MessageType::TerminalMouse,
        &TerminalMouse {
            attachment_id: attached.attachment_id,
            action_id: 2,
            kind: TerminalMouseKind::Release,
            button: 0,
            modifiers: TerminalKeyV2Modifiers::NONE,
            col: 2,
            row: 1,
            click_count: 0,
        }
        .encode(),
    );
    harness.send(
        MessageType::Input,
        &InputRef {
            attachment_id: attached.attachment_id,
            bytes: b"x",
        }
        .encode(),
    );
    wait_for_text(&mut harness, &mut cache, "120");
}
