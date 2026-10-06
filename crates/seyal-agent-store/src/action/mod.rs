//! Durable ActionIntent authority (ADR-014 / SPEC-016 §3).
//!
//! Insert-only intent rows. Material change always mints a new ActionId.
//! Dispatch fencing and approval consumption are sibling Issues.

mod ops;
pub(crate) mod schema_v11;

#[cfg(test)]
mod tests;

pub use ops::{ActionAuthority, ActionError, PrepareOutcome};

use crate::sqlite::AgentStore;

impl AgentStore {
    pub fn actions(&self) -> ActionAuthority<'_> {
        ActionAuthority::new(self)
    }
}
