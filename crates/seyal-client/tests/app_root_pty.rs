#![cfg(target_os = "macos")]

use std::{
    path::PathBuf,
    process::Command,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use seyal_client::app::{ApplicationRoot, PresentationEligibility};
use seyal_client::LocalDisplayClient;
use seyal_exec::{CommandSpec, WindowSize};
use seyal_protocol::runtime_dir::{
    override_test_lock, reset_explicit_runtime_dir, set_explicit_runtime_dir,
};
use seyal_runtime::{local_ipc::framing::Role, ExecutionId, LocalIpcMode, Runtime, RuntimeConfig};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn start_runtime(command: &str) -> (PathBuf, ExecutionId, thread::JoinHandle<()>) {
    let command = command.to_owned();
    let suffix = COUNTER.fetch_add(1, Ordering::Relaxed);
    let (ready_tx, ready_rx) = mpsc::channel();
    let join = thread::spawn(move || {
        let mut config = RuntimeConfig::m001().expect("M001 Runtime config");
        config.singleton_path = std::env::temp_dir().join(format!("s906-{suffix:x}.lock"));
        config.local_ipc = LocalIpcMode::Enabled {
            runtime_dir_override: Some(std::env::temp_dir().join(format!("s906d-{suffix:x}"))),
        };
        config.graceful_termination = Duration::from_millis(50);
        config.forced_reap = Duration::from_millis(250);
        config.final_drain = Duration::from_millis(100);

        let mut runtime = Runtime::new(config).expect("Runtime");
        let execution_id = runtime
            .create_execution(
                CommandSpec::new("/bin/sh").args(["-c", command.as_str()]),
                WindowSize::new(80, 24, 0, 0).expect("geometry"),
            )
            .expect("execution");
        let socket_path = runtime
            .local_ipc_socket_path()
            .expect("local IPC socket")
            .to_path_buf();
        ready_tx
            .send((socket_path, execution_id))
            .expect("test receiver");

        let deadline = Instant::now() + Duration::from_secs(5);
        while runtime.execution_count() != 0 && Instant::now() < deadline {
            runtime
                .poll_once(Some(Duration::from_millis(5)))
                .expect("Runtime poll");
        }
        runtime.begin_shutdown().expect("begin shutdown");
        runtime
            .run_until_empty(Instant::now() + Duration::from_secs(2))
            .expect("shutdown");
    });
    let (socket_path, execution_id) = ready_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("Runtime ready");
    (socket_path, execution_id, join)
}

struct RuntimeDirReset;

impl Drop for RuntimeDirReset {
    fn drop(&mut self) {
        reset_explicit_runtime_dir();
    }
}

#[test]
fn window_close_preserves_live_child_and_unpresented_disposal_preserves_window() {
    use seyal_client::app::AppAction;

    let _lock = override_test_lock();
    reset_explicit_runtime_dir();
    let _reset = RuntimeDirReset;
    let suffix = COUNTER.fetch_add(1, Ordering::Relaxed);
    let runtime_dir = std::env::temp_dir().join(format!("s1269d-{suffix:x}"));
    let singleton_path = std::env::temp_dir().join(format!("s1269-{suffix:x}.lock"));
    let anchor_pid_path = runtime_dir.join("anchor.pid");
    let anchor_pid_for_runtime = anchor_pid_path.clone();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (finalized_tx, finalized_rx) = mpsc::channel();
    let (verify_anchor_tx, verify_anchor_rx) = mpsc::channel();
    let (anchor_alive_tx, anchor_alive_rx) = mpsc::channel();
    let (stop_tx, stop_rx) = mpsc::channel();
    let runtime = thread::spawn(move || {
        let mut config = RuntimeConfig::m001().expect("M001 Runtime config");
        config.singleton_path = singleton_path;
        config.local_ipc = LocalIpcMode::Enabled {
            runtime_dir_override: Some(runtime_dir),
        };
        config.graceful_termination = Duration::from_millis(50);
        config.forced_reap = Duration::from_millis(250);
        config.final_drain = Duration::from_millis(100);
        let mut runtime = Runtime::new(config).expect("Runtime");
        let anchor_script = format!(
            "printf '%s' \"$$\" > '{}'; exec sleep 30",
            anchor_pid_for_runtime
                .display()
                .to_string()
                .replace('\'', "'\\''")
        );
        let anchor = runtime
            .create_execution(
                CommandSpec::new("/bin/sh").args(["-c", &anchor_script]),
                WindowSize::cells(80, 24).expect("geometry"),
            )
            .expect("anchor execution");
        let unpresented = runtime
            .create_execution(
                CommandSpec::new("/bin/sh").args(["-c", "sleep 30"]),
                WindowSize::cells(80, 24).expect("geometry"),
            )
            .expect("unpresented execution");
        let pid_deadline = Instant::now() + Duration::from_secs(2);
        while !anchor_pid_for_runtime.exists() && Instant::now() < pid_deadline {
            runtime
                .poll_once(Some(Duration::from_millis(5)))
                .expect("wait for anchor process to start");
        }
        assert!(
            anchor_pid_for_runtime.exists(),
            "anchor child wrote its pid"
        );
        let socket_path = runtime
            .local_ipc_socket_path()
            .expect("local IPC socket")
            .to_path_buf();
        ready_tx
            .send((socket_path, anchor, unpresented))
            .expect("test receiver");

        let deadline = Instant::now() + Duration::from_secs(8);
        while (runtime.execution(anchor).is_some() && runtime.execution(unpresented).is_some())
            && Instant::now() < deadline
        {
            runtime
                .poll_once(Some(Duration::from_millis(5)))
                .expect("Runtime poll");
        }
        finalized_tx
            .send((
                runtime.execution(anchor).is_some(),
                runtime.execution(unpresented).is_some(),
            ))
            .expect("finalization receiver");
        if verify_anchor_rx
            .recv_timeout(Duration::from_secs(10))
            .is_ok()
        {
            // Give Runtime multiple turns to reap an incorrectly terminated
            // child. Registry presence alone is insufficient until this poll.
            let deadline = Instant::now() + Duration::from_millis(500);
            while Instant::now() < deadline {
                runtime
                    .poll_once(Some(Duration::from_millis(5)))
                    .expect("poll Runtime after anchor window close");
            }
            anchor_alive_tx
                .send(runtime.execution(anchor).is_some())
                .expect("anchor liveness receiver");
        }
        let _ = stop_rx.recv_timeout(Duration::from_secs(10));
        runtime.begin_shutdown().expect("begin shutdown");
        runtime
            .run_until_empty(Instant::now() + Duration::from_secs(2))
            .expect("shutdown");
    });
    let (socket_path, anchor, unpresented) = ready_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("Runtime ready");
    set_explicit_runtime_dir(
        socket_path
            .parent()
            .expect("runtime directory")
            .to_path_buf(),
    )
    .expect("install isolated runtime directory");

    let client = LocalDisplayClient::connect_execution(&socket_path, anchor, Role::Controller)
        .expect("anchor Controller");
    let mut root = ApplicationRoot::new();
    root.attach_client(root.fence(), client)
        .expect("bind anchor execution");
    let anchor_window = root.snapshot().shell.active_window.expect("anchor window");
    while !root.snapshot().pending_effects.is_empty() {
        root.apply(AppAction::AckEffect).expect("ack window effect");
    }
    root.apply(AppAction::CreateWindow)
        .expect("open independent window");
    let open_window = root.snapshot().shell.active_window.expect("new window");
    let workspace = root.snapshot().shell.active_workspace;
    root.apply(AppAction::RecordUnpresented {
        execution: unpresented,
        workspace,
    })
    .expect("record unpresented execution");
    root.apply(AppAction::TerminateUnpresented {
        execution: unpresented,
    })
    .expect("request disposal");

    let deadline = Instant::now() + Duration::from_secs(7);
    let mut runtime_finalized = false;
    while Instant::now() < deadline {
        if let Err(error) = root.poll_client(root.fence()) {
            panic!(
                "drive attach and terminate: {error:?}; extra_clients={}; snapshot={:?}",
                root.extra_pane_client_count(),
                root.snapshot()
            );
        }
        assert!(
            root.snapshot()
                .shell
                .windows
                .iter()
                .any(|window| window.id == open_window),
            "disposing an unpresented execution must preserve the open window"
        );
        if let Ok((anchor_alive, unpresented_alive)) = finalized_rx.try_recv() {
            assert!(
                anchor_alive,
                "closing the anchor window must preserve that exact Runtime execution"
            );
            assert!(
                !unpresented_alive,
                "disposing the unpresented execution must finalize that exact Runtime execution"
            );
            runtime_finalized = true;
        }
        if runtime_finalized && !root.live_unpresented().contains(&unpresented) {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        runtime_finalized,
        "Runtime must finalize the disposed child"
    );
    assert!(!root.live_unpresented().contains(&unpresented));
    assert!(
        root.snapshot()
            .shell
            .windows
            .iter()
            .any(|window| window.id == anchor_window),
        "the bound anchor window must remain open while the unbound focused window is active"
    );
    root.apply(AppAction::CloseWindow {
        id: anchor_window,
        containment_generation: root.snapshot().shell.containment_generation,
    })
    .expect("close anchor window");
    while !root.snapshot().pending_effects.is_empty() {
        root.apply(AppAction::AckEffect).expect("ack close effect");
    }
    verify_anchor_tx
        .send(())
        .expect("Runtime anchor liveness request");
    assert!(
        anchor_alive_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("Runtime anchor liveness result"),
        "closing the bound window must leave that exact Runtime execution registered after polling"
    );
    let anchor_pid = std::fs::read_to_string(&anchor_pid_path)
        .expect("anchor child pid")
        .parse::<u32>()
        .expect("valid anchor child pid");
    assert!(
        Command::new("/bin/kill")
            .args(["-0", &anchor_pid.to_string()])
            .status()
            .expect("check anchor process")
            .success(),
        "closing the bound window must leave the exact OS child alive"
    );
    assert!(
        root.snapshot()
            .shell
            .windows
            .iter()
            .any(|window| window.id == open_window),
        "the independent window must remain open after child finalization"
    );
    assert_eq!(
        root.live_unpresented(),
        vec![anchor],
        "the closed window's Runtime child must remain live after the other child is disposed"
    );
    let _ = stop_tx.send(());
    runtime.join().expect("Runtime thread");
}

#[test]
fn application_root_projects_real_pty_output_through_one_execution() {
    let (socket_path, execution_id, runtime) = start_runtime("printf 'SEYAL-APP-906'; sleep 1");
    let client = LocalDisplayClient::connect_execution(&socket_path, execution_id, Role::Observer)
        .expect("attach production client");
    assert_eq!(client.execution_id(), execution_id);

    let mut root = ApplicationRoot::new();
    let created = execution_id;
    root.attach_client(root.fence(), client)
        .expect("bind live client");
    assert_eq!(root.snapshot().execution, Some(created));
    assert_ne!(
        root.snapshot().eligibility,
        PresentationEligibility::Unbound
    );

    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        root.poll_client(root.fence()).expect("poll through root");
        let snap = root.snapshot();
        assert_eq!(snap.execution, Some(created));
        assert_eq!(snap.shell.panes.len(), 1);
        if snap.output_utf8.contains("SEYAL-APP-906") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "output did not reach root snapshot"
        );
        thread::sleep(Duration::from_millis(10));
    }

    let snap = root.snapshot();
    assert_eq!(snap.execution, Some(created));
    assert!(!snap.output_utf8.is_empty());
    drop(root);
    runtime.join().expect("Runtime thread");
}

#[test]
fn application_root_live_client_owns_only_clients_registry_entry() {
    let (socket_path, execution_id, runtime) = start_runtime("printf 'SEYAL-C2'; sleep 1");
    let client = LocalDisplayClient::connect_execution(&socket_path, execution_id, Role::Observer)
        .expect("attach production client");

    let mut root = ApplicationRoot::new();
    root.attach_client(root.fence(), client)
        .expect("bind live client");
    let handle = root
        .live_client_handle_for_test()
        .expect("root must retain registry handle only");
    assert!(
        seyal_client::ffi_test_client_registry_contains(handle),
        "live client must reside in the sole CLIENTS registry"
    );

    root.poll_client(root.fence()).expect("poll via registry");
    drop(root);
    assert!(
        !seyal_client::ffi_test_client_registry_contains(handle),
        "drop must unregister the sole registry entry"
    );
    runtime.join().expect("Runtime thread");
}

#[test]
fn poll_client_without_live_client_sets_last_error() {
    let mut root = ApplicationRoot::new();
    let err = root.poll_client(root.fence()).expect_err("no live client");
    assert_eq!(err, seyal_client::app::AppError::NoLiveClient);
    assert_eq!(
        root.snapshot().last_error,
        Some(seyal_client::app::AppError::NoLiveClient)
    );
}

#[test]
fn attach_client_rejects_second_live_client_for_same_execution() {
    let (socket_path, execution_id, runtime) = start_runtime("printf 'SEYAL-B3'; sleep 1");
    let first = LocalDisplayClient::connect_execution(&socket_path, execution_id, Role::Observer)
        .expect("first observer");
    let second = LocalDisplayClient::connect_execution(&socket_path, execution_id, Role::Observer)
        .expect("second observer");

    let mut root = ApplicationRoot::new();
    root.attach_client(root.fence(), first)
        .expect("first attach owns the sole registry slot");
    assert!(seyal_client::ffi_test_client_registry_has_execution(
        execution_id
    ));

    let mut other = ApplicationRoot::new();
    let err = other
        .attach_client(other.fence(), second)
        .expect_err("second live client for same ExecutionId must fail");
    assert_eq!(err, seyal_client::app::AppError::AlreadyBound);
    assert_eq!(other.live_client_handle_for_test(), None);

    drop(root);
    runtime.join().expect("Runtime thread");
}

#[test]
fn attach_handle_binds_already_registered_client_without_second_insert() {
    let (socket_path, execution_id, runtime) = start_runtime("printf 'SEYAL-HND'; sleep 1");
    let client = LocalDisplayClient::connect_execution(&socket_path, execution_id, Role::Observer)
        .expect("attach production client");
    let handle =
        seyal_client::test_register_pending_client(client, 9).expect("register pending handle");
    assert_eq!(
        seyal_client::seyal_bridge_adopt_handle(handle),
        0,
        "adopt into CLIENTS"
    );

    let mut root = ApplicationRoot::new();
    root.attach_handle(root.fence(), handle)
        .expect("borrow adopted handle");
    assert_eq!(root.live_client_handle_for_test(), Some(handle));
    assert!(seyal_client::ffi_test_client_registry_contains(handle));
    root.poll_client(root.fence())
        .expect("poll via adopted handle");

    drop(root);
    assert!(
        !seyal_client::ffi_test_client_registry_contains(handle),
        "root drop still tears down the sole registry entry"
    );
    runtime.join().expect("Runtime thread");
}

#[test]
fn application_root_registry_handle_is_thread_affine() {
    // Inverse of the B2 Send leak: ClientRegistryHandle is !Send/!Sync, so the
    // root cannot migrate to another executor. TLS affinity is proven by
    // registering on a worker and observing the handle absent on this thread.
    let (socket_path, execution_id, runtime) = start_runtime("printf 'SEYAL-B2'; sleep 30");
    let (ready_tx, ready_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let client =
            LocalDisplayClient::connect_execution(&socket_path, execution_id, Role::Observer)
                .expect("attach on worker");
        let mut root = ApplicationRoot::new();
        root.attach_client(root.fence(), client)
            .expect("register on worker TLS");
        let handle = root.live_client_handle_for_test().expect("handle");
        ready_tx.send(handle).expect("send handle");
        done_rx
            .recv()
            .expect("keep root alive until probe finishes");
        drop(root);
    });

    let handle = ready_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("worker registered");
    assert!(
        !seyal_client::ffi_test_client_registry_contains(handle),
        "origin-thread CLIENTS entry must not be visible on this thread"
    );
    done_tx.send(()).expect("release worker");
    worker.join().expect("worker");
    assert!(
        !seyal_client::ffi_test_client_registry_contains(handle),
        "worker drop must unregister on its own TLS map"
    );
    runtime.join().expect("Runtime thread");
}

#[test]
fn bridge_disconnect_clears_stale_root_handle() {
    let (socket_path, execution_id, runtime) = start_runtime("printf 'SEYAL-N2'; sleep 1");
    let client = LocalDisplayClient::connect_execution(&socket_path, execution_id, Role::Observer)
        .expect("attach production client");
    let mut root = ApplicationRoot::new();
    root.attach_client(root.fence(), client)
        .expect("bind live client");
    let handle = root.live_client_handle_for_test().expect("handle");
    assert_eq!(seyal_client::seyal_bridge_select(handle), 0);
    seyal_client::seyal_bridge_disconnect_handle(handle);
    let err = root
        .poll_client(root.fence())
        .expect_err("disconnected handle");
    assert_eq!(err, seyal_client::app::AppError::NoLiveClient);
    assert_eq!(root.live_client_handle_for_test(), None);
    assert_eq!(
        root.snapshot().last_error,
        Some(seyal_client::app::AppError::NoLiveClient)
    );
    runtime.join().expect("Runtime thread");
}
