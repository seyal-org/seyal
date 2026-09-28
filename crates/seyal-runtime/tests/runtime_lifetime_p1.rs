#![cfg(target_os = "macos")]

//! SPEC-003 §4.1 / ADR-017 P1: Runtime process lifetime vs live-execution count.

use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use seyal_exec::{CommandSpec, WindowSize};
use seyal_runtime::{
    explicit_startup_command,
    local_ipc::framing::{
        encode_frame, ClientHello, ExecutionList, FrameHeader, MessageType, ServerHello, HEADER_LEN,
    },
    Runtime, RuntimeConfig,
};

fn unique_dir(tag: &str) -> PathBuf {
    PathBuf::from(format!(
        "/tmp/s1093{tag}{}{:x}",
        std::process::id() % 100_000,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            % 0xFFFF
    ))
}

fn isolated_config(tag: &str) -> RuntimeConfig {
    let mut config = RuntimeConfig::m001()
        .expect("M001 Runtime config")
        .isolated_to(unique_dir(tag));
    config.graceful_termination = Duration::from_millis(50);
    config.forced_reap = Duration::from_millis(250);
    config.final_drain = Duration::from_millis(100);
    config
}

fn size() -> WindowSize {
    WindowSize::new(80, 24, 0, 0).expect("valid size")
}

fn wait_until(runtime: &mut Runtime, deadline: Instant, predicate: impl Fn(&Runtime) -> bool) {
    while !predicate(runtime) {
        assert!(Instant::now() < deadline, "condition timed out");
        runtime
            .poll_once(Some(Duration::from_millis(20)))
            .expect("Runtime poll");
    }
}

fn shutdown(runtime: &mut Runtime) {
    runtime.begin_shutdown().expect("begin controlled shutdown");
    runtime
        .run_until_empty(Instant::now() + Duration::from_secs(2))
        .expect("controlled shutdown completes");
    assert!(runtime.shutdown_complete());
}

struct ProtocolClient {
    stream: UnixStream,
    buffered: Vec<u8>,
}

impl ProtocolClient {
    fn connect(socket: &PathBuf, runtime: &mut Runtime) -> Self {
        let deadline = Instant::now() + Duration::from_secs(2);
        let stream = loop {
            match UnixStream::connect(socket) {
                Ok(stream) => break stream,
                Err(_) => {
                    assert!(Instant::now() < deadline, "connect timed out");
                    runtime
                        .poll_once(Some(Duration::from_millis(5)))
                        .expect("poll while connecting");
                }
            }
        };
        stream.set_nonblocking(true).expect("nonblocking");
        runtime
            .poll_once(Some(Duration::from_millis(5)))
            .expect("accept poll");
        Self {
            stream,
            buffered: Vec::new(),
        }
    }

    fn send(&mut self, runtime: &mut Runtime, kind: MessageType, payload: &[u8]) {
        let frame = encode_frame(kind, payload);
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut sent = 0;
        while sent < frame.len() {
            match self.stream.write(&frame[sent..]) {
                Ok(0) => panic!("client write returned zero"),
                Ok(count) => sent += count,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    runtime
                        .poll_once(Some(Duration::from_millis(5)))
                        .expect("poll write");
                }
                Err(error) => panic!("client write failed: {error}"),
            }
            assert!(Instant::now() < deadline, "write timed out");
        }
        runtime
            .poll_once(Some(Duration::from_millis(5)))
            .expect("poll after write");
    }

    fn expect_frame(&mut self, runtime: &mut Runtime, deadline: Instant) -> (u16, Vec<u8>) {
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
            let mut chunk = [0u8; 4096];
            match self.stream.read(&mut chunk) {
                Ok(0) => panic!("connection closed while awaiting frame"),
                Ok(count) => self.buffered.extend_from_slice(&chunk[..count]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    runtime
                        .poll_once(Some(Duration::from_millis(5)))
                        .expect("poll read");
                }
                Err(error) => panic!("client read failed: {error}"),
            }
            assert!(Instant::now() < deadline, "timed out awaiting frame");
        }
    }

    fn hello(&mut self, runtime: &mut Runtime) {
        self.send(
            runtime,
            MessageType::ClientHello,
            &ClientHello {
                client_capabilities: 0,
            }
            .encode(),
        );
        let (kind, payload) = self.expect_frame(runtime, Instant::now() + Duration::from_secs(2));
        assert_eq!(kind, MessageType::ServerHello as u16);
        let _ = ServerHello::decode(&payload).expect("ServerHello");
    }

    fn list_executions(&mut self, runtime: &mut Runtime) -> ExecutionList {
        self.send(runtime, MessageType::ListExecutions, &[]);
        let (kind, payload) = self.expect_frame(runtime, Instant::now() + Duration::from_secs(2));
        assert_eq!(kind, MessageType::ExecutionList as u16);
        ExecutionList::decode(&payload).expect("ExecutionList")
    }
}

struct HelperChild {
    child: Child,
    dir: PathBuf,
}

impl HelperChild {
    fn spawn(args: &[&str]) -> Self {
        // Do not pre-create `dir`: Runtime owns verified directory creation
        // (mode/ownership). A plain create_dir_all fails closed at bind time.
        let dir = unique_dir("bin");
        let mut command = Command::new(env!("CARGO_BIN_EXE_seyal-runtime"));
        command
            .args(["--runtime-dir", dir.to_str().expect("utf8")])
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let child = command.spawn().expect("spawn seyal-runtime");
        Self { child, dir }
    }

    fn socket(&self) -> PathBuf {
        self.dir.join("control.sock")
    }

    fn wait_for_socket(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !self.socket().exists() {
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                let mut stderr = String::new();
                if let Some(mut pipe) = self.child.stderr.take() {
                    let _ = pipe.read_to_string(&mut stderr);
                }
                panic!("helper exited before binding control socket: {status}; stderr={stderr}");
            }
            assert!(
                Instant::now() < deadline,
                "helper did not bind control socket"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for HelperChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn empty_argv_composition_creates_no_startup_command() {
    assert!(explicit_startup_command(Vec::new()).is_none());
    let command = explicit_startup_command(vec!["/bin/true".into()]).expect("command");
    assert_eq!(command.program(), "/bin/true");
}

#[test]
fn headless_empty_start_has_zero_executions_and_stable_identity() {
    let mut runtime = Runtime::new(isolated_config("empty")).expect("Runtime");
    let runtime_id = runtime.id();
    assert_eq!(runtime.execution_count(), 0);
    assert!(runtime.list().is_empty());

    let mut client = ProtocolClient::connect(
        &runtime
            .local_ipc_socket_path()
            .expect("socket")
            .to_path_buf(),
        &mut runtime,
    );
    client.hello(&mut runtime);
    let list = client.list_executions(&mut runtime);
    assert!(list.entries.is_empty(), "ListExecutions must be empty");

    // Idle wait must block on the reactor, not busy-poll.
    let started = Instant::now();
    runtime
        .poll_once(Some(Duration::from_millis(50)))
        .expect("idle poll");
    assert!(
        started.elapsed() >= Duration::from_millis(35),
        "zero-execution idle must wait, not spin"
    );
    assert_eq!(runtime.id(), runtime_id);
    assert_eq!(runtime.execution_count(), 0);

    shutdown(&mut runtime);
}

#[test]
fn last_execution_finalization_does_not_end_runtime_or_leak() {
    let mut runtime = Runtime::new(isolated_config("last-exit")).expect("Runtime");
    let runtime_id = runtime.id();

    let id = runtime
        .create_execution(CommandSpec::new("/bin/sh").args(["-c", "exit 0"]), size())
        .expect("one execution");
    assert_eq!(runtime.execution_count(), 1);

    wait_until(
        &mut runtime,
        Instant::now() + Duration::from_secs(3),
        |runtime| runtime.execution_count() == 0,
    );

    // No registration, Workspace association, or block metadata remains.
    assert!(runtime.list().is_empty());
    assert!(runtime.lookup(id).is_none());
    assert_eq!(runtime.block_count(), 0);
    assert_eq!(runtime.aggregate_accepted_but_unwritten_bytes(), 0);
    assert_eq!(runtime.id(), runtime_id);
    assert!(!runtime.shutdown_complete());

    // Still serves new composition/attachment after the last execution is gone.
    let next = runtime
        .create_execution(CommandSpec::new("/bin/sh").args(["-c", "sleep 30"]), size())
        .expect("create after empty");
    let attachment = runtime.attach(next).expect("attach after empty");
    assert_eq!(runtime.execution_count(), 1);
    runtime.detach(next, attachment).expect("detach");

    shutdown(&mut runtime);
}

#[test]
fn explicit_command_composition_creates_exactly_one_execution() {
    let mut runtime = Runtime::new(isolated_config("explicit")).expect("Runtime");
    let command = explicit_startup_command(vec!["/bin/sh".into(), "-c".into(), "sleep 30".into()])
        .expect("explicit startup command");
    let id = runtime.create_execution(command, size()).expect("create");
    assert_eq!(runtime.execution_count(), 1);
    assert_eq!(runtime.list().len(), 1);
    assert_eq!(runtime.list()[0].id, id);
    shutdown(&mut runtime);
}

#[test]
fn controlled_shutdown_still_progresses_live_executions() {
    let mut runtime = Runtime::new(isolated_config("shutdown")).expect("Runtime");
    runtime
        .create_execution(CommandSpec::new("/bin/sh").args(["-c", "sleep 30"]), size())
        .expect("live execution");
    assert_eq!(runtime.execution_count(), 1);
    shutdown(&mut runtime);
    assert_eq!(runtime.execution_count(), 0);
    assert!(runtime.list().is_empty());
}

#[test]
fn helper_empty_argv_stays_alive_with_empty_list_executions() {
    let mut helper = HelperChild::spawn(&[]);
    helper.wait_for_socket();

    // Process must remain resident at zero executions.
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        helper.child.try_wait().expect("try_wait").is_none(),
        "empty-argv helper must not exit when live-execution count is zero"
    );

    // Drive ListExecutions against the live helper via a peer Runtime is not
    // possible (singleton). Use a direct client socket instead.
    let mut stream = UnixStream::connect(helper.socket()).expect("connect helper");
    stream.set_nonblocking(false).expect("blocking client");
    stream
        .write_all(&encode_frame(
            MessageType::ClientHello,
            &ClientHello {
                client_capabilities: 0,
            }
            .encode(),
        ))
        .expect("ClientHello");
    let mut buffered = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(2);
    let server_hello = read_frame(&mut stream, &mut buffered, deadline);
    assert_eq!(server_hello.0, MessageType::ServerHello as u16);

    stream
        .write_all(&encode_frame(MessageType::ListExecutions, &[]))
        .expect("ListExecutions");
    let (kind, payload) = read_frame(&mut stream, &mut buffered, deadline);
    assert_eq!(kind, MessageType::ExecutionList as u16);
    let list = ExecutionList::decode(&payload).expect("ExecutionList");
    assert!(
        list.entries.is_empty(),
        "empty-argv helper must report zero executions"
    );

    assert!(
        helper.child.try_wait().expect("try_wait").is_none(),
        "helper must stay alive after empty ListExecutions"
    );
}

#[test]
fn helper_explicit_command_creates_one_execution_and_stays_after_exit() {
    let mut helper = HelperChild::spawn(&["/bin/sh", "-c", "sleep 30"]);
    helper.wait_for_socket();

    let mut stream = UnixStream::connect(helper.socket()).expect("connect helper");
    stream.set_nonblocking(false).expect("blocking client");
    stream
        .write_all(&encode_frame(
            MessageType::ClientHello,
            &ClientHello {
                client_capabilities: 0,
            }
            .encode(),
        ))
        .expect("ClientHello");
    let mut buffered = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(2);
    let _ = read_frame(&mut stream, &mut buffered, deadline);

    // Wait briefly for startup create to publish, then list.
    std::thread::sleep(Duration::from_millis(100));
    stream
        .write_all(&encode_frame(MessageType::ListExecutions, &[]))
        .expect("ListExecutions");
    let (kind, payload) = read_frame(
        &mut stream,
        &mut buffered,
        Instant::now() + Duration::from_secs(2),
    );
    assert_eq!(kind, MessageType::ExecutionList as u16);
    let list = ExecutionList::decode(&payload).expect("ExecutionList");
    assert_eq!(
        list.entries.len(),
        1,
        "explicit command must create exactly one execution"
    );

    // Replace with a short-lived command helper to prove finalization ≠ exit.
    drop(helper);
    let mut helper = HelperChild::spawn(&["/bin/sh", "-c", "exit 0"]);
    helper.wait_for_socket();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        assert!(
            helper.child.try_wait().expect("try_wait").is_none(),
            "helper must not exit when its last startup execution finalizes"
        );
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn read_frame(
    stream: &mut UnixStream,
    buffered: &mut Vec<u8>,
    deadline: Instant,
) -> (u16, Vec<u8>) {
    loop {
        if buffered.len() >= HEADER_LEN {
            let header = FrameHeader::decode(&buffered[..HEADER_LEN]).expect("header");
            let total = HEADER_LEN + header.payload_len as usize;
            if buffered.len() >= total {
                let frame = buffered.drain(..total).collect::<Vec<_>>();
                return (header.message_type, frame[HEADER_LEN..].to_vec());
            }
        }
        let mut chunk = [0u8; 4096];
        match stream.read(&mut chunk) {
            Ok(0) => panic!("connection closed while awaiting frame"),
            Ok(count) => buffered.extend_from_slice(&chunk[..count]),
            Err(error) => panic!("read failed: {error}"),
        }
        assert!(Instant::now() < deadline, "timed out awaiting frame");
    }
}
