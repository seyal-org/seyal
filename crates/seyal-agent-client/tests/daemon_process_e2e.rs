//! SessionClient process E2E against the production `seyal-agent-backend` binary.
//!
//! This crate must not depend on `seyal-agent-backend`. The daemon is a separate
//! process located via `SEYAL_AGENT_BACKEND_BIN` or the workspace target dir.

#![cfg(unix)]

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use seyal_agent_client::{handshake, ClientError, SessionClient};
use seyal_agent_core::WorkScopeKind;
use seyal_agent_protocol::{
    AggregateRef, CommandResult, Hello, ProtocolVersion, ABSOLUTE_MAX_FRAME_SIZE,
};

struct ChildDaemon(Child);

impl Drop for ChildDaemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn backend_bin() -> PathBuf {
    if let Ok(path) = env::var("SEYAL_AGENT_BACKEND_BIN") {
        return PathBuf::from(path);
    }
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("../../target");
    path.push(profile);
    path.push("seyal-agent-backend");
    path
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = env::temp_dir().join(format!(
        "seyal-agent-client-e2e-{}-{}-{}",
        label,
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&dir).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&dir, permissions).unwrap();
    }
    dir
}

fn spawn_daemon(dir: &Path, output_bytes: usize, max_connections: Option<u64>) -> ChildDaemon {
    let bin = backend_bin();
    assert!(
        bin.is_file(),
        "seyal-agent-backend binary missing at {}; build with `cargo build -p seyal-agent-backend --bin seyal-agent-backend` or set SEYAL_AGENT_BACKEND_BIN",
        bin.display()
    );
    let mut command = Command::new(&bin);
    command
        .args([
            "--directory",
            dir.to_str().expect("utf-8 daemon directory"),
            "--output-bytes",
            &output_bytes.to_string(),
            "--deadline-secs",
            "60",
        ])
        .stdin(Stdio::null());
    if let Some(limit) = max_connections {
        command.args(["--max-connections", &limit.to_string()]);
    }
    ChildDaemon(command.spawn().expect("spawn seyal-agent-backend"))
}

fn probe_hello() -> Hello {
    Hello {
        supported_versions: vec![ProtocolVersion::V1],
        max_frame_size: ABSOLUTE_MAX_FRAME_SIZE,
        event_window: 64,
        client_principal_evidence: Vec::new(),
    }
}

fn wait_ready(socket: &Path) {
    let hello = probe_hello();
    let started = Instant::now();
    loop {
        // Hello-only probe; dropping the stream ends one serve_one turn so the
        // daemon loops back for the real SessionClient connection.
        if handshake(socket, &hello).is_ok() {
            return;
        }
        if started.elapsed() > Duration::from_secs(10) {
            panic!("daemon did not accept Hello at {}", socket.display());
        }
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn session_client_hello_work_run_snapshot_replay_against_daemon_binary() {
    let dir = temp_dir("path");
    let socket = dir.join("agent.sock");
    let mut child = spawn_daemon(&dir, 4096, None);
    wait_ready(&socket);

    let socket_for_client = socket.clone();
    let worker = thread::spawn(move || {
        let mut client = SessionClient::connect(&socket_for_client).unwrap();
        let scope = client.create_work_scope(WorkScopeKind::Repository).unwrap();
        let item = client.create_work_item(scope).unwrap();
        let attempt = client.create_attempt(item).unwrap();
        let started = client.start_agent_run(attempt).unwrap();
        assert!(started.event_count > 0);
        let snapshot = client
            .snapshot(AggregateRef::AgentRun(started.run_id))
            .unwrap()
            .expect("snapshot present");
        assert_eq!(snapshot.incorporated_through, started.event_count);
        let replay = client
            .replay(AggregateRef::AgentRun(started.run_id), None)
            .unwrap();
        match replay {
            CommandResult::Replay { events } => {
                assert_eq!(events.len() as u64, started.event_count);
            }
            other => panic!("unexpected replay: {other:?}"),
        }
        client.session_id()
    });
    let session_id = worker.join().unwrap();
    assert!(child.0.try_wait().unwrap().is_none());

    child.0.kill().unwrap();
    child.0.wait().unwrap();

    let restarted = spawn_daemon(&dir, 4096, None);
    wait_ready(&socket);
    match SessionClient::resume(&socket, session_id) {
        Err(ClientError::RejectedSession) => {}
        Ok(_) => panic!("old session was accepted after daemon restart"),
        Err(other) => panic!("unexpected resume error: {other:?}"),
    }
    drop(restarted);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn daemon_binary_survives_malformed_client_and_serves_next() {
    use seyal_agent_protocol::encode_hello;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;

    let dir = temp_dir("malformed");
    let socket = dir.join("agent.sock");
    let mut child = spawn_daemon(&dir, 1024, None);
    wait_ready(&socket);

    // Hello succeeds, then a bad-magic frame: daemon must keep accepting.
    let mut stream = UnixStream::connect(&socket).expect("connect bad client");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let hello = probe_hello();
    let hello_frame = encode_hello(&hello, ABSOLUTE_MAX_FRAME_SIZE).unwrap();
    stream.write_all(&hello_frame).unwrap();
    let mut ack_buf = [0_u8; 512];
    let _ = stream.read(&mut ack_buf);
    let mut junk = [0_u8; 10];
    junk[..4].copy_from_slice(b"BAD!");
    let _ = stream.write_all(&junk);
    drop(stream);

    let started = Instant::now();
    while child.0.try_wait().unwrap().is_none() && started.elapsed() < Duration::from_millis(300) {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        child.0.try_wait().unwrap().is_none(),
        "daemon exited after malformed client"
    );

    let socket_for_client = socket.clone();
    let ok = thread::spawn(move || {
        let mut client = SessionClient::connect(&socket_for_client).unwrap();
        client.create_work_scope(WorkScopeKind::Repository).unwrap()
    });
    ok.join().expect("SessionClient after malformed peer");
    assert!(child.0.try_wait().unwrap().is_none());
    drop(child);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn daemon_binary_exits_cleanly_after_bounded_connections() {
    let dir = temp_dir("shutdown");
    let socket = dir.join("agent.sock");
    // One successful serve_one (the SessionClient connection) then exit 0.
    // Do not Hello-probe first; that would consume the single allowed turn.
    let mut child = spawn_daemon(&dir, 1024, Some(1));
    let started = Instant::now();
    let mut client = loop {
        match SessionClient::connect(&socket) {
            Ok(client) => break client,
            Err(_) if started.elapsed() > Duration::from_secs(10) => {
                panic!("daemon never accepted SessionClient");
            }
            Err(_) => thread::sleep(Duration::from_millis(5)),
        }
    };
    let _ = client.create_work_scope(WorkScopeKind::AdHoc).unwrap();
    drop(client);
    let status = child.0.wait().expect("daemon wait");
    assert!(
        status.success(),
        "controlled shutdown must exit 0, got {status}"
    );
    let _ = fs::remove_dir_all(dir);
}
