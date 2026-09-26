//! Seyal.app ↔ `seyal-client` C ABI bridge.
//!
//! # Panic / unwind policy
//!
//! Every `extern "C"` entry point in this module is a C ABI surface consumed by
//! Swift. Unwinding across that boundary is undefined behavior. The workspace
//! `dev`/`release` profiles set `panic = "abort"`, so a Rust panic terminates
//! the process instead of unwinding into Swift. Callers must treat abort as the
//! only defined panic outcome; there is no catch-and-continue path across FFI.
//!
//! # Borrow lifetimes
//!
//! Pointer-carrying returns (`seyal_bridge_frame`, history rows, block command
//! bytes) borrow executor-local Rust storage. They are valid only until the next
//! mutating bridge call that can replace that storage (poll, disconnect, or a
//! later history consume). Swift must copy synchronously before returning to the
//! run loop; `NativePreparedFrame` owns a cell copy at construction so Rust
//! `PreparedCell` pointers never escape into long-lived Swift state.

mod app;
mod display;
mod errors;
mod input;
mod session;
mod types;

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    },
};

use crate::LocalDisplayClient;

pub(crate) use types::{
    SeyalBlockRecord, SeyalComposerResult, SeyalComposerStatus, SeyalCopiedText,
    SeyalExecutionBlockMetadata, SeyalHistoryCell, SeyalHistoryRange, SeyalHistoryRow,
    SeyalHistorySidecar, SeyalPass9DiagSnapshot, SeyalPreparedFrame, SeyalRecoveryResult,
};

#[allow(unused_imports)]
pub use app::{
    seyal_app_accessibility, seyal_app_apply, seyal_app_block_action_count,
    seyal_app_block_action_row, seyal_app_block_row, seyal_app_block_span, seyal_app_chrome,
    seyal_app_chrome_row, seyal_app_composer, seyal_app_copy, seyal_app_create, seyal_app_destroy,
    seyal_app_last_error, seyal_app_option_as_alt, seyal_app_palette, seyal_app_palette_row,
    seyal_app_recovery_param, seyal_app_request_block_copy, seyal_app_shell, seyal_app_shell_row,
    seyal_app_snapshot, seyal_app_test_reload_ui_configuration, seyal_app_theme, seyal_app_visual,
    seyal_app_visual_warning,
};
#[allow(unused_imports)]
pub use display::{
    seyal_bridge_block_count, seyal_bridge_block_record, seyal_bridge_block_timeline_revision,
    seyal_bridge_composer_result, seyal_bridge_ensure_prepared,
    seyal_bridge_execution_block_metadata, seyal_bridge_flush_writable, seyal_bridge_frame,
    seyal_bridge_history_range_consume, seyal_bridge_history_range_peek_for,
    seyal_bridge_history_range_row_for, seyal_bridge_history_range_sidecar_for,
    seyal_bridge_history_range_text_for, seyal_bridge_next_composer_request_id,
    seyal_bridge_next_history_request_id, seyal_bridge_poll, seyal_bridge_request_history_range,
    seyal_bridge_take_block_copy, seyal_bridge_wants_write,
};
pub(crate) use errors::error_code;
#[allow(unused_imports)]
pub use errors::{
    seyal_bridge_input_failure, seyal_bridge_last_recovery_result,
    seyal_bridge_pass9_diag_snapshot, seyal_bridge_resize_failure,
};
#[allow(unused_imports)]
pub use input::{
    seyal_bridge_copied_text, seyal_bridge_copied_text_consume, seyal_bridge_mouse_cell,
    seyal_bridge_propose_geometry, seyal_bridge_retry_resize, seyal_bridge_submit_composer,
    seyal_bridge_submit_host_search, seyal_bridge_submit_host_selection, seyal_bridge_submit_key,
    seyal_bridge_submit_mouse, seyal_bridge_submit_paste, seyal_bridge_submit_utf8,
};
#[allow(unused_imports)]
pub use session::{
    seyal_bridge_adopt_handle, seyal_bridge_attachment_id_high, seyal_bridge_attachment_id_low,
    seyal_bridge_connect_first, seyal_bridge_disconnect, seyal_bridge_disconnect_handle,
    seyal_bridge_execution_id_high, seyal_bridge_execution_id_low, seyal_bridge_open_execution,
    seyal_bridge_open_execution_until, seyal_bridge_open_first,
    seyal_bridge_open_first_observer_until, seyal_bridge_open_first_until,
    seyal_bridge_runtime_id_high, seyal_bridge_runtime_id_low, seyal_bridge_select,
    seyal_bridge_set_runtime_dir, seyal_bridge_socket_fd, test_register_pending_client,
};

/// A completed lifecycle connection crosses executors exactly once, before it
/// becomes a Pane's nonblocking event-loop client. The mutex is never used by
/// poll, input, resize or rendering: those paths use the executor-local map.
pub(crate) struct PendingClient {
    pub(crate) client: Box<LocalDisplayClient>,
    pub(crate) origin: u8,
}

pub(crate) static PENDING_CLIENTS: OnceLock<Mutex<HashMap<u64, PendingClient>>> = OnceLock::new();
pub(crate) static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);
pub(crate) const DEFAULT_RECOVERY_BUDGET_MICROS: u64 = 1_000_000;

thread_local! {
    // A Pane's live Runtime client belongs to its AppKit executor. It is never
    // shared with the lifecycle queue after adoption, keeping all steady-state
    // terminal calls free of cross-pane locks.
    pub(crate) static CLIENTS: RefCell<HashMap<u64, Box<LocalDisplayClient>>> = RefCell::new(HashMap::new());
    pub(crate) static ACTIVE_HANDLE: Cell<u64> = const { Cell::new(0) };
    pub(crate) static LAST_RECOVERY_RESULT: Cell<SeyalRecoveryResult> = const { Cell::new(SeyalRecoveryResult::empty()) };
}

pub(crate) fn pending_clients() -> &'static Mutex<HashMap<u64, PendingClient>> {
    PENDING_CLIENTS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn identity_words(value: u128) -> (u64, u64) {
    let bytes = value.to_le_bytes();
    (
        u64::from_le_bytes(bytes[..8].try_into().unwrap()),
        u64::from_le_bytes(bytes[8..].try_into().unwrap()),
    )
}

pub(crate) fn allocate_handle() -> u64 {
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    if handle == 0 {
        1
    } else {
        handle
    }
}

pub(crate) fn active_handle() -> u64 {
    ACTIVE_HANDLE.with(Cell::get)
}

pub(crate) fn with_active_client<R>(operation: impl FnOnce(&LocalDisplayClient) -> R) -> Option<R> {
    let handle = active_handle();
    CLIENTS.with(|clients| {
        clients
            .borrow()
            .get(&handle)
            .map(|client| operation(client))
    })
}

pub(crate) fn with_active_client_mut<R>(
    operation: impl FnOnce(&mut LocalDisplayClient) -> R,
) -> Option<R> {
    let handle = active_handle();
    CLIENTS.with(|clients| {
        clients
            .borrow_mut()
            .get_mut(&handle)
            .map(|client| operation(client))
    })
}

#[cfg(test)]
mod adversarial_ffi_misuse_tests {
    use std::{
        ptr,
        sync::{Arc, Barrier},
        thread,
    };

    use super::{
        seyal_bridge_adopt_handle, seyal_bridge_disconnect_handle, seyal_bridge_frame,
        seyal_bridge_poll, seyal_bridge_select, seyal_bridge_set_runtime_dir,
        seyal_bridge_submit_composer, seyal_bridge_submit_paste, seyal_bridge_submit_utf8,
        SeyalPreparedFrame,
    };

    #[test]
    fn submit_utf8_rejects_null_with_nonzero_len() {
        let code = unsafe { seyal_bridge_submit_utf8(ptr::null(), 4) };
        assert_eq!(code, -4);
    }

    #[test]
    fn submit_utf8_accepts_empty_without_pointer() {
        let code = unsafe { seyal_bridge_submit_utf8(ptr::null(), 0) };
        assert_eq!(code, 0);
    }

    #[test]
    fn submit_paste_rejects_null_or_empty() {
        assert_eq!(unsafe { seyal_bridge_submit_paste(ptr::null(), 0) }, -4);
        assert_eq!(unsafe { seyal_bridge_submit_paste(ptr::null(), 3) }, -4);
    }

    #[test]
    fn submit_composer_rejects_null_or_empty() {
        assert_eq!(unsafe { seyal_bridge_submit_composer(ptr::null(), 0) }, -4);
        assert_eq!(unsafe { seyal_bridge_submit_composer(ptr::null(), 3) }, -4);
    }

    #[test]
    fn frame_without_active_client_is_empty() {
        seyal_bridge_disconnect_handle(u64::MAX);
        let frame = seyal_bridge_frame();
        assert!(frame.cells.is_null());
        assert_eq!(frame.cell_count, 0);
        assert_eq!(SeyalPreparedFrame::empty().cell_count, 0);
    }

    #[test]
    fn poll_without_active_client_fails_closed() {
        seyal_bridge_disconnect_handle(u64::MAX);
        assert_eq!(seyal_bridge_poll(), -1);
    }

    #[test]
    fn select_unknown_handle_fails_closed() {
        assert_eq!(seyal_bridge_select(0), -1);
        assert_eq!(seyal_bridge_select(u64::MAX - 1), -1);
    }

    #[test]
    fn adopt_missing_handle_fails_closed() {
        assert_eq!(seyal_bridge_adopt_handle(0), -1);
        assert_eq!(seyal_bridge_adopt_handle(u64::MAX), -1);
    }

    #[test]
    fn double_adopt_of_absent_handle_stays_fail_closed() {
        // Absent-handle branch only. Live double-adopt after a successful first
        // adopt is covered by `tests/ffi_misuse_macos.rs`.
        let handle = 0x0ff1_ceda_u64;
        assert_eq!(seyal_bridge_adopt_handle(handle), -1);
        assert_eq!(seyal_bridge_adopt_handle(handle), -1);
    }

    #[test]
    fn wrong_thread_cannot_select_unadopted_handle() {
        // Absent-handle / unadopted branch only. Cross-thread select after a
        // successful adopt is covered by `tests/ffi_misuse_macos.rs`.
        let barrier = Arc::new(Barrier::new(2));
        let handle = 0x7ead_u64;
        let barrier_thread = Arc::clone(&barrier);
        let worker = thread::spawn(move || {
            assert_eq!(seyal_bridge_adopt_handle(handle), -1);
            barrier_thread.wait();
            assert_eq!(seyal_bridge_select(handle), -1);
        });
        barrier.wait();
        assert_eq!(seyal_bridge_select(handle), -1);
        worker.join().expect("worker");
    }

    #[test]
    fn use_after_poll_without_client_keeps_frame_empty() {
        // No-client fail-closed branch only. Live poll → disconnect invalidation
        // is covered by `tests/ffi_misuse_macos.rs`.
        assert_eq!(seyal_bridge_poll(), -1);
        let frame = seyal_bridge_frame();
        assert!(frame.cells.is_null());
        assert_eq!(seyal_bridge_poll(), -1);
        let after = seyal_bridge_frame();
        assert!(after.cells.is_null());
        assert_eq!(after.cell_count, 0);
    }

    #[test]
    fn set_runtime_dir_rejects_null_and_relative_paths() {
        let _lock = seyal_runtime::runtime_dir::override_test_lock();
        seyal_runtime::runtime_dir::reset_explicit_runtime_dir();
        assert_eq!(unsafe { seyal_bridge_set_runtime_dir(ptr::null()) }, -2);
        let relative = std::ffi::CString::new("relative").unwrap();
        assert_eq!(
            unsafe { seyal_bridge_set_runtime_dir(relative.as_ptr()) },
            -2
        );
        let absolute = std::ffi::CString::new("/tmp/seyal-860-ffi").unwrap();
        assert_eq!(
            unsafe { seyal_bridge_set_runtime_dir(absolute.as_ptr()) },
            0
        );
        seyal_runtime::runtime_dir::reset_explicit_runtime_dir();
    }
}
