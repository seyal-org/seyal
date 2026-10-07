//! Durable Attention / Artifact presentation store (SPEC-028 / #1306).

mod ops;
pub(crate) mod schema_v12;

#[cfg(test)]
mod tests;

pub use ops::{
    AttentionAuthority, AttentionError, AttentionProtocolView, MarkAllReadResult, MintTrustedInput,
    ProtocolClientKind,
};

use crate::sqlite::AgentStore;

impl AgentStore {
    pub fn attention(&self) -> AttentionAuthority<'_> {
        AttentionAuthority::new(self)
    }
}
