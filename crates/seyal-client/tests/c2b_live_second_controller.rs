//! Issue #1175 / M003 C2b — live CreateTab create→attach→bind on a second
//! Controller connection with a real Runtime `AttachmentId` (no fabrication).

#![cfg(target_os = "macos")]

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};

use seyal_client::app::{AppAction, ApplicationRoot};
use seyal_client::LocalDisplayClient;
use seyal_core::AttachmentId;
use seyal_protocol::runtime_dir::{
    override_test_lock, reset_explicit_runtime_dir, set_explicit_runtime_dir,
};
use seyal_runtime::{Runtime, RuntimeConfig};

struct OverrideReset;

impl Drop for OverrideReset {
    fn drop(&mut self) {
        reset_explicit_runtime_dir();
    }
}

fn start_empty_isolated_runtime() -> (PathBuf, Arc<AtomicBool>, thread::JoinHandle<()>) {
    start_isolated_runtime(None)
}

fn start_isolated_runtime(
    max_executions: Option<usize>,
) -> (PathBuf, Arc<AtomicBool>, thread::JoinHandle<()>) {
    let runtime_dir = PathBuf::from(format!(
        "/tmp/s1175e{}{:x}",
        std::process::id() % 100_000,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            % 0xFFFF
    ));
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = Arc::clone(&stop);
    let (ready_tx, ready_rx) = mpsc::channel();
    let join = thread::spawn(move || {
        let mut config = RuntimeConfig::m001()
            .expect("M001 Runtime config")
            .isolated_to(runtime_dir);
        if let Some(max_executions) = max_executions {
            config.max_executions = max_executions;
        }
        let mut runtime = Runtime::new(config).expect("isolated Runtime");
        let socket_path = runtime
            .local_ipc_socket_path()
            .expect("local IPC socket")
            .to_path_buf();
        ready_tx.send(socket_path).expect("test receiver");
        let deadline = Instant::now() + Duration::from_secs(30);
        while !stop_thread.load(Ordering::Relaxed) && Instant::now() < deadline {
            runtime
                .poll_once(Some(Duration::from_millis(5)))
                .expect("Runtime poll");
        }
        runtime.begin_shutdown().expect("begin shutdown");
        runtime
            .run_until_empty(Instant::now() + Duration::from_secs(3))
            .expect("shutdown");
    });
    let socket_path = ready_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("Runtime ready");
    (socket_path, stop, join)
}

fn poll_until(
    root: &mut ApplicationRoot,
    deadline: Instant,
    mut pred: impl FnMut(&ApplicationRoot) -> bool,
) {
    while Instant::now() < deadline {
        let _ = root.poll_client(root.fence());
        if pred(root) {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("deadline exceeded waiting for CreateTab live bind");
}

#[test]
fn create_tab_live_second_controller_attach_binds_distinct_execution() {
    let _lock = override_test_lock();
    reset_explicit_runtime_dir();
    let _reset = OverrideReset;
    let (socket_path, stop, runtime) = start_empty_isolated_runtime();
    let runtime_dir = socket_path.parent().expect("socket parent").to_path_buf();
    set_explicit_runtime_dir(runtime_dir).expect("install isolated dir");

    let first =
        LocalDisplayClient::connect_first_running_until(Instant::now() + Duration::from_secs(5))
            .expect("Controller creates profile 0 when empty Runtime has no execution");
    let first_execution = first.execution_id();
    assert!(
        first.execution_provisioning_negotiated(),
        "C2b requires CAP_EXECUTION_PROVISIONING on the create connection"
    );

    let mut root = ApplicationRoot::new();
    assert!(
        root.snapshot().shell.allows_tab_creation,
        "production composition enables CreateTab"
    );
    let first_pane = root.snapshot().shell.focused_pane;
    let fence = root.fence();
    root.attach_client(fence, first)
        .expect("bind first Controller");
    assert_eq!(root.snapshot().execution, Some(first_execution));

    root.apply(AppAction::CreateTab)
        .expect("CreateTab admits type-36 on the live create connection");
    let second_pane = root.snapshot().shell.focused_pane;
    assert_ne!(first_pane, second_pane);
    assert!(
        root.provisioning().pending_intent(second_pane).is_some(),
        "CreateTab must begin a pending create intent"
    );

    let mut first_stayed_pollable = false;
    poll_until(&mut root, Instant::now() + Duration::from_secs(8), |root| {
        if root.pane_client_poll_ok(first_pane) {
            first_stayed_pollable = true;
        }
        root.provisioning()
            .recorded_execution(second_pane)
            .is_some()
    });
    assert!(
        first_stayed_pollable,
        "first Controller must keep polling while the second tab attaches"
    );

    let second_execution = root
        .provisioning()
        .recorded_execution(second_pane)
        .expect("second pane bound");
    assert_ne!(
        second_execution, first_execution,
        "second tab must bind a distinct live ExecutionId"
    );
    let second_shell_exec = root
        .snapshot()
        .shell
        .panes
        .iter()
        .find(|pane| pane.id == second_pane)
        .and_then(|pane| pane.execution);
    assert_eq!(second_shell_exec, Some(second_execution));
    assert_eq!(root.snapshot().execution, Some(second_execution));
    let attachment = root
        .snapshot()
        .attachment
        .expect("live attach must record a real Runtime AttachmentId");
    // Fabrication ban: must not match the C2a harness helper pattern.
    let fabricated = AttachmentId::from_bytes([second_execution.to_bytes()[0]; 16]);
    assert_ne!(
        attachment, fabricated,
        "production path must not fabricate AttachmentId from ExecutionId bytes"
    );

    let first_handle = root
        .live_client_handle_for_test()
        .expect("first Controller remains registered");
    assert_eq!(
        seyal_client::ffi_test_active_registry_execution(),
        Some(second_execution),
        "focused tab's Controller must own display/input after CreateTab"
    );
    assert_eq!(
        seyal_client::seyal_bridge_select(first_handle),
        0,
        "first-connect handle stays live for create"
    );
    assert_eq!(
        seyal_client::ffi_test_active_registry_execution(),
        Some(second_execution),
        "seyal_bridge_select of the first handle must not steal focused display"
    );

    let second_tab = root.snapshot().shell.active_tab;
    root.apply(AppAction::CloseTab { id: second_tab })
        .expect("close second tab (detach-only)");
    assert!(root.provisioning().is_unreferenced(second_execution));
    assert_eq!(
        root.provisioning().recorded_execution(first_pane),
        Some(first_execution),
        "unrelated first execution must remain recorded after detach-only close"
    );
    assert_eq!(root.provisioning().automatic_retries(), 0);
    assert_eq!(root.extra_pane_client_count(), 0);

    stop.store(true, Ordering::Relaxed);
    runtime.join().expect("Runtime thread");
}

#[test]
fn create_tab_explicit_terminate_rides_owning_second_controller() {
    let _lock = override_test_lock();
    reset_explicit_runtime_dir();
    let _reset = OverrideReset;
    let (socket_path, stop, runtime) = start_empty_isolated_runtime();
    let runtime_dir = socket_path.parent().expect("socket parent").to_path_buf();
    set_explicit_runtime_dir(runtime_dir).expect("install isolated dir");

    let first =
        LocalDisplayClient::connect_first_running_until(Instant::now() + Duration::from_secs(5))
            .expect("first Controller");
    let first_execution = first.execution_id();
    let mut root = ApplicationRoot::new();
    let first_pane = root.snapshot().shell.focused_pane;
    let fence = root.fence();
    root.attach_client(fence, first).expect("bind first");

    root.apply(AppAction::CreateTab).expect("CreateTab");
    let second_pane = root.snapshot().shell.focused_pane;
    poll_until(&mut root, Instant::now() + Duration::from_secs(8), |root| {
        root.provisioning()
            .recorded_execution(second_pane)
            .is_some()
    });
    let second_execution = root
        .provisioning()
        .recorded_execution(second_pane)
        .expect("second pane bound");
    assert_ne!(second_execution, first_execution);

    root.apply(AppAction::TerminateExecution {
        fence: root.fence(),
    })
    .expect("P4 terminate on focused second-tab Controller");
    poll_until(&mut root, Instant::now() + Duration::from_secs(8), |root| {
        root.provisioning()
            .recorded_execution(second_pane)
            .is_none()
            && root.extra_pane_client_count() == 0
    });
    assert_eq!(
        root.provisioning().recorded_execution(first_pane),
        Some(first_execution),
        "explicit terminate of tab 2 must not dispose tab 1"
    );
    assert_eq!(root.extra_pane_client_count(), 0);
    assert_eq!(root.provisioning().automatic_retries(), 0);

    stop.store(true, Ordering::Relaxed);
    runtime.join().expect("Runtime thread");
}

#[test]
fn create_tab_close_before_created_disposes_via_section_6_3() {
    let _lock = override_test_lock();
    reset_explicit_runtime_dir();
    let _reset = OverrideReset;
    let (socket_path, stop, runtime) = start_empty_isolated_runtime();
    let runtime_dir = socket_path.parent().expect("socket parent").to_path_buf();
    set_explicit_runtime_dir(runtime_dir).expect("install isolated dir");

    let first =
        LocalDisplayClient::connect_first_running_until(Instant::now() + Duration::from_secs(5))
            .expect("first Controller");
    let first_execution = first.execution_id();
    let mut root = ApplicationRoot::new();
    let first_pane = root.snapshot().shell.focused_pane;
    let fence = root.fence();
    root.attach_client(fence, first).expect("bind first");

    root.apply(AppAction::CreateTab).expect("CreateTab");
    let second_tab = root.snapshot().shell.active_tab;
    root.apply(AppAction::CloseTab { id: second_tab })
        .expect("CloseTab before type-37");

    poll_until(&mut root, Instant::now() + Duration::from_secs(8), |root| {
        root.snapshot().shell.tabs.len() == 1
            && root.extra_pane_client_count() == 0
            && !root.provisioning().has_outstanding_intent()
    });
    thread::sleep(Duration::from_millis(200));
    let _ = root.poll_client(root.fence());
    assert_eq!(
        root.provisioning().recorded_execution(first_pane),
        Some(first_execution)
    );
    assert_eq!(root.extra_pane_client_count(), 0);
    assert_eq!(root.snapshot().shell.tabs.len(), 1);

    stop.store(true, Ordering::Relaxed);
    runtime.join().expect("Runtime thread");
}

#[test]
fn create_tab_attach_failure_n_times_stays_bounded() {
    let _lock = override_test_lock();
    reset_explicit_runtime_dir();
    let _reset = OverrideReset;
    let (socket_path, stop, runtime) = start_empty_isolated_runtime();
    let runtime_dir = socket_path.parent().expect("socket parent").to_path_buf();
    set_explicit_runtime_dir(runtime_dir).expect("install isolated dir");

    let first =
        LocalDisplayClient::connect_first_running_until(Instant::now() + Duration::from_secs(5))
            .expect("first Controller");
    let first_execution = first.execution_id();
    let mut root = ApplicationRoot::new();
    let first_pane = root.snapshot().shell.focused_pane;
    let fence = root.fence();
    root.attach_client(fence, first).expect("bind first");

    for _ in 0..3 {
        root.inject_live_attach_failures(2);
        let before_tabs = root.snapshot().shell.tabs.len();
        root.apply(AppAction::CreateTab).expect("CreateTab admits");
        poll_until(&mut root, Instant::now() + Duration::from_secs(8), |root| {
            root.provisioning().last_failure().is_some()
                && root.extra_pane_client_count() == 0
                && !root.provisioning().has_outstanding_intent()
        });
        assert_eq!(
            root.provisioning().recorded_execution(first_pane),
            Some(first_execution)
        );
        assert_eq!(root.provisioning().automatic_retries(), 0);
        if root.snapshot().shell.tabs.len() > before_tabs {
            let extra = root.snapshot().shell.active_tab;
            let _ = root.apply(AppAction::CloseTab { id: extra });
            let _ = root.poll_client(root.fence());
        }
    }

    stop.store(true, Ordering::Relaxed);
    runtime.join().expect("Runtime thread");
}

#[test]
fn create_tab_live_three_tabs_keep_distinct_executions() {
    let _lock = override_test_lock();
    reset_explicit_runtime_dir();
    let _reset = OverrideReset;
    let (socket_path, stop, runtime) = start_empty_isolated_runtime();
    let runtime_dir = socket_path.parent().expect("socket parent").to_path_buf();
    set_explicit_runtime_dir(runtime_dir).expect("install isolated dir");

    let first =
        LocalDisplayClient::connect_first_running_until(Instant::now() + Duration::from_secs(5))
            .expect("first Controller");
    let first_execution = first.execution_id();
    let mut root = ApplicationRoot::new();
    let first_pane = root.snapshot().shell.focused_pane;
    root.attach_client(root.fence(), first)
        .expect("bind first Controller");

    let mut executions = vec![first_execution];
    for _ in 0..2 {
        root.apply(AppAction::CreateTab).expect("CreateTab");
        let pane = root.snapshot().shell.focused_pane;
        poll_until(&mut root, Instant::now() + Duration::from_secs(8), |root| {
            root.provisioning().recorded_execution(pane).is_some()
        });
        let execution = root
            .provisioning()
            .recorded_execution(pane)
            .expect("bound extra tab");
        assert!(
            !executions.contains(&execution),
            "each CreateTab must bind a distinct ExecutionId"
        );
        executions.push(execution);
    }
    assert_eq!(root.snapshot().shell.tabs.len(), 3);
    assert_eq!(root.extra_pane_client_count(), 2);
    assert_eq!(
        root.provisioning().recorded_execution(first_pane),
        Some(first_execution)
    );

    stop.store(true, Ordering::Relaxed);
    runtime.join().expect("Runtime thread");
}

#[test]
fn create_tab_live_capacity_exceeded_is_bounded_without_retry() {
    let _lock = override_test_lock();
    reset_explicit_runtime_dir();
    let _reset = OverrideReset;
    let (socket_path, stop, runtime) = start_isolated_runtime(Some(2));
    let runtime_dir = socket_path.parent().expect("socket parent").to_path_buf();
    set_explicit_runtime_dir(runtime_dir).expect("install isolated dir");

    let first =
        LocalDisplayClient::connect_first_running_until(Instant::now() + Duration::from_secs(5))
            .expect("first Controller");
    let first_execution = first.execution_id();
    let mut root = ApplicationRoot::new();
    let first_pane = root.snapshot().shell.focused_pane;
    root.attach_client(root.fence(), first)
        .expect("bind first Controller");

    root.apply(AppAction::CreateTab).expect("fill last slot");
    let second_pane = root.snapshot().shell.focused_pane;
    poll_until(&mut root, Instant::now() + Duration::from_secs(8), |root| {
        root.provisioning()
            .recorded_execution(second_pane)
            .is_some()
    });

    root.apply(AppAction::CreateTab)
        .expect("CreateTab still admits until Runtime rejects");
    poll_until(&mut root, Instant::now() + Duration::from_secs(8), |root| {
        matches!(
            root.provisioning().last_failure(),
            Some((
                _,
                seyal_client::provisioning::ProvisioningFailure::CreateRejected(
                    seyal_protocol::framing::ErrorCode::CapacityExceeded
                )
            ))
        ) && !root.provisioning().has_outstanding_intent()
    });
    assert_eq!(root.provisioning().automatic_retries(), 0);
    assert_eq!(
        root.provisioning().recorded_execution(first_pane),
        Some(first_execution)
    );
    assert_eq!(root.extra_pane_client_count(), 1);

    stop.store(true, Ordering::Relaxed);
    runtime.join().expect("Runtime thread");
}
