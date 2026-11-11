#![cfg(all(target_os = "macos", feature = "test-fault-injection"))]

use std::{
    fs,
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::{Duration, Instant},
};

use seyal_exec::{CommandSpec, WindowSize};
use seyal_protocol::pass8::{BlockState, BLOCK_STATE_MESSAGE_TYPE, CAP_BLOCK_METADATA};
use seyal_runtime::{
    local_ipc::framing::{
        encode_frame, Attach, ClientHello, FrameHeader, Lifecycle, LifecycleMessage, MessageType,
        Role, HEADER_LEN,
    },
    test_fault::{self, FaultPoint},
    ExecutionId, LocalIpcMode, Runtime, RuntimeConfig,
};

fn config(tag: &str) -> RuntimeConfig {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let suffix = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut config = RuntimeConfig::m001().expect("bundled capability profile");
    config.singleton_path = std::env::temp_dir().join(format!("s8f-{tag}-{suffix:x}.lock"));
    config.local_ipc = LocalIpcMode::Enabled {
        runtime_dir_override: Some(std::env::temp_dir().join(format!("s8fd-{tag}-{suffix:x}"))),
    };
    config.final_drain = Duration::from_millis(30);
    config
}

struct Client {
    stream: UnixStream,
    buffered: Vec<u8>,
}

struct Harness {
    runtime: Runtime,
}

struct ChildExitGate {
    path: PathBuf,
}

impl ChildExitGate {
    fn release(&self) {
        fs::write(&self.path, b"release").expect("release child exit gate");
    }
}

impl Drop for ChildExitGate {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

impl Harness {
    fn new(tag: &str) -> Self {
        Self {
            runtime: Runtime::new(config(tag)).expect("Runtime"),
        }
    }

    fn pump(&mut self) {
        self.runtime
            .poll_once(Some(Duration::from_millis(5)))
            .expect("Runtime poll");
    }

    fn spawn(&mut self) -> ExecutionId {
        self.runtime
            .create_execution(
                CommandSpec::new("/bin/sh").args(["-c", "sleep 0.25; printf TAIL"]),
                WindowSize::new(40, 4, 0, 0).expect("valid geometry"),
            )
            .expect("execution")
    }

    fn spawn_gated(&mut self, tag: &str) -> (ExecutionId, ChildExitGate) {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let suffix = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("s8f-gate-{}-{tag}-{suffix:x}", std::process::id()));
        let _ = fs::remove_file(&path);
        let script = format!(
            "while [ ! -e '{}' ]; do sleep 0.01; done; printf TAIL",
            path.display()
        );
        let execution_id = self
            .runtime
            .create_execution(
                CommandSpec::new("/bin/sh").args(["-c", &script]),
                WindowSize::new(40, 4, 0, 0).expect("valid geometry"),
            )
            .expect("gated execution");
        (execution_id, ChildExitGate { path })
    }

    fn connect(&mut self) -> Client {
        let path = self
            .runtime
            .local_ipc_socket_path()
            .expect("local IPC path")
            .to_path_buf();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match UnixStream::connect(&path) {
                Ok(stream) => {
                    stream.set_nonblocking(true).unwrap();
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
        client
            .stream
            .write_all(&encode_frame(kind, payload))
            .expect("send frame");
        self.pump();
    }

    fn next_frame_or_close(
        &mut self,
        client: &mut Client,
        deadline: Instant,
    ) -> Option<(u16, Vec<u8>)> {
        loop {
            if client.buffered.len() >= HEADER_LEN {
                let header = FrameHeader::decode(&client.buffered[..HEADER_LEN]).unwrap();
                let total = HEADER_LEN + header.payload_len as usize;
                if client.buffered.len() >= total {
                    let raw = client.buffered.drain(..total).collect::<Vec<_>>();
                    return Some((header.message_type, raw[HEADER_LEN..].to_vec()));
                }
            }

            let mut chunk = [0u8; 16 * 1024];
            match client.stream.read(&mut chunk) {
                Ok(0) => return None,
                Ok(count) => client.buffered.extend_from_slice(&chunk[..count]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => self.pump(),
                Err(error) => panic!("read failed: {error}"),
            }
            assert!(Instant::now() < deadline, "read timed out");
        }
    }

    fn attach_until_current(&mut self, client: &mut Client, execution_id: ExecutionId) {
        self.send(
            client,
            MessageType::ClientHello,
            &ClientHello {
                client_capabilities: CAP_BLOCK_METADATA,
            }
            .encode(),
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        let Some((kind, _)) = self.next_frame_or_close(client, deadline) else {
            panic!("closed before ServerHello");
        };
        assert_eq!(kind, MessageType::ServerHello as u16);

        self.send(
            client,
            MessageType::Attach,
            &Attach {
                execution_id,
                requested_role: Role::Observer,
            }
            .encode(),
        );

        loop {
            let Some((kind, payload)) = self.next_frame_or_close(client, deadline) else {
                panic!("closed before Current BlockState");
            };
            if kind == BLOCK_STATE_MESSAGE_TYPE {
                let current = BlockState::decode(&payload).expect("Current BlockState");
                assert_eq!(current.execution_id, execution_id);
                assert_eq!(current.revision, 1);
                return;
            }
        }
    }

    fn assert_fails_closed_before_finalized(&mut self, client: &mut Client) {
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            match self.next_frame_or_close(client, deadline) {
                Some((kind, payload)) if kind == MessageType::Lifecycle as u16 => {
                    let lifecycle = LifecycleMessage::decode(&payload).unwrap();
                    assert_ne!(
                        lifecycle.lifecycle,
                        Lifecycle::Finalized,
                        "Block-capable connection observed Finalized after completion metadata failure"
                    );
                }
                Some((kind, payload)) if kind == BLOCK_STATE_MESSAGE_TYPE => {
                    let block = BlockState::decode(&payload).expect("BlockState");
                    assert_ne!(
                        block.state,
                        seyal_protocol::pass8::BlockLifecycle::Completed,
                        "completion failure unexpectedly emitted completed BlockState"
                    );
                }
                Some(_) => {}
                None => break,
            }
        }

        while self.runtime.execution_count() != 0 {
            assert!(Instant::now() < deadline, "execution retirement timed out");
            self.pump();
        }
        assert_eq!(self.runtime.block_count(), 0);
    }
}

#[test]
fn block_admission_failure_leaves_execution_live_and_raw_terminal_capable() {
    let mut harness = Harness::new("admit");
    test_fault::fail_next(FaultPoint::BlockAdmission);
    let execution_id = harness.spawn();

    assert!(harness.runtime.lookup(execution_id).is_some());
    assert_eq!(harness.runtime.block(execution_id), None);
    assert_eq!(harness.runtime.block_count(), 0);

    harness.runtime.request_termination(execution_id).unwrap();
    harness
        .runtime
        .run_until_empty(Instant::now() + Duration::from_secs(3))
        .unwrap();
    assert_eq!(harness.runtime.execution_count(), 0);
    assert_eq!(harness.runtime.block_count(), 0);
}

#[test]
fn completion_mutation_failure_disconnects_before_finalized_and_retires_block() {
    let mut harness = Harness::new("mutation");
    let (execution_id, gate) = harness.spawn_gated("mutation");
    let mut client = harness.connect();
    harness.attach_until_current(&mut client, execution_id);

    test_fault::fail_next(FaultPoint::BlockCompletionMutation);
    gate.release();
    harness.assert_fails_closed_before_finalized(&mut client);
}

#[test]
fn completion_encode_failure_disconnects_before_finalized_and_retires_block() {
    let mut harness = Harness::new("encode");
    let (execution_id, gate) = harness.spawn_gated("encode");
    let mut client = harness.connect();
    harness.attach_until_current(&mut client, execution_id);

    test_fault::fail_next(FaultPoint::BlockCompletionEncode);
    gate.release();
    harness.assert_fails_closed_before_finalized(&mut client);
}

#[test]
fn completion_encode_failure_is_global_across_all_block_capable_clients() {
    let mut harness = Harness::new("encode-multi");
    let (execution_id, gate) = harness.spawn_gated("encode-multi");
    let mut first = harness.connect();
    let mut second = harness.connect();
    harness.attach_until_current(&mut first, execution_id);
    harness.attach_until_current(&mut second, execution_id);

    test_fault::fail_next(FaultPoint::BlockCompletionEncode);
    gate.release();
    harness.assert_fails_closed_before_finalized(&mut first);
    harness.assert_fails_closed_before_finalized(&mut second);
}

#[test]
fn completion_admission_failure_disconnects_before_finalized_and_retires_block() {
    let mut harness = Harness::new("admission");
    let (execution_id, gate) = harness.spawn_gated("admission");
    let mut client = harness.connect();
    harness.attach_until_current(&mut client, execution_id);

    test_fault::fail_next(FaultPoint::BlockCompletionAdmission);
    gate.release();
    harness.assert_fails_closed_before_finalized(&mut client);
}
