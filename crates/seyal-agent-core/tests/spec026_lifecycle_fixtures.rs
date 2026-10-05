//! SPEC-026 §15 required fixtures 1–29.
//!
//! Each test asserts the identity outcome named in the fixture table:
//! same AgentRun / new AgentRun / new Attempt / reconciliation-required.

use seyal_agent_core::{
    AcceptanceContractMode, AccountingValue, AdapterId, AgentDomain, AgentRunLifecycle,
    AgentRunLineage, AttachmentAccess, AttemptDisposition, AttemptLifecycle, AttemptOrigin,
    BindingGeneration, ClientSessionId, ControlGeneration, DomainError, ExecutionHostKind,
    ExecutionLiveness, ExecutionRef, ExternalIdentityKey, LaunchDescriptorRef, ObservationFact,
    ObservationKind, ObservationRecordResult, ResumabilityFact, RouteOfferingId, RoutingDecision,
    SelectionKind, TerminationKind, TerminationSource, WorkItemLifecycle, WorkItemOutcome,
    WorkScopeKind,
};

fn routing_decision(manifest_generation: u64) -> RoutingDecision {
    RoutingDecision {
        adapter_id: AdapterId::new(),
        adapter_manifest_generation: manifest_generation,
        route_offering_id: RouteOfferingId::new(),
        execution_host_kind: ExecutionHostKind::StandaloneProcess,
        launch_descriptor_ref: LaunchDescriptorRef::new(manifest_generation),
        selection_kind: SelectionKind::Singleton,
    }
}

fn seeded() -> (
    AgentDomain,
    seyal_agent_core::WorkItemId,
    seyal_agent_core::AttemptId,
    seyal_agent_core::AgentRunId,
) {
    let mut domain = AgentDomain::new();
    let scope = domain.create_work_scope(WorkScopeKind::Repository);
    let item = domain.create_work_item(scope).unwrap();
    let attempt = domain.create_attempt(item).unwrap();
    let run = domain.create_agent_run(attempt).unwrap();
    (domain, item, attempt, run)
}

fn start_to_active(domain: &mut AgentDomain, run: seyal_agent_core::AgentRunId) {
    domain
        .start_prepare_and_dispatch(run, routing_decision(1))
        .unwrap();
    domain.activate_agent_run(run).unwrap();
}

#[test]
fn fixture_01_start_active_harness_completes_same_run_no_work_item_outcome() {
    let (mut domain, item, _attempt, run) = seeded();
    start_to_active(&mut domain, run);
    domain
        .terminate_completed(run, TerminationSource::Harness)
        .unwrap();
    let agent = domain.agent_run(run).unwrap();
    assert_eq!(agent.lifecycle(), AgentRunLifecycle::Terminated);
    assert_eq!(
        agent.termination().unwrap().kind,
        TerminationKind::Completed
    );
    assert_eq!(
        domain.work_item(item).unwrap().lifecycle(),
        WorkItemLifecycle::Open
    );
    assert_eq!(domain.work_item(item).unwrap().outcome(), None);
}

#[test]
fn fixture_02_cancel_in_created_or_prepared_terminates_cancelled() {
    let (mut domain, _item, attempt, run) = seeded();
    domain
        .prepare_agent_run(run, routing_decision(1))
        .unwrap();
    let control = domain.agent_run(run).unwrap().control_generation();
    domain.cancel_agent_run(run, control, None, false).unwrap();
    assert_eq!(
        domain.agent_run(run).unwrap().lifecycle(),
        AgentRunLifecycle::Terminated
    );
    assert_eq!(
        domain.agent_run(run).unwrap().termination().unwrap().kind,
        TerminationKind::Cancelled
    );
    domain
        .close_attempt(attempt, AttemptDisposition::Cancelled)
        .unwrap();
    assert_eq!(
        domain.attempt(attempt).unwrap().lifecycle(),
        AttemptLifecycle::Closed
    );
}

#[test]
fn fixture_03_cancel_while_active_terminating_then_cancelled() {
    let (mut domain, _item, _attempt, run) = seeded();
    start_to_active(&mut domain, run);
    let control = domain.agent_run(run).unwrap().control_generation();
    domain.cancel_agent_run(run, control, None, false).unwrap();
    assert_eq!(
        domain.agent_run(run).unwrap().lifecycle(),
        AgentRunLifecycle::Terminating
    );
    domain.confirm_cancel_termination(run).unwrap();
    assert_eq!(
        domain.agent_run(run).unwrap().lifecycle(),
        AgentRunLifecycle::Terminated
    );
    assert_eq!(
        domain.agent_run(run).unwrap().termination().unwrap().kind,
        TerminationKind::Cancelled
    );
}

#[test]
fn fixture_04_cancel_ambiguous_effect_reconciliation_required() {
    let (mut domain, _item, _attempt, run) = seeded();
    start_to_active(&mut domain, run);
    let control = domain.agent_run(run).unwrap().control_generation();
    assert_eq!(
        domain.cancel_agent_run(run, control, None, true),
        Err(DomainError::ReconciliationRequired)
    );
    assert_eq!(
        domain.agent_run(run).unwrap().resumability(),
        ResumabilityFact::ReconciliationRequired
    );
    assert_eq!(
        domain.agent_run(run).unwrap().lifecycle(),
        AgentRunLifecycle::Active
    );
}

#[test]
fn fixture_05_client_detach_reconnect_same_attempt_and_run() {
    let (mut domain, _item, attempt, run) = seeded();
    start_to_active(&mut domain, run);
    let session = ClientSessionId::new();
    domain
        .attach_client(run, session, AttachmentAccess::Control)
        .unwrap();
    domain.detach_client(session).unwrap();
    let session2 = ClientSessionId::new();
    domain
        .attach_client(run, session2, AttachmentAccess::Control)
        .unwrap();
    assert_eq!(domain.agent_run(run).unwrap().attempt_id(), attempt);
    assert_eq!(
        domain.agent_run(run).unwrap().lifecycle(),
        AgentRunLifecycle::Active
    );
    assert_eq!(
        domain.retry_budget_consumed(domain.agent_run(run).unwrap().work_item_id()),
        0
    );
}

#[test]
fn fixture_06_second_client_observe_control_not_authorized() {
    let (mut domain, _item, _attempt, run) = seeded();
    start_to_active(&mut domain, run);
    let owner = ClientSessionId::new();
    domain
        .attach_client(run, owner, AttachmentAccess::Control)
        .unwrap();
    let observer = ClientSessionId::new();
    domain
        .attach_client(run, observer, AttachmentAccess::Observe)
        .unwrap();
    let epoch = domain.agent_run(run).unwrap().control_generation();
    let rev = domain.agent_run(run).unwrap().run_revision();
    assert_eq!(
        domain.client_control(observer, run, epoch, rev),
        Err(DomainError::NotAuthorized)
    );
}

#[test]
fn fixture_07_adapter_crash_external_alive_same_run_new_generation() {
    let (mut domain, _item, _attempt, run) = seeded();
    start_to_active(&mut domain, run);
    domain
        .set_observation_fact(run, ObservationFact::Disconnected)
        .unwrap();
    let prior = domain.agent_run(run).unwrap().binding_generation();
    let next = domain.rebind_agent_run(run, prior).unwrap();
    assert!(next > prior);
    assert_eq!(domain.agent_run(run).unwrap().id(), run);
    assert_eq!(
        domain.agent_run(run).unwrap().observation(),
        ObservationFact::Disconnected
    );
}

#[test]
fn fixture_08_stale_adapter_cancel_after_rebind() {
    let (mut domain, _item, _attempt, run) = seeded();
    start_to_active(&mut domain, run);
    let stale = domain.agent_run(run).unwrap().binding_generation();
    let _next = domain.rebind_agent_run(run, stale).unwrap();
    assert_eq!(
        domain.validate_binding_generation(run, stale),
        Err(DomainError::StaleBinding {
            current: domain.agent_run(run).unwrap().binding_generation(),
            presented: stale,
        })
    );
    let result = domain
        .record_observation(run, 7, stale, Some(1), ObservationKind::Output, false)
        .unwrap();
    assert_eq!(result, ObservationRecordResult::StaleRetained);
    assert!(domain.observation_log().iter().any(|o| o.stale));
}

#[test]
fn fixture_09_never_issued_generation_stale_binding() {
    let (domain, _item, _attempt, run) = seeded();
    let never = BindingGeneration::from_raw(99).unwrap();
    assert_eq!(
        domain.validate_binding_generation(run, never),
        Err(DomainError::StaleBinding {
            current: BindingGeneration::FIRST,
            presented: never,
        })
    );
}

#[test]
fn fixture_10_resume_with_retained_prerequisites_same_run_no_retry() {
    let (mut domain, item, attempt, run) = seeded();
    start_to_active(&mut domain, run);
    domain
        .set_resumability(run, ResumabilityFact::BehavioralResumeAvailable)
        .unwrap();
    domain.resume_agent_run(run).unwrap();
    assert_eq!(domain.agent_run(run).unwrap().attempt_id(), attempt);
    assert_eq!(domain.retry_budget_consumed(item), 0);
}

#[test]
fn fixture_11_resume_unavailable_continuation_is_new_attempt_and_run() {
    let (mut domain, item, attempt, run) = seeded();
    start_to_active(&mut domain, run);
    domain
        .set_resumability(run, ResumabilityFact::ResumeUnavailable)
        .unwrap();
    assert!(matches!(
        domain.resume_agent_run(run),
        Err(DomainError::ResumeNotAvailable {
            reason: ResumabilityFact::ResumeUnavailable
        })
    ));
    domain
        .terminate_failed(run, TerminationSource::Provider)
        .unwrap();
    let ids = domain
        .fresh_retry(attempt, AttemptDisposition::Rejected)
        .unwrap();
    assert_ne!(ids.attempt_id, attempt);
    assert_ne!(ids.agent_run_id, run);
    assert_eq!(ids.work_item_id, item);
    assert!(matches!(
        domain.attempt(ids.attempt_id).unwrap().origin(),
        AttemptOrigin::RetryOf(prior) if prior == attempt
    ));
}

#[test]
fn fixture_12_same_strategy_retry_new_attempt() {
    let (mut domain, item, attempt, run) = seeded();
    start_to_active(&mut domain, run);
    domain
        .set_attempt_usage(
            attempt,
            AccountingValue::Observed(10),
            AccountingValue::Observed(3),
        )
        .unwrap();
    domain
        .terminate_failed(run, TerminationSource::Harness)
        .unwrap();
    let ids = domain
        .fresh_retry(attempt, AttemptDisposition::Rejected)
        .unwrap();
    assert_ne!(ids.attempt_id, attempt);
    assert_ne!(ids.agent_run_id, run);
    let prior = domain.attempt(attempt).unwrap();
    assert_eq!(prior.disposition(), Some(AttemptDisposition::Rejected));
    assert_eq!(prior.usage(), AccountingValue::Observed(10));
    assert_eq!(prior.cost(), AccountingValue::Observed(3));
    assert_eq!(domain.retry_budget_consumed(item), 1);
    assert!(matches!(
        domain.attempt(ids.attempt_id).unwrap().origin(),
        AttemptOrigin::RetryOf(_)
    ));
}

#[test]
fn fixture_13_fork_new_attempt_and_lineage() {
    let (mut domain, item, attempt, run) = seeded();
    start_to_active(&mut domain, run);
    let ids = domain.fork_run(run).unwrap();
    assert_ne!(ids.attempt_id, attempt);
    assert_ne!(ids.agent_run_id, run);
    assert_eq!(ids.work_item_id, item);
    assert!(matches!(
        domain.attempt(ids.attempt_id).unwrap().origin(),
        AttemptOrigin::ForkOf(parent) if parent == attempt
    ));
    assert_eq!(
        domain.agent_run(ids.agent_run_id).unwrap().lineage(),
        Some(AgentRunLineage::ForkOf(run))
    );
}

#[test]
fn fixture_14_two_parallel_candidates() {
    let (mut domain, item, attempt, run) = seeded();
    start_to_active(&mut domain, run);
    let a = domain.parallel_candidate(attempt).unwrap();
    let b = domain.parallel_candidate(attempt).unwrap();
    assert_ne!(a.attempt_id, b.attempt_id);
    assert_ne!(a.agent_run_id, b.agent_run_id);
    assert_eq!(a.work_item_id, item);
    domain
        .terminate_failed(a.agent_run_id, TerminationSource::Policy)
        .unwrap();
    domain
        .close_attempt(a.attempt_id, AttemptDisposition::Superseded)
        .unwrap();
    assert_eq!(
        domain.attempt(a.attempt_id).unwrap().disposition(),
        Some(AttemptDisposition::Superseded)
    );
    let _ = b;
}

#[test]
fn fixture_15_second_agent_run_multiple_runs_not_permitted() {
    let (mut domain, _item, attempt, _run) = seeded();
    assert_eq!(
        domain.create_agent_run(attempt),
        Err(DomainError::MultipleRunsNotPermitted)
    );
}

#[test]
fn fixture_16_rate_limit_not_started_dispatching_to_prepared() {
    let (mut domain, _item, _attempt, run) = seeded();
    domain
        .start_prepare_and_dispatch(run, routing_decision(1))
        .unwrap();
    assert_eq!(
        domain.agent_run(run).unwrap().lifecycle(),
        AgentRunLifecycle::Dispatching
    );
    domain
        .pre_start_fallback(run, routing_decision(2), true)
        .unwrap();
    let agent = domain.agent_run(run).unwrap();
    assert_eq!(agent.lifecycle(), AgentRunLifecycle::Prepared);
    let new_ref = agent.routing_decision_ref().unwrap();
    assert_eq!(
        domain.routing_decision(new_ref).unwrap().adapter_manifest_generation,
        2
    );
    assert_eq!(agent.id(), run);
}

#[test]
fn fixture_17_dispatch_failure_without_proof_reconciliation() {
    let (mut domain, _item, _attempt, run) = seeded();
    domain
        .start_prepare_and_dispatch(run, routing_decision(1))
        .unwrap();
    assert_eq!(
        domain.pre_start_fallback(run, routing_decision(2), false),
        Err(DomainError::ReconciliationRequired)
    );
    assert_eq!(
        domain.agent_run(run).unwrap().resumability(),
        ResumabilityFact::ReconciliationRequired
    );
}

#[test]
fn fixture_18_backend_restart_active_reconciliation() {
    let (mut domain, _item, _attempt, run) = seeded();
    start_to_active(&mut domain, run);
    let session = ClientSessionId::new();
    domain
        .attach_client(run, session, AttachmentAccess::Control)
        .unwrap();
    let prior_binding = domain.agent_run(run).unwrap().binding_generation();
    let prior_control = domain.agent_run(run).unwrap().control_generation();
    let (next_b, next_c) = domain.backend_restart_recovery(run).unwrap();
    assert!(next_b > prior_binding);
    assert!(next_c > prior_control);
    let agent = domain.agent_run(run).unwrap();
    assert_eq!(agent.execution_liveness(), ExecutionLiveness::Unknown);
    assert_eq!(agent.observation(), ObservationFact::Disconnected);
    assert_eq!(
        agent.resumability(),
        ResumabilityFact::ReconciliationRequired
    );
    assert_eq!(
        domain.detach_client(session),
        Err(DomainError::NotAuthorized)
    );
}

#[test]
fn fixture_19_backend_restart_dispatching_no_blind_redispatch() {
    let (mut domain, _item, _attempt, run) = seeded();
    domain
        .start_prepare_and_dispatch(run, routing_decision(1))
        .unwrap();
    domain.backend_restart_recovery(run).unwrap();
    assert_eq!(
        domain.agent_run(run).unwrap().lifecycle(),
        AgentRunLifecycle::Dispatching
    );
    assert_eq!(
        domain.agent_run(run).unwrap().resumability(),
        ResumabilityFact::ReconciliationRequired
    );
    // Must not auto-activate or re-dispatch.
    assert_ne!(
        domain.agent_run(run).unwrap().lifecycle(),
        AgentRunLifecycle::Active
    );
}

#[test]
fn fixture_20_terminal_runtime_replaces_execution_new_id() {
    let (mut domain, _item, _attempt, run) = seeded();
    start_to_active(&mut domain, run);
    let old = ExecutionRef::new(10);
    let new = ExecutionRef::new(11);
    {
        // Seed current execution via detection-style bind field.
        domain.replace_execution(run, new, old).unwrap();
    }
    assert!(domain.execution_retired(old));
    assert_eq!(
        domain.agent_run(run).unwrap().current_execution(),
        Some(new)
    );
    assert_eq!(
        domain.replace_execution(run, old, old),
        Err(DomainError::InvalidTransition)
    );
}

#[test]
fn fixture_21_repeated_detection_idempotent_one_run() {
    let mut domain = AgentDomain::new();
    let scope = domain.create_work_scope(WorkScopeKind::AdHoc);
    let key = ExternalIdentityKey {
        execution_ref: ExecutionRef::new(42),
        external_identity: 7,
    };
    let (first, dup1) = domain.detect_and_bind(scope, key).unwrap();
    assert!(!dup1);
    let (second, dup2) = domain.detect_and_bind(scope, key).unwrap();
    assert!(dup2);
    assert_eq!(first.agent_run_id, second.agent_run_id);
    assert_eq!(first.attempt_id, second.attempt_id);
    assert_eq!(
        domain
            .work_item(first.work_item_id)
            .unwrap()
            .acceptance_mode(),
        AcceptanceContractMode::HumanFinal
    );
}

#[test]
fn fixture_22_duplicate_observation_acknowledged_no_new_event() {
    let (mut domain, _item, _attempt, run) = seeded();
    start_to_active(&mut domain, run);
    let binding = domain.agent_run(run).unwrap().binding_generation();
    let first = domain
        .record_observation(run, 1, binding, Some(9), ObservationKind::Progress, false)
        .unwrap();
    let second = domain
        .record_observation(run, 1, binding, Some(9), ObservationKind::Progress, false)
        .unwrap();
    assert_eq!(first, ObservationRecordResult::Accepted);
    assert_eq!(second, ObservationRecordResult::DuplicateAcknowledged);
    assert_eq!(
        domain
            .observation_log()
            .iter()
            .filter(|o| o.kind == ObservationKind::Progress)
            .count(),
        1
    );
}

#[test]
fn fixture_23_exited_before_started_retained_order_respected() {
    let (mut domain, _item, _attempt, run) = seeded();
    domain
        .start_prepare_and_dispatch(run, routing_decision(1))
        .unwrap();
    let binding = domain.agent_run(run).unwrap().binding_generation();
    // Out-of-order: exited while still Dispatching / NotStarted — retained.
    domain
        .record_observation(
            run,
            2,
            binding,
            Some(1),
            ObservationKind::HarnessExited,
            false,
        )
        .unwrap();
    assert_eq!(
        domain.agent_run(run).unwrap().lifecycle(),
        AgentRunLifecycle::Dispatching
    );
    domain
        .record_observation(
            run,
            2,
            binding,
            Some(2),
            ObservationKind::HarnessStarted,
            false,
        )
        .unwrap();
    assert_eq!(
        domain.agent_run(run).unwrap().lifecycle(),
        AgentRunLifecycle::Active
    );
}

#[test]
fn fixture_24_late_evidence_after_attempt_closure() {
    let (mut domain, _item, attempt, run) = seeded();
    start_to_active(&mut domain, run);
    domain
        .terminate_completed(run, TerminationSource::Harness)
        .unwrap();
    domain
        .close_attempt(attempt, AttemptDisposition::CandidateAccepted)
        .unwrap();
    let binding = domain.agent_run(run).unwrap().binding_generation();
    let result = domain
        .record_observation(run, 3, binding, Some(1), ObservationKind::Usage, true)
        .unwrap();
    assert_eq!(result, ObservationRecordResult::LateRetained);
    assert_eq!(
        domain.attempt(attempt).unwrap().disposition(),
        Some(AttemptDisposition::CandidateAccepted)
    );
    assert!(domain.observation_log().iter().any(|o| o.late));
}

#[test]
fn fixture_25_mutation_of_finalized_work_item() {
    let (mut domain, item, _attempt, _run) = seeded();
    domain
        .finalize_work_item(item, WorkItemOutcome::Accepted, true)
        .unwrap();
    assert_eq!(
        domain.create_attempt(item),
        Err(DomainError::WorkItemFinalized)
    );
    let related = domain.create_related_work_item(item).unwrap();
    assert_ne!(related, item);
    assert_eq!(domain.work_item(related).unwrap().related_to(), Some(item));
    assert_eq!(
        domain.work_item(related).unwrap().lifecycle(),
        WorkItemLifecycle::Open
    );
}

#[test]
fn fixture_26_critical_persistence_failure_no_published_transition() {
    // Domain-level model of SPEC-026 §13 / fixture 26: a rejected transition
    // (stand-in for persistence failure before commit) publishes no state change.
    let (mut domain, _item, _attempt, run) = seeded();
    let before = *domain.agent_run(run).unwrap();
    assert_eq!(
        domain.dispatch_agent_run(run),
        Err(DomainError::InvalidTransition)
    );
    assert_eq!(*domain.agent_run(run).unwrap(), before);
}

#[test]
fn fixture_27_generation_exhaustion_reconciliation_required() {
    let (mut domain, _item, attempt, _run) = seeded();
    // Restore a run sitting on the last binding generation.
    let max = BindingGeneration::from_raw(u64::MAX).unwrap();
    let run = seyal_agent_core::AgentRunId::new();
    domain
        .restore_agent_run(run, attempt, max, ControlGeneration::FIRST)
        .unwrap();
    assert_eq!(
        domain.rebind_agent_run(run, max),
        Err(DomainError::ReconciliationRequired)
    );
    assert_eq!(
        domain.agent_run(run).unwrap().resumability(),
        ResumabilityFact::ReconciliationRequired
    );
}

#[test]
fn fixture_28_missing_usage_cost_unknown_not_zero() {
    let (domain, _item, attempt, _run) = seeded();
    let a = domain.attempt(attempt).unwrap();
    assert!(a.usage().is_unknown());
    assert!(a.cost().is_unknown());
    assert_ne!(a.usage(), AccountingValue::Observed(0));
    assert_ne!(a.cost(), AccountingValue::Observed(0));
}

#[test]
fn fixture_29_worker_loss_ambiguous_effect_same_run_reconciliation() {
    let (mut domain, _item, attempt, run) = seeded();
    start_to_active(&mut domain, run);
    domain
        .set_resumability(run, ResumabilityFact::ReconciliationRequired)
        .unwrap();
    domain
        .set_observation_fact(run, ObservationFact::Disconnected)
        .unwrap();
    assert_eq!(domain.agent_run(run).unwrap().attempt_id(), attempt);
    assert_eq!(
        domain.resume_agent_run(run),
        Err(DomainError::ReconciliationRequired)
    );
    // No blind retry while reconciliation is required.
    assert_eq!(
        domain.retry_budget_consumed(domain.agent_run(run).unwrap().work_item_id()),
        0
    );
}
