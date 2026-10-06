//! Durable ActionIntent authority, EffectUnknown recovery, and
//! persistence-failure pause (ADR-014 / SPEC-016 §§3, 9–16, 21).
//!
//! Intent rows stay insert-only. Mutable lifecycle lives on additive columns
//! so canonical intent/digest never rewrite. Dispatch fencing / approval
//! consumption remain #1310 (fixtures seed Dispatching here). Schema stays v13:
//! pause health is process-local because a failing store cannot record it.

mod ops;
pub(crate) mod schema_v11;
pub(crate) mod schema_v13;

#[cfg(test)]
mod tests;

pub use ops::{ActionAuthority, ActionError, PersistResume, PersistedAction, PrepareOutcome};

use crate::sqlite::AgentStore;

impl AgentStore {
    pub fn actions(&self) -> ActionAuthority<'_> {
        ActionAuthority::new(self)
    }
}
