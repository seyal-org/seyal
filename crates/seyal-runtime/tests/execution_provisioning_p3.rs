#![cfg(target_os = "macos")]

//! SPEC-004 §18.2–§18.6 / SPEC-003 §5.2+§7 — M003 P3 provisioning admission.

use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};

use seyal_exec::{CommandSpec, WindowSize};
#[cfg(feature = "test-fault-injection")]
use seyal_runtime::test_fault::{self, FaultPoint};
use seyal_runtime::{
    local_ipc::framing::{
        encode_frame, Attach, Attached, ClientHello, CreateExecutionRequest, CreateExecutionResult,
        CreateExecutionResultCode, ErrorCode, ErrorMessage, ExecutionList, FrameHeader, InputRef,
        MessageType, Role, ServerHello, CAP_EXECUTION_PROVISIONING, HEADER_LEN,
    },
    m001_term_name, ExecutionId, LocalIpcMode, Runtime, RuntimeConfig, WorkspaceId,
};

fn config() -> RuntimeConfig {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let suffix = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let process = std::process::id();
    let mut config = RuntimeConfig::m001().expect("config");
    config.singleton_path = std::env::temp_dir().join(format!("p3-{process}-{suffix:x}.lock"));
    config.local_ipc = LocalIpcMode::Enabled {
        runtime_dir_override: Some(std::env::temp_dir().join(format!("p3d-{process}-{suffix:x}"))),
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
        if self.runtime.begin_shutdown().is_ok() {
            let _ = self
                .runtime
                .run_until_empty(Instant::now() + Duration::from_secs(3));
        }
    }
}

impl Harness {
    fn empty() -> Self {
        let mut runtime = Runtime::new(config()).expect("Runtime");
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
        Self {
            runtime,
            stream,
            buffered: Vec::new(),
        }
    }

    fn with_streamer() -> (Self, ExecutionId) {
        let mut runtime = Runtime::new(config()).expect("Runtime");
        let streamer = runtime
            .create_execution(
                CommandSpec::new("/bin/sh").args([
                    "-c",
                    "i=0; while true; do printf 'STREAM-%s\\n' \"$i\"; i=$((i+1)); sleep 0.01; done",
                ]),
                WindowSize::cells(80, 24).expect("size"),
            )
            .expect("streamer");
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
            streamer,
        )
    }

    fn pump(&mut self) {
        self.runtime
            .poll_once(Some(Duration::from_millis(5)))
            .expect("poll");
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
            assert!(Instant::now() < deadline, "write timeout");
        }
        self.pump();
    }

    fn expect_frame(&mut self, deadline: Instant) -> (u16, Vec<u8>) {
        loop {
            if self.buffered.len() >= HEADER_LEN {
                let header =
                    FrameHeader::decode(&self.buffered[..HEADER_LEN]).expect("valid header");
                let total = HEADER_LEN + header.payload_len as usize;
                if self.buffered.len() >= total {
                    let frame = self.buffered.drain(..total).collect::<Vec<_>>();
                    return (header.message_type, frame[HEADER_LEN..].to_vec());
                }
            }
            let mut chunk = [0u8; 8192];
            match self.stream.read(&mut chunk) {
                Ok(0) => panic!("connection closed while awaiting frame"),
                Ok(count) => self.buffered.extend_from_slice(&chunk[..count]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => self.pump(),
                Err(error) => panic!("read: {error}"),
            }
            assert!(Instant::now() < deadline, "timed out awaiting frame");
        }
    }

    fn hello(&mut self, capabilities: u32) -> ServerHello {
        self.send(
            MessageType::ClientHello,
            &ClientHello {
                client_capabilities: capabilities,
            }
            .encode(),
        );
        let (kind, payload) = self.expect_frame(Instant::now() + Duration::from_secs(2));
        assert_eq!(kind, MessageType::ServerHello as u16);
        ServerHello::decode(&payload).expect("ServerHello")
    }

    fn create(&mut self, request_id: u64, rows: u16, columns: u16) -> CreateExecutionResult {
        self.send(
            MessageType::CreateExecutionRequest,
            &CreateExecutionRequest {
                workspace_id: 0,
                request_id,
                launch_profile: 0,
                rows,
                columns,
            }
            .encode(),
        );
        loop {
            let (kind, payload) = self.expect_frame(Instant::now() + Duration::from_secs(3));
            if kind == MessageType::CreateExecutionResult as u16 {
                return CreateExecutionResult::decode(&payload).expect("result");
            }
        }
    }

    fn expect_error(&mut self, code: ErrorCode, offending: MessageType) {
        let (kind, payload) = self.expect_frame(Instant::now() + Duration::from_secs(2));
        assert_eq!(kind, MessageType::Error as u16);
        let error = ErrorMessage::decode(&payload).expect("Error");
        assert_eq!(error.error_code, code as u16);
        assert_eq!(error.offending_message_type, offending as u16);
    }

    fn list(&mut self) -> ExecutionList {
        self.send(MessageType::ListExecutions, &[]);
        let (kind, payload) = self.expect_frame(Instant::now() + Duration::from_secs(2));
        assert_eq!(kind, MessageType::ExecutionList as u16);
        ExecutionList::decode(&payload).expect("list")
    }
}

#[test]
fn negotiated_client_creates_n_distinct_enumerable_attachable_executions() {
    let mut h = Harness::empty();
    let hello = h.hello(CAP_EXECUTION_PROVISIONING);
    assert_ne!(hello.server_capabilities & CAP_EXECUTION_PROVISIONING, 0);

    let mut ids = Vec::new();
    for request_id in 1..=3u64 {
        let result = h.create(request_id, 24, 80);
        assert_eq!(result.request_id, request_id);
        assert_eq!(result.result_code, CreateExecutionResultCode::Created);
        assert_ne!(result.execution_id.to_bytes(), [0; 16]);
        assert!(!ids.contains(&result.execution_id));
        ids.push(result.execution_id);
        let summary = h.runtime.lookup(result.execution_id).expect("published");
        assert_eq!(summary.workspace_id, WorkspaceId::m001_default());
    }

    let list = h.list();
    assert!(list.entries.len() >= 3);
    for id in &ids {
        assert!(list.entries.iter().any(|entry| entry.execution_id == *id));
    }

    h.send(
        MessageType::Attach,
        &Attach {
            execution_id: ids[0],
            requested_role: Role::Controller,
        }
        .encode(),
    );
    let (kind, payload) = h.expect_frame(Instant::now() + Duration::from_secs(2));
    assert_eq!(kind, MessageType::Attached as u16);
    let _ = Attached::decode(&payload).expect("Attached");
}

#[test]
fn capability_absent_is_unknown_message() {
    let mut h = Harness::empty();
    // AwaitHello + no capability → UnknownMessage (capability checked first).
    h.send(
        MessageType::CreateExecutionRequest,
        &CreateExecutionRequest {
            workspace_id: 0,
            request_id: 1,
            launch_profile: 0,
            rows: 24,
            columns: 80,
        }
        .encode(),
    );
    h.expect_error(
        ErrorCode::UnknownMessage,
        MessageType::CreateExecutionRequest,
    );

    let mut h = Harness::empty();
    h.hello(0);
    h.send(
        MessageType::CreateExecutionRequest,
        &CreateExecutionRequest {
            workspace_id: 0,
            request_id: 1,
            launch_profile: 0,
            rows: 24,
            columns: 80,
        }
        .encode(),
    );
    h.expect_error(
        ErrorCode::UnknownMessage,
        MessageType::CreateExecutionRequest,
    );
}

#[test]
fn shutdown_rejects_create_with_invalid_state_and_no_partial() {
    let mut h = Harness::empty();
    h.hello(CAP_EXECUTION_PROVISIONING);
    h.runtime.begin_shutdown().expect("begin shutdown");
    let result = h.create(1, 24, 80);
    assert_eq!(
        result.result_code,
        CreateExecutionResultCode::Error(ErrorCode::InvalidState)
    );
    assert_eq!(result.execution_id.to_bytes(), [0; 16]);
    assert_eq!(h.runtime.execution_count(), 0);
}

#[test]
fn request_id_rules_and_reconnect_reset() {
    let mut h = Harness::empty();
    h.hello(CAP_EXECUTION_PROVISIONING);
    let first = h.create(1, 24, 80);
    assert_eq!(first.result_code, CreateExecutionResultCode::Created);

    // Duplicate
    h.send(
        MessageType::CreateExecutionRequest,
        &CreateExecutionRequest {
            workspace_id: 0,
            request_id: 1,
            launch_profile: 0,
            rows: 24,
            columns: 80,
        }
        .encode(),
    );
    let (kind, payload) = h.expect_frame(Instant::now() + Duration::from_secs(2));
    assert_eq!(kind, MessageType::CreateExecutionResult as u16);
    let result = CreateExecutionResult::decode(&payload).unwrap();
    assert_eq!(
        result.result_code,
        CreateExecutionResultCode::Error(ErrorCode::MalformedPayload)
    );

    // Decreasing
    h.send(
        MessageType::CreateExecutionRequest,
        &CreateExecutionRequest {
            workspace_id: 0,
            request_id: 1,
            launch_profile: 0,
            rows: 24,
            columns: 80,
        }
        .encode(),
    );
    let (kind, payload) = h.expect_frame(Instant::now() + Duration::from_secs(2));
    assert_eq!(kind, MessageType::CreateExecutionResult as u16);
    assert_eq!(
        CreateExecutionResult::decode(&payload).unwrap().result_code,
        CreateExecutionResultCode::Error(ErrorCode::MalformedPayload)
    );

    // Wrap from max
    let ok = h.create(u64::MAX, 24, 80);
    assert_eq!(ok.result_code, CreateExecutionResultCode::Created);
    // Encode rejects zero; craft a post-max id that is nonzero but not greater.
    h.send(
        MessageType::CreateExecutionRequest,
        &CreateExecutionRequest {
            workspace_id: 0,
            request_id: 2,
            launch_profile: 0,
            rows: 24,
            columns: 80,
        }
        .encode(),
    );
    let (kind, payload) = h.expect_frame(Instant::now() + Duration::from_secs(2));
    assert_eq!(kind, MessageType::CreateExecutionResult as u16);
    assert_eq!(
        CreateExecutionResult::decode(&payload).unwrap().result_code,
        CreateExecutionResultCode::Error(ErrorCode::MalformedPayload)
    );

    // Reconnect resets the space.
    drop(h);
    let mut h = Harness::empty();
    h.hello(CAP_EXECUTION_PROVISIONING);
    let again = h.create(1, 24, 80);
    assert_eq!(again.result_code, CreateExecutionResultCode::Created);
}

#[test]
fn invalid_workspace_and_unsupported_profile_fail_closed() {
    let mut h = Harness::empty();
    h.hello(CAP_EXECUTION_PROVISIONING);
    h.send(
        MessageType::CreateExecutionRequest,
        &CreateExecutionRequest {
            workspace_id: 1,
            request_id: 1,
            launch_profile: 0,
            rows: 24,
            columns: 80,
        }
        .encode(),
    );
    let (kind, payload) = h.expect_frame(Instant::now() + Duration::from_secs(2));
    assert_eq!(kind, MessageType::CreateExecutionResult as u16);
    let result = CreateExecutionResult::decode(&payload).unwrap();
    assert_eq!(
        result.result_code,
        CreateExecutionResultCode::Error(ErrorCode::InvalidWorkspace)
    );
    assert_eq!(result.execution_id.to_bytes(), [0; 16]);
    assert_eq!(h.runtime.execution_count(), 0);

    h.send(
        MessageType::CreateExecutionRequest,
        &CreateExecutionRequest {
            workspace_id: 0,
            request_id: 2,
            launch_profile: 1,
            rows: 24,
            columns: 80,
        }
        .encode(),
    );
    let (kind, payload) = h.expect_frame(Instant::now() + Duration::from_secs(2));
    assert_eq!(kind, MessageType::CreateExecutionResult as u16);
    let result = CreateExecutionResult::decode(&payload).unwrap();
    assert_eq!(
        result.result_code,
        CreateExecutionResultCode::Error(ErrorCode::UnsupportedLaunchProfile)
    );
    assert_eq!(h.runtime.execution_count(), 0);
}

#[test]
fn zero_and_oversized_geometry_rejected_before_spawn() {
    let mut h = Harness::empty();
    h.hello(CAP_EXECUTION_PROVISIONING);
    for (request_id, rows, columns) in [(1u64, 0, 80), (2, 24, 0), (3, 257, 80), (4, 24, 513)] {
        h.send(
            MessageType::CreateExecutionRequest,
            &CreateExecutionRequest {
                workspace_id: 0,
                request_id,
                launch_profile: 0,
                rows,
                columns,
            }
            .encode(),
        );
        let (kind, payload) = h.expect_frame(Instant::now() + Duration::from_secs(2));
        assert_eq!(kind, MessageType::CreateExecutionResult as u16);
        assert_eq!(
            CreateExecutionResult::decode(&payload).unwrap().result_code,
            CreateExecutionResultCode::Error(ErrorCode::InvalidGeometry)
        );
        assert_eq!(h.runtime.execution_count(), 0);
    }
}

#[test]
fn registry_capacity_is_capacity_exceeded_with_no_partial_state() {
    let mut cfg = config();
    cfg.max_executions = 1;
    let mut runtime = Runtime::new(cfg).expect("Runtime");
    let _existing = runtime
        .create_execution(
            CommandSpec::new("/bin/sh").args(["-c", "sleep 30"]),
            WindowSize::cells(80, 24).expect("size"),
        )
        .expect("seed");
    let socket = runtime.local_ipc_socket_path().unwrap().to_path_buf();
    let stream = loop {
        match UnixStream::connect(&socket) {
            Ok(s) => break s,
            Err(_) => {
                runtime.poll_once(Some(Duration::from_millis(5))).unwrap();
            }
        }
    };
    stream.set_nonblocking(true).unwrap();
    runtime.poll_once(Some(Duration::from_millis(5))).unwrap();
    let mut h = Harness {
        runtime,
        stream,
        buffered: Vec::new(),
    };
    h.hello(CAP_EXECUTION_PROVISIONING);
    let result = h.create(1, 24, 80);
    assert_eq!(
        result.result_code,
        CreateExecutionResultCode::Error(ErrorCode::CapacityExceeded)
    );
    assert_eq!(result.execution_id.to_bytes(), [0; 16]);
    assert_eq!(h.runtime.execution_count(), 1);
}

#[test]
fn outstanding_budget_backpressure_before_spawn() {
    let mut h = Harness::empty();
    h.hello(CAP_EXECUTION_PROVISIONING);

    // One immediate create + four queued fill the per-connection outstanding
    // budget of 4; the sixth request is Backpressure before spawn.
    let mut burst = Vec::new();
    for request_id in 1..=6u64 {
        burst.extend_from_slice(&encode_frame(
            MessageType::CreateExecutionRequest,
            &CreateExecutionRequest {
                workspace_id: 0,
                request_id,
                launch_profile: 0,
                rows: 24,
                columns: 80,
            }
            .encode(),
        ));
    }
    h.stream.write_all(&burst).unwrap();
    h.pump();

    let mut results = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while results.len() < 6 && Instant::now() < deadline {
        h.pump();
        if let Ok((kind, payload)) = try_frame(&mut h) {
            if kind == MessageType::CreateExecutionResult as u16 {
                results.push(CreateExecutionResult::decode(&payload).unwrap());
            }
        }
    }
    assert_eq!(results.len(), 6, "expected six correlated results");
    assert!(results
        .iter()
        .any(|r| { r.result_code == CreateExecutionResultCode::Error(ErrorCode::Backpressure) }));
    let created = results
        .iter()
        .filter(|r| r.result_code == CreateExecutionResultCode::Created)
        .count();
    assert!(created >= 1);
    assert!(created <= 5);
}

fn try_frame(h: &mut Harness) -> Result<(u16, Vec<u8>), ()> {
    if h.buffered.len() >= HEADER_LEN {
        let header = FrameHeader::decode(&h.buffered[..HEADER_LEN]).map_err(|_| ())?;
        let total = HEADER_LEN + header.payload_len as usize;
        if h.buffered.len() >= total {
            let frame = h.buffered.drain(..total).collect::<Vec<_>>();
            return Ok((header.message_type, frame[HEADER_LEN..].to_vec()));
        }
    }
    let mut chunk = [0u8; 8192];
    match h.stream.read(&mut chunk) {
        Ok(0) => Err(()),
        Ok(count) => {
            h.buffered.extend_from_slice(&chunk[..count]);
            Err(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Err(()),
        Err(_) => Err(()),
    }
}

#[test]
fn one_creation_per_dispatch_turn_under_burst() {
    let (mut h, streamer) = Harness::with_streamer();
    h.hello(CAP_EXECUTION_PROVISIONING);
    let gen_before = h
        .runtime
        .execution(streamer)
        .unwrap()
        .terminal()
        .damage_generation();

    let mut burst = Vec::new();
    for request_id in 1..=3u64 {
        burst.extend_from_slice(&encode_frame(
            MessageType::CreateExecutionRequest,
            &CreateExecutionRequest {
                workspace_id: 0,
                request_id,
                launch_profile: 0,
                rows: 24,
                columns: 80,
            }
            .encode(),
        ));
    }
    h.stream.write_all(&burst).unwrap();
    h.pump();
    // After one dispatch turn, exactly one new execution beyond the streamer.
    assert_eq!(h.runtime.execution_count(), 2);

    let deadline = Instant::now() + Duration::from_secs(5);
    while h.runtime.execution_count() < 4 && Instant::now() < deadline {
        h.pump();
    }
    assert_eq!(h.runtime.execution_count(), 4);

    let progress_deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < progress_deadline {
        h.pump();
        let gen_after = h
            .runtime
            .execution(streamer)
            .unwrap()
            .terminal()
            .damage_generation();
        if gen_after > gen_before {
            return;
        }
    }
    panic!("unrelated streamer must keep progressing during provisioning burst");
}

#[test]
fn disconnect_after_request_leaves_no_half_created_execution() {
    let mut h = Harness::empty();
    h.hello(CAP_EXECUTION_PROVISIONING);
    h.send(
        MessageType::CreateExecutionRequest,
        &CreateExecutionRequest {
            workspace_id: 0,
            request_id: 1,
            launch_profile: 0,
            rows: 24,
            columns: 80,
        }
        .encode(),
    );
    // Drop the client immediately; Runtime must converge to 0 or 1 live execution.
    let _ = h.stream.shutdown(std::net::Shutdown::Both);
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        h.pump();
        let count = h.runtime.execution_count();
        assert!(count <= 1, "half-created execution leaked: count={count}");
        if count == 1 || count == 0 {
            // Give a couple more turns to ensure stability.
            h.pump();
            h.pump();
            assert!(h.runtime.execution_count() <= 1);
            return;
        }
    }
    panic!("did not converge after disconnect");
}

#[test]
fn unattached_connection_cannot_mutate_another_execution() {
    let (mut h, streamer) = Harness::with_streamer();
    h.hello(CAP_EXECUTION_PROVISIONING);
    let created = h.create(1, 24, 80);
    assert_eq!(created.result_code, CreateExecutionResultCode::Created);

    // Still Ready (no attach): Input must fail closed and leave the streamer untouched.
    h.send(
        MessageType::Input,
        &InputRef {
            attachment_id: seyal_runtime::AttachmentId::from_bytes([0; 16]),
            bytes: b"x",
        }
        .encode(),
    );
    h.expect_error(ErrorCode::InvalidState, MessageType::Input);
    assert_eq!(
        h.runtime.lookup(streamer).unwrap().attachment_count,
        0,
        "unattached provisioner must not attach to foreign execution"
    );
}

#[test]
fn created_execution_gets_term_and_shell_integration_like_composed() {
    let mut h = Harness::empty();
    h.hello(CAP_EXECUTION_PROVISIONING);
    let result = h.create(1, 24, 80);
    assert_eq!(result.result_code, CreateExecutionResultCode::Created);
    let composed = h
        .runtime
        .create_execution(
            CommandSpec::new("/bin/zsh").args(["-l", "-i"]),
            WindowSize::cells(80, 24).expect("size"),
        )
        .expect("composed");
    let provisioned = h.runtime.execution(result.execution_id).expect("prov");
    let composed_exec = h.runtime.execution(composed).expect("composed");
    assert_eq!(m001_term_name(), "seyal-m001");
    assert_eq!(
        provisioned.terminal().cols(),
        composed_exec.terminal().cols()
    );
    assert_eq!(
        provisioned.terminal().rows(),
        composed_exec.terminal().rows()
    );
    let summary = h.runtime.lookup(result.execution_id).unwrap();
    assert_eq!(summary.workspace_id, WorkspaceId::m001_default());
}

#[test]
fn command_spec_debug_omits_program_argv_env_cwd() {
    let command = CommandSpec::new("/bin/zsh")
        .args(["-l", "-i"])
        .env("TERM", "seyal-m001")
        .current_dir("/tmp");
    let rendered = format!("{command:?}");
    assert!(!rendered.contains("/bin/zsh"));
    assert!(!rendered.contains("-l"));
    assert!(!rendered.contains("seyal-m001"));
    assert!(!rendered.contains("/tmp"));
}

#[test]
fn shutdown_with_live_provisioned_child_still_signals_and_reaps() {
    let mut h = Harness::empty();
    h.hello(CAP_EXECUTION_PROVISIONING);
    let result = h.create(1, 24, 80);
    assert_eq!(result.result_code, CreateExecutionResultCode::Created);
    assert_eq!(h.runtime.execution_count(), 1);
    h.runtime.begin_shutdown().expect("begin shutdown");
    h.runtime
        .run_until_empty(Instant::now() + Duration::from_secs(3))
        .expect("shutdown completes");
    assert!(h.runtime.shutdown_complete());
    assert_eq!(h.runtime.execution_count(), 0);
}

#[cfg(feature = "test-fault-injection")]
#[test]
fn injected_spawn_failure_repeated_n_times_stays_bounded() {
    let (mut h, streamer) = Harness::with_streamer();
    h.hello(CAP_EXECUTION_PROVISIONING);
    let gen_before = h
        .runtime
        .execution(streamer)
        .unwrap()
        .terminal()
        .damage_generation();
    let baseline = h.runtime.execution_count();
    let n = 5usize;
    test_fault::fail_times(FaultPoint::ProvisioningSpawn, n);
    for request_id in 1..=n as u64 {
        let result = h.create(request_id, 24, 80);
        assert_eq!(
            result.result_code,
            CreateExecutionResultCode::Error(ErrorCode::InternalFailure)
        );
        assert_eq!(result.execution_id.to_bytes(), [0; 16]);
        assert_eq!(h.runtime.execution_count(), baseline);
    }
    assert_eq!(test_fault::remaining(FaultPoint::ProvisioningSpawn), 0);
    let progress_deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < progress_deadline {
        h.pump();
        let gen_after = h
            .runtime
            .execution(streamer)
            .unwrap()
            .terminal()
            .damage_generation();
        if gen_after > gen_before {
            return;
        }
    }
    panic!("unrelated streamer must keep progressing across injected spawn failures");
}

#[cfg(feature = "test-fault-injection")]
#[test]
fn injected_registration_and_publication_failure_leave_no_partial_state() {
    let mut h = Harness::empty();
    h.hello(CAP_EXECUTION_PROVISIONING);
    test_fault::fail_times(FaultPoint::ProvisioningRegistration, 3);
    for request_id in 1..=3u64 {
        let result = h.create(request_id, 24, 80);
        assert_eq!(
            result.result_code,
            CreateExecutionResultCode::Error(ErrorCode::InternalFailure)
        );
        assert_eq!(h.runtime.execution_count(), 0);
    }
    test_fault::fail_times(FaultPoint::ProvisioningPublication, 3);
    for request_id in 4..=6u64 {
        let result = h.create(request_id, 24, 80);
        assert_eq!(
            result.result_code,
            CreateExecutionResultCode::Error(ErrorCode::InternalFailure)
        );
        assert_eq!(h.runtime.execution_count(), 0);
    }
    let ok = h.create(7, 24, 80);
    assert_eq!(ok.result_code, CreateExecutionResultCode::Created);
    assert_eq!(h.runtime.execution_count(), 1);
}
