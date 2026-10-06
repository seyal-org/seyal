//! Durable ActionId / ActionIntent identity and EffectUnknown recovery
//! (ADR-014 / SPEC-016 §§3–4, 9–16).
//!
//! Dispatch fencing and approval consumption belong to #1310. Persistence-failure
//! pause policy belongs to #1312. Mutable lifecycle is stored beside immutable
//! intent bytes so the identity digest stays frozen.

mod intent;
mod recovery;

pub use intent::{
    action_intent_digest, material_fields_changed, ActionIntent, ActionIntentError,
    ActionLifecycle, ArgumentFingerprint, AuthorizationClass, CapabilityRef, EffectClass,
    ExecutorCapabilityRef, PrivacyDependencyId, RequestProvenance, ResourceIdentity,
};
pub use recovery::{
    linearize_cancel, reconcile, recover, ActionRuntime, CausalMarker, CausalMarkerKind,
    CrashBoundary, EvidenceKind, ReconciliationRequiredHook, RecoveryDecision, RecoveryError,
    RecoveryEvidence, DEFAULT_AUTOMATIC_RECONCILIATION_BUDGET,
};

#[cfg(test)]
mod tests;
