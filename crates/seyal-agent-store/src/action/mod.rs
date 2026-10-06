//! Durable ActionIntent authority, dispatch fencing, EffectUnknown recovery,
//! and persistence-failure pause (ADR-014 / SPEC-016).
//!
//! Intent rows stay insert-only. Mutable lifecycle lives on additive columns
//! so canonical intent/digest never rewrite. Schema stays v14.

mod dispatch;
mod ops;
pub(crate) mod schema_v11;
pub(crate) mod schema_v13;

#[cfg(test)]
mod dispatch_tests;
#[cfg(test)]
mod tests;

pub use dispatch::{DispatchInput, DispatchOutcome};
pub use ops::{ActionAuthority, ActionError, PersistResume, PersistedAction, PrepareOutcome};

use crate::sqlite::AgentStore;

impl AgentStore {
    pub fn actions(&self) -> ActionAuthority<'_> {
        ActionAuthority::new(self)
    }
}
