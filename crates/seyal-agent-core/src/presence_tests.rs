use super::*;

#[test]
fn enforcement_class_round_trip_codes() {
    for class in [
        EnforcementClass::Observed,
        EnforcementClass::UpstreamRequestable,
        EnforcementClass::BackendEnforced,
    ] {
        assert_eq!(EnforcementClass::from_u8(class.as_u8()), Some(class));
        assert!(!class.as_str().is_empty());
    }
    assert_eq!(EnforcementClass::from_u8(0), None);
    assert_eq!(EnforcementClass::from_u8(4), None);
    assert_eq!(
        EnforcementClass::BackendEnforced.as_str(),
        "BackendEnforced"
    );
}

#[test]
fn presence_source_round_trip_codes() {
    for tier in [
        PresenceSourceTier::StructuredAdapter,
        PresenceSourceTier::OfficialHooks,
        PresenceSourceTier::ProcessShellSignals,
        PresenceSourceTier::LowConfidenceHeuristic,
    ] {
        assert_eq!(PresenceSourceTier::from_u8(tier.as_u8()), Some(tier));
    }
    assert_eq!(PresenceSourceTier::from_u8(0), None);
}

#[test]
fn capability_support_round_trip_codes() {
    for support in [
        CapabilitySupport::Unsupported,
        CapabilitySupport::Unknown,
        CapabilitySupport::Supported {
            enforcement: EnforcementClass::Observed,
        },
        CapabilitySupport::Supported {
            enforcement: EnforcementClass::UpstreamRequestable,
        },
        CapabilitySupport::Supported {
            enforcement: EnforcementClass::BackendEnforced,
        },
    ] {
        assert_eq!(CapabilitySupport::from_u8(support.as_u8()), Some(support));
    }
}

fn official_hooks_presence() -> PresenceObservation {
    PresenceObservation::new(
        PresenceSourceTier::OfficialHooks,
        EnforcementClass::UpstreamRequestable,
        false,
    )
    .unwrap()
}

fn structured_backend_enforced_presence() -> PresenceObservation {
    PresenceObservation::new(
        PresenceSourceTier::StructuredAdapter,
        EnforcementClass::BackendEnforced,
        true,
    )
    .unwrap()
}

#[test]
fn presence_enforcement_class_observed_no_local_claim() {
    let mut projection = PresenceCapabilityProjection::new(vec![NegotiatedCapability::supported(
        CapabilityId::Pause,
        EnforcementClass::Observed,
    )]);
    // Observe may use caps alone.
    assert!(projection
        .authorize_claim(CapabilityId::Pause, ClaimMode::Observe)
        .is_ok());
    // Privileged modes require presence (MEDIUM-1) and still fail on class.
    assert_eq!(
        projection.authorize_claim(CapabilityId::Pause, ClaimMode::UpstreamRequest),
        Err(PresenceError::PresenceEvidenceRequired {
            mode: ClaimMode::UpstreamRequest,
        })
    );
    projection
        .record_presence(official_hooks_presence())
        .unwrap();
    assert_eq!(
        projection.authorize_claim(CapabilityId::Pause, ClaimMode::UpstreamRequest),
        Err(PresenceError::EnforcementInsufficient {
            id: CapabilityId::Pause,
            have: EnforcementClass::Observed,
            mode: ClaimMode::UpstreamRequest,
        })
    );
    assert_eq!(
        projection.authorize_claim(CapabilityId::Pause, ClaimMode::LocalEnforcement),
        Err(PresenceError::EnforcementInsufficient {
            id: CapabilityId::Pause,
            have: EnforcementClass::Observed,
            mode: ClaimMode::LocalEnforcement,
        })
    );
    // Unsupported stays explicit.
    assert_eq!(
        projection.authorize_claim(CapabilityId::Approve, ClaimMode::LocalEnforcement),
        Err(PresenceError::CapabilityNotSupported {
            id: CapabilityId::Approve,
        })
    );
}

#[test]
fn presence_enforcement_class_upstream_requestable_not_enforced() {
    let mut projection = PresenceCapabilityProjection::new(vec![NegotiatedCapability::supported(
        CapabilityId::Approve,
        EnforcementClass::UpstreamRequestable,
    )]);
    projection
        .record_presence(official_hooks_presence())
        .unwrap();
    assert!(projection
        .authorize_claim(CapabilityId::Approve, ClaimMode::UpstreamRequest)
        .is_ok());
    assert_eq!(
        projection.authorize_claim(CapabilityId::Approve, ClaimMode::LocalEnforcement),
        Err(PresenceError::EnforcementInsufficient {
            id: CapabilityId::Approve,
            have: EnforcementClass::UpstreamRequestable,
            mode: ClaimMode::LocalEnforcement,
        })
    );
}

#[test]
fn presence_enforcement_class_backend_enforced_requires_typed_boundary() {
    assert_eq!(
        PresenceObservation::new(
            PresenceSourceTier::StructuredAdapter,
            EnforcementClass::BackendEnforced,
            false,
        ),
        Err(PresenceError::BackendEnforcedRequiresTypedBoundary)
    );
    let ok = PresenceObservation::new(
        PresenceSourceTier::StructuredAdapter,
        EnforcementClass::BackendEnforced,
        true,
    )
    .expect("typed boundary permits BackendEnforced");
    assert_eq!(ok.enforcement, EnforcementClass::BackendEnforced);

    let mut projection = PresenceCapabilityProjection::new(vec![NegotiatedCapability::supported(
        CapabilityId::ModelSelect,
        EnforcementClass::BackendEnforced,
    )]);
    // Without presence, privileged LocalEnforcement fails closed.
    assert_eq!(
        projection.authorize_claim(CapabilityId::ModelSelect, ClaimMode::LocalEnforcement),
        Err(PresenceError::PresenceEvidenceRequired {
            mode: ClaimMode::LocalEnforcement,
        })
    );
    projection
        .record_presence(structured_backend_enforced_presence())
        .unwrap();
    assert_eq!(
        projection.authorize_claim(CapabilityId::ModelSelect, ClaimMode::LocalEnforcement),
        Ok(EnforcementClass::BackendEnforced)
    );
}

#[test]
fn presence_sy006_heuristic_never_backend_enforced() {
    for source in [
        PresenceSourceTier::LowConfidenceHeuristic,
        PresenceSourceTier::ProcessShellSignals,
    ] {
        assert_eq!(
            PresenceObservation::new(source, EnforcementClass::BackendEnforced, true),
            Err(PresenceError::IllegalEnforcementForSource {
                source,
                requested: EnforcementClass::BackendEnforced,
                max: EnforcementClass::Observed,
            })
        );
        assert_eq!(
            PresenceObservation::new(source, EnforcementClass::UpstreamRequestable, false),
            Err(PresenceError::IllegalEnforcementForSource {
                source,
                requested: EnforcementClass::UpstreamRequestable,
                max: EnforcementClass::Observed,
            })
        );
        assert!(PresenceObservation::new(source, EnforcementClass::Observed, false).is_ok());
    }

    // Official hooks cannot self-upgrade to BackendEnforced.
    assert_eq!(
        PresenceObservation::new(
            PresenceSourceTier::OfficialHooks,
            EnforcementClass::BackendEnforced,
            true,
        ),
        Err(PresenceError::IllegalEnforcementForSource {
            source: PresenceSourceTier::OfficialHooks,
            requested: EnforcementClass::BackendEnforced,
            max: EnforcementClass::UpstreamRequestable,
        })
    );
}

#[test]
fn presence_terminal_text_never_approval_authority() {
    assert_eq!(
        terminal_text_authorizes_approval("Approve? [y/N]"),
        Err(PresenceError::TerminalTextNotAuthority)
    );
    assert_eq!(
        classify_external_cli_effect_evidence(PresenceSourceTier::LowConfidenceHeuristic, false,),
        Err(PresenceError::ExternalCliEffectNotBackendEnforced)
    );
    assert_eq!(
        classify_external_cli_effect_evidence(PresenceSourceTier::ProcessShellSignals, false,),
        Err(PresenceError::ExternalCliEffectNotBackendEnforced)
    );

    let mut projection = PresenceCapabilityProjection::new(vec![NegotiatedCapability::supported(
        CapabilityId::Approve,
        EnforcementClass::BackendEnforced,
    )]);
    projection
        .record_presence(
            PresenceObservation::new(
                PresenceSourceTier::LowConfidenceHeuristic,
                EnforcementClass::Observed,
                false,
            )
            .unwrap(),
        )
        .unwrap();
    // Even with a BackendEnforced capability advertisement, heuristic
    // presence cannot authorize privileged local claims.
    assert_eq!(
        projection.authorize_claim(CapabilityId::Approve, ClaimMode::LocalEnforcement),
        Err(PresenceError::TerminalTextNotAuthority)
    );
}

#[test]
fn illegal_enforcement_upgrade_rejected() {
    let obs = PresenceObservation::new(
        PresenceSourceTier::StructuredAdapter,
        EnforcementClass::Observed,
        false,
    )
    .unwrap();
    assert_eq!(
        obs.reclassify(EnforcementClass::BackendEnforced),
        Err(PresenceError::IllegalEnforcementUpgrade {
            from: EnforcementClass::Observed,
            to: EnforcementClass::BackendEnforced,
        })
    );
    assert_eq!(obs.reclassify(EnforcementClass::Observed), Ok(obs));
}

#[test]
fn unknown_and_unsupported_capabilities_remain_explicit() {
    let projection = PresenceCapabilityProjection::new(vec![
        NegotiatedCapability::unsupported(CapabilityId::Deny),
        NegotiatedCapability::unknown(CapabilityId::Account),
    ]);
    assert_eq!(
        projection.authorize_claim(CapabilityId::Deny, ClaimMode::Observe),
        Err(PresenceError::CapabilityNotSupported {
            id: CapabilityId::Deny,
        })
    );
    assert_eq!(
        projection.authorize_claim(CapabilityId::Account, ClaimMode::Observe),
        Err(PresenceError::CapabilityNotSupported {
            id: CapabilityId::Account,
        })
    );
}

#[test]
fn presence_authorize_claim_official_hooks_caps_backend_enforced_no_local() {
    let mut projection = PresenceCapabilityProjection::new(vec![NegotiatedCapability::supported(
        CapabilityId::Approve,
        EnforcementClass::BackendEnforced,
    )]);
    projection
        .record_presence(official_hooks_presence())
        .unwrap();
    assert_eq!(
        projection.authorize_claim(CapabilityId::Approve, ClaimMode::LocalEnforcement),
        Err(PresenceError::EnforcementInsufficient {
            id: CapabilityId::Approve,
            have: EnforcementClass::UpstreamRequestable,
            mode: ClaimMode::LocalEnforcement,
        })
    );
}

#[test]
fn presence_authorize_claim_structured_adapter_observed_no_local() {
    let mut projection = PresenceCapabilityProjection::new(vec![NegotiatedCapability::supported(
        CapabilityId::Approve,
        EnforcementClass::BackendEnforced,
    )]);
    projection
        .record_presence(
            PresenceObservation::new(
                PresenceSourceTier::StructuredAdapter,
                EnforcementClass::Observed,
                false,
            )
            .unwrap(),
        )
        .unwrap();
    // Observed presence caps effective class below BackendEnforced.
    assert_eq!(
        projection.authorize_claim(CapabilityId::Approve, ClaimMode::LocalEnforcement),
        Err(PresenceError::EnforcementInsufficient {
            id: CapabilityId::Approve,
            have: EnforcementClass::Observed,
            mode: ClaimMode::LocalEnforcement,
        })
    );
}

#[test]
fn presence_authorize_claim_requires_typed_boundary_for_local() {
    let mut projection = PresenceCapabilityProjection::new(vec![NegotiatedCapability::supported(
        CapabilityId::ModelSelect,
        EnforcementClass::BackendEnforced,
    )]);
    // Forge a BackendEnforced observation without typed boundary (public fields)
    // to prove authorize_claim still fails closed.
    projection.presence = Some(PresenceObservation {
        source: PresenceSourceTier::StructuredAdapter,
        enforcement: EnforcementClass::BackendEnforced,
        typed_backend_boundary: false,
    });
    assert_eq!(
        projection.authorize_claim(CapabilityId::ModelSelect, ClaimMode::LocalEnforcement),
        Err(PresenceError::BackendEnforcedRequiresTypedBoundary)
    );
}

#[test]
fn presence_none_fails_closed_for_privileged_claim_modes() {
    let projection = PresenceCapabilityProjection::new(vec![NegotiatedCapability::supported(
        CapabilityId::Approve,
        EnforcementClass::BackendEnforced,
    )]);
    assert!(projection
        .authorize_claim(CapabilityId::Approve, ClaimMode::Observe)
        .is_ok());
    assert_eq!(
        projection.authorize_claim(CapabilityId::Approve, ClaimMode::UpstreamRequest),
        Err(PresenceError::PresenceEvidenceRequired {
            mode: ClaimMode::UpstreamRequest,
        })
    );
    assert_eq!(
        projection.authorize_claim(CapabilityId::Approve, ClaimMode::LocalEnforcement),
        Err(PresenceError::PresenceEvidenceRequired {
            mode: ClaimMode::LocalEnforcement,
        })
    );
}

#[test]
fn presence_untrusted_install_rejects_backend_enforced_caps() {
    let mut projection = PresenceCapabilityProjection::new(Vec::new());
    assert_eq!(
        projection.install_capabilities(
            vec![NegotiatedCapability::supported(
                CapabilityId::Approve,
                EnforcementClass::BackendEnforced,
            )],
            CapabilityInstallTrust::UntrustedPeer,
        ),
        Err(PresenceError::UntrustedBackendEnforcedCapability {
            id: CapabilityId::Approve,
        })
    );
    projection
        .install_capabilities(
            vec![NegotiatedCapability::supported(
                CapabilityId::Approve,
                EnforcementClass::BackendEnforced,
            )],
            CapabilityInstallTrust::BackendPolicyTrusted,
        )
        .unwrap();
    assert_eq!(
        projection
            .capability(CapabilityId::Approve)
            .unwrap()
            .enforcement(),
        Some(EnforcementClass::BackendEnforced)
    );
}

#[test]
fn presence_decode_rejects_duplicate_capability_ids() {
    let caps = vec![
        NegotiatedCapability::supported(CapabilityId::Pause, EnforcementClass::Observed),
        NegotiatedCapability::supported(CapabilityId::Pause, EnforcementClass::UpstreamRequestable),
    ];
    assert_eq!(
        reject_duplicate_capability_ids(&caps),
        Err(PresenceError::DuplicateCapabilityId {
            id: CapabilityId::Pause,
        })
    );
    assert_eq!(
        PresenceCapabilityProjection::try_from_capabilities(caps),
        Err(PresenceError::DuplicateCapabilityId {
            id: CapabilityId::Pause,
        })
    );
}
