//! Durable ActionId / ActionIntent identity, EffectUnknown recovery, and
//! persistence-failure pause (ADR-014 / SPEC-016 §§3–4, 9–16, 21–22).
//!
//! Dispatch fencing and approval consumption belong to #1310. Mutable
//! lifecycle is stored beside immutable intent bytes so the identity digest
//! stays frozen.

mod intent;
mod persist_pause;
mod recovery;

pub use intent::{
    action_intent_digest, material_fields_changed, ActionIntent, ActionIntentError,
    ActionLifecycle, ArgumentFingerprint, AuthorizationClass, CapabilityRef, EffectClass,
    ExecutorCapabilityRef, PrivacyDependencyId, RequestProvenance, ResourceIdentity,
};
pub use persist_pause::{
    resume_crash_boundary, PersistAdmitError, PersistFailurePolicy, PersistHealth,
    PersistPauseReason, DEFAULT_PERSIST_FAILURE_BUDGET, DEFAULT_PERSIST_RETRY_DEADLINE_MS,
};
pub use recovery::{
    linearize_cancel, reconcile, recover, ActionRuntime, CausalMarker, CausalMarkerKind,
    CrashBoundary, EvidenceKind, ReconciliationRequiredHook, RecoveryDecision, RecoveryError,
    RecoveryEvidence, DEFAULT_AUTOMATIC_RECONCILIATION_BUDGET,
};

#[cfg(test)]
mod tests;
