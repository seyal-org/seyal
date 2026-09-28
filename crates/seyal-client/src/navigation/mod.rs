//! Local `ResourceAddress` and pure resolution over authoritative shell state.
//!
//! This module owns the M003 closed address set and the fail-closed resolver
//! (SPEC-022 §2–§3, ADR-019). It reads [`crate::shell::ShellState`] and a
//! borrowed execution inventory; it does not own workspaces, tabs, panes,
//! focus, history, FFI, or a second registry. Resolution never mutates state.

mod address;
mod resolve;

#[cfg(test)]
mod tests;

pub use address::{
    decode_resource_address, ResourceAddress, RESOURCE_ADDRESS_ABI_VERSION,
    RESOURCE_ADDRESS_KIND_EXECUTION, RESOURCE_ADDRESS_KIND_PANE, RESOURCE_ADDRESS_KIND_TAB,
    RESOURCE_ADDRESS_KIND_WORKSPACE,
};
pub use resolve::{
    resolve, EmptyExecutionInventory, ExecutionInventory, ExecutionPresence, NavigationPrincipal,
    NavigationRejection, ResolvedTarget, WorkspaceAccess,
};
