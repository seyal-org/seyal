//! Backend presence / capability enforcement projection (ADR-012 §12, SY-006).
//!
//! Owns the fail-closed claim gate used by session/API surfaces. Does not write
//! AgentRun transitions — [`ObservationAuthority`] / [`AgentDomain`] remain the
//! sole lifecycle writers (ADR-016).

use seyal_agent_core::{
    CapabilityId, ClaimMode, EnforcementClass, NegotiatedCapability, PresenceCapabilityProjection,
    PresenceError, PresenceObservation,
};
use seyal_agent_protocol::{
    decode_presence_capability_projection, encode_presence_capability_projection, FrameError,
};

/// Backend-owned projection of negotiated capabilities and presence evidence.
///
/// Clients may read this surface; they must not invent a second presence
/// authority in Swift/UI.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PresenceEnforcementPlane {
    projection: PresenceCapabilityProjection,
}

impl PresenceEnforcementPlane {
    pub fn new(capabilities: Vec<NegotiatedCapability>) -> Self {
        Self {
            projection: PresenceCapabilityProjection::new(capabilities),
        }
    }

    pub fn projection(&self) -> &PresenceCapabilityProjection {
        &self.projection
    }

    /// Replace the negotiated capability set (explicit classes only).
    pub fn set_capabilities(&mut self, capabilities: Vec<NegotiatedCapability>) {
        let presence = self.projection.presence;
        self.projection = PresenceCapabilityProjection::new(capabilities);
        self.projection.presence = presence;
    }

    pub fn record_presence(
        &mut self,
        observation: PresenceObservation,
    ) -> Result<(), PresenceError> {
        self.projection.record_presence(observation)
    }

    pub fn authorize_claim(
        &self,
        id: CapabilityId,
        mode: ClaimMode,
    ) -> Result<EnforcementClass, PresenceError> {
        self.projection.authorize_claim(id, mode)
    }

    /// Handshake / API encode of the current projection.
    pub fn encode_handshake_payload(&self) -> Result<Vec<u8>, FrameError> {
        encode_presence_capability_projection(&self.projection)
    }

    /// Handshake / API decode into this plane (fail-closed on illegal classes).
    pub fn apply_handshake_payload(&mut self, body: &[u8]) -> Result<(), FrameError> {
        self.projection = decode_presence_capability_projection(body)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use seyal_agent_core::BackendInstanceId;
    use seyal_agent_core::{
        classify_external_cli_effect_evidence, terminal_text_authorizes_approval, CapabilityId,
        EnforcementClass, PresenceSourceTier,
    };
    use seyal_agent_protocol::{negotiate_hello, Hello, ProtocolVersion};

    #[test]
    fn presence_enforcement_class_observed_no_local_claim() {
        let plane = PresenceEnforcementPlane::new(vec![NegotiatedCapability::supported(
            CapabilityId::Pause,
            EnforcementClass::Observed,
        )]);
        assert!(plane
            .authorize_claim(CapabilityId::Pause, ClaimMode::Observe)
            .is_ok());
        assert!(plane
            .authorize_claim(CapabilityId::Pause, ClaimMode::LocalEnforcement)
            .is_err());
    }

    #[test]
    fn presence_enforcement_class_upstream_requestable_not_enforced() {
        let plane = PresenceEnforcementPlane::new(vec![NegotiatedCapability::supported(
            CapabilityId::Deny,
            EnforcementClass::UpstreamRequestable,
        )]);
        assert!(plane
            .authorize_claim(CapabilityId::Deny, ClaimMode::UpstreamRequest)
            .is_ok());
        assert_eq!(
            plane.authorize_claim(CapabilityId::Deny, ClaimMode::LocalEnforcement),
            Err(PresenceError::EnforcementInsufficient {
                id: CapabilityId::Deny,
                have: EnforcementClass::UpstreamRequestable,
                mode: ClaimMode::LocalEnforcement,
            })
        );
    }

    #[test]
    fn presence_enforcement_class_backend_enforced_requires_typed_boundary() {
        let mut plane = PresenceEnforcementPlane::new(vec![NegotiatedCapability::supported(
            CapabilityId::Approve,
            EnforcementClass::BackendEnforced,
        )]);
        assert_eq!(
            PresenceObservation::new(
                PresenceSourceTier::StructuredAdapter,
                EnforcementClass::BackendEnforced,
                false,
            ),
            Err(PresenceError::BackendEnforcedRequiresTypedBoundary)
        );
        plane
            .record_presence(
                PresenceObservation::new(
                    PresenceSourceTier::StructuredAdapter,
                    EnforcementClass::BackendEnforced,
                    true,
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(
            plane.authorize_claim(CapabilityId::Approve, ClaimMode::LocalEnforcement),
            Ok(EnforcementClass::BackendEnforced)
        );
    }

    #[test]
    fn presence_sy006_heuristic_never_backend_enforced() {
        let mut plane = PresenceEnforcementPlane::new(vec![NegotiatedCapability::supported(
            CapabilityId::Account,
            EnforcementClass::BackendEnforced,
        )]);
        assert_eq!(
            PresenceObservation::new(
                PresenceSourceTier::LowConfidenceHeuristic,
                EnforcementClass::BackendEnforced,
                true,
            ),
            Err(PresenceError::IllegalEnforcementForSource {
                source: PresenceSourceTier::LowConfidenceHeuristic,
                requested: EnforcementClass::BackendEnforced,
                max: EnforcementClass::Observed,
            })
        );
        plane
            .record_presence(
                PresenceObservation::new(
                    PresenceSourceTier::ProcessShellSignals,
                    EnforcementClass::Observed,
                    false,
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(
            plane.authorize_claim(CapabilityId::Account, ClaimMode::LocalEnforcement),
            Err(PresenceError::TerminalTextNotAuthority)
        );
        assert_eq!(
            classify_external_cli_effect_evidence(
                PresenceSourceTier::LowConfidenceHeuristic,
                false
            ),
            Err(PresenceError::ExternalCliEffectNotBackendEnforced)
        );
    }

    #[test]
    fn presence_terminal_text_never_approval_authority() {
        assert_eq!(
            terminal_text_authorizes_approval("please approve this tool call"),
            Err(PresenceError::TerminalTextNotAuthority)
        );
    }

    #[test]
    fn capability_handshake_carries_enforcement_classes() {
        // Hello/HelloAck still negotiates transport; enforcement-qualified
        // adapter capabilities ride the dedicated projection payload that
        // clients/backends exchange alongside session attach.
        let hello = Hello {
            supported_versions: vec![ProtocolVersion::V1],
            max_frame_size: 4096,
            event_window: 32,
            client_principal_evidence: vec![1, 2, 3],
        };
        let ack = negotiate_hello(&hello, BackendInstanceId::new(), 4096, 64).unwrap();
        assert!(ack.capabilities.local_session);

        let plane = PresenceEnforcementPlane::new(vec![
            NegotiatedCapability::supported(CapabilityId::Pause, EnforcementClass::Observed),
            NegotiatedCapability::supported(
                CapabilityId::Approve,
                EnforcementClass::UpstreamRequestable,
            ),
            NegotiatedCapability::supported(
                CapabilityId::ModelSelect,
                EnforcementClass::BackendEnforced,
            ),
        ]);
        let payload = plane.encode_handshake_payload().unwrap();
        let mut peer = PresenceEnforcementPlane::new(Vec::new());
        peer.apply_handshake_payload(&payload).unwrap();
        assert_eq!(peer.projection().capabilities.len(), 3);
        assert!(peer
            .authorize_claim(CapabilityId::Pause, ClaimMode::LocalEnforcement)
            .is_err());
        assert!(peer
            .authorize_claim(CapabilityId::Approve, ClaimMode::UpstreamRequest)
            .is_ok());
        assert!(peer
            .authorize_claim(CapabilityId::ModelSelect, ClaimMode::LocalEnforcement)
            .is_ok());
    }

    #[test]
    fn production_binary_source_excludes_fake_execution_host() {
        // AC6 regression: production main must not compose FakeExecutionHost.
        let main_src = include_str!("main.rs");
        assert!(
            main_src.contains("StandaloneProcessHost"),
            "production entry must compose StandaloneProcessHost"
        );
        assert!(
            !main_src.contains("FakeExecutionHost::"),
            "production entry must not construct FakeExecutionHost"
        );
        assert!(
            main_src.contains("FakeExecutionHost"),
            "production entry must document FakeExecutionHost exclusion"
        );
    }
}
