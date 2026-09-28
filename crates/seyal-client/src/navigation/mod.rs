//! Local `ResourceAddress`, pure resolution, and atomic Navigate commit.
//!
//! This module owns the M003 closed address set, the fail-closed resolver, and
//! the atomic navigation commit (SPEC-022 §2–§5, ADR-019). It reads and, for
//! Navigate only, mutates focus on [`crate::shell::ShellState`] through a
//! single validated write, and may emit one WindowActivation when the target
//! Tab lives in a non-active Window. It does not own workspaces, tabs, panes,
//! focus history, or a second registry. Resolution never mutates state;
//! Navigate mutates only the focus triple plus at most one activation effect.

mod address;
mod commit;
mod encode;
mod resolve;

#[cfg(test)]
mod activation_tests;
#[cfg(test)]
mod tests;

pub use address::{
    decode_resource_address, ResourceAddress, RESOURCE_ADDRESS_ABI_VERSION,
    RESOURCE_ADDRESS_KIND_EXECUTION, RESOURCE_ADDRESS_KIND_PANE, RESOURCE_ADDRESS_KIND_TAB,
    RESOURCE_ADDRESS_KIND_WORKSPACE,
};
pub use commit::navigate;
pub use encode::{encode_resource_address, RESOURCE_ADDRESS_MAX_PAYLOAD};
pub use resolve::{
    resolve, EmptyExecutionInventory, ExecutionInventory, ExecutionPresence, NavigationPrincipal,
    NavigationRejection, ResolvedTarget, WorkspaceAccess,
};
