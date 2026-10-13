//! Backend presence / capability enforcement projection (ADR-012 §12, SY-006).
//!
//! Owns the fail-closed claim gate used by session/API surfaces. Does not write
//! AgentRun transitions — [`ObservationAuthority`] / [`AgentDomain`] remain the
//! sole lifecycle writers (ADR-016).

use seyal_agent_core::{
    CapabilityId, CapabilityInstallTrust, ClaimMode, EnforcementClass, NegotiatedCapability,
    PresenceCapabilityProjection, PresenceError, PresenceObservation,
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
    ///
    /// This is a backend-owned trusted install path. Peer/wire blobs must use
    /// [`Self::apply_handshake_payload`] (untrusted) or
    /// [`Self::install_trusted_handshake_payload`] after adapter-policy binding.
    pub fn set_capabilities(
        &mut self,
        capabilities: Vec<NegotiatedCapability>,
    ) -> Result<(), PresenceError> {
        self.projection
            .install_capabilities(capabilities, CapabilityInstallTrust::BackendPolicyTrusted)
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

    /// Untrusted peer/wire handshake apply — rejects inbound `BackendEnforced`
    /// capabilities without adapter-policy trust binding (HIGH-2).
    pub fn apply_handshake_payload(&mut self, body: &[u8]) -> Result<(), FrameError> {
        self.apply_handshake_payload_with_trust(body, CapabilityInstallTrust::UntrustedPeer)
    }

    /// Trusted backend install after verified adapter-policy trust binding.
    pub fn install_trusted_handshake_payload(&mut self, body: &[u8]) -> Result<(), FrameError> {
        self.apply_handshake_payload_with_trust(body, CapabilityInstallTrust::BackendPolicyTrusted)
    }

    fn apply_handshake_payload_with_trust(
        &mut self,
        body: &[u8],
        trust: CapabilityInstallTrust,
    ) -> Result<(), FrameError> {
        let decoded = decode_presence_capability_projection(body)?;
        let presence = decoded.presence;
        self.projection
            .install_capabilities(decoded.capabilities, trust)
            .map_err(|err| match err {
                PresenceError::UntrustedBackendEnforcedCapability { .. }
                | PresenceError::DuplicateCapabilityId { .. } => FrameError::Malformed,
                _ => FrameError::Malformed,
            })?;
        if let Some(obs) = presence {
            self.projection
                .record_presence(obs)
                .map_err(|_| FrameError::Malformed)?;
        }
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

    fn structured_backend_enforced_presence() -> PresenceObservation {
        PresenceObservation::new(
            PresenceSourceTier::StructuredAdapter,
            EnforcementClass::BackendEnforced,
            true,
        )
        .unwrap()
    }

    fn official_hooks_upstream_presence() -> PresenceObservation {
        PresenceObservation::new(
            PresenceSourceTier::OfficialHooks,
            EnforcementClass::UpstreamRequestable,
            false,
        )
        .unwrap()
    }

    #[test]
    fn presence_enforcement_class_observed_no_local_claim() {
        let mut plane = PresenceEnforcementPlane::new(vec![NegotiatedCapability::supported(
            CapabilityId::Pause,
            EnforcementClass::Observed,
        )]);
        plane
            .record_presence(official_hooks_upstream_presence())
            .unwrap();
        assert!(plane
            .authorize_claim(CapabilityId::Pause, ClaimMode::Observe)
            .is_ok());
        assert!(plane
            .authorize_claim(CapabilityId::Pause, ClaimMode::LocalEnforcement)
            .is_err());
    }

    #[test]
    fn presence_enforcement_class_upstream_requestable_not_enforced() {
        let mut plane = PresenceEnforcementPlane::new(vec![NegotiatedCapability::supported(
            CapabilityId::Deny,
            EnforcementClass::UpstreamRequestable,
        )]);
        plane
            .record_presence(official_hooks_upstream_presence())
            .unwrap();
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
            .record_presence(structured_backend_enforced_presence())
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

        let mut plane = PresenceEnforcementPlane::new(vec![
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
        plane
            .record_presence(structured_backend_enforced_presence())
            .unwrap();
        let payload = plane.encode_handshake_payload().unwrap();

        // Untrusted peer apply must reject BackendEnforced caps (HIGH-2).
        let mut untrusted = PresenceEnforcementPlane::new(Vec::new());
        assert_eq!(
            untrusted.apply_handshake_payload(&payload),
            Err(FrameError::Malformed)
        );

        // Trusted backend install after adapter-policy binding accepts them.
        let mut trusted = PresenceEnforcementPlane::new(Vec::new());
        trusted.install_trusted_handshake_payload(&payload).unwrap();
        assert_eq!(trusted.projection().capabilities.len(), 3);
        assert!(trusted
            .authorize_claim(CapabilityId::Pause, ClaimMode::LocalEnforcement)
            .is_err());
        assert!(trusted
            .authorize_claim(CapabilityId::Approve, ClaimMode::UpstreamRequest)
            .is_ok());
        assert!(trusted
            .authorize_claim(CapabilityId::ModelSelect, ClaimMode::LocalEnforcement)
            .is_ok());
    }

    #[test]
    fn presence_authorize_claim_official_hooks_caps_backend_enforced_no_local() {
        let mut plane = PresenceEnforcementPlane::new(vec![NegotiatedCapability::supported(
            CapabilityId::Approve,
            EnforcementClass::BackendEnforced,
        )]);
        plane
            .record_presence(official_hooks_upstream_presence())
            .unwrap();
        assert_eq!(
            plane.authorize_claim(CapabilityId::Approve, ClaimMode::LocalEnforcement),
            Err(PresenceError::EnforcementInsufficient {
                id: CapabilityId::Approve,
                have: EnforcementClass::UpstreamRequestable,
                mode: ClaimMode::LocalEnforcement,
            })
        );
        assert_eq!(
            plane.authorize_claim(CapabilityId::Approve, ClaimMode::UpstreamRequest),
            Ok(EnforcementClass::UpstreamRequestable)
        );
    }

    #[test]
    fn presence_authorize_claim_structured_adapter_observed_no_local() {
        let mut plane = PresenceEnforcementPlane::new(vec![NegotiatedCapability::supported(
            CapabilityId::Approve,
            EnforcementClass::BackendEnforced,
        )]);
        plane
            .record_presence(
                PresenceObservation::new(
                    PresenceSourceTier::StructuredAdapter,
                    EnforcementClass::Observed,
                    false,
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(
            plane.authorize_claim(CapabilityId::Approve, ClaimMode::LocalEnforcement),
            Err(PresenceError::EnforcementInsufficient {
                id: CapabilityId::Approve,
                have: EnforcementClass::Observed,
                mode: ClaimMode::LocalEnforcement,
            })
        );
    }

    #[test]
    fn presence_authorize_claim_requires_typed_boundary_for_local() {
        // Defense in depth: forged BackendEnforced without typed boundary must
        // still fail closed at authorize_claim (core gate; plane delegates).
        let mut projection =
            PresenceCapabilityProjection::new(vec![NegotiatedCapability::supported(
                CapabilityId::ModelSelect,
                EnforcementClass::BackendEnforced,
            )]);
        projection.presence = Some(PresenceObservation {
            source: PresenceSourceTier::StructuredAdapter,
            enforcement: EnforcementClass::BackendEnforced,
            typed_backend_boundary: false,
        });
        assert_eq!(
            projection.authorize_claim(CapabilityId::ModelSelect, ClaimMode::LocalEnforcement),
            Err(PresenceError::BackendEnforcedRequiresTypedBoundary)
        );

        let mut plane = PresenceEnforcementPlane::new(vec![NegotiatedCapability::supported(
            CapabilityId::ModelSelect,
            EnforcementClass::BackendEnforced,
        )]);
        plane
            .record_presence(structured_backend_enforced_presence())
            .unwrap();
        assert_eq!(
            plane.authorize_claim(CapabilityId::ModelSelect, ClaimMode::LocalEnforcement),
            Ok(EnforcementClass::BackendEnforced)
        );
    }

    #[test]
    fn presence_untrusted_handshake_rejects_backend_enforced_caps() {
        let plane = PresenceEnforcementPlane::new(vec![NegotiatedCapability::supported(
            CapabilityId::Approve,
            EnforcementClass::BackendEnforced,
        )]);
        let payload = plane.encode_handshake_payload().unwrap();
        let mut peer = PresenceEnforcementPlane::new(Vec::new());
        assert_eq!(
            peer.apply_handshake_payload(&payload),
            Err(FrameError::Malformed)
        );
    }

    #[test]
    fn presence_trusted_install_allows_backend_enforced_after_policy_binding() {
        let mut source = PresenceEnforcementPlane::new(vec![NegotiatedCapability::supported(
            CapabilityId::Approve,
            EnforcementClass::BackendEnforced,
        )]);
        source
            .record_presence(structured_backend_enforced_presence())
            .unwrap();
        let payload = source.encode_handshake_payload().unwrap();
        let mut plane = PresenceEnforcementPlane::new(Vec::new());
        plane.install_trusted_handshake_payload(&payload).unwrap();
        assert_eq!(
            plane.authorize_claim(CapabilityId::Approve, ClaimMode::LocalEnforcement),
            Ok(EnforcementClass::BackendEnforced)
        );
    }

    #[test]
    fn presence_none_fails_closed_for_privileged_claim_modes() {
        let plane = PresenceEnforcementPlane::new(vec![NegotiatedCapability::supported(
            CapabilityId::Approve,
            EnforcementClass::BackendEnforced,
        )]);
        assert!(plane
            .authorize_claim(CapabilityId::Approve, ClaimMode::Observe)
            .is_ok());
        assert_eq!(
            plane.authorize_claim(CapabilityId::Approve, ClaimMode::UpstreamRequest),
            Err(PresenceError::PresenceEvidenceRequired {
                mode: ClaimMode::UpstreamRequest,
            })
        );
        assert_eq!(
            plane.authorize_claim(CapabilityId::Approve, ClaimMode::LocalEnforcement),
            Err(PresenceError::PresenceEvidenceRequired {
                mode: ClaimMode::LocalEnforcement,
            })
        );
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
