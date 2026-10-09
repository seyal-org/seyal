//! Local `ResourceAddress`, pure resolution, and atomic Navigate commit.
//!
//! This module owns the M003 closed address set, the fail-closed resolver, and
//! the atomic navigation commit (SPEC-022 §2–§5, ADR-019). It reads and, for
//! Navigate only, mutates focus on [`crate::shell::ShellState`] through a
//! single validated write. Placement looks up the Tab → Window map; `WindowId`
//! appears only there and on the `WindowActivation` effect. Resolution never
//! mutates state; Navigate mutates the focus triple and may queue activation.

mod activation;
mod address;
mod commit;
mod encode;
mod resolve;

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
pub use commit::{navigate, reveal_attention_target, AttentionReveal};
pub use encode::{
    encode_resource_address, pack_resource_address, unpack_resource_address,
    RESOURCE_ADDRESS_MAX_PAYLOAD,
};
pub use resolve::{
    resolve, EmptyExecutionInventory, ExecutionInventory, ExecutionPresence, NavigationPrincipal,
    NavigationRejection, ResolvedTarget, WorkspaceAccess,
};
