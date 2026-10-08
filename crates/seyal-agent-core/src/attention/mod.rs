//! Attention / Artifact presentation authority types (SPEC-028).
//!
//! Agent Backend owns durable AttentionItem state. UI/CLI project the same
//! authority. OSC/raw terminal text may create informational Attention only —
//! never privileged `ApprovalRequired`. ApprovalRequest/Decision recording is
//! in [`approval`].

mod approval;
mod chrome;
mod lifecycle;
mod mint;
mod types;

#[cfg(test)]
mod tests;

pub use approval::{
    authorize_decide, auto_approve_reconciliation, evaluate_consume,
    request_from_untrusted_terminal, ApprovalDecision, ApprovalError, ApprovalRequest,
    ApprovalRequestSpec, ApprovalVerdict, ConsumptionWitness, ControlMode, DecisionAuthority,
};
pub use chrome::{
    activate, badges, in_stack_approve_allowed, next_attention, note_os_delivery_failure,
    notification_preview, os_banner_dismiss, preserve_navigation_order, stack_for_run,
    stack_overlay, AttentionActivation, AttentionBadge, OsBannerDismiss, OsDeliveryContext,
    OsDeliveryDecision, OsNotificationController, MAX_OS_DELIVERIES_PER_SOURCE,
    MAX_OS_DELIVERIES_PER_WINDOW, OS_RATE_WINDOW_MS,
};
pub use lifecycle::{allowed_attention_transition, AttentionTransitionError};
pub use mint::{
    coalesce_key, mint_from_trusted_source, mint_from_untrusted_terminal,
    reject_untrusted_privileged, MintError, MintSource, TrustedMintSpec,
    MAX_OPEN_ATTENTION_PER_RUN, MAX_TERMINAL_INFORMATIONAL_PER_WINDOW,
};
pub use types::{
    ArtifactKind, ArtifactRef, AttentionItem, AttentionKind, AttentionPriority, AttentionState,
    AttentionTarget, PresentationText, ATTENTION_SCHEMA_VERSION,
};
