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
    seyal_bridge_history_range_peek_for_handle, seyal_bridge_history_range_row_for_handle,
    seyal_bridge_next_history_request_id_for, seyal_bridge_poll, seyal_bridge_poll_for,
    seyal_bridge_request_history_range_for, seyal_bridge_select, test_register_pending_client,
    LocalDisplayClient,
};
use seyal_exec::{CommandSpec, WindowSize};
use seyal_runtime::{
    local_ipc::framing::{ComposerEligibility, Role},
    ExecutionId, LocalIpcMode, Runtime, RuntimeConfig,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct RuntimeHarness {
    socket_path: PathBuf,
    execution_id: ExecutionId,
    stop: mpsc::Sender<()>,
    join: thread::JoinHandle<()>,
}

impl RuntimeHarness {
    fn start() -> Self {
        Self::start_with_command(
            CommandSpec::new("/bin/sh").args(["-c", "printf 'ffi-misuse'; sleep 30"]),
        )
    }

    fn start_zsh() -> Self {
        Self::start_with_command(CommandSpec::new("/bin/zsh"))
    }

    fn start_with_command(command: CommandSpec) -> Self {
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
                .create_execution(command, WindowSize::cells(40, 12).expect("geometry"))
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

fn pump_until(client: &mut LocalDisplayClient, predicate: impl Fn(&LocalDisplayClient) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if predicate(client) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "Runtime client condition timed out"
        );
        client.poll_prepare().expect("Runtime client poll");
        if client.wants_write() {
            client.flush_control_write().expect("client flush");
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn submit_completed_command(client: &mut LocalDisplayClient, command: &str) {
    pump_until(client, |client| {
        client
            .composer_status()
            .is_some_and(|status| status.eligibility == ComposerEligibility::Available)
    });
    client
        .submit_composer_command(command)
        .expect("composer command admission");
    pump_until(client, |client| {
        client
            .block_timeline()
            .records
            .last()
            .is_some_and(|block| block.end_line.is_some())
    });
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

#[test]
fn history_request_on_unfocused_controller_reads_its_own_same_id_block() {
    const TOKEN_A: &str = "FFI_HISTORY_PANE_A";
    const TOKEN_B: &str = "FFI_HISTORY_PANE_B";

    let runtime_a = RuntimeHarness::start_zsh();
    let runtime_b = RuntimeHarness::start_zsh();
    let mut client_a = runtime_a.connect();
    let mut client_b = runtime_b.connect();
    submit_completed_command(&mut client_a, &format!("printf {TOKEN_A}"));
    submit_completed_command(&mut client_b, &format!("printf {TOKEN_B}"));

    let block_a = client_a.block_timeline().records.last().expect("Block A");
    let block_b = client_b.block_timeline().records.last().expect("Block B");
    assert_eq!(
        block_a.id, block_b.id,
        "per-execution Block IDs should collide"
    );
    let (block_id, start_line, end_line) =
        (block_b.id, block_b.start_line, block_b.end_line.unwrap());

    let handle_a = test_register_pending_client(client_a, 9).expect("register Controller A");
    let handle_b = test_register_pending_client(client_b, 9).expect("register Controller B");
    assert_eq!(seyal_bridge_adopt_handle(handle_a), 0);
    assert_eq!(seyal_bridge_adopt_handle(handle_b), 0);
    ffi_test_claim_focused_display_if_unset(handle_a);

    let request_id = seyal_bridge_next_history_request_id_for(handle_b);
    assert_ne!(request_id, 0);
    assert_eq!(
        seyal_bridge_request_history_range_for(
            handle_b, block_id, start_line, end_line, 128, 131_072, 0,
        ),
        0
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let _ = seyal_bridge_poll_for(handle_b);
        let range = seyal_bridge_history_range_peek_for_handle(handle_b, block_id, request_id);
        if range.row_count > 0 {
            break;
        }
        assert!(Instant::now() < deadline, "Controller B history timed out");
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        seyal_bridge_history_range_peek_for_handle(handle_a, block_id, request_id).row_count,
        0,
        "focused Controller A must not receive B's same-ID history response"
    );
    let range = seyal_bridge_history_range_peek_for_handle(handle_b, block_id, request_id);
    let mut body = String::new();
    for index in 0..range.row_count {
        let row = seyal_bridge_history_range_row_for_handle(handle_b, block_id, request_id, index);
        // SAFETY: row cell pointers borrow B's still-registered history cache.
        for cell in unsafe { std::slice::from_raw_parts(row.cells, row.cell_count as usize) } {
            if let Some(scalar) = char::from_u32(cell.scalar) {
                body.push(scalar);
            }
        }
    }
    assert!(
        body.contains(TOKEN_B),
        "B's history should contain its token: {body:?}"
    );
    assert!(
        !body.contains(TOKEN_A),
        "B's history must not contain A's token: {body:?}"
    );

    seyal_bridge_disconnect_handle(handle_a);
    seyal_bridge_disconnect_handle(handle_b);
    runtime_a.finish();
    runtime_b.finish();
}
