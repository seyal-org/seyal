//! Issue #1217 / M003 C3 — live SplitFocused create→attach→bind with one
//! ExecutionId per terminal leaf and honest SPEC-004 capacity failures.

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
use seyal_client::shell::SplitAxis;
use seyal_client::LocalDisplayClient;
use seyal_core::PaneId;
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

fn start_isolated_runtime(
    max_executions: Option<usize>,
) -> (PathBuf, Arc<AtomicBool>, thread::JoinHandle<()>) {
    let runtime_dir = PathBuf::from(format!(
        "/tmp/s1217e{}{:x}",
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
    panic!("deadline exceeded waiting for SplitFocused live bind");
}

fn wait_bound(root: &mut ApplicationRoot, pane: PaneId) {
    poll_until(root, Instant::now() + Duration::from_secs(8), |root| {
        root.provisioning().recorded_execution(pane).is_some()
    });
}

fn live_split(root: &mut ApplicationRoot, axis: SplitAxis) -> PaneId {
    root.apply(AppAction::SplitFocused { axis })
        .expect("SplitFocused");
    let pane = root.snapshot().shell.focused_pane;
    wait_bound(root, pane);
    pane
}

#[test]
fn split_live_two_by_two_binds_four_distinct_executions() {
    let _lock = override_test_lock();
    reset_explicit_runtime_dir();
    let _reset = OverrideReset;
    let (socket_path, stop, runtime) = start_isolated_runtime(None);
    let runtime_dir = socket_path.parent().expect("socket parent").to_path_buf();
    set_explicit_runtime_dir(runtime_dir).expect("install isolated dir");

    let first =
        LocalDisplayClient::connect_first_running_until(Instant::now() + Duration::from_secs(5))
            .expect("first Controller");
    let first_execution = first.execution_id();
    assert!(
        first.execution_provisioning_negotiated(),
        "C3 requires CAP_EXECUTION_PROVISIONING on the create connection"
    );

    let mut root = ApplicationRoot::new();
    assert!(root.snapshot().shell.allows_pane_splitting);
    let a = root.snapshot().shell.focused_pane;
    root.attach_client(root.fence(), first)
        .expect("bind first Controller");
    assert_eq!(root.snapshot().execution, Some(first_execution));

    let mut first_stayed_pollable = false;
    let b = {
        root.apply(AppAction::SplitFocused {
            axis: SplitAxis::Right,
        })
        .expect("split right");
        let pane = root.snapshot().shell.focused_pane;
        poll_until(&mut root, Instant::now() + Duration::from_secs(8), |root| {
            if root.pane_client_poll_ok(a) {
                first_stayed_pollable = true;
            }
            root.provisioning().recorded_execution(pane).is_some()
        });
        pane
    };
    assert!(
        first_stayed_pollable,
        "first Controller must keep polling while a split leaf attaches"
    );

    root.apply(AppAction::FocusPane { id: a }).unwrap();
    let d = live_split(&mut root, SplitAxis::Down);
    root.apply(AppAction::FocusPane { id: b }).unwrap();
    let c = live_split(&mut root, SplitAxis::Down);

    assert_eq!(root.snapshot().shell.panes.len(), 4);
    let mut executions = std::collections::HashSet::new();
    for pane in [a, b, d, c] {
        let execution = root
            .provisioning()
            .recorded_execution(pane)
            .or_else(|| {
                root.snapshot()
                    .shell
                    .panes
                    .iter()
                    .find(|row| row.id == pane)
                    .and_then(|row| row.execution)
            })
            .expect("each 2×2 leaf must bind an execution");
        assert!(
            executions.insert(execution),
            "2×2 leaves must not share ExecutionId"
        );
    }
    assert_eq!(executions.len(), 4);
    assert!(executions.contains(&first_execution));
    assert_eq!(root.provisioning().automatic_retries(), 0);

    stop.store(true, Ordering::Relaxed);
    runtime.join().expect("Runtime thread");
}

#[test]
fn split_live_close_pane_detaches_only_and_leaves_siblings() {
    let _lock = override_test_lock();
    reset_explicit_runtime_dir();
    let _reset = OverrideReset;
    let (socket_path, stop, runtime) = start_isolated_runtime(None);
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

    let second = live_split(&mut root, SplitAxis::Right);
    let third = live_split(&mut root, SplitAxis::Down);
    let second_execution = root
        .provisioning()
        .recorded_execution(second)
        .expect("second bound");
    let third_execution = root
        .provisioning()
        .recorded_execution(third)
        .expect("third bound");

    root.apply(AppAction::ClosePane { id: third })
        .expect("close split leaf");
    assert!(root.provisioning().is_unreferenced(third_execution));
    assert_eq!(
        root.provisioning().recorded_execution(second),
        Some(second_execution)
    );
    assert_eq!(
        root.snapshot()
            .shell
            .panes
            .iter()
            .find(|pane| pane.id == first_pane)
            .and_then(|pane| pane.execution),
        Some(first_execution)
    );
    assert!(!root
        .snapshot()
        .shell
        .panes
        .iter()
        .any(|pane| pane.id == third));
    assert_eq!(root.provisioning().automatic_retries(), 0);

    stop.store(true, Ordering::Relaxed);
    runtime.join().expect("Runtime thread");
}

#[test]
fn split_live_capacity_exceeded_is_bounded_without_retry() {
    let _lock = override_test_lock();
    reset_explicit_runtime_dir();
    let _reset = OverrideReset;
    // Bootstrap + one successful split fill max_executions=2; next split fails.
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

    let second = live_split(&mut root, SplitAxis::Right);
    let second_execution = root
        .provisioning()
        .recorded_execution(second)
        .expect("second bound");

    root.apply(AppAction::SplitFocused {
        axis: SplitAxis::Down,
    })
    .expect("SplitFocused still admits until Runtime rejects");
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
    assert_eq!(
        root.provisioning().recorded_execution(second),
        Some(second_execution)
    );

    stop.store(true, Ordering::Relaxed);
    runtime.join().expect("Runtime thread");
}
