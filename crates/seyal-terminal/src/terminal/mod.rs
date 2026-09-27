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

pub use resize::PreparedResize;
pub use state::{
    Diagnostics, ShellIntegrationEvent, ShellIntegrationToken, TerminalState, MAX_TERMINAL_COLUMNS,
    MAX_TERMINAL_ROWS,
};
