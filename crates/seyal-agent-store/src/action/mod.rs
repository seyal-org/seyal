//! Durable ActionIntent authority and EffectUnknown recovery
//! (ADR-014 / SPEC-016 §§3, 9–16).
//!
//! Intent rows stay insert-only. Mutable lifecycle lives on additive columns
//! so canonical intent/digest never rewrite. Dispatch fencing / approval
//! consumption remain #1310 (fixtures seed Dispatching here).

mod ops;
pub(crate) mod schema_v11;
pub(crate) mod schema_v13;

#[cfg(test)]
mod tests;

pub use ops::{ActionAuthority, ActionError, PersistedAction, PrepareOutcome};

use crate::sqlite::AgentStore;

impl AgentStore {
    pub fn actions(&self) -> ActionAuthority<'_> {
        ActionAuthority::new(self)
    }
}
