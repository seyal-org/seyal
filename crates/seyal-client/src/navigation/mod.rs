//! Local `ResourceAddress`, pure resolution, atomic Navigate commit, and focus
//! history.
//!
//! This module owns the M003 closed address set, the fail-closed resolver, the
//! atomic navigation commit (SPEC-022 §2–§5, ADR-019), and the application-
//! scoped focus history with Back/Forward (SPEC-022 §6). It reads and, for
//! Navigate / traversal only, mutates focus on [`crate::shell::ShellState`]
//! through a single validated write, and may emit one WindowActivation when
//! the target Tab lives in a non-active Window. It does not own workspaces,
//! tabs, panes, or a second registry. Resolution never mutates state; Navigate
//! mutates only the focus triple, at most one activation effect, and optionally
//! the history store.

mod address;
mod commit;
mod encode;
mod history;
mod resolve;

#[cfg(test)]
mod activation_tests;
#[cfg(test)]
mod history_integration_tests;
#[cfg(test)]
mod tests;

pub use address::{
    decode_resource_address, ResourceAddress, RESOURCE_ADDRESS_ABI_VERSION,
    RESOURCE_ADDRESS_KIND_EXECUTION, RESOURCE_ADDRESS_KIND_PANE, RESOURCE_ADDRESS_KIND_TAB,
    RESOURCE_ADDRESS_KIND_WORKSPACE,
};
pub use commit::{history_back, history_forward, navigate, NavigateHistory};
pub use encode::{encode_resource_address, RESOURCE_ADDRESS_MAX_PAYLOAD};
pub use history::{
    matches_destroyed_pane, matches_destroyed_tab, matches_destroyed_workspace, FocusHistory,
    FocusHistoryEntry, FocusSeq, FOCUS_HISTORY_CAPACITY,
};
pub use resolve::{
    resolve, EmptyExecutionInventory, ExecutionInventory, ExecutionPresence, NavigationPrincipal,
    NavigationRejection, ResolvedTarget, WorkspaceAccess,
};
