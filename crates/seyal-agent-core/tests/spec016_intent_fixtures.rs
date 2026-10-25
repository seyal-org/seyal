//! SPEC-016 §3 / §20 identity-intent fixtures (preparation only).

use seyal_agent_core::{
    action_intent_digest, material_fields_changed, ActionId, ActionIntent, AgentRunId,
    ArgumentFingerprint, AuthorizationClass, CapabilityRef, EffectClass, PrivacyDependencyId,
    RequestProvenance, ResourceIdentity, RevocationFence, RevocationFenceMember,
    RevocationGeneration, ScopeIdentity, ScopeKind,
};

fn fence_at(generation: RevocationGeneration) -> RevocationFence {
    RevocationFence::new(vec![RevocationFenceMember {
        scope: ScopeIdentity::new(ScopeKind::Workspace, [9; 16]),
        generation,
    }])
    .unwrap()
}

#[allow(clippy::too_many_arguments)]
fn sample(
    action_id: ActionId,
    run: AgentRunId,
    args: &[u8],
    version: u8,
    effect: EffectClass,
    auth: AuthorizationClass,
    policy: u64,
    fence: RevocationFence,
) -> ActionIntent {
    ActionIntent::prepare(
        action_id,
        run,
        CapabilityRef::new(b"tool.apply").unwrap(),
        ResourceIdentity::new(b"repo-file", [11; 16], [12; 16], [version; 32]).unwrap(),
        ArgumentFingerprint::of(args),
        effect,
        policy,
        PrivacyDependencyId([13; 16]),
        fence,
        RequestProvenance::Harness,
        auth,
        10_000,
        Some(20_000),
        None,
    )
    .unwrap()
}

#[test]
fn material_argument_change_requires_new_action_id_old_digest_unchanged() {
    let run = AgentRunId::new();
    let original_id = ActionId::new();
    let original = sample(
        original_id,
        run,
        b"delete /tmp/a",
        1,
        EffectClass::NonReplayable,
        AuthorizationClass::HumanApproval,
        4,
        fence_at(RevocationGeneration::FIRST),
    );
    let digest = action_intent_digest(&original);
    let changed = original
        .material_successor(
            original.capability().clone(),
            original.resource().clone(),
            ArgumentFingerprint::of(b"delete /tmp/b"),
            original.effect_class(),
            original.policy_generation(),
            original.privacy_dependency(),
            original.revocation_fence().clone(),
            original.authorization_class(),
            11_000,
            Some(21_000),
            None,
        )
        .unwrap();
    assert_ne!(changed.action_id(), original_id);
    assert_eq!(original.action_id(), original_id);
    assert_eq!(action_intent_digest(&original), digest);
    assert!(material_fields_changed(&original, &changed));
}

#[test]
fn same_action_id_and_canonical_intent_share_one_digest() {
    let run = AgentRunId::new();
    let id = ActionId::new();
    let first = sample(
        id,
        run,
        b"unchanged",
        3,
        EffectClass::IdempotentExternal,
        AuthorizationClass::Policy,
        2,
        fence_at(RevocationGeneration::FIRST),
    );
    let second = ActionIntent::decode(&first.encode()).unwrap();
    assert_eq!(second.action_id(), id);
    assert_eq!(action_intent_digest(&first), action_intent_digest(&second));
}

#[test]
fn reused_action_id_with_mismatched_material_fields_has_distinct_digest() {
    let run = AgentRunId::new();
    let id = ActionId::new();
    let stored = sample(
        id,
        run,
        b"v1",
        1,
        EffectClass::FilesystemIsolated,
        AuthorizationClass::Policy,
        1,
        fence_at(RevocationGeneration::FIRST),
    );
    let mismatched = sample(
        id,
        run,
        b"v2",
        1,
        EffectClass::FilesystemIsolated,
        AuthorizationClass::Policy,
        1,
        fence_at(RevocationGeneration::FIRST),
    );
    assert_eq!(stored.action_id(), mismatched.action_id());
    assert_ne!(
        action_intent_digest(&stored),
        action_intent_digest(&mismatched)
    );
}

#[test]
fn generation_only_fence_advance_does_not_count_as_material_field_change() {
    let run = AgentRunId::new();
    let original = sample(
        ActionId::new(),
        run,
        b"payload",
        1,
        EffectClass::Pure,
        AuthorizationClass::Policy,
        8,
        fence_at(RevocationGeneration::FIRST),
    );
    let advanced = original
        .material_successor(
            original.capability().clone(),
            original.resource().clone(),
            original.argument_fingerprint(),
            original.effect_class(),
            original.policy_generation(),
            original.privacy_dependency(),
            fence_at(RevocationGeneration::FIRST.next().unwrap()),
            original.authorization_class(),
            original.created_at_ms(),
            original.expires_at_ms(),
            original.executor(),
        )
        .unwrap();
    assert!(!material_fields_changed(&original, &advanced));
    assert_ne!(advanced.action_id(), original.action_id());
}

#[test]
fn each_named_material_field_change_mints_a_distinct_action_id() {
    let run = AgentRunId::new();
    let base = sample(
        ActionId::new(),
        run,
        b"base",
        1,
        EffectClass::NonReplayable,
        AuthorizationClass::HumanApproval,
        3,
        fence_at(RevocationGeneration::FIRST),
    );
    let variants = [
        base.material_successor(
            CapabilityRef::new(b"tool.other").unwrap(),
            base.resource().clone(),
            base.argument_fingerprint(),
            base.effect_class(),
            base.policy_generation(),
            base.privacy_dependency(),
            base.revocation_fence().clone(),
            base.authorization_class(),
            12_000,
            None,
            None,
        )
        .unwrap(),
        base.material_successor(
            base.capability().clone(),
            ResourceIdentity::new(b"repo-file", [99; 16], [12; 16], [1; 32]).unwrap(),
            base.argument_fingerprint(),
            base.effect_class(),
            base.policy_generation(),
            base.privacy_dependency(),
            base.revocation_fence().clone(),
            base.authorization_class(),
            12_000,
            None,
            None,
        )
        .unwrap(),
        base.material_successor(
            base.capability().clone(),
            ResourceIdentity::new(b"repo-file", [11; 16], [12; 16], [9; 32]).unwrap(),
            base.argument_fingerprint(),
            base.effect_class(),
            base.policy_generation(),
            base.privacy_dependency(),
            base.revocation_fence().clone(),
            base.authorization_class(),
            12_000,
            None,
            None,
        )
        .unwrap(),
        base.material_successor(
            base.capability().clone(),
            base.resource().clone(),
            ArgumentFingerprint::of(b"other-args"),
            base.effect_class(),
            base.policy_generation(),
            base.privacy_dependency(),
            base.revocation_fence().clone(),
            base.authorization_class(),
            12_000,
            None,
            None,
        )
        .unwrap(),
        base.material_successor(
            base.capability().clone(),
            base.resource().clone(),
            base.argument_fingerprint(),
            EffectClass::Pure,
            base.policy_generation(),
            base.privacy_dependency(),
            base.revocation_fence().clone(),
            base.authorization_class(),
            12_000,
            None,
            None,
        )
        .unwrap(),
        base.material_successor(
            base.capability().clone(),
            base.resource().clone(),
            base.argument_fingerprint(),
            base.effect_class(),
            base.policy_generation(),
            PrivacyDependencyId([77; 16]),
            base.revocation_fence().clone(),
            base.authorization_class(),
            12_000,
            None,
            None,
        )
        .unwrap(),
        base.material_successor(
            base.capability().clone(),
            base.resource().clone(),
            base.argument_fingerprint(),
            base.effect_class(),
            base.policy_generation(),
            base.privacy_dependency(),
            base.revocation_fence().clone(),
            AuthorizationClass::Policy,
            12_000,
            None,
            None,
        )
        .unwrap(),
    ];
    for changed in &variants {
        assert_ne!(changed.action_id(), base.action_id());
        assert_eq!(action_intent_digest(&base), base.digest());
        assert!(material_fields_changed(&base, changed));
    }
}
