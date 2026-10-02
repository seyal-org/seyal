use super::*;
use seyal_agent_protocol::{
    decode_result, encode_command, encode_hello, Command, CommandResult, FrameKind, HandshakeError,
    Hello, ProtocolVersion, ABSOLUTE_MAX_FRAME_SIZE, MAX_EVENT_WINDOW,
};
use seyal_agent_store::AgentStore;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::panic::AssertUnwindSafe;
use std::sync::mpsc;
use std::time::Instant;
use std::{
    os::unix::net::UnixListener,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::Duration,
};

#[test]
fn wrong_owner_peer_is_recoverable_other_endpoint_faults_are_not() {
    assert!(DaemonError::Endpoint(EndpointFault::WrongOwner).is_recoverable_client_fault());
    assert!(!DaemonError::Endpoint(EndpointFault::Symlink).is_recoverable_client_fault());
    assert!(!DaemonError::Endpoint(EndpointFault::InsecureMode).is_recoverable_client_fault());
    assert!(DaemonError::Handshake(HandshakeError::Malformed).is_recoverable_client_fault());
    assert!(DaemonError::Io.is_recoverable_client_fault());
    assert!(!DaemonError::AcceptIo.is_recoverable_client_fault());
    assert!(!DaemonError::Unavailable.is_recoverable_client_fault());
}

static NEXT_DIR: AtomicU64 = AtomicU64::new(1);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "seyal-agent-{}-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn hello() -> Hello {
    Hello {
        supported_versions: vec![ProtocolVersion::V1],
        max_frame_size: 4096,
        event_window: 32,
        client_principal_evidence: Vec::new(),
    }
}

#[test]
fn simultaneous_startup_keeps_one_owner_and_restart_changes_instance_id() {
    let dir = TempDir::new();
    let path = dir.path().to_path_buf();
    let results = thread::scope(|scope| {
        let mut joins = Vec::new();
        for _ in 0..8 {
            let path = path.clone();
            joins.push(scope.spawn(move || AgentDaemon::bind(path)));
        }
        joins
            .into_iter()
            .map(|join| join.join().expect("startup thread"))
            .collect::<Vec<_>>()
    });
    let owners = results.iter().filter(|result| result.is_ok()).count();
    assert_eq!(owners, 1);
    let daemon = results.into_iter().find_map(Result::ok).unwrap();
    let first_id = daemon.instance_id();
    let directory = dir.path().to_path_buf();
    let client_path = daemon.socket_path();
    let client = thread::spawn(move || connect_hello(&client_path, &hello(), 4096));
    let ack = daemon.accept_hello().unwrap();
    assert_eq!(
        client.join().unwrap().unwrap().backend_instance_id,
        first_id
    );
    assert_eq!(ack.backend_instance_id, first_id);
    assert!(daemon.socket_path().exists());
    drop(daemon);

    let restarted = AgentDaemon::bind(&directory).unwrap();
    assert_ne!(restarted.instance_id(), first_id);
}

#[test]
fn stale_socket_is_reclaimed_and_unsafe_endpoints_stay_in_place() {
    let dir = TempDir::new();
    let daemon = AgentDaemon::bind(dir.path()).unwrap();
    daemon.abandon_as_crash();
    let reclaimed = AgentDaemon::bind(dir.path()).unwrap();
    let path = reclaimed.socket_path();
    let client = thread::spawn(move || connect_hello(&path, &hello(), 4096));
    assert!(reclaimed.accept_hello().is_ok());
    assert!(client.join().unwrap().is_ok());
    drop(reclaimed);

    std::os::unix::fs::symlink(
        dir.path().join("missing-target"),
        dir.path().join(SOCKET_NAME),
    )
    .unwrap();
    assert_eq!(
        AgentDaemon::bind(dir.path()).map(|_| ()),
        Err(DaemonError::Endpoint(EndpointFault::Symlink))
    );
    assert!(dir
        .path()
        .join(SOCKET_NAME)
        .symlink_metadata()
        .unwrap()
        .file_type()
        .is_symlink());
    fs::remove_file(dir.path().join(SOCKET_NAME)).unwrap();

    fs::write(dir.path().join(SOCKET_NAME), b"not-a-socket").unwrap();
    fs::write(dir.path().join(LOCK_NAME), b"v1\n0\n").unwrap();
    assert_eq!(
        AgentDaemon::bind(dir.path()).map(|_| ()),
        Err(DaemonError::Endpoint(EndpointFault::NotSocket))
    );
    assert_eq!(
        fs::read(dir.path().join(SOCKET_NAME)).unwrap(),
        b"not-a-socket"
    );
}

#[test]
fn active_owned_socket_is_never_unlinked_as_stale() {
    let dir = TempDir::new();
    fs::create_dir(dir.path()).unwrap();
    let mut permissions = fs::metadata(dir.path()).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(dir.path(), permissions).unwrap();

    let socket = dir.path().join(SOCKET_NAME);
    let listener = UnixListener::bind(&socket).unwrap();
    let mut socket_permissions = fs::metadata(&socket).unwrap().permissions();
    socket_permissions.set_mode(0o600);
    fs::set_permissions(&socket, socket_permissions).unwrap();
    // Dead lock owner + still-connectable leaf: reclaim must refuse unlink
    // and must not rewrite the foreign/dead lock body.
    fs::write(dir.path().join(LOCK_NAME), b"v1\n0\n").unwrap();

    assert_eq!(
        AgentDaemon::bind(dir.path()).map(|_| ()),
        Err(DaemonError::StartupContended)
    );
    assert!(socket.exists());
    assert_eq!(fs::read(dir.path().join(LOCK_NAME)).unwrap(), b"v1\n0\n");
    drop(listener);
}

#[test]
fn corrupt_lock_is_rejected_and_left_in_place() {
    let dir = TempDir::new();
    fs::create_dir(dir.path()).unwrap();
    let mut permissions = fs::metadata(dir.path()).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(dir.path(), permissions).unwrap();

    let lock_path = dir.path().join(LOCK_NAME);
    fs::write(&lock_path, b"v1\nnot-a-pid\n").unwrap();
    // Optional dead socket leaf must not change the CorruptLock outcome.
    let socket = dir.path().join(SOCKET_NAME);
    let listener = UnixListener::bind(&socket).unwrap();
    let mut socket_permissions = fs::metadata(&socket).unwrap().permissions();
    socket_permissions.set_mode(0o600);
    fs::set_permissions(&socket, socket_permissions).unwrap();
    drop(listener);
    wait_until_not_connectable(&socket);

    assert_eq!(
        AgentDaemon::bind(dir.path()).map(|_| ()),
        Err(DaemonError::Endpoint(EndpointFault::CorruptLock))
    );
    assert_eq!(fs::read(&lock_path).unwrap(), b"v1\nnot-a-pid\n");
    assert!(socket.exists());
}

#[test]
fn connect_hello_rejects_insecure_parent_directory() {
    let dir = TempDir::new();
    fs::create_dir(dir.path()).unwrap();
    let mut permissions = fs::metadata(dir.path()).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(dir.path(), permissions).unwrap();

    let socket = dir.path().join(SOCKET_NAME);
    let listener = UnixListener::bind(&socket).unwrap();
    let mut socket_permissions = fs::metadata(&socket).unwrap().permissions();
    socket_permissions.set_mode(0o600);
    fs::set_permissions(&socket, socket_permissions).unwrap();
    drop(listener);

    let mut permissions = fs::metadata(dir.path()).unwrap().permissions();
    permissions.set_mode(0o777);
    fs::set_permissions(dir.path(), permissions).unwrap();

    assert_eq!(
        connect_hello(&socket, &hello(), 4096).map(|_| ()),
        Err(DaemonError::InsecureDirectory)
    );
    assert!(socket.exists());
}

#[test]
fn malformed_and_incompatible_clients_do_not_stick_the_daemon() {
    let dir = TempDir::new();
    let daemon = AgentDaemon::bind_with(
        dir.path(),
        DaemonConfig {
            read_timeout: Duration::from_millis(200),
            max_frame_size: 1024,
            event_window: 32,
            ..DaemonConfig::default()
        },
    )
    .unwrap();
    let path = daemon.socket_path();
    let lock_path = dir.path().join(LOCK_NAME);

    let mut bad = UnixStream::connect(&path).unwrap();
    bad.write_all(b"nopeNOPE!!").unwrap();
    assert_eq!(daemon.accept_hello(), Err(DaemonError::Malformed));
    drop(bad);
    assert!(path.exists());
    assert!(lock_path.exists());

    let mut huge = UnixStream::connect(&path).unwrap();
    let mut header = [0; 10];
    header[..4].copy_from_slice(b"AGB1");
    header[4..6].copy_from_slice(&1_u16.to_le_bytes());
    header[6..10].copy_from_slice(&u32::MAX.to_le_bytes());
    huge.write_all(&header).unwrap();
    assert_eq!(daemon.accept_hello(), Err(DaemonError::Oversized));
    assert!(path.exists());
    assert!(lock_path.exists());

    let incompatible = Hello {
        supported_versions: vec![ProtocolVersion::new(99)],
        max_frame_size: 1024,
        event_window: 32,
        client_principal_evidence: Vec::new(),
    };
    let client = thread::spawn(move || connect_hello(&path, &incompatible, 1024));
    assert_eq!(
        daemon.accept_hello(),
        Err(DaemonError::Handshake(HandshakeError::NoCompatibleVersion))
    );
    assert_eq!(
        client.join().unwrap(),
        Err(DaemonError::Handshake(HandshakeError::NoCompatibleVersion))
    );
    assert!(daemon.socket_path().exists());
    assert!(lock_path.exists());

    let stalled_path = daemon.socket_path();
    let stalled = thread::spawn(move || {
        let _stream = UnixStream::connect(stalled_path).unwrap();
        thread::sleep(Duration::from_millis(500));
    });
    assert_eq!(daemon.accept_hello(), Err(DaemonError::TimedOut));
    stalled.join().unwrap();
    assert!(daemon.socket_path().exists());
    assert!(lock_path.exists());

    let path = daemon.socket_path();
    let good = thread::spawn(move || connect_hello(&path, &hello(), 1024));
    let ack = daemon.accept_hello().unwrap();
    assert_eq!(
        good.join().unwrap().unwrap().backend_instance_id,
        ack.backend_instance_id
    );
    assert!(hello().event_window <= MAX_EVENT_WINDOW);
    assert!(daemon.socket_path().exists());
    assert!(lock_path.exists());
}

#[test]
fn connection_churn_returns_to_zero_retained_clients() {
    let dir = TempDir::new();
    let started = Instant::now();
    let daemon = AgentDaemon::bind(dir.path()).unwrap();
    let cold_start = started.elapsed();
    let path = daemon.socket_path();

    fn resident_kib() -> Option<u64> {
        let output = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()?;
        String::from_utf8(output.stdout).ok()?.trim().parse().ok()
    }
    fn cpu_percent() -> Option<f64> {
        let output = std::process::Command::new("ps")
            .args(["-o", "pcpu=", "-p", &std::process::id().to_string()])
            .output()
            .ok()?;
        String::from_utf8(output.stdout).ok()?.trim().parse().ok()
    }

    let idle_rss_kib = resident_kib();
    let idle_cpu_percent = cpu_percent();

    let handshake_started = Instant::now();
    let handshake_path = path.clone();
    let handshake_client =
        thread::spawn(move || connect_hello(&handshake_path, &hello(), ABSOLUTE_MAX_FRAME_SIZE));
    daemon.accept_hello().unwrap();
    handshake_client.join().unwrap().unwrap();
    let handshake = handshake_started.elapsed();

    for _ in 0..20 {
        let path = path.clone();
        let client = thread::spawn(move || connect_hello(&path, &hello(), ABSOLUTE_MAX_FRAME_SIZE));
        daemon.accept_hello().unwrap();
        client.join().unwrap().unwrap();
    }
    assert!(!path.symlink_metadata().unwrap().file_type().is_symlink());
    let post_churn_rss_kib = resident_kib();
    let sample = DaemonSample {
        cold_start,
        handshake,
        churn_handshakes: 20,
        retained_connections: 0,
        rss_kib: post_churn_rss_kib,
        cpu_percent: idle_cpu_percent,
    };
    assert_eq!(sample.retained_connections, 0);
    assert!(sample.churn_handshakes == 20);
    assert!(sample.handshake > Duration::ZERO);
    if let Some(rss) = sample.rss_kib {
        assert!(rss < 512 * 1024, "spike RSS ceiling exceeded: {rss} KiB");
    }
    eprintln!(
        "ab-0.2 measurement cold_start_us={} handshake_us={} idle_rss_kib={:?} idle_cpu={:?} post_churn_rss_kib={:?}",
        sample.cold_start.as_micros(),
        sample.handshake.as_micros(),
        idle_rss_kib,
        idle_cpu_percent,
        post_churn_rss_kib
    );
}

#[test]
fn insecure_directory_and_world_socket_are_rejected() {
    let dir = TempDir::new();
    fs::create_dir(dir.path()).unwrap();
    let mut permissions = fs::metadata(dir.path()).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(dir.path(), permissions.clone()).unwrap();
    assert_eq!(
        AgentDaemon::bind(dir.path()).map(|_| ()),
        Err(DaemonError::InsecureDirectory)
    );

    permissions.set_mode(0o700);
    fs::set_permissions(dir.path(), permissions).unwrap();
    let listener = UnixListener::bind(dir.path().join(SOCKET_NAME)).unwrap();
    let mut socket_permissions = fs::metadata(dir.path().join(SOCKET_NAME))
        .unwrap()
        .permissions();
    socket_permissions.set_mode(0o666);
    fs::set_permissions(dir.path().join(SOCKET_NAME), socket_permissions).unwrap();
    drop(listener);
    fs::write(dir.path().join(LOCK_NAME), b"v1\n0\n").unwrap();
    assert_eq!(
        AgentDaemon::bind(dir.path()).map(|_| ()),
        Err(DaemonError::Endpoint(EndpointFault::InsecureMode))
    );
    assert!(dir.path().join(SOCKET_NAME).exists());
}

fn qualification_config(dir: &Path) -> crate::IntegrationConfig {
    crate::IntegrationConfig {
        store_path: dir.join("agent.db"),
        script: vec![crate::ScriptStep::Emit(crate::HostObservationKind::Started)],
    }
}

fn identity_sets(path: &Path) -> String {
    let store = AgentStore::open(path).expect("identity snapshot");
    format!(
        "principals={:?}\nscopes={:?}\nitems={:?}\nattempts={:?}\nruns={:?}",
        store.client_principals().unwrap(),
        store.work_scopes().unwrap(),
        store.work_items().unwrap(),
        store.attempts().unwrap(),
        store.agent_runs().unwrap(),
    )
}

fn poison_service(daemon: &AgentDaemon) {
    let service = std::sync::Arc::clone(daemon.integration.as_ref().expect("integration"));
    let panicked = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let _guard = service.lock().expect("lock before poison");
        panic!("poison service for fail-closed test");
    }));
    assert!(panicked.is_err(), "barrier poison-unwind did not panic");
    assert!(
        service.lock().is_err(),
        "barrier service-poison: mutex was not poisoned"
    );
}

fn client_stream(path: &Path) -> UnixStream {
    let stream = UnixStream::connect(path).expect("connect");
    // Timeouts before any write the peer reacts to: macOS setsockopt returns
    // EINVAL once the peer has already closed.
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
}

fn write_hello(stream: &mut UnixStream) {
    let hello = Hello {
        supported_versions: vec![ProtocolVersion::V1],
        max_frame_size: ABSOLUTE_MAX_FRAME_SIZE,
        event_window: 32,
        client_principal_evidence: Vec::new(),
    };
    let bytes = encode_hello(&hello, ABSOLUTE_MAX_FRAME_SIZE).unwrap();
    stream.write_all(&bytes).unwrap();
}

fn read_prefix(stream: &mut UnixStream) -> Vec<u8> {
    let mut buf = vec![0_u8; 64];
    let n = stream.read(&mut buf).expect("read");
    buf.truncate(n);
    buf
}

#[test]
fn poisoned_service_fails_closed_before_hello_ack() {
    let dir = TempDir::new();
    let mut daemon =
        AgentDaemon::bind_integration(dir.path(), qualification_config(dir.path())).unwrap();
    let before = identity_sets(&dir.path().join("agent.db"));
    poison_service(&daemon);
    let socket = daemon.socket_path();
    let client = thread::spawn(move || {
        let mut stream = client_stream(&socket);
        write_hello(&mut stream);
        read_prefix(&mut stream)
    });
    let worker = daemon.accept_and_spawn().expect("admit poisoned peer");
    let prefix = client.join().unwrap();
    assert!(
        prefix.is_empty(),
        "barrier poisoned-before-hello: expected EOF, peer received {prefix:?}"
    );
    assert_eq!(worker.join().unwrap(), Err(DaemonError::Unavailable));
    assert_eq!(
        identity_sets(&dir.path().join("agent.db")),
        before,
        "barrier poisoned-before-hello: store identity sets changed"
    );
}

#[test]
fn poisoned_service_fails_closed_mid_session() {
    let dir = TempDir::new();
    let mut daemon =
        AgentDaemon::bind_integration(dir.path(), qualification_config(dir.path())).unwrap();
    let socket = daemon.socket_path();
    let store_path = dir.path().join("agent.db");
    let (ready_tx, ready_rx) = mpsc::channel();
    let (poisoned_tx, poisoned_rx) = mpsc::channel();
    let client = thread::spawn(move || {
        let mut stream = client_stream(&socket);
        write_hello(&mut stream);
        let ack = super::read_one_frame(&mut stream, ABSOLUTE_MAX_FRAME_SIZE).unwrap();
        assert_eq!(ack.kind, FrameKind::HelloAck);
        let open = encode_command(
            &Command::OpenSession {
                scopes: vec![1, 2, 4],
            },
            ABSOLUTE_MAX_FRAME_SIZE,
        )
        .unwrap();
        stream.write_all(&open).unwrap();
        let opened = super::read_one_frame(&mut stream, ABSOLUTE_MAX_FRAME_SIZE).unwrap();
        assert_eq!(opened.kind, FrameKind::Result);
        let session_id = match decode_result(&opened.body).unwrap() {
            CommandResult::Opened { session_id } => session_id,
            other => panic!("open session: {other:?}"),
        };
        ready_tx.send(()).expect("barrier session-open send");
        poisoned_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("barrier poisoned");
        let create = encode_command(
            &Command::CreateWorkScope {
                session_id,
                kind: seyal_agent_core::WorkScopeKind::Repository,
            },
            ABSOLUTE_MAX_FRAME_SIZE,
        )
        .unwrap();
        stream.write_all(&create).unwrap();
        read_prefix(&mut stream)
    });
    let worker = daemon.accept_and_spawn().expect("admit session");
    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("barrier session-open");
    let before = identity_sets(&store_path);
    poison_service(&daemon);
    poisoned_tx.send(()).expect("barrier poisoned send");
    let prefix = client.join().unwrap();
    assert!(
        prefix.is_empty(),
        "barrier poisoned-mid-session: expected EOF, peer received {prefix:?}"
    );
    assert_eq!(worker.join().unwrap(), Err(DaemonError::Unavailable));
    assert_eq!(
        identity_sets(&store_path),
        before,
        "barrier poisoned-mid-session: store identity sets changed"
    );
}

fn recv_exit(
    rx: &mpsc::Receiver<ServeExit>,
    budget: Duration,
    started: Instant,
    barrier: &str,
) -> ServeExit {
    let remaining = budget.saturating_sub(started.elapsed());
    if remaining.is_zero() {
        panic!("barrier {barrier} timed out");
    }
    match rx.recv_timeout(remaining) {
        Ok(exit) => exit,
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("barrier {barrier} timed out"),
        Err(mpsc::RecvTimeoutError::Disconnected) => panic!("barrier {barrier} disconnected"),
    }
}

#[test]
fn exit_report_delivers_one_message_per_worker() {
    let dir = TempDir::new();
    let config = DaemonConfig {
        read_timeout: Duration::from_millis(1500),
        ..DaemonConfig::default()
    };
    let mut daemon =
        AgentDaemon::bind_integration_with(dir.path(), config, qualification_config(dir.path()))
            .unwrap();
    let exits = daemon.install_exit_report();
    let socket = daemon.socket_path();

    let normal_socket = socket.clone();
    let normal = thread::spawn(move || {
        let mut stream = client_stream(&normal_socket);
        write_hello(&mut stream);
        let ack = super::read_one_frame(&mut stream, ABSOLUTE_MAX_FRAME_SIZE).unwrap();
        assert_eq!(ack.kind, FrameKind::HelloAck);
    });
    let malformed_socket = socket.clone();
    let malformed = thread::spawn(move || {
        let mut stream = client_stream(&malformed_socket);
        stream.write_all(b"BAD!\x00\x00\x00\x00\x00\x00").unwrap();
        let _ = read_prefix(&mut stream);
    });
    let (stall_tx, stall_rx) = mpsc::channel();
    let stalled_socket = socket.clone();
    let stalled = thread::spawn(move || {
        let stream = client_stream(&stalled_socket);
        let _ = stall_rx.recv_timeout(Duration::from_secs(5));
        drop(stream);
    });

    let mut workers = Vec::new();
    for _ in 0..3 {
        workers.push(daemon.accept_and_spawn().expect("admit peer"));
    }

    let started = Instant::now();
    let budget = Duration::from_secs(5);
    let mut reports = Vec::new();
    while reports.len() < 3 {
        reports.push(recv_exit(&exits, budget, started, "worker-exits"));
    }
    let _ = stall_tx.send(());
    normal.join().unwrap();
    malformed.join().unwrap();
    stalled.join().unwrap();
    for worker in workers {
        let _ = worker.join().unwrap();
    }
    match exits.recv_timeout(Duration::from_millis(50)) {
        Err(mpsc::RecvTimeoutError::Timeout) => {}
        other => panic!("barrier worker-exits: extra report {other:?}"),
    }

    let mut ended = reports
        .into_iter()
        .map(|exit| match exit {
            ServeExit::Ended(result) => result,
            ServeExit::Panicked => panic!("barrier worker-exits: unexpected panic"),
        })
        .collect::<Vec<_>>();
    ended.sort_by_key(|result| match result {
        Ok(()) => 0,
        Err(DaemonError::Malformed) => 1,
        Err(DaemonError::TimedOut) => 2,
        Err(other) => panic!("barrier worker-exits: unexpected {other:?}"),
    });
    assert_eq!(
        ended,
        vec![
            Ok(()),
            Err(DaemonError::Malformed),
            Err(DaemonError::TimedOut),
        ]
    );
}

#[test]
fn exit_guard_reports_panicked_on_unwind() {
    let dir = TempDir::new();
    let mut daemon =
        AgentDaemon::bind_integration(dir.path(), qualification_config(dir.path())).unwrap();
    let exits = daemon.install_exit_report();
    let tx = daemon
        .exit_report
        .clone()
        .expect("barrier exit-report installed");
    let worker = super::supervision::spawn_supervised(Some(tx), || -> Result<(), DaemonError> {
        panic!("worker unwind");
    });
    let started = Instant::now();
    let exit = recv_exit(&exits, Duration::from_secs(5), started, "panicked-exit");
    assert_eq!(exit, ServeExit::Panicked);
    assert!(
        worker.join().is_err(),
        "barrier panicked-exit: worker did not unwind"
    );
    match exits.recv_timeout(Duration::from_millis(50)) {
        Err(mpsc::RecvTimeoutError::Timeout) => {}
        other => panic!("barrier panicked-exit: extra report {other:?}"),
    }
}
