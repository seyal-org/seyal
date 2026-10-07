use super::*;
use crate::memory::{RevocationFence, RevocationFenceMember, ScopeIdentity, ScopeKind};
use crate::{ActionId, AgentRunId, RevocationGeneration};

fn fence() -> RevocationFence {
    RevocationFence::new(vec![RevocationFenceMember {
        scope: ScopeIdentity::new(ScopeKind::Workspace, [7; 16]),
        generation: RevocationGeneration::FIRST,
    }])
    .unwrap()
}

fn resource(version_byte: u8) -> ResourceIdentity {
    ResourceIdentity::new(b"file", [1; 16], [2; 16], [version_byte; 32]).unwrap()
}

fn intent_with(
    action_id: ActionId,
    args: &[u8],
    version_byte: u8,
    effect: EffectClass,
    auth: AuthorizationClass,
) -> ActionIntent {
    ActionIntent::prepare(
        action_id,
        AgentRunId::new(),
        CapabilityRef::new(b"fs.write").unwrap(),
        resource(version_byte),
        ArgumentFingerprint::of(args),
        effect,
        1,
        PrivacyDependencyId([3; 16]),
        fence(),
        RequestProvenance::AgentBackend,
        auth,
        1_000,
        None,
        None,
    )
    .unwrap()
}

#[test]
fn encode_round_trips_and_digest_is_stable() {
    let intent = intent_with(
        ActionId::new(),
        b"{\"path\":\"a\"}",
        1,
        EffectClass::NonReplayable,
        AuthorizationClass::HumanApproval,
    );
    let encoded = intent.encode();
    let decoded = ActionIntent::decode(&encoded).unwrap();
    assert_eq!(decoded, intent);
    assert_eq!(action_intent_digest(&decoded), intent.digest());
}

#[test]
fn material_successor_mints_new_id_and_leaves_predecessor_unchanged() {
    let original_id = ActionId::new();
    let original = intent_with(
        original_id,
        b"a=1",
        1,
        EffectClass::NonReplayable,
        AuthorizationClass::HumanApproval,
    );
    let original_digest = original.digest();
    let successor = original
        .material_successor(
            CapabilityRef::new(b"fs.write").unwrap(),
            resource(1),
            ArgumentFingerprint::of(b"a=2"),
            EffectClass::NonReplayable,
            original.policy_generation(),
            original.privacy_dependency(),
            original.revocation_fence().clone(),
            original.authorization_class(),
            2_000,
            None,
            None,
        )
        .unwrap();
    assert_ne!(successor.action_id(), original.action_id());
    assert_eq!(original.action_id(), original_id);
    assert_eq!(original.digest(), original_digest);
    assert!(material_fields_changed(&original, &successor));
}

#[test]
fn policy_generation_advance_is_not_a_silent_edit_of_material_fields() {
    let original = intent_with(
        ActionId::new(),
        b"same",
        1,
        EffectClass::Pure,
        AuthorizationClass::Policy,
    );
    let successor = original
        .material_successor(
            original.capability().clone(),
            original.resource().clone(),
            original.argument_fingerprint(),
            original.effect_class(),
            original.policy_generation() + 1,
            original.privacy_dependency(),
            original.revocation_fence().clone(),
            original.authorization_class(),
            original.created_at_ms(),
            original.expires_at_ms(),
            original.executor(),
        )
        .unwrap();
    assert!(!material_fields_changed(&original, &successor));
    assert_ne!(successor.action_id(), original.action_id());
    assert_ne!(successor.digest(), original.digest());
}
