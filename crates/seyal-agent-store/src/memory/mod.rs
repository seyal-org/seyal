//! Durable MemoryStore authority (SPEC-012) plus working-set and revocation (SPEC-014/015).

mod codec;
mod ops;
mod persist;
pub(crate) mod schema_v10;
mod working_set_ops;

#[cfg(test)]
mod tests;

pub use ops::{MemoryAuthority, MemoryError, ProposeInput, ProposeResult, RevocationBundle};
pub use working_set_ops::WorkingSetError;

use crate::sqlite::AgentStore;

impl AgentStore {
    pub fn memory(&self) -> MemoryAuthority<'_> {
        MemoryAuthority::new(self)
    }
}
