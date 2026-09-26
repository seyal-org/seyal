#![cfg(target_os = "macos")]

use std::{
    path::PathBuf,
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
