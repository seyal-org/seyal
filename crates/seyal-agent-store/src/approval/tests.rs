//! SPEC-028 §12 fixtures 4–13, 23–24 plus SPEC-016 consume seam.

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::thread;

use seyal_agent_core::{
    auto_approve_reconciliation, request_from_untrusted_terminal, ActionId, AgentRunId,
    ApprovalError, ApprovalId, ApprovalRequestSpec, ApprovalVerdict, ArgumentFingerprint,
    AttentionKind, AttentionState, AttentionTarget, CapabilityRef, ClientSessionId,
    ConsumptionWitness, ControlMode, DecisionAuthority, EffectClass, ResourceIdentity,
    RevocationFence, RevocationFenceMember, RevocationGeneration, ScopeIdentity, ScopeKind,
};

use crate::approval::{ApprovalStoreError, DecideInput};
use crate::attention::MintTrustedInput;
use crate::AgentStore;

static NEXT_DIR: AtomicU64 = AtomicU64::new(1);

fn open_store() -> AgentStore {
    let dir = std::env::temp_dir().join(format!(
        "seyal-approval-{}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    AgentStore::open(dir.join("agent.db")).unwrap()
}

fn fence_at(generation: u64) -> RevocationFence {
    RevocationFence::new(vec![RevocationFenceMember {
        scope: ScopeIdentity::new(ScopeKind::Workspace, [7; 16]),
        generation: RevocationGeneration::from_raw(generation).unwrap(),
    }])
    .unwrap()
}

fn resource(version: u8) -> ResourceIdentity {
    ResourceIdentity::new(b"file", [1; 16], [2; 16], [version; 32]).unwrap()
}

fn spec_for(
    action: ActionId,
    run: AgentRunId,
    args: &[u8],
    version: u8,
    policy: u64,
    fence: RevocationFence,
    expires: Option<u64>,
) -> ApprovalRequestSpec {
    ApprovalRequestSpec {
        approval_id: ApprovalId::new(),
        action_id: Some(action),
        action_intent_digest: Some([9; 32]),
        agent_run_id: Some(run),
        capability: Some(CapabilityRef::new(b"fs.write").unwrap()),
        resource: Some(resource(version)),
        argument_fingerprint: Some(ArgumentFingerprint::of(args)),
        effect_class: Some(EffectClass::NonReplayable),
        policy_generation: Some(policy),
        revocation_fence: Some(fence),
        expires_at_unix_ms: expires,
        requested_at_unix_ms: 1_000,
        attention_id: None,
        control_mode: ControlMode::SeyalControlled,
    }
}

fn approve(store: &AgentStore, request: &seyal_agent_core::ApprovalRequest, now: u64) {
    store
        .approvals()
        .decide(DecideInput {
            approval_id: request.approval_id,
            action_id: request.action_id,
            agent_run_id: request.agent_run_id,
            verdict: ApprovalVerdict::Approved,
            authority: DecisionAuthority::User,
            decision_policy_generation: request.policy_generation,
            decision_principal_id: None,
            require_session: false,
            client_session: None,
            session_valid: true,
            has_approval_decide_scope: true,
            now_unix_ms: Some(now),
        })
        .expect("approve");
}

#[test]
fn spec028_12_04_missing_field_is_not_recorded() {
    let store = open_store();
    let mut spec = spec_for(
        ActionId::new(),
        AgentRunId::new(),
        b"a=1",
        1,
        1,
        fence_at(1),
        None,
    );
    spec.action_id = None;
    assert_eq!(
        store.approvals().record_request(spec, "approve"),
        Err(ApprovalStoreError::Domain(ApprovalError::IncompleteBinding))
    );
}

#[test]
fn spec028_12_05_exact_approve_is_consumable_and_resolves_attention() {
    let store = open_store();
    let action = ActionId::new();
    let run = AgentRunId::new();
    let request = store
        .approvals()
        .record_request(
            spec_for(action, run, b"a=1", 1, 1, fence_at(1), None),
            "write",
        )
        .expect("request");
    approve(&store, &request, 1_100);
    let item = store.attention().get(request.attention_id).unwrap();
    assert_eq!(item.state, AttentionState::Resolved);
    assert_eq!(item.kind, AttentionKind::ApprovalRequired);
    let consumed = store
        .approvals()
        .consume_exact(
            request.approval_id,
            &ConsumptionWitness::from_request(&request),
            1_200,
        )
        .expect("consume");
    assert!(consumed.consumed);
    assert_eq!(consumed.decision, ApprovalVerdict::Approved);
}

#[test]
fn spec028_12_06_argument_fingerprint_change_not_consumable() {
    let store = open_store();
    let request = store
        .approvals()
        .record_request(
            spec_for(
                ActionId::new(),
                AgentRunId::new(),
                b"a=1",
                1,
                1,
                fence_at(1),
                None,
            ),
            "write",
        )
        .unwrap();
    approve(&store, &request, 1_100);
    let mut witness = ConsumptionWitness::from_request(&request);
    witness.argument_fingerprint = ArgumentFingerprint::of(b"a=2");
    assert_eq!(
        store
            .approvals()
            .consume_exact(request.approval_id, &witness, 1_200),
        Err(ApprovalStoreError::Domain(ApprovalError::BindingMismatch))
    );
}

#[test]
fn spec028_12_07_resource_version_change_not_consumable() {
    let store = open_store();
    let request = store
        .approvals()
        .record_request(
            spec_for(
                ActionId::new(),
                AgentRunId::new(),
                b"a=1",
                1,
                1,
                fence_at(1),
                None,
            ),
            "write",
        )
        .unwrap();
    approve(&store, &request, 1_100);
    let mut witness = ConsumptionWitness::from_request(&request);
    witness.resource = resource(2);
    assert_eq!(
        store
            .approvals()
            .consume_exact(request.approval_id, &witness, 1_200),
        Err(ApprovalStoreError::Domain(ApprovalError::BindingMismatch))
    );
}

#[test]
fn spec028_12_08_policy_or_fence_advance_stales_approval() {
    let store = open_store();
    let request = store
        .approvals()
        .record_request(
            spec_for(
                ActionId::new(),
                AgentRunId::new(),
                b"a=1",
                1,
                1,
                fence_at(1),
                None,
            ),
            "write",
        )
        .unwrap();
    approve(&store, &request, 1_100);
    let mut policy = ConsumptionWitness::from_request(&request);
    policy.policy_generation = 2;
    assert_eq!(
        store
            .approvals()
            .consume_exact(request.approval_id, &policy, 1_200),
        Err(ApprovalStoreError::Domain(ApprovalError::BindingMismatch))
    );
    let mut fence = ConsumptionWitness::from_request(&request);
    fence.revocation_fence = fence_at(2);
    assert_eq!(
        store
            .approvals()
            .consume_exact(request.approval_id, &fence, 1_200),
        Err(ApprovalStoreError::Domain(ApprovalError::BindingMismatch))
    );
}

#[test]
fn spec028_12_09_expiry_rejects_approve() {
    let store = open_store();
    let request = store
        .approvals()
        .record_request(
            spec_for(
                ActionId::new(),
                AgentRunId::new(),
                b"a=1",
                1,
                1,
                fence_at(1),
                Some(2_000),
            ),
            "write",
        )
        .unwrap();
    let err = store
        .approvals()
        .decide(DecideInput {
            approval_id: request.approval_id,
            action_id: request.action_id,
            agent_run_id: request.agent_run_id,
            verdict: ApprovalVerdict::Approved,
            authority: DecisionAuthority::User,
            decision_policy_generation: 1,
            decision_principal_id: None,
            require_session: false,
            client_session: None,
            session_valid: true,
            has_approval_decide_scope: true,
            now_unix_ms: Some(2_000),
        })
        .unwrap_err();
    assert_eq!(err, ApprovalStoreError::Domain(ApprovalError::Expired));
}

#[test]
fn spec028_12_10_replay_consumed_approval_rejected() {
    let store = open_store();
    let request = store
        .approvals()
        .record_request(
            spec_for(
                ActionId::new(),
                AgentRunId::new(),
                b"a=1",
                1,
                1,
                fence_at(1),
                None,
            ),
            "write",
        )
        .unwrap();
    approve(&store, &request, 1_100);
    let witness = ConsumptionWitness::from_request(&request);
    store
        .approvals()
        .consume_exact(request.approval_id, &witness, 1_200)
        .unwrap();
    assert_eq!(
        store
            .approvals()
            .consume_exact(request.approval_id, &witness, 1_300),
        Err(ApprovalStoreError::Domain(ApprovalError::AlreadyConsumed))
    );
}

#[test]
fn spec028_12_11_duplicate_concurrent_approve_one_consumable() {
    let store = Arc::new(open_store());
    let request = store
        .approvals()
        .record_request(
            spec_for(
                ActionId::new(),
                AgentRunId::new(),
                b"a=1",
                1,
                1,
                fence_at(1),
                None,
            ),
            "write",
        )
        .unwrap();
    let mut joins = Vec::new();
    for _ in 0..8 {
        let store = Arc::clone(&store);
        let request = request.clone();
        joins.push(thread::spawn(move || {
            store.approvals().decide(DecideInput {
                approval_id: request.approval_id,
                action_id: request.action_id,
                agent_run_id: request.agent_run_id,
                verdict: ApprovalVerdict::Approved,
                authority: DecisionAuthority::User,
                decision_policy_generation: 1,
                decision_principal_id: None,
                require_session: false,
                client_session: None,
                session_valid: true,
                has_approval_decide_scope: true,
                now_unix_ms: Some(1_100),
            })
        }));
    }
    let mut ok = 0usize;
    for join in joins {
        if join.join().unwrap().is_ok() {
            ok += 1;
        }
    }
    assert_eq!(ok, 8);
    let recorded = store.approvals().get(request.approval_id).unwrap();
    let decision = recorded.decision.expect("one decision");
    assert_eq!(decision.decision, ApprovalVerdict::Approved);
    assert!(!decision.consumed);
    store
        .approvals()
        .consume_exact(
            request.approval_id,
            &ConsumptionWitness::from_request(&request),
            1_200,
        )
        .unwrap();
    assert_eq!(
        store
            .approvals()
            .consume_exact(
                request.approval_id,
                &ConsumptionWitness::from_request(&request),
                1_300
            )
            .unwrap_err(),
        ApprovalStoreError::Domain(ApprovalError::AlreadyConsumed)
    );
}

#[test]
fn spec028_12_12_terminal_text_cannot_record_request() {
    assert!(request_from_untrusted_terminal("Approve? [y/N]").is_err());
    let store = open_store();
    let item = store
        .attention()
        .mint_untrusted_terminal("Approve? [y/N]", Some(AgentRunId::new()))
        .unwrap();
    assert_ne!(item.kind, AttentionKind::ApprovalRequired);
    assert_eq!(
        store.approvals().get(ApprovalId::new()).unwrap_err(),
        ApprovalStoreError::Domain(ApprovalError::UnknownApproval)
    );
}

#[test]
fn spec028_12_13_external_observed_cannot_record() {
    let store = open_store();
    let mut spec = spec_for(
        ActionId::new(),
        AgentRunId::new(),
        b"a=1",
        1,
        1,
        fence_at(1),
        None,
    );
    spec.control_mode = ControlMode::ExternalObserved;
    assert_eq!(
        store.approvals().record_request(spec, "obs"),
        Err(ApprovalStoreError::Domain(
            ApprovalError::ExternalObservedForbidden
        ))
    );
}

#[test]
fn spec028_12_23_stale_session_or_missing_scope_cannot_decide() {
    let store = open_store();
    let request = store
        .approvals()
        .record_request(
            spec_for(
                ActionId::new(),
                AgentRunId::new(),
                b"a=1",
                1,
                1,
                fence_at(1),
                None,
            ),
            "write",
        )
        .unwrap();
    let stale = store
        .approvals()
        .decide(DecideInput {
            approval_id: request.approval_id,
            action_id: request.action_id,
            agent_run_id: request.agent_run_id,
            verdict: ApprovalVerdict::Approved,
            authority: DecisionAuthority::User,
            decision_policy_generation: 1,
            decision_principal_id: None,
            require_session: true,
            client_session: Some(ClientSessionId::new()),
            session_valid: false,
            has_approval_decide_scope: true,
            now_unix_ms: Some(1_100),
        })
        .unwrap_err();
    assert_eq!(
        stale,
        ApprovalStoreError::Domain(ApprovalError::StaleSession)
    );
    let missing = store
        .approvals()
        .decide(DecideInput {
            approval_id: request.approval_id,
            action_id: request.action_id,
            agent_run_id: request.agent_run_id,
            verdict: ApprovalVerdict::Approved,
            authority: DecisionAuthority::User,
            decision_policy_generation: 1,
            decision_principal_id: None,
            require_session: true,
            client_session: Some(ClientSessionId::new()),
            session_valid: true,
            has_approval_decide_scope: false,
            now_unix_ms: Some(1_100),
        })
        .unwrap_err();
    assert_eq!(
        missing,
        ApprovalStoreError::Domain(ApprovalError::MissingDecideScope)
    );
}

#[test]
fn spec028_12_24_reconciliation_attention_does_not_auto_approve() {
    let store = open_store();
    let run = AgentRunId::new();
    store
        .attention()
        .mint_trusted(MintTrustedInput {
            kind: AttentionKind::ReconciliationRequired,
            summary: "unknown effect".into(),
            target: AttentionTarget {
                resource_address: None,
                agent_run_id: Some(run),
                action_id: Some(ActionId::new()),
                artifact_id: None,
                requires_spatial_focus: false,
            },
            agent_run_id: Some(run),
            action_id: Some(ActionId::new()),
            approval_id: None,
            require_session: false,
            client_session: None,
            session_valid: true,
            has_attention_scope: true,
        })
        .unwrap();
    assert_eq!(
        auto_approve_reconciliation(),
        Err(ApprovalError::ReconciliationDoesNotAuthorize)
    );
    assert_eq!(
        store.approvals().get(ApprovalId::new()).unwrap_err(),
        ApprovalStoreError::Domain(ApprovalError::UnknownApproval)
    );
}

#[test]
fn dismiss_without_decision_is_not_consumable() {
    let store = open_store();
    let request = store
        .approvals()
        .record_request(
            spec_for(
                ActionId::new(),
                AgentRunId::new(),
                b"a=1",
                1,
                1,
                fence_at(1),
                None,
            ),
            "write",
        )
        .unwrap();
    store
        .attention()
        .dismiss(request.attention_id, false, None, true, true)
        .unwrap();
    assert_eq!(
        store
            .approvals()
            .consume_exact(
                request.approval_id,
                &ConsumptionWitness::from_request(&request),
                1_200
            )
            .unwrap_err(),
        ApprovalStoreError::Domain(ApprovalError::UnknownApproval)
    );
}

#[test]
fn rejected_decision_is_not_consumable() {
    let store = open_store();
    let request = store
        .approvals()
        .record_request(
            spec_for(
                ActionId::new(),
                AgentRunId::new(),
                b"a=1",
                1,
                1,
                fence_at(1),
                None,
            ),
            "write",
        )
        .unwrap();
    store
        .approvals()
        .decide(DecideInput {
            approval_id: request.approval_id,
            action_id: request.action_id,
            agent_run_id: request.agent_run_id,
            verdict: ApprovalVerdict::Rejected,
            authority: DecisionAuthority::User,
            decision_policy_generation: 1,
            decision_principal_id: None,
            require_session: false,
            client_session: None,
            session_valid: true,
            has_approval_decide_scope: true,
            now_unix_ms: Some(1_100),
        })
        .unwrap();
    assert_eq!(
        store
            .approvals()
            .consume_exact(
                request.approval_id,
                &ConsumptionWitness::from_request(&request),
                1_200
            )
            .unwrap_err(),
        ApprovalStoreError::Domain(ApprovalError::RejectedDoesNotAuthorize)
    );
}

#[test]
fn forged_approval_id_fails_closed() {
    let store = open_store();
    assert_eq!(
        store.approvals().get(ApprovalId::new()).unwrap_err(),
        ApprovalStoreError::Domain(ApprovalError::UnknownApproval)
    );
}
