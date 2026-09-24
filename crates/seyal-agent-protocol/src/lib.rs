//! Versioned local Agent Backend protocol value boundary.
//!
//! AB-0.1 intentionally defines no transport or daemon behavior. This crate
//! carries provider-neutral wire-facing identities and protocol versioning only.

pub use seyal_agent_core::{
    AgentRunId, AttemptId, BackendInstanceId, BindingGeneration, ClientPrincipalId,
    ClientSessionId, ControlGeneration, WorkItemId, WorkScopeId,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProtocolVersion(u16);

impl ProtocolVersion {
    pub const V1: Self = Self(1);

    pub const fn get(self) -> u16 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_v1_is_explicit_and_stable() {
        assert_eq!(ProtocolVersion::V1.get(), 1);
    }
}
