//! Local `ResourceAddress`, pure resolution, atomic Navigate commit, and focus
//! history.
//!
//! This module owns the M003 closed address set, fail-closed resolver, atomic
//! navigation commit (SPEC-022 §2–§5, ADR-019), and application-scoped focus
//! history with Back/Forward (SPEC-022 §6). Navigate and traversal mutate focus
//! through a single validated write and may queue a native window activation
//! effect for the Tab's owning Window. The module does not own workspaces, tabs,
//! panes, window placement, or a second registry. Resolution never mutates state.

mod activation;
mod address;
mod commit;
mod encode;
mod history;
mod resolve;

#[cfg(test)]
mod history_integration_tests;
#[cfg(test)]
mod reveal_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod window_activation_tests;

pub use activation::{may_retry_activation, WINDOW_ACTIVATION_MAX_ATTEMPTS};
pub use address::{
    decode_resource_address, ResourceAddress, RESOURCE_ADDRESS_ABI_VERSION,
    RESOURCE_ADDRESS_KIND_EXECUTION, RESOURCE_ADDRESS_KIND_PANE, RESOURCE_ADDRESS_KIND_TAB,
    RESOURCE_ADDRESS_KIND_WORKSPACE,
};
pub use commit::{
    history_back, history_forward, navigate, reveal_attention_target, AttentionReveal,
    NavigateHistory,
};
pub use encode::{
    encode_resource_address, pack_resource_address, unpack_resource_address,
    RESOURCE_ADDRESS_MAX_PAYLOAD,
};
pub use history::{
    matches_destroyed_pane, matches_destroyed_tab, FocusHistory, FocusHistoryEntry, FocusSeq,
    FOCUS_HISTORY_CAPACITY,
};
pub use resolve::{
    resolve, EmptyExecutionInventory, ExecutionInventory, ExecutionPresence, NavigationPrincipal,
    NavigationRejection, ResolvedTarget, WorkspaceAccess,
};
