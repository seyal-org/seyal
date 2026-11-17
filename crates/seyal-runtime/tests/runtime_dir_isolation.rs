#![cfg(target_os = "macos")]

use std::{
    path::PathBuf,
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant},
};

use seyal_exec::{CommandSpec, WindowSize};
use seyal_runtime::{LocalIpcMode, Runtime, RuntimeConfig, RuntimeError};

/// The canonical user control socket is one global endpoint. Tests that
/// sample its existence before/after an isolated fixture, and the one test
/// that legitimately binds it in-process when no user Runtime is running,
/// must not interleave, or a sample straddles the sibling's bind/unlink and
/// the assertion reports a change the fixture never made. Only those tests
/// take this lock; nothing else in the file is serialized.
static CANONICAL_ENDPOINT: Mutex<()> = Mutex::new(());

fn canonical_endpoint_guard() -> MutexGuard<'static, ()> {
    // A panic in one holder must not turn every later sample into a
    // poisoned-lock failure that hides the original assertion.
    CANONICAL_ENDPOINT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn unique_dir(tag: &str) -> PathBuf {
    PathBuf::from(format!(
        "/tmp/s860{tag}{}{:x}",
        std::process::id() % 100_000,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            % 0xFFFF
    ))
}

fn isolated_config(tag: &str) -> RuntimeConfig {
    RuntimeConfig::m001()
        .expect("M001 Runtime config")
        .isolated_to(unique_dir(tag))
}

fn shutdown(runtime: &mut Runtime) {
    runtime.begin_shutdown().expect("begin shutdown");
    runtime
        .run_until_empty(Instant::now() + Duration::from_secs(2))
        .expect("shutdown");
}

#[test]
fn m001_defaults_remain_the_canonical_user_scope() {
    let config = RuntimeConfig::m001().expect("M001 Runtime config");
    assert!(config.singleton_path.ends_with("seyal/runtime.lock"));
    match config.local_ipc {
        LocalIpcMode::Enabled {
            runtime_dir_override: None,
        } => {}
        other => panic!("production M001 config must not isolate IPC: {other:?}"),
    }
}

#[test]
fn two_isolated_runtimes_coexist_and_do_not_bind_the_canonical_socket() {
    let _canonical_endpoint = canonical_endpoint_guard();
    let canonical = seyal_runtime::local_ipc::discovery::darwin_user_runtime_dir()
        .expect("canonical runtime dir")
        .join("control.sock");
    let existed_before = canonical.exists();

    let mut first = Runtime::new(isolated_config("a")).expect("first isolated Runtime");
    let mut second = Runtime::new(isolated_config("b")).expect("second isolated Runtime");
    let first_socket = first
        .local_ipc_socket_path()
        .expect("first socket")
        .to_path_buf();
    let second_socket = second
        .local_ipc_socket_path()
        .expect("second socket")
        .to_path_buf();

    assert_ne!(first_socket, second_socket);
    assert_ne!(first_socket, canonical);
    assert_ne!(second_socket, canonical);
    assert!(first_socket.ends_with("control.sock"));
    assert!(second_socket.ends_with("control.sock"));
    assert_eq!(
        canonical.exists(),
        existed_before,
        "isolated fixtures must not create or remove the user control socket"
    );

    shutdown(&mut first);
    shutdown(&mut second);
}

#[test]
fn isolated_runtime_starts_while_the_production_m001_singleton_is_held() {
    let _canonical_endpoint = canonical_endpoint_guard();
    let canonical = seyal_runtime::local_ipc::discovery::darwin_user_runtime_dir()
        .expect("canonical runtime dir")
        .join("control.sock");
    let production_lock = RuntimeConfig::m001()
        .expect("M001 Runtime config")
        .singleton_path;

    let mut production = match Runtime::new(RuntimeConfig::m001().expect("M001 Runtime config")) {
        Ok(runtime) => Some(runtime),
        Err(RuntimeError::AlreadyRunning) => None,
        Err(error) => panic!("unexpected production Runtime start error: {error}"),
    };
    let production_held_in_process = production.is_some();
    if let Some(runtime) = production.as_ref() {
        assert_eq!(
            runtime.local_ipc_socket_path().map(PathBuf::from),
            Some(canonical.clone()),
            "in-process stand-in must occupy the canonical user endpoint"
        );
        assert!(
            canonical.exists(),
            "held production Runtime must bind the canonical control socket"
        );
    }

    let mut isolated = Runtime::new(isolated_config("held"))
        .expect("isolated Runtime must start while the production singleton is held");
    let isolated_socket = isolated
        .local_ipc_socket_path()
        .expect("isolated socket")
        .to_path_buf();
    assert_ne!(isolated_socket, canonical);
    assert!(isolated_socket.ends_with("control.sock"));
    if production_held_in_process {
        assert!(
            canonical.exists(),
            "isolated fixture must not steal or unlink the held production socket"
        );
    }
    let isolated_lock = isolated_socket
        .parent()
        .expect("isolated runtime dir")
        .join("runtime.lock");
    assert_ne!(isolated_lock, production_lock);

    shutdown(&mut isolated);
    if let Some(runtime) = production.as_mut() {
        shutdown(runtime);
    }
}

#[test]
fn incorrect_isolated_endpoint_is_rejected_without_using_production() {
    let mut runtime = Runtime::new(isolated_config("live")).expect("isolated Runtime");
    let live = runtime
        .local_ipc_socket_path()
        .expect("live socket")
        .to_path_buf();
    runtime
        .create_execution(
            CommandSpec::new("/bin/sh").args(["-c", "sleep 30"]),
            WindowSize::new(80, 24, 0, 0).expect("geometry"),
        )
        .expect("execution");

    let deadline = Instant::now() + Duration::from_secs(2);
    while !live.exists() && Instant::now() < deadline {
        runtime
            .poll_once(Some(Duration::from_millis(10)))
            .expect("poll");
    }
    assert!(live.exists(), "isolated control socket must exist");
    std::os::unix::net::UnixStream::connect(&live).expect("live isolated endpoint is connectable");

    let missing = unique_dir("missing").join("control.sock");
    assert!(std::os::unix::net::UnixStream::connect(&missing).is_err());

    shutdown(&mut runtime);
}

#[test]
fn isolated_helper_binary_does_not_occupy_the_canonical_socket() {
    let _canonical_endpoint = canonical_endpoint_guard();
    let canonical = seyal_runtime::local_ipc::discovery::darwin_user_runtime_dir()
        .expect("canonical runtime dir")
        .join("control.sock");
    let existed_before = canonical.exists();
    let dir = unique_dir("bin");
    let socket = dir.join("control.sock");
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_seyal-runtime"))
        .args([
            "--runtime-dir",
            dir.to_str().expect("utf8 runtime dir"),
            "/bin/sleep",
            "30",
        ])
        .env("SEYAL_RUNTIME_DIR", "/tmp/seyal-should-ignore")
        .spawn()
        .expect("spawn isolated seyal-runtime");

    let deadline = Instant::now() + Duration::from_secs(3);
    while !socket.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let started = socket.exists();
    let _ = child.kill();
    let _ = child.wait();
    assert!(started, "isolated helper must bind its own control socket");
    assert_eq!(
        canonical.exists(),
        existed_before,
        "helper --runtime-dir must not bind the user control socket"
    );
}

/// #1421: a dead Runtime that leaves `control.sock` behind must be recoverable
/// by a new singleton contender. Only Runtime may verify/remove the stale leaf.
#[test]
fn helper_recovers_verified_stale_control_socket_without_client_unlink() {
    use std::os::unix::net::{UnixListener, UnixStream};

    let dir = unique_dir("stale");
    std::fs::create_dir_all(&dir).expect("runtime dir");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).expect("0700");
    }
    let socket = dir.join("control.sock");
    {
        let listener = UnixListener::bind(&socket).expect("bind stale leaf");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))
                .expect("0600");
        }
        drop(listener);
    }
    assert!(
        matches!(
            UnixStream::connect(&socket)
                .map(|_| ())
                .map_err(|error| error.kind()),
            Err(std::io::ErrorKind::ConnectionRefused)
        ),
        "precondition: stale leaf must refuse connects"
    );
    let before = std::fs::symlink_metadata(&socket).expect("stale metadata");
    assert!(
        std::os::unix::fs::FileTypeExt::is_socket(&before.file_type()),
        "precondition: leaf must be a socket"
    );

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_seyal-runtime"))
        .args([
            "--runtime-dir",
            dir.to_str().expect("utf8 runtime dir"),
            "/bin/sleep",
            "30",
        ])
        .spawn()
        .expect("spawn helper against stale socket");
    let helper_pid = child.id();

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut live = false;
    while Instant::now() < deadline {
        if UnixStream::connect(&socket).is_ok() {
            live = true;
            break;
        }
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        live,
        "helper must replace the stale leaf and accept connects"
    );
    assert_eq!(
        child.try_wait().expect("try_wait"),
        None,
        "helper pid {helper_pid} must still be running after bind"
    );

    let _ = child.kill();
    let _ = child.wait();
}
