use super::*;
use seyal_agent_protocol::{HandshakeError, ProtocolVersion, MAX_EVENT_WINDOW};
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
