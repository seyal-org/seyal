use super::*;
use crate::{AgentStore, AttemptId, WorkItemId, WorkScopeId};
use seyal_agent_core::{
    ActionId, ActionIntent, AgentRunId, ArgumentFingerprint, AuthorizationClass, CapabilityRef,
    EffectClass, PrivacyDependencyId, RequestProvenance, ResourceIdentity, RevocationFence,
    RevocationFenceMember, RevocationGeneration, ScopeIdentity, ScopeKind,
};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);

fn temp_store() -> (std::path::PathBuf, AgentStore) {
    let dir = std::env::temp_dir().join(format!(
        "seyal-agent-store-action-{}-{}",
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

fn intent(run: AgentRunId, action_id: ActionId, args: &[u8]) -> ActionIntent {
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
        AuthorizationClass::HumanApproval,
        50_000,
        None,
        None,
    )
    .unwrap()
}

#[test]
fn persist_before_dispatch_survives_reopen_as_prepared() {
    let (path, store) = temp_store();
    let run = seed_run(&store);
    let action_id = ActionId::new();
    let prepared = intent(run, action_id, b"mkdir x");
    assert_eq!(
        store.actions().prepare(&prepared).unwrap(),
        PrepareOutcome::NewlyPrepared
    );
    drop(store);
    let store = AgentStore::open(&path).unwrap();
    let loaded = store.actions().get(action_id).unwrap().unwrap();
    assert_eq!(loaded, prepared);
    assert_eq!(
        loaded.lifecycle(),
        seyal_agent_core::ActionLifecycle::Prepared
    );
}

#[test]
fn crash_before_prepare_commit_leaves_no_durable_action() {
    let (path, store) = temp_store();
    let run = seed_run(&store);
    store.fail_after_writes(0);
    let action_id = ActionId::new();
    assert_eq!(
        store.actions().prepare(&intent(run, action_id, b"touch y")),
        Err(ActionError::Store(crate::StoreError::WriteFailed))
    );
    drop(store);
    let store = AgentStore::open(&path).unwrap();
    assert!(store.actions().get(action_id).unwrap().is_none());
}

#[test]
fn duplicate_same_id_and_digest_does_not_create_a_second_row() {
    let (_path, store) = temp_store();
    let run = seed_run(&store);
    let action_id = ActionId::new();
    let prepared = intent(run, action_id, b"same");
    assert_eq!(
        store.actions().prepare(&prepared).unwrap(),
        PrepareOutcome::NewlyPrepared
    );
    assert_eq!(
        store.actions().prepare(&prepared).unwrap(),
        PrepareOutcome::Duplicate
    );
    assert_eq!(store.actions().list_for_run(run).unwrap().len(), 1);
}

#[test]
fn mismatched_intent_for_existing_action_id_is_rejected_and_stored_row_is_unchanged() {
    let (_path, store) = temp_store();
    let run = seed_run(&store);
    let action_id = ActionId::new();
    let stored = intent(run, action_id, b"v1");
    store.actions().prepare(&stored).unwrap();
    let mismatched = intent(run, action_id, b"v2");
    assert_eq!(
        store.actions().prepare(&mismatched),
        Err(ActionError::IdentityMismatch)
    );
    let loaded = store.actions().get(action_id).unwrap().unwrap();
    assert_eq!(loaded, stored);
}

#[test]
fn material_change_inserts_a_new_row_and_never_edits_the_old_intent() {
    let (_path, store) = temp_store();
    let run = seed_run(&store);
    let original = intent(run, ActionId::new(), b"old-args");
    store.actions().prepare(&original).unwrap();
    let successor = original
        .material_successor(
            original.capability().clone(),
            original.resource().clone(),
            ArgumentFingerprint::of(b"new-args"),
            original.effect_class(),
            original.policy_generation(),
            original.privacy_dependency(),
            original.revocation_fence().clone(),
            original.authorization_class(),
            60_000,
            None,
            None,
        )
        .unwrap();
    store.actions().prepare(&successor).unwrap();
    let old = store.actions().get(original.action_id()).unwrap().unwrap();
    let new = store.actions().get(successor.action_id()).unwrap().unwrap();
    assert_eq!(old, original);
    assert_eq!(new, successor);
    assert_ne!(old.action_id(), new.action_id());
    assert_eq!(store.actions().list_for_run(run).unwrap().len(), 2);
}

#[test]
fn effect_unknown_after_dispatch_crash_does_not_rewrite_intent_digest() {
    let (_path, store) = temp_store();
    let run = seed_run(&store);
    let action_id = ActionId::new();
    let prepared = intent(run, action_id, b"pay invoice");
    let digest = prepared.digest();
    store.actions().prepare(&prepared).unwrap();
    store.actions().fixture_mark_authorized(action_id).unwrap();
    store
        .actions()
        .fixture_mark_dispatching(action_id, 11)
        .unwrap();
    let decision = store
        .actions()
        .recover(
            action_id,
            seyal_agent_core::CrashBoundary::DuringExecutor,
            &seyal_agent_core::RecoveryEvidence::none(),
        )
        .unwrap();
    assert_eq!(
        decision.runtime.lifecycle,
        seyal_agent_core::ActionLifecycle::EffectUnknown
    );
    assert!(!decision.may_retry_effect);
    let loaded = store.actions().get(action_id).unwrap().unwrap();
    assert_eq!(loaded.digest(), digest);
    assert_eq!(
        loaded.lifecycle(),
        seyal_agent_core::ActionLifecycle::Prepared
    );
    let record = store.actions().get_record(action_id).unwrap().unwrap();
    assert_eq!(
        record.runtime.lifecycle,
        seyal_agent_core::ActionLifecycle::EffectUnknown
    );
}

#[test]
fn persist_failure_of_recovery_leaves_prior_runtime_and_does_not_retry() {
    let (path, store) = temp_store();
    let run = seed_run(&store);
    let action_id = ActionId::new();
    store
        .actions()
        .prepare(&intent(run, action_id, b"transfer"))
        .unwrap();
    store.actions().fixture_mark_authorized(action_id).unwrap();
    store
        .actions()
        .fixture_mark_dispatching(action_id, 4)
        .unwrap();
    store.fail_after_writes(0);
    assert_eq!(
        store.actions().recover(
            action_id,
            seyal_agent_core::CrashBoundary::AfterEffectBeforeResultPersist,
            &seyal_agent_core::RecoveryEvidence::none(),
        ),
        Err(ActionError::Store(crate::StoreError::WriteFailed))
    );
    drop(store);
    let store = AgentStore::open(&path).unwrap();
    let record = store.actions().get_record(action_id).unwrap().unwrap();
    assert_eq!(
        record.runtime.lifecycle,
        seyal_agent_core::ActionLifecycle::Dispatching
    );
}

#[test]
fn cancel_after_dispatching_does_not_claim_rollback() {
    let (_path, store) = temp_store();
    let run = seed_run(&store);
    let action_id = ActionId::new();
    store
        .actions()
        .prepare(&intent(run, action_id, b"post tweet"))
        .unwrap();
    store.actions().fixture_mark_authorized(action_id).unwrap();
    store
        .actions()
        .fixture_mark_dispatching(action_id, 2)
        .unwrap();
    let decision = store.actions().cancel(action_id).unwrap();
    assert_eq!(
        decision.runtime.lifecycle,
        seyal_agent_core::ActionLifecycle::CancelledAfterDispatch
    );
    assert!(!decision.claims_rollback);
    assert!(!decision.may_retry_effect);
}

#[test]
fn prepare_without_agent_run_fails_closed() {
    let (_path, store) = temp_store();
    let action_id = ActionId::new();
    let missing_run = AgentRunId::new();
    assert_eq!(
        store
            .actions()
            .prepare(&intent(missing_run, action_id, b"no-run")),
        Err(ActionError::UnknownAgentRun)
    );
    assert!(store.actions().get(action_id).unwrap().is_none());
}

fn seed_dispatching(store: &AgentStore, run: AgentRunId, args: &[u8], generation: u64) -> ActionId {
    let action_id = ActionId::new();
    store
        .actions()
        .prepare(&intent(run, action_id, args))
        .unwrap();
    store.actions().fixture_mark_authorized(action_id).unwrap();
    store
        .actions()
        .fixture_mark_dispatching(action_id, generation)
        .unwrap();
    action_id
}

fn boundary_label(boundary: seyal_agent_core::CrashBoundary) -> &'static [u8] {
    match boundary {
        seyal_agent_core::CrashBoundary::AuthorizedBeforeDispatchCommit => b"auth-boundary",
        seyal_agent_core::CrashBoundary::BeforeIntentPersist => b"before-intent",
        seyal_agent_core::CrashBoundary::AfterPrepared => b"after-prepared",
        _ => b"other",
    }
}

#[test]
fn recover_cannot_persist_post_dispatch_as_prepared_without_known_not_dispatched() {
    let (_path, store) = temp_store();
    let run = seed_run(&store);
    let illegal_boundaries = [
        seyal_agent_core::CrashBoundary::AuthorizedBeforeDispatchCommit,
        seyal_agent_core::CrashBoundary::BeforeIntentPersist,
        seyal_agent_core::CrashBoundary::AfterPrepared,
    ];
    for boundary in illegal_boundaries {
        let action_id = seed_dispatching(&store, run, boundary_label(boundary), 11);
        let digest = store.actions().get(action_id).unwrap().unwrap().digest();
        let result = store.actions().recover(
            action_id,
            boundary,
            &seyal_agent_core::RecoveryEvidence::none(),
        );
        let record = store.actions().get_record(action_id).unwrap().unwrap();
        assert_eq!(
            store.actions().get(action_id).unwrap().unwrap().digest(),
            digest
        );
        assert_ne!(
            record.runtime.lifecycle,
            seyal_agent_core::ActionLifecycle::Prepared,
            "{boundary:?} persisted Prepared"
        );
        match boundary {
            seyal_agent_core::CrashBoundary::BeforeIntentPersist => {
                assert_eq!(result, Err(ActionError::IllegalLifecycle));
                assert_eq!(
                    record.runtime.lifecycle,
                    seyal_agent_core::ActionLifecycle::Dispatching
                );
            }
            _ => {
                let decision = result.unwrap();
                assert_eq!(
                    decision.runtime.lifecycle,
                    seyal_agent_core::ActionLifecycle::EffectUnknown
                );
                assert_eq!(
                    record.runtime.lifecycle,
                    seyal_agent_core::ActionLifecycle::EffectUnknown
                );
                assert!(!decision.may_retry_effect);
            }
        }
    }
}

#[test]
fn cancelled_after_dispatch_recover_cannot_persist_prepared() {
    let (_path, store) = temp_store();
    let run = seed_run(&store);
    let action_id = seed_dispatching(&store, run, b"cancelled-recover", 5);
    store.actions().cancel(action_id).unwrap();
    let digest = store.actions().get(action_id).unwrap().unwrap().digest();
    let decision = store
        .actions()
        .recover(
            action_id,
            seyal_agent_core::CrashBoundary::AuthorizedBeforeDispatchCommit,
            &seyal_agent_core::RecoveryEvidence::none(),
        )
        .unwrap();
    assert_eq!(
        decision.runtime.lifecycle,
        seyal_agent_core::ActionLifecycle::CancelledAfterDispatch
    );
    let record = store.actions().get_record(action_id).unwrap().unwrap();
    assert_eq!(
        record.runtime.lifecycle,
        seyal_agent_core::ActionLifecycle::CancelledAfterDispatch
    );
    assert_eq!(
        store.actions().get(action_id).unwrap().unwrap().digest(),
        digest
    );
    assert_eq!(
        store.actions().reconcile(
            action_id,
            &seyal_agent_core::RecoveryEvidence::known_not_dispatched(5),
        ),
        Err(ActionError::Recovery(
            seyal_agent_core::RecoveryError::IllegalReconcile
        ))
    );
    let after = store.actions().get_record(action_id).unwrap().unwrap();
    assert_eq!(
        after.runtime.lifecycle,
        seyal_agent_core::ActionLifecycle::CancelledAfterDispatch
    );
}

#[test]
fn reconcile_prepared_cannot_persist_succeeded() {
    let (_path, store) = temp_store();
    let run = seed_run(&store);
    let action_id = ActionId::new();
    store
        .actions()
        .prepare(&intent(run, action_id, b"no-dispatch"))
        .unwrap();
    let marker = seyal_agent_core::CausalMarker {
        kind: seyal_agent_core::CausalMarkerKind::OperationId,
        bytes: [8; 16],
    };
    assert_eq!(
        store.actions().reconcile(
            action_id,
            &seyal_agent_core::RecoveryEvidence::causal_success(1, marker),
        ),
        Err(ActionError::Recovery(
            seyal_agent_core::RecoveryError::IllegalReconcile
        ))
    );
    let record = store.actions().get_record(action_id).unwrap().unwrap();
    assert_eq!(
        record.runtime.lifecycle,
        seyal_agent_core::ActionLifecycle::Prepared
    );
}
