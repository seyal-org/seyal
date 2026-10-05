#![cfg(target_os = "macos")]

//! End-to-end Candidate-D local attachment tests using a real Runtime, PTY,
//! Seyal VT state and Unix-domain socket. The client owns only a disposable
//! display cache; it never maps canonical/runtime memory or reparses PTY bytes.

use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::{Duration, Instant},
};

use seyal_exec::{CommandSpec, WindowSize};
use seyal_runtime::{
    display::{decode_chunk, empty_cache, DecodedDisplayChunk, DisplayCache},
    local_ipc::framing::{
        encode_frame, Attach, Attached, ClientHello, ErrorCode, ErrorMessage, FrameHeader,
        InputRef, MessageType, Resize as WireResize, ResumeDelivery, Role, ServerHello,
        SuspendDelivery, CAP_ATTACHMENT_DELIVERY_CONTROL, HEADER_LEN,
    },
    ExecutionId, LocalIpcMode, Runtime, RuntimeConfig,
};

fn unique_suffix() -> u64 {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

fn config(_test: &str) -> RuntimeConfig {
    let mut config = RuntimeConfig::m001().expect("bundled capability profile");
    let suffix = unique_suffix();
    config.singleton_path = std::env::temp_dir().join(format!("s5-{suffix:x}.lock"));
    config.local_ipc = LocalIpcMode::Enabled {
        runtime_dir_override: Some(std::env::temp_dir().join(format!("s5d-{suffix:x}"))),
    };
    config.graceful_termination = Duration::from_millis(50);
    config.forced_reap = Duration::from_millis(250);
    config.final_drain = Duration::from_millis(100);
    config
}

fn size() -> WindowSize {
    WindowSize::new(80, 24, 0, 0).expect("valid size")
}

struct Client {
    stream: UnixStream,
    buffered: Vec<u8>,
}

struct Harness {
    runtime: Runtime,
}

impl Harness {
    fn new(test: &str) -> Self {
        Self {
            runtime: Runtime::new(config(test)).expect("Runtime"),
        }
    }

    fn socket_path(&self) -> PathBuf {
        self.runtime
            .local_ipc_socket_path()
            .expect("local IPC bound")
            .to_path_buf()
    }

    fn spawn(&mut self, command: CommandSpec) -> ExecutionId {
        self.runtime
            .create_execution(command, size())
            .expect("execution")
    }

    fn pump(&mut self) {
        self.runtime
            .poll_once(Some(Duration::from_millis(5)))
            .expect("Runtime poll");
    }

    fn connect(&mut self) -> Client {
        let path = self.socket_path();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match UnixStream::connect(&path) {
                Ok(stream) => {
                    stream.set_nonblocking(true).expect("nonblocking client");
                    self.pump();
                    return Client {
                        stream,
                        buffered: Vec::new(),
                    };
                }
                Err(_) => {
                    assert!(Instant::now() < deadline, "connect timed out");
                    self.pump();
                }
            }
        }
    }

    fn send(&mut self, client: &mut Client, kind: MessageType, payload: &[u8]) {
        let frame = encode_frame(kind, payload);
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut sent = 0;
        while sent < frame.len() {
            match client.stream.write(&frame[sent..]) {
                Ok(0) => panic!("client write returned zero"),
                Ok(count) => sent += count,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => self.pump(),
                Err(error) => panic!("client write failed: {error}"),
            }
            assert!(Instant::now() < deadline, "client write timed out");
        }
        self.pump();
    }

    fn expect_frame(&mut self, client: &mut Client, deadline: Instant) -> (u16, Vec<u8>) {
        loop {
            if client.buffered.len() >= HEADER_LEN {
                let header =
                    FrameHeader::decode(&client.buffered[..HEADER_LEN]).expect("valid header");
                let total = HEADER_LEN + header.payload_len as usize;
                if client.buffered.len() >= total {
                    let frame = client.buffered.drain(..total).collect::<Vec<_>>();
                    return (header.message_type, frame[HEADER_LEN..].to_vec());
                }
            }

            let mut chunk = [0u8; 16 * 1024];
            match client.stream.read(&mut chunk) {
                Ok(0) => panic!("connection closed while awaiting frame"),
                Ok(count) => client.buffered.extend_from_slice(&chunk[..count]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => self.pump(),
                Err(error) => panic!("client read failed: {error}"),
            }
            assert!(Instant::now() < deadline, "timed out awaiting frame");
        }
    }

    fn hello(&mut self, client: &mut Client) {
        self.hello_caps(client, 0);
    }

    fn hello_caps(&mut self, client: &mut Client, client_capabilities: u32) {
        self.send(
            client,
            MessageType::ClientHello,
            &ClientHello {
                client_capabilities,
            }
            .encode(),
        );
        let (kind, payload) = self.expect_frame(client, Instant::now() + Duration::from_secs(2));
        assert_eq!(kind, MessageType::ServerHello as u16);
        let hello = ServerHello::decode(&payload).expect("ServerHello");
        assert_ne!(
            hello.server_capabilities & seyal_runtime::local_ipc::framing::CAP_BINARY_DISPLAY,
            0
        );
    }

    fn attach(
        &mut self,
        client: &mut Client,
        execution_id: ExecutionId,
        role: Role,
    ) -> (Attached, DisplayCache) {
        self.send(
            client,
            MessageType::Attach,
            &Attach {
                execution_id,
                requested_role: role,
            }
            .encode(),
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        let (kind, payload) = self.expect_frame(client, deadline);
        assert_eq!(kind, MessageType::Attached as u16);
        let attached = Attached::decode(&payload).expect("Attached");
        let chunks = self.expect_display_batch(client, MessageType::DisplaySnapshot, deadline);
        let mut cache = empty_cache();
        cache.apply_chunks(&chunks).expect("initial snapshot apply");
        assert_eq!(cache.generation, attached.current_generation);
        (attached, cache)
    }

    fn expect_display_batch(
        &mut self,
        client: &mut Client,
        expected_kind: MessageType,
        deadline: Instant,
    ) -> Vec<DecodedDisplayChunk> {
        let (kind, payload) = self.expect_frame(client, deadline);
        assert_eq!(kind, expected_kind as u16);
        let first_frame = encode_frame(expected_kind, &payload);
        let first = decode_chunk(&first_frame).expect("display chunk");
        let count = first.chunk_count as usize;
        let mut chunks = vec![first];
        for _ in 1..count {
            let (kind, payload) = self.expect_frame(client, deadline);
            assert_eq!(kind, expected_kind as u16);
            chunks
                .push(decode_chunk(&encode_frame(expected_kind, &payload)).expect("display chunk"));
        }
        chunks
    }

    fn apply_next_display(&mut self, client: &mut Client, cache: &mut DisplayCache) {
        let deadline = Instant::now() + Duration::from_secs(5);
        let (kind, payload) = self.expect_frame(client, deadline);
        let message_type = MessageType::from_u16(kind).expect("known message type");
        assert!(matches!(
            message_type,
            MessageType::DisplaySnapshot | MessageType::DisplayDelta
        ));
        let first = decode_chunk(&encode_frame(message_type, &payload)).expect("display chunk");
        let count = first.chunk_count as usize;
        let mut chunks = vec![first];
        for _ in 1..count {
            let (next_kind, next_payload) = self.expect_frame(client, deadline);
            assert_eq!(next_kind, kind);
            chunks.push(
                decode_chunk(&encode_frame(message_type, &next_payload)).expect("display chunk"),
            );
        }
        cache.apply_chunks(&chunks).expect("display apply");
    }

    fn wait_for_row(
        &mut self,
        client: &mut Client,
        cache: &mut DisplayCache,
        predicate: impl Fn(&str) -> bool,
    ) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let row = first_row_text(cache);
            if predicate(&row) {
                return row;
            }
            assert!(
                Instant::now() < deadline,
                "display condition timed out: {row:?}"
            );
            self.apply_next_display(client, cache);
        }
    }
}

fn first_row_text(cache: &DisplayCache) -> String {
    if cache.rows == 0 || cache.columns == 0 {
        return String::new();
    }
    cache.cells[..cache.columns as usize]
        .iter()
        .map(|cell| cell.scalar)
        .collect()
}

#[test]
fn attach_receives_current_snapshot_from_real_pty_state() {
    let mut harness = Harness::new("initial-snapshot");
    let execution_id =
        harness.spawn(CommandSpec::new("/bin/sh").args(["-c", "printf hi; sleep 2"]));
    let mut client = harness.connect();
    harness.hello(&mut client);
    let (_attached, mut cache) = harness.attach(&mut client, execution_id, Role::Observer);
    let row = harness.wait_for_row(&mut client, &mut cache, |row| row.starts_with("hi"));
    assert!(row.starts_with("hi"));
}

#[test]
fn controller_input_reaches_pty_and_arrives_as_display_state_without_ack() {
    let mut harness = Harness::new("controller-input");
    let execution_id = harness.spawn(CommandSpec::new("/bin/cat"));
    let mut client = harness.connect();
    harness.hello(&mut client);
    let (attached, mut cache) = harness.attach(&mut client, execution_id, Role::Controller);
    harness.send(
        &mut client,
        MessageType::Input,
        &InputRef {
            attachment_id: attached.attachment_id,
            bytes: b"AB",
        }
        .encode(),
    );
    let row = harness.wait_for_row(&mut client, &mut cache, |row| row.starts_with("AB"));
    assert!(row.starts_with("AB"));
}

#[test]
fn observer_input_is_denied_without_mutating_execution() {
    let mut harness = Harness::new("observer-denied");
    let execution_id = harness.spawn(CommandSpec::new("/bin/cat"));
    let mut client = harness.connect();
    harness.hello(&mut client);
    let (attached, _cache) = harness.attach(&mut client, execution_id, Role::Observer);
    harness.send(
        &mut client,
        MessageType::Input,
        &InputRef {
            attachment_id: attached.attachment_id,
            bytes: b"ZZ",
        }
        .encode(),
    );
    let (kind, payload) =
        harness.expect_frame(&mut client, Instant::now() + Duration::from_secs(2));
    assert_eq!(kind, MessageType::Error as u16);
    assert_eq!(
        ErrorMessage::decode(&payload).unwrap().error_code,
        ErrorCode::PermissionDenied as u16
    );
}

#[test]
fn second_controller_is_rejected_without_preempting_first() {
    let mut harness = Harness::new("controller-lease");
    let execution_id = harness.spawn(CommandSpec::new("/bin/cat"));
    let mut first = harness.connect();
    harness.hello(&mut first);
    let (_attached, _cache) = harness.attach(&mut first, execution_id, Role::Controller);

    let mut second = harness.connect();
    harness.hello(&mut second);
    harness.send(
        &mut second,
        MessageType::Attach,
        &Attach {
            execution_id,
            requested_role: Role::Controller,
        }
        .encode(),
    );
    let (kind, payload) =
        harness.expect_frame(&mut second, Instant::now() + Duration::from_secs(2));
    assert_eq!(kind, MessageType::Error as u16);
    assert_eq!(
        ErrorMessage::decode(&payload).unwrap().error_code,
        ErrorCode::ControllerBusy as u16
    );
}

#[test]
fn multiple_viewers_receive_same_canonical_generation() {
    let mut harness = Harness::new("fanout");
    let execution_id = harness.spawn(CommandSpec::new("/bin/cat"));
    let mut controller = harness.connect();
    harness.hello(&mut controller);
    let (attached, mut controller_cache) =
        harness.attach(&mut controller, execution_id, Role::Controller);
    let mut observer = harness.connect();
    harness.hello(&mut observer);
    let (_observer_attached, mut observer_cache) =
        harness.attach(&mut observer, execution_id, Role::Observer);

    harness.send(
        &mut controller,
        MessageType::Input,
        &InputRef {
            attachment_id: attached.attachment_id,
            bytes: b"FAN",
        }
        .encode(),
    );
    harness.wait_for_row(&mut controller, &mut controller_cache, |row| {
        row.starts_with("FAN")
    });
    harness.wait_for_row(&mut observer, &mut observer_cache, |row| {
        row.starts_with("FAN")
    });
    assert_eq!(controller_cache.generation, observer_cache.generation);
    assert_eq!(controller_cache.cells, observer_cache.cells);
}

#[test]
fn resize_rebuilds_cache_without_projection_replacement_fd() {
    let mut harness = Harness::new("resize");
    let execution_id = harness.spawn(CommandSpec::new("/bin/cat"));
    let mut client = harness.connect();
    harness.hello(&mut client);
    let (attached, mut cache) = harness.attach(&mut client, execution_id, Role::Controller);
    harness.send(
        &mut client,
        MessageType::Resize,
        &WireResize {
            attachment_id: attached.attachment_id,
            rows: 40,
            columns: 120,
        }
        .encode(),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while (cache.rows, cache.columns) != (40, 120) {
        assert!(Instant::now() < deadline, "resize display update timed out");
        harness.apply_next_display(&mut client, &mut cache);
    }
    assert_eq!((cache.rows, cache.columns), (40, 120));
}

#[test]
fn explicit_resync_returns_current_self_contained_snapshot() {
    let mut harness = Harness::new("resync");
    let execution_id = harness.spawn(CommandSpec::new("/bin/cat"));
    let mut client = harness.connect();
    harness.hello(&mut client);
    let (attached, mut cache) = harness.attach(&mut client, execution_id, Role::Controller);
    harness.send(
        &mut client,
        MessageType::Input,
        &InputRef {
            attachment_id: attached.attachment_id,
            bytes: b"RS",
        }
        .encode(),
    );
    harness.wait_for_row(&mut client, &mut cache, |row| row.starts_with("RS"));

    harness.send(
        &mut client,
        MessageType::Resync,
        &seyal_runtime::local_ipc::framing::Resync {
            attachment_id: attached.attachment_id,
        }
        .encode(),
    );
    let chunks = harness.expect_display_batch(
        &mut client,
        MessageType::DisplaySnapshot,
        Instant::now() + Duration::from_secs(3),
    );
    let mut rebuilt = empty_cache();
    rebuilt.apply_chunks(&chunks).unwrap();
    assert_eq!(first_row_text(&rebuilt), first_row_text(&cache));
}

#[test]
fn reconnect_gets_current_state_without_pty_byte_replay() {
    let mut harness = Harness::new("reconnect");
    let execution_id = harness.spawn(CommandSpec::new("/bin/cat"));
    let mut first = harness.connect();
    harness.hello(&mut first);
    let (attached, mut first_cache) = harness.attach(&mut first, execution_id, Role::Controller);
    harness.send(
        &mut first,
        MessageType::Input,
        &InputRef {
            attachment_id: attached.attachment_id,
            bytes: b"XY",
        }
        .encode(),
    );
    harness.wait_for_row(&mut first, &mut first_cache, |row| row.starts_with("XY"));
    drop(first);
    harness.pump();

    let mut second = harness.connect();
    harness.hello(&mut second);
    let (second_attached, second_cache) = harness.attach(&mut second, execution_id, Role::Observer);
    assert_ne!(second_attached.attachment_id, attached.attachment_id);
    assert!(first_row_text(&second_cache).starts_with("XY"));
}

#[test]
fn killed_or_nonreading_client_never_blocks_healthy_viewer_or_pty() {
    let mut harness = Harness::new("slow-client");
    let execution_id = harness.spawn(CommandSpec::new("/bin/sh").args([
        "-c",
        "i=0; while [ $i -lt 20000 ]; do printf 'LINE%05d\\r\\n' $i; i=$((i+1)); done; sleep 1",
    ]));

    let mut slow = harness.connect();
    harness.hello(&mut slow);
    let (_slow_attached, _slow_cache) = harness.attach(&mut slow, execution_id, Role::Observer);
    // Stop reading from this connection while Runtime continues processing.

    let mut healthy = harness.connect();
    harness.hello(&mut healthy);
    let (_healthy_attached, mut healthy_cache) =
        harness.attach(&mut healthy, execution_id, Role::Observer);
    let deadline = Instant::now() + Duration::from_secs(8);
    while healthy_cache.generation < 2 {
        assert!(
            Instant::now() < deadline,
            "healthy viewer stopped making progress"
        );
        harness.apply_next_display(&mut healthy, &mut healthy_cache);
    }
    drop(slow);
    harness.pump();
}

#[test]
fn finalized_execution_cannot_be_attached() {
    let mut harness = Harness::new("finalized-attach");
    let execution_id = harness.spawn(CommandSpec::new("/bin/sh").args(["-c", "printf done"]));
    let deadline = Instant::now() + Duration::from_secs(3);
    while harness.runtime.lookup(execution_id).is_some() {
        assert!(Instant::now() < deadline, "execution never finalized");
        harness.pump();
    }

    let mut client = harness.connect();
    harness.hello(&mut client);
    harness.send(
        &mut client,
        MessageType::Attach,
        &Attach {
            execution_id,
            requested_role: Role::Observer,
        }
        .encode(),
    );
    let (kind, payload) = harness.expect_frame(&mut client, deadline);
    assert_eq!(kind, MessageType::Error as u16);
    assert_eq!(
        ErrorMessage::decode(&payload).unwrap().error_code,
        ErrorCode::InvalidExecution as u16
    );
}

#[test]
fn suspend_delivery_drops_deltas_keeps_attachment_and_pty_progress() {
    let mut harness = Harness::new("w5-suspend");
    let execution_id = harness.spawn(CommandSpec::new("/bin/cat"));
    let mut client = harness.connect();
    harness.hello_caps(&mut client, CAP_ATTACHMENT_DELIVERY_CONTROL);
    let (attached, mut cache) = harness.attach(&mut client, execution_id, Role::Controller);
    let attachment = attached.attachment_id;

    harness.send(
        &mut client,
        MessageType::SuspendDelivery,
        &SuspendDelivery {
            attachment_id: attachment,
        }
        .encode(),
    );

    harness.send(
        &mut client,
        MessageType::Input,
        &InputRef {
            attachment_id: attachment,
            bytes: b"HID",
        }
        .encode(),
    );
    let quiet_until = Instant::now() + Duration::from_millis(250);
    while Instant::now() < quiet_until {
        harness.pump();
        let mut chunk = [0u8; 4096];
        match client.stream.read(&mut chunk) {
            Ok(0) => panic!("connection closed while Hidden"),
            Ok(count) => {
                client.buffered.extend_from_slice(&chunk[..count]);
                while client.buffered.len() >= HEADER_LEN {
                    let header =
                        FrameHeader::decode(&client.buffered[..HEADER_LEN]).expect("header");
                    let total = HEADER_LEN + header.payload_len as usize;
                    if client.buffered.len() < total {
                        break;
                    }
                    let kind = header.message_type;
                    let _ = client.buffered.drain(..total);
                    assert_ne!(
                        kind,
                        MessageType::DisplayDelta as u16,
                        "Runtime must not write DisplayDelta for a suspended attachment"
                    );
                    assert_ne!(
                        kind,
                        MessageType::DisplaySnapshot as u16,
                        "Runtime must not write DisplaySnapshot for a suspended attachment"
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("client read failed: {error}"),
        }
    }

    harness.send(
        &mut client,
        MessageType::ResumeDelivery,
        &ResumeDelivery {
            attachment_id: attachment,
        }
        .encode(),
    );
    let chunks = harness.expect_display_batch(
        &mut client,
        MessageType::DisplaySnapshot,
        Instant::now() + Duration::from_secs(3),
    );
    cache.apply_chunks(&chunks).unwrap();
    assert!(
        first_row_text(&cache).contains("HID"),
        "reveal resync must include PTY progress accumulated while Hidden"
    );
    assert_eq!(attached.attachment_id, attachment);
}

#[test]
fn child_exit_while_hidden_still_finalizes() {
    let mut harness = Harness::new("w5-exit-hidden");
    let execution_id =
        harness.spawn(CommandSpec::new("/bin/sh").args(["-c", "printf bye; exit 0"]));
    let mut client = harness.connect();
    harness.hello_caps(&mut client, CAP_ATTACHMENT_DELIVERY_CONTROL);
    let (attached, _cache) = harness.attach(&mut client, execution_id, Role::Controller);
    harness.send(
        &mut client,
        MessageType::SuspendDelivery,
        &SuspendDelivery {
            attachment_id: attached.attachment_id,
        }
        .encode(),
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut saw_final = false;
    while Instant::now() < deadline {
        harness.pump();
        let mut chunk = [0u8; 8192];
        match client.stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                client.buffered.extend_from_slice(&chunk[..count]);
                while client.buffered.len() >= HEADER_LEN {
                    let header =
                        FrameHeader::decode(&client.buffered[..HEADER_LEN]).expect("header");
                    let total = HEADER_LEN + header.payload_len as usize;
                    if client.buffered.len() < total {
                        break;
                    }
                    let kind = header.message_type;
                    let _ = client.buffered.drain(..total);
                    if kind == MessageType::Lifecycle as u16 {
                        saw_final = true;
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("{error}"),
        }
        if saw_final {
            break;
        }
    }
    assert!(
        saw_final,
        "SPEC-009 §11.3 final drain must still reach a Hidden attachment"
    );
}
