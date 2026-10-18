#![cfg(target_os = "macos")]

//! ADR-009 2026-09-19 duration amendment / #1043 over the real local IPC
//! socket: capability negotiation and per-schema `BlockTimeline` fan-out from
//! a live zsh execution with the bundled shell-integration hooks.

use std::{
    fs,
    io::{Read, Write},
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use seyal_exec::{CommandSpec, WindowSize};
use seyal_runtime::{
    local_ipc::framing::{
        encode_frame, Attach, Attached, BlockTimeline, BlockTimelineSchema, ClientHello,
        CommandBlockState, ComposerCommandRef, ComposerResult, ComposerResultCode, ErrorCode,
        ErrorMessage, FrameHeader, MessageType, Role, ServerHello, CAP_COMMAND_BLOCKS,
        CAP_COMMAND_BLOCK_DURATION, HEADER_LEN,
    },
    ExecutionId, LocalIpcMode, Runtime, RuntimeConfig, ShellIntegrationPolicy,
};

const DEADLINE: Duration = Duration::from_secs(10);

struct Client {
    stream: UnixStream,
    buffered: Vec<u8>,
}

struct Harness {
    runtime: Runtime,
    root: PathBuf,
}

fn unique_dir() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "s1043-{}-{}-{nonce:x}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).expect("test dir");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).expect("chmod");
    dir
}

fn write_private(path: &Path, content: &str) {
    fs::write(path, content).expect("write");
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).expect("chmod");
}

impl Harness {
    fn new() -> Self {
        let root = unique_dir();
        let zdotdir = root.join("bundle");
        fs::create_dir(&zdotdir).expect("bundle dir");
        fs::set_permissions(&zdotdir, fs::Permissions::from_mode(0o700)).expect("chmod");
        write_private(
            &zdotdir.join(".zshenv"),
            ShellIntegrationPolicy::bundled_zshenv(),
        );
        let mut config = RuntimeConfig::m001().expect("config");
        config.singleton_path = root.join("runtime.lock");
        config.local_ipc = LocalIpcMode::Enabled {
            runtime_dir_override: Some(root.join("ipc")),
        };
        config.shell_integration_policy =
            Some(ShellIntegrationPolicy::from_zdotdir(&zdotdir).expect("policy"));
        config.graceful_termination = Duration::from_millis(100);
        config.forced_reap = Duration::from_millis(250);
        config.final_drain = Duration::from_millis(100);
        Self {
            runtime: Runtime::new(config).expect("Runtime"),
            root,
        }
    }

    fn spawn_zsh(&mut self) -> ExecutionId {
        let home = self.root.join("home");
        fs::create_dir(&home).expect("home dir");
        write_private(&home.join(".zshrc"), "PROMPT='T%% '\n");
        let command = CommandSpec::new("/bin/zsh")
            .args(["-i"])
            .clear_environment()
            .env("HOME", &home)
            .env("PATH", "/usr/bin:/bin")
            .env("HISTFILE", home.join(".zsh_history"))
            .env("LANG", "C");
        self.runtime
            .create_execution(command, WindowSize::cells(80, 24).expect("size"))
            .expect("spawn zsh")
    }

    fn pump(&mut self) {
        self.runtime
            .poll_once(Some(Duration::from_millis(5)))
            .expect("Runtime poll");
    }

    fn connect(&mut self) -> Client {
        let path = self
            .runtime
            .local_ipc_socket_path()
            .expect("local IPC bound")
            .to_path_buf();
        let deadline = Instant::now() + DEADLINE;
        loop {
            if let Ok(stream) = UnixStream::connect(&path) {
                stream.set_nonblocking(true).expect("nonblocking");
                self.pump();
                return Client {
                    stream,
                    buffered: Vec::new(),
                };
            }
            assert!(Instant::now() < deadline, "connect timed out");
            self.pump();
        }
    }

    fn send(&mut self, client: &mut Client, kind: MessageType, payload: &[u8]) {
        let frame = encode_frame(kind, payload);
        let deadline = Instant::now() + DEADLINE;
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

    fn next_frame(&mut self, client: &mut Client) -> (u16, Vec<u8>) {
        let deadline = Instant::now() + DEADLINE;
        loop {
            if client.buffered.len() >= HEADER_LEN {
                let header =
                    FrameHeader::decode(&client.buffered[..HEADER_LEN]).expect("valid header");
                let total = HEADER_LEN + header.payload_len as usize;
                if client.buffered.len() >= total {
                    let frame: Vec<u8> = client.buffered.drain(..total).collect();
                    return (header.message_type, frame[HEADER_LEN..].to_vec());
                }
            }
            let mut chunk = [0u8; 16 * 1024];
            match client.stream.read(&mut chunk) {
                Ok(0) => panic!("connection closed while awaiting a frame"),
                Ok(count) => client.buffered.extend_from_slice(&chunk[..count]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => self.pump(),
                Err(error) => panic!("client read failed: {error}"),
            }
            assert!(Instant::now() < deadline, "timed out awaiting a frame");
        }
    }

    /// Skip unrelated frames until one of `kind` arrives.
    fn expect_kind(&mut self, client: &mut Client, kind: MessageType) -> Vec<u8> {
        let deadline = Instant::now() + DEADLINE;
        loop {
            let (got, payload) = self.next_frame(client);
            if got == kind as u16 {
                return payload;
            }
            assert!(Instant::now() < deadline, "timed out awaiting {kind:?}");
        }
    }

    fn hello(&mut self, client: &mut Client, client_capabilities: u32) -> (u16, Vec<u8>) {
        self.send(
            client,
            MessageType::ClientHello,
            &ClientHello {
                client_capabilities,
            }
            .encode(),
        );
        self.next_frame(client)
    }

    fn attach(&mut self, client: &mut Client, execution_id: ExecutionId, role: Role) -> Attached {
        self.send(
            client,
            MessageType::Attach,
            &Attach {
                execution_id,
                requested_role: role,
            }
            .encode(),
        );
        Attached::decode(&self.expect_kind(client, MessageType::Attached)).expect("Attached")
    }

    /// Submit through the controller until the shell is at a prompt.
    fn submit_when_ready(&mut self, client: &mut Client, attached: &Attached, command: &str) {
        let deadline = Instant::now() + DEADLINE;
        for request_id in 1.. {
            self.send(
                client,
                MessageType::ComposerCommand,
                &ComposerCommandRef {
                    attachment_id: attached.attachment_id,
                    request_id,
                    command,
                }
                .encode(),
            );
            let result =
                ComposerResult::decode(&self.expect_kind(client, MessageType::ComposerResult))
                    .expect("ComposerResult");
            if result.code == ComposerResultCode::Accepted {
                return;
            }
            assert!(Instant::now() < deadline, "composer never became available");
            for _ in 0..20 {
                self.pump();
            }
        }
    }

    /// The first replacement timeline under `schema` whose first record has
    /// completed.
    fn completed_timeline(
        &mut self,
        client: &mut Client,
        schema: BlockTimelineSchema,
    ) -> BlockTimeline {
        let deadline = Instant::now() + DEADLINE;
        loop {
            let payload = self.expect_kind(client, MessageType::BlockTimeline);
            let timeline = BlockTimeline::decode_schema(&payload, schema)
                .unwrap_or_else(|error| panic!("{schema:?} timeline must decode: {error:?}"));
            if timeline
                .records
                .first()
                .is_some_and(|record| matches!(record.state, CommandBlockState::Completed { .. }))
            {
                let other = match schema {
                    BlockTimelineSchema::Legacy => BlockTimelineSchema::Duration,
                    BlockTimelineSchema::Duration => BlockTimelineSchema::Legacy,
                };
                assert!(
                    BlockTimeline::decode_schema(&payload, other).is_err(),
                    "a {schema:?} client receives only its negotiated shape"
                );
                return timeline;
            }
            assert!(Instant::now() < deadline, "Block never completed");
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn duration_without_command_blocks_is_rejected_at_hello() {
    let mut harness = Harness::new();
    let mut client = harness.connect();
    let (kind, payload) = harness.hello(&mut client, CAP_COMMAND_BLOCK_DURATION);
    assert_eq!(kind, MessageType::Error as u16);
    let error = ErrorMessage::decode(&payload).expect("Error");
    assert_eq!(error.error_code, ErrorCode::MalformedPayload as u16);
    assert_eq!(
        error.offending_message_type,
        MessageType::ClientHello as u16
    );
}

#[test]
fn server_hello_advertises_duration_alongside_command_blocks() {
    let mut harness = Harness::new();
    let mut client = harness.connect();
    let (kind, payload) =
        harness.hello(&mut client, CAP_COMMAND_BLOCKS | CAP_COMMAND_BLOCK_DURATION);
    assert_eq!(kind, MessageType::ServerHello as u16);
    let hello = ServerHello::decode(&payload).expect("ServerHello");
    assert_ne!(hello.server_capabilities & CAP_COMMAND_BLOCKS, 0);
    assert_ne!(hello.server_capabilities & CAP_COMMAND_BLOCK_DURATION, 0);
}

#[test]
fn mixed_clients_each_receive_their_negotiated_timeline_shape() {
    let mut harness = Harness::new();
    let execution = harness.spawn_zsh();

    let mut legacy = harness.connect();
    let (kind, _) = harness.hello(&mut legacy, CAP_COMMAND_BLOCKS);
    assert_eq!(kind, MessageType::ServerHello as u16);
    let controller = harness.attach(&mut legacy, execution, Role::Controller);

    let mut duration = harness.connect();
    let (kind, _) = harness.hello(
        &mut duration,
        CAP_COMMAND_BLOCKS | CAP_COMMAND_BLOCK_DURATION,
    );
    assert_eq!(kind, MessageType::ServerHello as u16);
    harness.attach(&mut duration, execution, Role::Observer);

    harness.submit_when_ready(&mut legacy, &controller, "sleep 0.3");

    let legacy_view = harness.completed_timeline(&mut legacy, BlockTimelineSchema::Legacy);
    let duration_view = harness.completed_timeline(&mut duration, BlockTimelineSchema::Duration);

    assert_eq!(
        legacy_view.records[0].duration_ns, None,
        "legacy peers never get duration"
    );
    let measured = duration_view.records[0]
        .duration_ns
        .expect("the duration-capable peer receives the Runtime-measured value");
    assert!(
        (250_000_000..5_000_000_000).contains(&measured),
        "sleep 0.3 measured {measured} ns"
    );
    let mut same_records = duration_view.records.clone();
    same_records[0].duration_ns = None;
    assert_eq!(
        legacy_view.records, same_records,
        "both groups see the same Block apart from duration"
    );
}
