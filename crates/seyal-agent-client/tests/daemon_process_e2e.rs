//! SessionClient process E2E against the production `seyal-agent-backend` binary.
//!
//! This crate must not depend on `seyal-agent-backend`. The daemon is a separate
//! process located via `SEYAL_AGENT_BACKEND_BIN` or the workspace target dir.

#![cfg(unix)]

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
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
    let name = format!(
        "seyal-agent-client-e2e-{}-{}-{}",
        label,
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    );
    let mut dir = env::temp_dir().join(&name);
    // macOS `sun_path` is 104 bytes including the trailing NUL. `agent.sock`
    // needs ten more bytes, so a long `TMPDIR` cannot host the socket.
    if dir.join("agent.sock").as_os_str().len() >= 104 {
        dir = PathBuf::from("/tmp").join(name);
    }
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
    spawn_daemon_opts(dir, output_bytes, Some(60), max_connections, false)
}

fn spawn_daemon_opts(
    dir: &Path,
    output_bytes: usize,
    deadline_secs: Option<u64>,
    max_connections: Option<u64>,
    capture_stderr: bool,
) -> ChildDaemon {
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
        ])
        .stdin(Stdio::null());
    if let Some(deadline) = deadline_secs {
        command.args(["--deadline-secs", &deadline.to_string()]);
    }
    if let Some(limit) = max_connections {
        command.args(["--max-connections", &limit.to_string()]);
    }
    if capture_stderr {
        command.stderr(Stdio::piped()).stdout(Stdio::null());
    }
    ChildDaemon(command.spawn().expect("spawn seyal-agent-backend"))
}

fn connect_ready(socket: &Path) -> SessionClient {
    let started = Instant::now();
    loop {
        match SessionClient::connect(socket) {
            Ok(client) => return client,
            Err(_) if started.elapsed() > Duration::from_secs(10) => {
                panic!(
                    "barrier daemon-ready: SessionClient never connected at {}",
                    socket.display()
                );
            }
            Err(_) => thread::park_timeout(Duration::from_millis(5)),
        }
    }
}

fn recv_barrier<T>(rx: &mpsc::Receiver<T>, timeout: Duration, barrier: &str) -> T {
    match rx.recv_timeout(timeout) {
        Ok(value) => value,
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("barrier {barrier} timed out"),
        Err(mpsc::RecvTimeoutError::Disconnected) => panic!("barrier {barrier} disconnected"),
    }
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
        // Hello-only probe on its own worker. Dropping the stream ends that
        // worker; the accept loop keeps admitting the SessionClient connection.
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
    // One admitted connection (the SessionClient) then exit 0.
    // Do not Hello-probe first; that would consume the single admission.
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

#[test]
fn bounded_daemon_exits_zero_after_two_concurrent_sessions() {
    let dir = temp_dir("two-bounded");
    let socket = dir.join("agent.sock");
    let mut child = spawn_daemon(&dir, 1024, Some(2));
    let (live_tx, live_rx) = mpsc::channel();
    let (go_a_tx, go_a_rx) = mpsc::channel();
    let (go_b_tx, go_b_rx) = mpsc::channel();

    let socket_a = socket.clone();
    let live_a = live_tx.clone();
    let peer_a = thread::spawn(move || {
        let mut client = connect_ready(&socket_a);
        live_a.send("a").unwrap();
        recv_barrier(&go_a_rx, Duration::from_secs(5), "two-sessions-release-a");
        client.create_work_scope(WorkScopeKind::AdHoc).unwrap();
    });
    let peer_b = thread::spawn(move || {
        let mut client = connect_ready(&socket);
        live_tx.send("b").unwrap();
        recv_barrier(&go_b_rx, Duration::from_secs(5), "two-sessions-release-b");
        client.create_work_scope(WorkScopeKind::Repository).unwrap();
    });
    let mut live = vec![
        recv_barrier(&live_rx, Duration::from_secs(10), "two-sessions-live"),
        recv_barrier(&live_rx, Duration::from_secs(10), "two-sessions-live"),
    ];
    live.sort_unstable();
    assert_eq!(live, vec!["a", "b"], "barrier two-sessions-live");
    go_a_tx.send(()).unwrap();
    go_b_tx.send(()).unwrap();
    peer_a.join().unwrap();
    peer_b.join().unwrap();
    let ended = Instant::now();
    let status = child.0.wait().expect("daemon wait");
    assert!(status.success(), "barrier bounded-exit: {status}");
    assert!(
        ended.elapsed() <= Duration::from_secs(2),
        "barrier bounded-exit took {:?}",
        ended.elapsed()
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn bounded_daemon_with_held_session_is_ended_by_deadline() {
    use std::io::Read;

    let dir = temp_dir("held-deadline");
    let socket = dir.join("agent.sock");
    let mut child = spawn_daemon_opts(&dir, 1024, Some(2), Some(1), true);
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let holder = thread::spawn(move || {
        let client = connect_ready(&socket);
        held_tx.send(()).unwrap();
        let _ = release_rx.recv_timeout(Duration::from_secs(10));
        drop(client);
    });
    recv_barrier(&held_rx, Duration::from_secs(10), "held-session-open");
    assert!(
        child.0.try_wait().unwrap().is_none(),
        "barrier held-session: daemon exited before the deadline"
    );
    let status = child.0.wait().expect("daemon wait");
    assert_eq!(
        status.code(),
        Some(2),
        "barrier child_deadline_exceeded: {status}"
    );
    let mut stderr = String::new();
    child
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(
        stderr.contains("child_deadline_exceeded"),
        "barrier child_deadline_exceeded: stderr={stderr}"
    );
    release_tx.send(()).unwrap();
    holder.join().unwrap();
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn bounded_daemon_requires_deadline() {
    use std::io::Read;

    let dir = temp_dir("needs-deadline");
    let socket = dir.join("agent.sock");
    let mut child = spawn_daemon_opts(&dir, 1024, None, Some(1), true);
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        if started.elapsed() >= Duration::from_secs(2) {
            let socket_exists = socket.exists();
            panic!(
                "barrier deadline-required: daemon still running after 2s (socket_exists={socket_exists})"
            );
        }
        thread::park_timeout(Duration::from_millis(20));
    };
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "barrier deadline-required: exit took {:?}",
        started.elapsed()
    );
    assert_eq!(
        status.code(),
        Some(2),
        "barrier deadline-required: {status}"
    );
    let mut stderr = String::new();
    child
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(
        stderr.contains("--max-connections requires --deadline-secs"),
        "barrier deadline-required: stderr={stderr}"
    );
    assert!(
        !socket.exists(),
        "barrier deadline-required: socket was created"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn bounded_daemon_counts_recoverable_fault_toward_budget() {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;

    let dir = temp_dir("recoverable-budget");
    let socket = dir.join("agent.sock");
    let mut child = spawn_daemon_opts(&dir, 1024, Some(60), Some(1), true);
    let started = Instant::now();
    let mut stream = loop {
        match UnixStream::connect(&socket) {
            Ok(stream) => break stream,
            Err(_) if started.elapsed() > Duration::from_secs(10) => {
                panic!("barrier recoverable-budget: daemon never accepted");
            }
            Err(_) => thread::park_timeout(Duration::from_millis(5)),
        }
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    // Hold the socket until the worker has the bytes. Closing before accept
    // can drop the connection from the listen queue, so the budget is never spent.
    stream.write_all(b"BAD!\x00\x00\x00\x00\x00\x00").unwrap();
    stream.flush().unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    let ended = Instant::now();
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        if ended.elapsed() >= Duration::from_secs(2) {
            panic!("barrier recoverable-budget: daemon still running after 2s");
        }
        thread::park_timeout(Duration::from_millis(20));
    };
    drop(stream);
    assert!(status.success(), "barrier recoverable-budget: {status}");
    assert!(
        ended.elapsed() <= Duration::from_secs(2),
        "barrier recoverable-budget took {:?}",
        ended.elapsed()
    );
    let mut stderr = String::new();
    child
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(
        stderr.contains("client serve ended: Malformed"),
        "barrier recoverable-budget: stderr={stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}
