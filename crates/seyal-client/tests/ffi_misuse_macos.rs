#![cfg(target_os = "macos")]
#![allow(unsafe_code)]

//! Live-handle adversarial coverage for Pass 10 / #760 FFI misuse cells.

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Barrier,
    },
    thread,
    time::{Duration, Instant},
};

use seyal_client::{
    ffi_test_claim_focused_display_if_unset, seyal_bridge_adopt_handle,
    seyal_bridge_disconnect_handle, seyal_bridge_ensure_prepared, seyal_bridge_frame,
    seyal_bridge_poll, seyal_bridge_select, test_register_pending_client, LocalDisplayClient,
};
use seyal_exec::{CommandSpec, WindowSize};
use seyal_runtime::{local_ipc::framing::Role, ExecutionId, LocalIpcMode, Runtime, RuntimeConfig};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct RuntimeHarness {
    socket_path: PathBuf,
    execution_id: ExecutionId,
    stop: mpsc::Sender<()>,
    join: thread::JoinHandle<()>,
}

impl RuntimeHarness {
    fn start() -> Self {
        let suffix = COUNTER.fetch_add(1, Ordering::Relaxed);
        let (ready_tx, ready_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();
        let join = thread::spawn(move || {
            let mut config = RuntimeConfig::m001().expect("M001 Runtime config");
            config.singleton_path = std::env::temp_dir().join(format!("s760f-{suffix:x}.lock"));
            config.local_ipc = LocalIpcMode::Enabled {
                runtime_dir_override: Some(std::env::temp_dir().join(format!("s760fd-{suffix:x}"))),
            };
            config.graceful_termination = Duration::from_millis(50);
            config.forced_reap = Duration::from_millis(250);
            config.final_drain = Duration::from_millis(100);

            let mut runtime = Runtime::new(config).expect("Runtime");
            let execution_id = runtime
                .create_execution(
                    CommandSpec::new("/bin/sh").args(["-c", "printf 'ffi-misuse'; sleep 30"]),
                    WindowSize::cells(40, 12).expect("geometry"),
                )
                .expect("execution");
            let socket_path = runtime
                .local_ipc_socket_path()
                .expect("local IPC socket")
                .to_path_buf();
            ready_tx
                .send((socket_path, execution_id))
                .expect("test receiver");

            let deadline = Instant::now() + Duration::from_secs(15);
            while stop_rx.try_recv().is_err() && Instant::now() < deadline {
                runtime
                    .poll_once(Some(Duration::from_millis(2)))
                    .expect("Runtime poll");
            }
            runtime.begin_shutdown().expect("begin shutdown");
            let _ = runtime.run_until_empty(Instant::now() + Duration::from_secs(2));
        });
        let (socket_path, execution_id) = ready_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("Runtime ready");
        Self {
            socket_path,
            execution_id,
            stop: stop_tx,
            join,
        }
    }

    fn connect(&self) -> LocalDisplayClient {
        LocalDisplayClient::connect_execution(
            &self.socket_path,
            self.execution_id,
            Role::Controller,
        )
        .expect("controller attach")
    }

    fn finish(self) {
        let _ = self.stop.send(());
        self.join.join().expect("Runtime thread");
    }
}

#[test]
fn double_adopt_of_live_handle_fails_on_second_call() {
    let harness = RuntimeHarness::start();
    let handle =
        test_register_pending_client(harness.connect(), 9).expect("register pending handle");
    assert_eq!(seyal_bridge_adopt_handle(handle), 0);
    assert_eq!(
        seyal_bridge_adopt_handle(handle),
        -1,
        "second adopt of the same live handle must fail closed"
    );
    seyal_bridge_disconnect_handle(handle);
    harness.finish();
}

#[test]
fn wrong_thread_cannot_select_handle_adopted_elsewhere() {
    let harness = RuntimeHarness::start();
    let handle =
        test_register_pending_client(harness.connect(), 9).expect("register pending handle");
    assert_eq!(seyal_bridge_adopt_handle(handle), 0);
    assert_eq!(seyal_bridge_select(handle), 0);

    let barrier = Arc::new(Barrier::new(2));
    let barrier_thread = Arc::clone(&barrier);
    let worker = thread::spawn(move || {
        barrier_thread.wait();
        assert_eq!(
            seyal_bridge_select(handle),
            -1,
            "executor-local map must not expose a handle adopted on another thread"
        );
    });
    barrier.wait();
    worker.join().expect("worker");
    assert_eq!(seyal_bridge_select(handle), 0);
    seyal_bridge_disconnect_handle(handle);
    harness.finish();
}

#[test]
fn frame_cells_are_cleared_after_disconnect_following_poll() {
    let harness = RuntimeHarness::start();
    let handle =
        test_register_pending_client(harness.connect(), 9).expect("register pending handle");
    assert_eq!(seyal_bridge_adopt_handle(handle), 0);
    ffi_test_claim_focused_display_if_unset(handle);

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let _ = seyal_bridge_poll();
        let _ = seyal_bridge_ensure_prepared();
        let frame = seyal_bridge_frame();
        if !frame.cells.is_null() && frame.cell_count > 0 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "prepared frame never became available"
        );
        thread::sleep(Duration::from_millis(10));
    }

    let live = seyal_bridge_frame();
    assert!(!live.cells.is_null());
    let retained_count = live.cell_count;

    assert!(seyal_bridge_poll() >= -1);
    seyal_bridge_disconnect_handle(handle);
    let after = seyal_bridge_frame();
    assert!(after.cells.is_null());
    assert_eq!(after.cell_count, 0);
    assert!(
        retained_count > 0,
        "precondition: retained a non-empty frame before invalidation"
    );

    harness.finish();
}
