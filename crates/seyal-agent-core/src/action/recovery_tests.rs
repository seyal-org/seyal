use super::*;
use crate::action::intent::EffectClass;
use crate::identity::{ActionId, AgentRunId};

fn ids() -> (ActionId, AgentRunId) {
    (ActionId::new(), AgentRunId::new())
}

fn dispatching() -> ActionRuntime {
    ActionRuntime {
        lifecycle: ActionLifecycle::Dispatching,
        dispatch_generation: Some(7),
        authorization_invalidated: false,
        cancel_requested: false,
        reconciliation_attempts: 0,
        automatic_budget: DEFAULT_AUTOMATIC_RECONCILIATION_BUDGET,
    }
}

#[test]
fn crash_before_intent_persist_claims_nothing() {
    let (action, run) = ids();
    let decision = recover(
        action,
        run,
        EffectClass::NonReplayable,
        &ActionRuntime::prepared(),
        CrashBoundary::BeforeIntentPersist,
        &RecoveryEvidence::none(),
    )
    .unwrap();
    assert!(!decision.may_retry_effect);
    assert!(!decision.may_reconsider_intent);
    assert_eq!(decision.runtime.lifecycle, ActionLifecycle::Prepared);
}

#[test]
fn authorized_crash_before_dispatch_commit_invalidates_authorization() {
    let (action, run) = ids();
    let mut runtime = ActionRuntime::prepared();
    runtime.lifecycle = ActionLifecycle::Authorized;
    let decision = recover(
        action,
        run,
        EffectClass::Pure,
        &runtime,
        CrashBoundary::AuthorizedBeforeDispatchCommit,
        &RecoveryEvidence::none(),
    )
    .unwrap();
    assert_eq!(decision.runtime.lifecycle, ActionLifecycle::Prepared);
    assert!(decision.runtime.authorization_invalidated);
    assert!(decision.fresh_authorization_required);
    assert!(!decision.may_retry_effect);
}

#[test]
fn dispatching_crash_without_evidence_is_effect_unknown_and_never_retried() {
    let (action, run) = ids();
    let decision = recover(
        action,
        run,
        EffectClass::NonReplayable,
        &dispatching(),
        CrashBoundary::DispatchingBeforeInvocation,
        &RecoveryEvidence::none(),
    )
    .unwrap();
    assert_eq!(decision.runtime.lifecycle, ActionLifecycle::EffectUnknown);
    assert!(!decision.may_retry_effect);
    assert!(decision.attention.is_some());
    assert!(!decision.claims_rollback);
}

#[test]
fn known_not_dispatched_returns_to_prepared_with_fresh_authorization() {
    let (action, run) = ids();
    let decision = recover(
        action,
        run,
        EffectClass::NonReplayable,
        &dispatching(),
        CrashBoundary::DispatchingBeforeInvocation,
        &RecoveryEvidence::known_not_dispatched(7),
    )
    .unwrap();
    assert_eq!(decision.runtime.lifecycle, ActionLifecycle::Prepared);
    assert!(decision.fresh_authorization_required);
    assert!(decision.runtime.authorization_invalidated);
    assert!(decision.runtime.dispatch_generation.is_none());
    assert!(!decision.may_retry_effect);
}

#[test]
fn result_persist_failure_without_causal_evidence_does_not_retry() {
    let (action, run) = ids();
    let decision = recover(
        action,
        run,
        EffectClass::NonIdempotentExternal,
        &dispatching(),
        CrashBoundary::AfterEffectBeforeResultPersist,
        &RecoveryEvidence::none(),
    )
    .unwrap();
    assert_eq!(decision.runtime.lifecycle, ActionLifecycle::EffectUnknown);
    assert!(!decision.may_retry_effect);
}

#[test]
fn causal_success_may_resolve_unknown_effect() {
    let (action, run) = ids();
    let mut unknown = dispatching();
    unknown.lifecycle = ActionLifecycle::EffectUnknown;
    let marker = CausalMarker {
        kind: CausalMarkerKind::OperationId,
        bytes: [9; 16],
    };
    let decision = reconcile(
        action,
        run,
        &unknown,
        &RecoveryEvidence::causal_success(7, marker),
    )
    .unwrap();
    assert_eq!(decision.runtime.lifecycle, ActionLifecycle::Succeeded);
    assert!(!decision.may_retry_effect);
}

#[test]
fn observed_state_without_causal_marker_stays_unknown() {
    let (action, run) = ids();
    let mut unknown = dispatching();
    unknown.lifecycle = ActionLifecycle::EffectUnknown;
    let decision = reconcile(
        action,
        run,
        &unknown,
        &RecoveryEvidence::observed_state_without_causal_marker(),
    )
    .unwrap();
    assert_eq!(decision.runtime.lifecycle, ActionLifecycle::EffectUnknown);
    assert!(!decision.may_retry_effect);
}

#[test]
fn untrusted_idempotency_is_treated_as_absent() {
    let (action, run) = ids();
    let decision = recover(
        action,
        run,
        EffectClass::IdempotentExternal,
        &dispatching(),
        CrashBoundary::DuringExecutor,
        &RecoveryEvidence::untrusted_idempotency_claim(),
    )
    .unwrap();
    assert_eq!(decision.runtime.lifecycle, ActionLifecycle::EffectUnknown);
    assert!(!decision.may_retry_effect);
    assert!(!decision.may_query_status);
}

#[test]
fn validated_idempotency_allows_status_query_not_blind_retry() {
    let (action, run) = ids();
    let decision = recover(
        action,
        run,
        EffectClass::IdempotentExternal,
        &dispatching(),
        CrashBoundary::DuringExecutor,
        &RecoveryEvidence::validated_idempotency_contract(7),
    )
    .unwrap();
    assert_eq!(decision.runtime.lifecycle, ActionLifecycle::Dispatching);
    assert!(decision.may_query_status);
    assert!(!decision.may_retry_effect);
}

#[test]
fn cancel_after_dispatch_never_claims_rollback() {
    let decision = linearize_cancel(&dispatching());
    assert_eq!(
        decision.runtime.lifecycle,
        ActionLifecycle::CancelledAfterDispatch
    );
    assert!(!decision.claims_rollback);
    assert!(!decision.may_retry_effect);
}

#[test]
fn cancel_before_dispatch_prevents_invocation() {
    let mut authorized = ActionRuntime::prepared();
    authorized.lifecycle = ActionLifecycle::Authorized;
    let decision = linearize_cancel(&authorized);
    assert_eq!(
        decision.runtime.lifecycle,
        ActionLifecycle::CancelledBeforeDispatch
    );
    assert!(!decision.may_retry_effect);
}

#[test]
fn completion_races_cancel_and_remains_admissible() {
    let mut cancelled = dispatching();
    cancelled.lifecycle = ActionLifecycle::CancelledAfterDispatch;
    cancelled.cancel_requested = true;
    let (action, run) = ids();
    let marker = CausalMarker {
        kind: CausalMarkerKind::CasWitness,
        bytes: [2; 16],
    };
    let decision = reconcile(
        action,
        run,
        &cancelled,
        &RecoveryEvidence::causal_success(7, marker),
    )
    .unwrap();
    assert_eq!(decision.runtime.lifecycle, ActionLifecycle::Succeeded);
    assert!(!decision.claims_rollback);
}

#[test]
fn operator_ack_cannot_fabricate_known_outcome() {
    let (action, run) = ids();
    let mut unknown = dispatching();
    unknown.lifecycle = ActionLifecycle::EffectUnknown;
    let decision = reconcile(
        action,
        run,
        &unknown,
        &RecoveryEvidence::operator_ack_without_causal_evidence(),
    )
    .unwrap();
    assert_eq!(decision.runtime.lifecycle, ActionLifecycle::EffectUnknown);
}

#[test]
fn stale_dispatch_generation_cannot_commit() {
    let (action, run) = ids();
    let err = recover(
        action,
        run,
        EffectClass::NonReplayable,
        &dispatching(),
        CrashBoundary::DuringExecutor,
        &RecoveryEvidence::known_not_dispatched(99),
    )
    .unwrap_err();
    assert_eq!(err, RecoveryError::StaleDispatchGeneration);
}

#[test]
fn budget_exhaustion_stops_automatic_reschedule() {
    let (action, run) = ids();
    let mut runtime = dispatching();
    runtime.lifecycle = ActionLifecycle::EffectUnknown;
    runtime.reconciliation_attempts = DEFAULT_AUTOMATIC_RECONCILIATION_BUDGET - 1;
    let decision = reconcile(action, run, &runtime, &RecoveryEvidence::none()).unwrap();
    assert!(decision.automatic_reschedule_stopped);
    assert_eq!(decision.runtime.lifecycle, ActionLifecycle::EffectUnknown);
    assert!(decision.attention.is_some());
    assert!(!decision.may_retry_effect);
}

#[test]
fn recovery_is_pure_control_plane_and_does_not_touch_terminal_state() {
    let (action, run) = ids();
    let decision = recover(
        action,
        run,
        EffectClass::NonReplayable,
        &ActionRuntime::prepared(),
        CrashBoundary::AfterPrepared,
        &RecoveryEvidence::none(),
    )
    .unwrap();
    assert!(!decision.may_retry_effect);
    assert!(decision.attention.is_none());
}
