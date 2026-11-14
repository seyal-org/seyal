//! Canonical terminal state authority.
//!
//! `TerminalState` is the sole public façade. Implementation is partitioned by
//! responsibility across sibling modules; there is still exactly one
//! `TerminalState` / `TerminalCore` authority.

mod history_api;
mod host_selection;
mod resize;
mod state;
mod vt;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_attribution_yield;
#[cfg(test)]
mod tests_native_blocks;

pub use resize::PreparedResize;
pub use state::{
    Diagnostics, SessionShellKind, ShellIntegrationEvent, ShellIntegrationToken, TerminalState,
    MAX_TERMINAL_COLUMNS, MAX_TERMINAL_ROWS,
};
