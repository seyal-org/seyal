//! SPEC-016 §5–§6 dispatch fencing / exact consumption fixtures.

use seyal_agent_core::{
    evaluate_dispatch, ActionId, ActionIntent, ActionLifecycle, ActionRuntime, AgentRunId,
    ArgumentFingerprint, AuthorizationClass, CapabilityRef, DispatchEval, EffectClass,
    PersistHealth, PrivacyDependencyId, RequestProvenance, ResourceIdentity, RevocationFence,
    RevocationFenceMember, RevocationGeneration, ScopeIdentity, ScopeKind, CANONICAL_INTENT_LAYOUT,
};

fn fence() -> RevocationFence {
    RevocationFence::new(vec![RevocationFenceMember {
        scope: ScopeIdentity::new(ScopeKind::Workspace, [2; 16]),
        generation: RevocationGeneration::FIRST,
    }])
    .unwrap()
}

fn sample() -> ActionIntent {
    ActionIntent::prepare(
        ActionId::new(),
        AgentRunId::new(),
        CapabilityRef::new(b"fs.write").unwrap(),
        ResourceIdentity::new(b"file", [1; 16], [2; 16], [3; 32]).unwrap(),
        ArgumentFingerprint::of(b"op"),
        EffectClass::NonReplayable,
        1,
        PrivacyDependencyId([4; 16]),
        fence(),
        RequestProvenance::Harness,
        AuthorizationClass::HumanApproval,
        10,
        Some(50),
        None,
    )
    .unwrap()
}

#[test]
fn spec016_prepared_cannot_skip_authorized() {
    let intent = sample();
    let runtime = ActionRuntime::prepared();
    assert!(evaluate_dispatch(DispatchEval {
        intent: &intent,
        runtime: &runtime,
        persist_health: PersistHealth::Healthy,
        now_ms: 20,
        current_policy_generation: 1,
        current_fence: intent.revocation_fence(),
        current_binding_generation: 1,
        expected_binding_generation: 1,
    })
    .is_err());
}

#[test]
fn spec016_canonical_layout_is_frozen_prepared() {
    let intent = sample();
    assert_eq!(CANONICAL_INTENT_LAYOUT, 1);
    assert_eq!(intent.lifecycle(), ActionLifecycle::Prepared);
    let encoded = intent.encode();
    assert_eq!(*encoded.last().unwrap(), ActionLifecycle::Prepared.code());
    assert_eq!(
        ActionIntent::decode(&encoded).unwrap().digest(),
        intent.digest()
    );
}
