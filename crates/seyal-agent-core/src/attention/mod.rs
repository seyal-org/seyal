//! Attention / Artifact presentation authority types (SPEC-028).
//!
//! Agent Backend owns durable AttentionItem state. UI/CLI project the same
//! authority. OSC/raw terminal text may create informational Attention only —
//! never privileged `ApprovalRequired`. ApprovalRequest/Decision recording is
//! owned by the #1308 sibling; this module exposes Attention lifecycle hooks.

mod lifecycle;
mod mint;
mod types;

#[cfg(test)]
mod tests;

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
