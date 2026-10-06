//! Durable ActionId / ActionIntent identity (ADR-014 / SPEC-016 §3).
//!
//! This module owns preparation-time identity only. Dispatch fencing, approval
//! consumption, EffectUnknown reconciliation, and persistence-failure pause
//! belong to sibling #841 children.

mod intent;

pub use intent::{
    action_intent_digest, material_fields_changed, ActionIntent, ActionIntentError,
    ActionLifecycle, ArgumentFingerprint, AuthorizationClass, CapabilityRef, EffectClass,
    ExecutorCapabilityRef, PrivacyDependencyId, RequestProvenance, ResourceIdentity,
};

#[cfg(test)]
mod tests;
