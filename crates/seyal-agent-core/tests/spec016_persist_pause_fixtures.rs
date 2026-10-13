//! SPEC-016 §21 / §25 failure-resource fixtures (persistence-failure pause).

use seyal_agent_core::{
    resume_crash_boundary, ActionLifecycle, PersistFailurePolicy, PersistHealth,
    PersistPauseReason, DEFAULT_PERSIST_FAILURE_BUDGET,
};

#[test]
fn persistent_store_failure_fails_closed_for_new_effects() {
    let mut policy = PersistFailurePolicy::default();
    assert_eq!(DEFAULT_PERSIST_FAILURE_BUDGET, 3);
    for t in 1..=3 {
        assert!(policy.health().allows_new_effect() || policy.health() == PersistHealth::Degraded);
        policy.record_failure(t);
    }
    assert_eq!(policy.health(), PersistHealth::Paused);
    assert!(!policy.health().allows_new_effect());
    assert_eq!(
        policy.pause_reason(),
        Some(PersistPauseReason::ConsecutiveFailureBudgetExhausted)
    );
}

#[test]
fn persist_pause_never_gates_pty_vt_metal() {
    assert!(!PersistHealth::Paused.may_gate_terminal_progress());
    assert!(!include_str!("../src/action/persist_pause.rs").contains("seyal_runtime"));
    assert!(!include_str!("../src/action/persist_pause.rs").contains("seyal_vt"));
}

#[test]
fn resume_maps_dispatching_to_conservative_boundary() {
    assert_eq!(
        resume_crash_boundary(ActionLifecycle::Dispatching),
        seyal_agent_core::CrashBoundary::AfterEffectBeforeResultPersist
    );
}
