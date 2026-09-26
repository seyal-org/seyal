//! Disposable Seyal.app-side Candidate-D client, renderer-preparation owner,
//! portable headed product composition (Workspace/Tab/Pane), Flow/Raw/TUI
//! presentation fencing, reconnect/recovery policy, portable theme/config
//! resolution, and the one-Pane application root / versioned host API.
//!
//! Runtime/TerminalExecution remain the sole PTY, VT and canonical TerminalState
//! authority. This crate owns a local socket attachment, an atomically
//! committed `DisplayCache`, derived `seyal-render` presentation state, and the
//! host-facing product shell reducer. It is not a second Workspace database.

pub mod app;
pub mod chrome;
pub mod composer;
pub mod input_policy;
pub mod palette;
pub mod presentation;
pub mod recovery;
pub mod shell;
pub mod theme;

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod block_cache;

#[cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
mod v2_error;

// Keep the existing internal import path mechanically stable while severing the
// production dependency on the Runtime crate. `seyal_runtime` below is only an
// alias for the authority-neutral protocol/value crate; integration tests still
// use the real Runtime as a dev-dependency.
#[cfg(target_os = "macos")]
extern crate seyal_protocol as seyal_runtime;

#[cfg(target_os = "macos")]
mod local;
#[cfg(all(target_os = "macos", feature = "benchmark-instrumentation"))]
#[doc(hidden)]
pub mod pass7_benchmark;
#[cfg(feature = "benchmark-instrumentation")]
#[doc(hidden)]
pub mod pass8_benchmark;

#[cfg(target_os = "macos")]
pub use local::{
    cell_from_point, derive_grid_geometry, ClientError, DiscoveryFailure, GridGeometry,
    InputAdmissionFailure, LocalDisplayClient, ResizeFailure,
};

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod ffi;

#[cfg(target_os = "macos")]
#[doc(hidden)]
pub use ffi::{
    seyal_app_accessibility, seyal_app_apply, seyal_app_block_row, seyal_app_block_span,
    seyal_app_chrome, seyal_app_chrome_row, seyal_app_composer, seyal_app_copy, seyal_app_create,
    seyal_app_destroy, seyal_app_last_error, seyal_app_option_as_alt, seyal_app_palette,
    seyal_app_palette_row, seyal_app_recovery_param, seyal_app_shell, seyal_app_shell_row,
    seyal_app_snapshot, seyal_app_test_reload_ui_configuration, seyal_app_theme, seyal_app_visual,
    seyal_app_visual_warning, seyal_bridge_adopt_handle, seyal_bridge_disconnect_handle,
    seyal_bridge_ensure_prepared, seyal_bridge_frame, seyal_bridge_poll, seyal_bridge_select,
    seyal_bridge_set_runtime_dir, test_register_pending_client,
};

#[cfg(target_os = "macos")]
#[doc(hidden)]
pub use ffi::client_registry_contains as ffi_test_client_registry_contains;

#[cfg(target_os = "macos")]
#[doc(hidden)]
pub use ffi::client_registry_has_execution as ffi_test_client_registry_has_execution;
