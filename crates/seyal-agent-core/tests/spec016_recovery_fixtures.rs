//! SPEC-016 §§9–16 / §22 recovery fixtures (EffectUnknown / reconciliation).

use seyal_agent_core::{
    linearize_cancel, reconcile, recover, ActionId, ActionLifecycle, ActionRuntime, AgentRunId,
    CausalMarker, CausalMarkerKind, CrashBoundary, EffectClass, RecoveryEvidence,
};

fn dispatching() -> ActionRuntime {
    let mut runtime = ActionRuntime::prepared();
    runtime.lifecycle = ActionLifecycle::Dispatching;
    runtime.dispatch_generation = Some(1);
    runtime
}

#[test]
fn crash_matrix_is_deterministic_and_never_blindly_retries_non_replayable() {
    let action = ActionId::new();
    let run = AgentRunId::new();
    let cases = [
        CrashBoundary::BeforeIntentPersist,
        CrashBoundary::AfterPrepared,
        CrashBoundary::AuthorizedBeforeDispatchCommit,
        CrashBoundary::DispatchingBeforeInvocation,
        CrashBoundary::DuringExecutor,
        CrashBoundary::AfterEffectBeforeResultPersist,
        CrashBoundary::AfterResultPersist,
    ];
    for boundary in cases {
        let mut runtime = ActionRuntime::prepared();
        match boundary {
            CrashBoundary::AuthorizedBeforeDispatchCommit => {
                runtime.lifecycle = ActionLifecycle::Authorized;
            }
            CrashBoundary::DispatchingBeforeInvocation
            | CrashBoundary::DuringExecutor
            | CrashBoundary::AfterEffectBeforeResultPersist
            | CrashBoundary::AfterResultPersist => {
                runtime = dispatching();
            }
            _ => {}
        }
        let decision = recover(
            action,
            run,
            EffectClass::NonReplayable,
            &runtime,
            boundary,
            &RecoveryEvidence::none(),
        )
        .unwrap();
        assert!(!decision.may_retry_effect, "blind retry at {boundary:?}");
        assert!(!decision.claims_rollback);
        if matches!(
            boundary,
            CrashBoundary::DispatchingBeforeInvocation
                | CrashBoundary::DuringExecutor
                | CrashBoundary::AfterEffectBeforeResultPersist
                | CrashBoundary::AfterResultPersist
        ) {
            assert_eq!(decision.runtime.lifecycle, ActionLifecycle::EffectUnknown);
            assert!(decision.attention.is_some());
        }
    }
}

#[test]
fn idempotent_executor_without_validated_contract_is_still_effect_unknown() {
    let decision = recover(
        ActionId::new(),
        AgentRunId::new(),
        EffectClass::IdempotentExternal,
        &dispatching(),
        CrashBoundary::DuringExecutor,
        &RecoveryEvidence::untrusted_idempotency_claim(),
    )
    .unwrap();
    assert_eq!(decision.runtime.lifecycle, ActionLifecycle::EffectUnknown);
    assert!(!decision.may_retry_effect);
}

#[test]
fn late_known_not_dispatched_after_unknown_returns_to_prepared() {
    let mut unknown = dispatching();
    unknown.lifecycle = ActionLifecycle::EffectUnknown;
    let decision = reconcile(
        ActionId::new(),
        AgentRunId::new(),
        EffectClass::NonReplayable,
        &unknown,
        &RecoveryEvidence::known_not_dispatched(1),
    )
    .unwrap();
    assert_eq!(decision.runtime.lifecycle, ActionLifecycle::Prepared);
    assert!(decision.fresh_authorization_required);
    assert!(!decision.may_retry_effect);
}

#[test]
fn cancelled_after_dispatch_is_reconciliation_only() {
    let decision = linearize_cancel(&dispatching());
    assert_eq!(
        decision.runtime.lifecycle,
        ActionLifecycle::CancelledAfterDispatch
    );
    assert!(!decision.may_retry_effect);
    assert!(!decision.claims_rollback);
    let marker = CausalMarker {
        kind: CausalMarkerKind::TransactionId,
        bytes: [4; 16],
    };
    let resolved = reconcile(
        ActionId::new(),
        AgentRunId::new(),
        EffectClass::NonReplayable,
        &decision.runtime,
        &RecoveryEvidence::causal_failed_known(1, marker),
    )
    .unwrap();
    assert_eq!(resolved.runtime.lifecycle, ActionLifecycle::FailedKnown);
}

#[test]
fn untrusted_terminal_narration_cannot_mint_success() {
    let mut unknown = dispatching();
    unknown.lifecycle = ActionLifecycle::EffectUnknown;
    let decision = reconcile(
        ActionId::new(),
        AgentRunId::new(),
        EffectClass::NonReplayable,
        &unknown,
        &RecoveryEvidence::untrusted_narration(),
    )
    .unwrap();
    assert_eq!(decision.runtime.lifecycle, ActionLifecycle::EffectUnknown);
}
