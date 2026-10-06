use super::*;
use crate::approval::DecideInput;
use crate::{AgentStore, AttemptId, WorkItemId, WorkScopeId};
use seyal_agent_core::{
    ActionId, ActionIntent, ActionLifecycle, AgentRunId, ApprovalId, ApprovalRequestSpec,
    ApprovalVerdict, ArgumentFingerprint, AuthorizationClass, CapabilityRef, ConsumptionWitness,
    ControlMode, DecisionAuthority, DispatchError, EffectClass, PersistHealth, PrivacyDependencyId,
    RequestProvenance, ResourceIdentity, RevocationFence, RevocationFenceMember,
    RevocationGeneration, ScopeIdentity, ScopeKind, TransitionReason,
};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::thread;

static NEXT: AtomicU64 = AtomicU64::new(1);

fn temp_store() -> (std::path::PathBuf, AgentStore) {
    let dir = std::env::temp_dir().join(format!(
        "seyal-agent-store-dispatch-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("agent.db");
    (path.clone(), AgentStore::open(&path).unwrap())
}

fn seed_run(store: &AgentStore) -> AgentRunId {
    let scope = WorkScopeId::new();
    store.commit_work_scope(scope, 2).unwrap();
    let item = WorkItemId::new();
    store.commit_work_item(item, scope).unwrap();
    let attempt = AttemptId::new();
    store.commit_attempt(attempt, item).unwrap();
    let run = AgentRunId::new();
    store
        .mutate_agent_run_and_append(run, attempt, 1, 1, 1, b"run")
        .unwrap();
    run
}

fn fence() -> RevocationFence {
    RevocationFence::new(vec![RevocationFenceMember {
        scope: ScopeIdentity::new(ScopeKind::Workspace, [4; 16]),
        generation: RevocationGeneration::FIRST,
    }])
    .unwrap()
}

fn intent_for(
    run: AgentRunId,
    action_id: ActionId,
    args: &[u8],
    auth: AuthorizationClass,
    expires: Option<u64>,
) -> ActionIntent {
    ActionIntent::prepare(
        action_id,
        run,
        CapabilityRef::new(b"fs.write").unwrap(),
        ResourceIdentity::new(b"file", [1; 16], [2; 16], [3; 32]).unwrap(),
        ArgumentFingerprint::of(args),
        EffectClass::NonReplayable,
        1,
        PrivacyDependencyId([8; 16]),
        fence(),
        RequestProvenance::AgentBackend,
        auth,
        50_000,
        expires,
        None,
    )
    .unwrap()
}

fn approve_exact(store: &AgentStore, intent: &ActionIntent, now: u64) -> ApprovalId {
    let spec = ApprovalRequestSpec {
        approval_id: ApprovalId::new(),
        action_id: Some(intent.action_id()),
        action_intent_digest: Some(intent.digest()),
        agent_run_id: Some(intent.agent_run_id()),
        capability: Some(intent.capability().clone()),
        resource: Some(intent.resource().clone()),
        argument_fingerprint: Some(intent.argument_fingerprint()),
        effect_class: Some(intent.effect_class()),
        policy_generation: Some(intent.policy_generation()),
        revocation_fence: Some(intent.revocation_fence().clone()),
        expires_at_unix_ms: None,
        requested_at_unix_ms: now,
        attention_id: None,
        control_mode: ControlMode::SeyalControlled,
    };
    let request = store
        .approvals()
        .record_request(spec, "write")
        .expect("request");
    store
        .approvals()
        .decide(DecideInput {
            approval_id: request.approval_id,
            action_id: request.action_id,
            agent_run_id: request.agent_run_id,
            verdict: ApprovalVerdict::Approved,
            authority: DecisionAuthority::User,
            decision_policy_generation: intent.policy_generation(),
            decision_principal_id: None,
            require_session: false,
            client_session: None,
            session_valid: true,
            has_approval_decide_scope: true,
            now_unix_ms: Some(now + 1),
        })
        .expect("approve");
    request.approval_id
}

fn input(now: u64) -> DispatchInput {
    DispatchInput {
        now_ms: now,
        current_policy_generation: 1,
        expected_binding_generation: 1,
    }
}

fn prepare_authorized(
    store: &AgentStore,
    auth: AuthorizationClass,
) -> (AgentRunId, ActionIntent, Option<ApprovalId>) {
    let run = seed_run(store);
    let intent = intent_for(run, ActionId::new(), b"mkdir", auth, None);
    store.actions().prepare(&intent).unwrap();
    let approval = if auth == AuthorizationClass::HumanApproval {
        Some(approve_exact(store, &intent, 51_000))
    } else {
        None
    };
    store
        .actions()
        .authorize(intent.action_id(), 52_000)
        .unwrap();
    (run, intent, approval)
}

#[test]
fn human_dispatch_consumes_once_and_enters_dispatching() {
    let (_path, store) = temp_store();
    let (_run, intent, approval) = prepare_authorized(&store, AuthorizationClass::HumanApproval);
    let outcome = store
        .actions()
        .dispatch(intent.action_id(), input(53_000))
        .unwrap();
    assert_eq!(outcome.dispatch_generation, 1);
    assert!(outcome.consumed_approval.unwrap().consumed);
    let record = store
        .actions()
        .get_record(intent.action_id())
        .unwrap()
        .unwrap();
    assert_eq!(record.runtime.lifecycle, ActionLifecycle::Dispatching);
    assert_eq!(record.runtime.dispatch_generation, Some(1));
    let loaded = store.actions().get(intent.action_id()).unwrap().unwrap();
    assert_eq!(loaded.digest(), intent.digest());
    assert_eq!(
        loaded.lifecycle(),
        ActionLifecycle::Prepared,
        "canonical identity stays Prepared"
    );
    let replay = store.approvals().consume_exact(
        approval.unwrap(),
        &ConsumptionWitness::from_live_intent(&intent, 1, intent.revocation_fence().clone()),
        54_000,
    );
    assert!(matches!(
        replay,
        Err(crate::ApprovalStoreError::Domain(
            seyal_agent_core::ApprovalError::AlreadyConsumed
        ))
    ));
}

#[test]
fn second_worker_cannot_double_consume() {
    let (_path, store) = temp_store();
    let (_run, intent, _) = prepare_authorized(&store, AuthorizationClass::HumanApproval);
    store
        .actions()
        .dispatch(intent.action_id(), input(53_000))
        .unwrap();
    let err = store
        .actions()
        .dispatch(intent.action_id(), input(53_100))
        .unwrap_err();
    assert_eq!(
        err,
        ActionError::Dispatch(DispatchError::AlreadyDispatching)
    );
    let record = store
        .actions()
        .get_record(intent.action_id())
        .unwrap()
        .unwrap();
    assert_eq!(record.runtime.dispatch_generation, Some(1));
}

#[test]
fn concurrent_dispatch_has_one_winner() {
    let (_path, store) = temp_store();
    let (_run, intent, _) = prepare_authorized(&store, AuthorizationClass::HumanApproval);
    let store = Arc::new(store);
    let action_id = intent.action_id();
    let a = store.clone();
    let b = store.clone();
    let left = thread::spawn(move || a.actions().dispatch(action_id, input(53_000)));
    let right = thread::spawn(move || b.actions().dispatch(action_id, input(53_000)));
    let results = [left.join().unwrap(), right.join().unwrap()];
    let wins = results.iter().filter(|r| r.is_ok()).count();
    let losses = results.iter().filter(|r| r.is_err()).count();
    assert_eq!(wins, 1);
    assert_eq!(losses, 1);
    let record = store.actions().get_record(action_id).unwrap().unwrap();
    assert_eq!(record.runtime.lifecycle, ActionLifecycle::Dispatching);
    assert_eq!(record.runtime.dispatch_generation, Some(1));
}

#[test]
fn crash_before_dispatch_commit_does_not_consume() {
    let (_path, store) = temp_store();
    let (_run, intent, approval) = prepare_authorized(&store, AuthorizationClass::HumanApproval);
    store.fail_action_writes_after(0);
    assert_eq!(
        store.actions().dispatch(intent.action_id(), input(53_000)),
        Err(ActionError::Store(crate::StoreError::WriteFailed))
    );
    let record = store
        .actions()
        .get_record(intent.action_id())
        .unwrap()
        .unwrap();
    assert_eq!(record.runtime.lifecycle, ActionLifecycle::Authorized);
    let decision = store.approvals().get(approval.unwrap()).unwrap().decision;
    assert!(!decision.unwrap().consumed);
}

#[test]
fn stale_fence_after_persist_resume_fails_closed() {
    let (_path, store) = temp_store();
    let (_run, intent, _) = prepare_authorized(&store, AuthorizationClass::HumanApproval);
    store.fail_action_writes_after(0);
    for _ in 0..3 {
        let _ = store.actions().authorize(ActionId::new(), 1);
    }
    assert_eq!(store.action_persist_health(), PersistHealth::Paused);
    store.fail_action_writes_after(u64::MAX);
    store
        .memory()
        .advance_revocation(
            &[ScopeIdentity::new(ScopeKind::Workspace, [4; 16])],
            None,
            TransitionReason::PrivacyRevocation,
        )
        .unwrap();
    let resumed = store
        .actions()
        .resume_after_persist_health(intent.action_id())
        .unwrap();
    assert!(!resumed.fences_current);
    assert!(resumed.decision.is_none());
    let err = store
        .actions()
        .dispatch(intent.action_id(), input(60_000))
        .unwrap_err();
    assert_eq!(err, ActionError::Dispatch(DispatchError::StaleFence));
    let record = store
        .actions()
        .get_record(intent.action_id())
        .unwrap()
        .unwrap();
    assert_eq!(record.runtime.lifecycle, ActionLifecycle::Prepared);
    assert!(record.runtime.authorization_invalidated);
}

#[test]
fn policy_mismatch_and_expiry_invalidate_authorization() {
    let (_path, store) = temp_store();
    let (_run, intent, _) = prepare_authorized(&store, AuthorizationClass::HumanApproval);
    let mut drifted = input(53_000);
    drifted.current_policy_generation = 2;
    assert_eq!(
        store.actions().dispatch(intent.action_id(), drifted),
        Err(ActionError::Dispatch(DispatchError::StalePolicy))
    );
    let (_path, store) = temp_store();
    let run = seed_run(&store);
    let intent = intent_for(
        run,
        ActionId::new(),
        b"exp",
        AuthorizationClass::Policy,
        Some(51_000),
    );
    store.actions().prepare(&intent).unwrap();
    assert_eq!(
        store.actions().authorize(intent.action_id(), 51_000),
        Err(ActionError::Dispatch(DispatchError::Expired))
    );
}

#[test]
fn prepared_to_dispatching_is_forbidden() {
    let (_path, store) = temp_store();
    let run = seed_run(&store);
    let intent = intent_for(run, ActionId::new(), b"x", AuthorizationClass::Policy, None);
    store.actions().prepare(&intent).unwrap();
    assert_eq!(
        store.actions().dispatch(intent.action_id(), input(53_000)),
        Err(ActionError::Dispatch(DispatchError::NotAuthorized))
    );
}

#[test]
fn policy_authorization_dispatches_without_approval() {
    let (_path, store) = temp_store();
    let (_run, intent, approval) = prepare_authorized(&store, AuthorizationClass::Policy);
    assert!(approval.is_none());
    let outcome = store
        .actions()
        .dispatch(intent.action_id(), input(53_000))
        .unwrap();
    assert!(outcome.consumed_approval.is_none());
    assert_eq!(outcome.dispatch_generation, 1);
}

#[test]
fn stale_binding_rejects_prepare_and_dispatch() {
    let (_path, store) = temp_store();
    let run = seed_run(&store);
    let intent = intent_for(run, ActionId::new(), b"x", AuthorizationClass::Policy, None);
    assert_eq!(
        store.actions().prepare_bound(&intent, Some(99)),
        Err(ActionError::StaleRunBinding)
    );
    store.actions().prepare(&intent).unwrap();
    store
        .actions()
        .authorize(intent.action_id(), 52_000)
        .unwrap();
    let mut stale = input(53_000);
    stale.expected_binding_generation = 9;
    assert_eq!(
        store.actions().dispatch(intent.action_id(), stale),
        Err(ActionError::Dispatch(DispatchError::StaleBinding))
    );
}

#[test]
fn resume_unknown_action_is_not_current_fence() {
    let (_path, store) = temp_store();
    let resumed = store
        .actions()
        .resume_after_persist_health(ActionId::new())
        .unwrap();
    assert!(!resumed.fences_current);
    assert!(resumed.decision.is_none());
}

#[test]
fn live_witness_rejects_echoed_request_when_policy_drifted() {
    let (_path, store) = temp_store();
    let (_run, intent, approval) = prepare_authorized(&store, AuthorizationClass::HumanApproval);
    let request = store.approvals().get(approval.unwrap()).unwrap().request;
    let echoed = ConsumptionWitness::from_request(&request);
    assert_eq!(echoed.policy_generation, 1);
    let live = ConsumptionWitness::from_live_intent(&intent, 2, intent.revocation_fence().clone());
    assert_ne!(live.policy_generation, echoed.policy_generation);
}
