use super::*;

#[test]
fn n_failures_pause_and_further_admits_do_not_reset() {
    let mut policy = PersistFailurePolicy::new(3, 30_000);
    policy.record_failure(1_000);
    assert_eq!(policy.health(), PersistHealth::Degraded);
    assert!(policy.admit(1_001).is_ok());
    policy.record_failure(1_002);
    policy.record_failure(1_003);
    assert_eq!(policy.health(), PersistHealth::Paused);
    assert_eq!(
        policy.admit(1_004),
        Err(PersistAdmitError::Paused(
            PersistPauseReason::ConsecutiveFailureBudgetExhausted
        ))
    );
    policy.record_success();
    assert_eq!(policy.health(), PersistHealth::Paused);
}

#[test]
fn deadline_pause_stops_retries_before_budget() {
    let mut policy = PersistFailurePolicy::new(8, 1_000);
    policy.record_failure(10);
    assert_eq!(policy.health(), PersistHealth::Degraded);
    assert_eq!(
        policy.admit(10 + 1_000),
        Err(PersistAdmitError::Paused(
            PersistPauseReason::RetryDeadlineExceeded
        ))
    );
}

#[test]
fn success_clears_degraded_but_resume_requires_healthy_store() {
    let mut policy = PersistFailurePolicy::new(3, 30_000);
    policy.record_failure(1);
    policy.record_success();
    assert_eq!(policy.health(), PersistHealth::Healthy);
    policy.record_failure(2);
    policy.record_failure(3);
    policy.record_failure(4);
    assert!(!policy.resume(false));
    assert_eq!(policy.health(), PersistHealth::Paused);
    assert!(policy.resume(true));
    assert_eq!(policy.health(), PersistHealth::Healthy);
    assert!(policy.admit(5).is_ok());
}

#[test]
fn persist_health_never_gates_terminal_progress() {
    for health in [
        PersistHealth::Healthy,
        PersistHealth::Degraded,
        PersistHealth::Paused,
    ] {
        assert!(!health.may_gate_terminal_progress());
    }
    assert!(!PersistHealth::Paused.allows_new_effect());
    assert!(PersistHealth::Healthy.allows_new_effect());
}

#[test]
fn backoff_is_bounded() {
    let mut policy = PersistFailurePolicy::new(3, 30_000);
    assert_eq!(policy.backoff_ms(), 0);
    policy.record_failure(1);
    assert_eq!(policy.backoff_ms(), 50);
    policy.record_failure(2);
    assert_eq!(policy.backoff_ms(), 100);
    assert!(policy.backoff_ms() <= 1_000);
}

#[test]
fn resume_crash_boundary_is_conservative() {
    assert_eq!(
        resume_crash_boundary(ActionLifecycle::Prepared),
        CrashBoundary::AfterPrepared
    );
    assert_eq!(
        resume_crash_boundary(ActionLifecycle::Dispatching),
        CrashBoundary::AfterEffectBeforeResultPersist
    );
}
