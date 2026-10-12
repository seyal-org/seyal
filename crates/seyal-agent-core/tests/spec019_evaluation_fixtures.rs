//! SPEC-019 §17 required fixtures as named production tests.

use seyal_agent_core::{
    ai_cost_per_accepted, attributions_distinct, check_criterion_eligibility, parse_id_bytes,
    required_metric_defs, scm_ci_commit_matches, self_report_implies_accepted,
    unbiased_comparison_allowed, AcceptanceContract, AcceptanceContractId, AcceptanceContractMode,
    AccountingValue, AgentDomain, AggregationKind, AttemptDisposition, AttemptOrigin,
    CalibrationArtifactId, CalibrationState, CohortAttemptRow, CostEvidence, CriterionId,
    CriterionResult, DerivedFeatureId, DerivedFeatureState, DerivedRoutingFeature,
    EvaluationObservation, EvaluationObservationId, EvaluationPlane, EvaluatorClass,
    IndependenceEvidence, LocalCalibrationArtifact, PricingAssumption, PricingAssumptionId,
    PurposeEligibility, QualityAttribution, RouteComparisonBias, RoutingQualityObservation,
    RoutingQualityObservationId, SelectionSupport, TargetRefs, TestIntegrityClass, TimeEvidence,
    UsageObservation, WorkItemOutcome, WorkScopeKind,
};

fn domain_seed(
    mode: AcceptanceContractMode,
) -> (
    AgentDomain,
    seyal_agent_core::WorkItemId,
    seyal_agent_core::AttemptId,
) {
    let mut domain = AgentDomain::new();
    let scope = domain.create_work_scope(WorkScopeKind::Repository);
    let item = domain.create_work_item_with_mode(scope, mode).unwrap();
    let attempt = domain.create_attempt(item).unwrap();
    (domain, item, attempt)
}

fn test_contract(
    mode: AcceptanceContractMode,
    auto: bool,
    forbidden: Vec<u64>,
    independence: bool,
) -> AcceptanceContract {
    AcceptanceContract {
        id: AcceptanceContractId::new(),
        version: 1,
        policy_generation: 1,
        mode,
        criteria: vec![seyal_agent_core::CriterionSpec {
            id: CriterionId::new(),
            required: true,
            eligible_classes: vec![
                EvaluatorClass::DeterministicTest,
                EvaluatorClass::ArchitecturePolicy,
                EvaluatorClass::ScmCiStatus,
                EvaluatorClass::IndependentModelReview,
                EvaluatorClass::HumanDecision,
            ],
            predicate_ref: 7,
            requires_independence: independence,
            min_test_integrity: Some(TestIntegrityClass::TrustedExisting),
            forbidden_path_refs: forbidden,
        }],
        permits_policy_final_auto_accept: auto,
    }
}

fn obs(
    work_item_id: seyal_agent_core::WorkItemId,
    attempt_id: seyal_agent_core::AttemptId,
    class: EvaluatorClass,
    result: CriterionResult,
) -> EvaluationObservation {
    EvaluationObservation {
        id: EvaluationObservationId::new(),
        work_item_id,
        attempt_id,
        agent_run_id: None,
        evaluator_class: class,
        evaluator_version: 1,
        target: TargetRefs {
            repository_generation: 1,
            worktree_fingerprint: 1,
            source_fingerprint: 1,
            artifact_fingerprint: Some(1),
            commit_identity: 100,
            check_identity: 200,
        },
        result,
        criterion_ref: Some(7),
        provenance: 1,
        observed_at_millis: 1,
        reproducibility_ref: None,
        independence: Some(IndependenceEvidence::independent()),
        test_integrity: Some(TestIntegrityClass::TrustedExisting),
        self_report_done: false,
        changed_path_refs: vec![],
        tests_weakened_or_deleted: false,
    }
}

#[test]
fn spec019_17_01_agent_done_tests_fail() {
    let (mut domain, item, attempt) = domain_seed(AcceptanceContractMode::PolicyFinal);
    let mut plane = EvaluationPlane::new();
    let contract = test_contract(AcceptanceContractMode::PolicyFinal, true, vec![], false);
    let cid = plane.register_contract(contract).unwrap();

    let mut done = obs(
        item,
        attempt,
        EvaluatorClass::HarnessReport,
        CriterionResult::Satisfied,
    );
    done.self_report_done = true;
    let fail = obs(
        item,
        attempt,
        EvaluatorClass::DeterministicTest,
        CriterionResult::NotSatisfied,
    );
    let oid1 = plane.submit_observation(done);
    let oid2 = plane.submit_observation(fail);
    let eid = plane.evaluate(cid, &[oid1, oid2], None, None).unwrap();
    let evaluation = plane.evaluation(eid).unwrap();

    assert!(!plane.policy_final_may_accept(eid).unwrap());
    assert_ne!(
        evaluation.recommended_outcome,
        Some(WorkItemOutcome::Accepted)
    );
    assert!(!self_report_implies_accepted(evaluation));
    // Domain finalize still requires authorization; self-report does not authorize.
    assert!(domain
        .finalize_work_item(item, WorkItemOutcome::Accepted, false)
        .is_err());
}

#[test]
fn spec019_17_02_tests_pass_forbidden_file_changed() {
    let (_domain, item, attempt) = domain_seed(AcceptanceContractMode::PolicyFinal);
    let mut plane = EvaluationPlane::new();
    let contract = test_contract(AcceptanceContractMode::PolicyFinal, true, vec![99], false);
    let cid = plane.register_contract(contract).unwrap();
    let mut o = obs(
        item,
        attempt,
        EvaluatorClass::DeterministicTest,
        CriterionResult::Satisfied,
    );
    o.changed_path_refs = vec![99];
    let oid = plane.submit_observation(o);
    let eid = plane.evaluate(cid, &[oid], None, None).unwrap();
    assert!(!plane.policy_final_may_accept(eid).unwrap());
    assert_eq!(
        plane.recommended_disposition(eid).unwrap(),
        AttemptDisposition::Rejected
    );
}

#[test]
fn spec019_17_03_tests_weakened_or_deleted() {
    let (_domain, item, attempt) = domain_seed(AcceptanceContractMode::PolicyFinal);
    let mut plane = EvaluationPlane::new();
    let contract = test_contract(AcceptanceContractMode::PolicyFinal, true, vec![], false);
    let cid = plane.register_contract(contract).unwrap();
    let mut o = obs(
        item,
        attempt,
        EvaluatorClass::DeterministicTest,
        CriterionResult::Satisfied,
    );
    o.tests_weakened_or_deleted = true;
    let oid = plane.submit_observation(o);
    let eid = plane.evaluate(cid, &[oid], None, None).unwrap();
    assert!(!plane.policy_final_may_accept(eid).unwrap());
}

#[test]
fn spec019_17_04_retry_accepted_both_costs_retained() {
    let (mut domain, item, a1) = domain_seed(AcceptanceContractMode::HumanFinal);
    domain
        .set_attempt_usage(
            a1,
            AccountingValue::Observed(10),
            AccountingValue::Observed(3),
        )
        .unwrap();
    domain
        .close_attempt(a1, AttemptDisposition::Rejected)
        .unwrap();
    let retry = domain
        .fresh_retry(a1, AttemptDisposition::Rejected)
        .unwrap();
    // fresh_retry mints a Created AgentRun; terminate before Attempt close.
    domain
        .terminate_completed(
            retry.agent_run_id,
            seyal_agent_core::TerminationSource::Backend,
        )
        .unwrap();
    domain
        .set_attempt_usage(
            retry.attempt_id,
            AccountingValue::Observed(20),
            AccountingValue::Observed(7),
        )
        .unwrap();
    domain
        .close_attempt(retry.attempt_id, AttemptDisposition::CandidateAccepted)
        .unwrap();
    domain
        .finalize_work_item(item, WorkItemOutcome::Accepted, true)
        .unwrap();

    let mut plane = EvaluationPlane::new();
    plane.record_usage(UsageObservation {
        work_item_id: item,
        attempt_id: a1,
        agent_run_id: None,
        input_units: AccountingValue::Observed(10),
        output_units: AccountingValue::Observed(0),
        cached_input_units: AccountingValue::Unknown,
        media_units: AccountingValue::Unknown,
        compute_duration_millis: AccountingValue::Unknown,
        provider_reported_charge: AccountingValue::Observed(3),
    });
    plane.record_usage(UsageObservation {
        work_item_id: item,
        attempt_id: retry.attempt_id,
        agent_run_id: None,
        input_units: AccountingValue::Observed(20),
        output_units: AccountingValue::Observed(0),
        cached_input_units: AccountingValue::Unknown,
        media_units: AccountingValue::Unknown,
        compute_duration_millis: AccountingValue::Unknown,
        provider_reported_charge: AccountingValue::Observed(7),
    });
    let c1 = plane.derive_cost(a1, None);
    let c2 = plane.derive_cost(retry.attempt_id, None);
    assert_eq!(c1.cost, AccountingValue::Observed(3));
    assert_eq!(c2.cost, AccountingValue::Observed(7));
    assert_eq!(
        domain.attempt(a1).unwrap().cost(),
        AccountingValue::Observed(3)
    );
    assert_eq!(
        domain.attempt(retry.attempt_id).unwrap().cost(),
        AccountingValue::Observed(7)
    );

    let rows = [
        CohortAttemptRow {
            work_item_id: item,
            attempt_id: a1,
            disposition: Some(AttemptDisposition::Rejected),
            outcome: Some(WorkItemOutcome::Accepted),
            cost: AccountingValue::Observed(3),
            is_initial: true,
            is_retry_or_fallback: false,
        },
        CohortAttemptRow {
            work_item_id: item,
            attempt_id: retry.attempt_id,
            disposition: Some(AttemptDisposition::CandidateAccepted),
            outcome: Some(WorkItemOutcome::Accepted),
            cost: AccountingValue::Observed(7),
            is_initial: false,
            is_retry_or_fallback: true,
        },
    ];
    let metric = plane.compute_ai_cost_per_accepted(&rows);
    assert_eq!(metric.numerator, AccountingValue::Observed(10));
    assert_eq!(metric.denominator, AccountingValue::Observed(1));
    assert_eq!(
        plane.attributed_costs(&rows, item),
        AccountingValue::Observed(10)
    );
}

#[test]
fn spec019_17_05_parallel_candidates_distinct_evidence() {
    let (mut domain, item, a1) = domain_seed(AcceptanceContractMode::HumanFinal);
    let a2 = domain
        .create_attempt_with_origin(item, AttemptOrigin::ParallelCandidateOf(a1))
        .unwrap();
    let mut plane = EvaluationPlane::new();
    let o1 = obs(
        item,
        a1,
        EvaluatorClass::DeterministicTest,
        CriterionResult::Satisfied,
    );
    let o2 = obs(
        item,
        a2,
        EvaluatorClass::DeterministicTest,
        CriterionResult::NotSatisfied,
    );
    let id1 = plane.submit_observation(o1);
    let id2 = plane.submit_observation(o2);
    assert_ne!(id1, id2);
    assert_eq!(plane.observations_for_attempt(a1).len(), 1);
    assert_eq!(plane.observations_for_attempt(a2).len(), 1);
    assert_ne!(
        plane.observation(id1).unwrap().result,
        plane.observation(id2).unwrap().result
    );
}

#[test]
fn spec019_17_06_delayed_ci_unresolved_then_resolved() {
    let (mut domain, item, attempt) = domain_seed(AcceptanceContractMode::HumanFinal);
    let mut plane = EvaluationPlane::new();
    let contract = test_contract(AcceptanceContractMode::HumanFinal, false, vec![], false);
    let cid = plane.register_contract(contract).unwrap();

    let pending = obs(
        item,
        attempt,
        EvaluatorClass::ScmCiStatus,
        CriterionResult::NotRun,
    );
    let oid = plane.submit_observation(pending);
    let eid1 = plane.evaluate(cid, &[oid], Some(100), None).unwrap();
    assert_eq!(
        plane.evaluation(eid1).unwrap().recommended_outcome,
        Some(WorkItemOutcome::Unresolved)
    );
    domain
        .finalize_work_item(item, WorkItemOutcome::Unresolved, true)
        .unwrap();

    // Late resolution creates a superseding evaluation; historical Unresolved stays
    // until an authorized correction path — here we only prove evaluation supersession.
    let resolved = obs(
        item,
        attempt,
        EvaluatorClass::ScmCiStatus,
        CriterionResult::Satisfied,
    );
    let oid2 = plane.submit_observation(resolved);
    let eid2 = plane.evaluate(cid, &[oid2], Some(100), Some(eid1)).unwrap();
    assert_eq!(plane.evaluation(eid2).unwrap().supersedes, Some(eid1));
    assert_eq!(
        plane.evaluation(eid2).unwrap().recommended_disposition,
        AttemptDisposition::CandidateAccepted
    );
}

#[test]
fn spec019_17_07_accepted_regression_new_workitem() {
    let (mut domain, item, attempt) = domain_seed(AcceptanceContractMode::HumanFinal);
    domain
        .close_attempt(attempt, AttemptDisposition::CandidateAccepted)
        .unwrap();
    domain
        .finalize_work_item(item, WorkItemOutcome::Accepted, true)
        .unwrap();
    let related = domain.create_related_work_item(item).unwrap();
    assert_ne!(related, item);
    assert_eq!(
        domain.work_item(item).unwrap().outcome(),
        Some(WorkItemOutcome::Accepted)
    );
    assert_eq!(domain.work_item(related).unwrap().outcome(), None);
    assert_eq!(domain.work_item(related).unwrap().related_to(), Some(item));
}

#[test]
fn spec019_17_08_ci_wrong_commit_rejected() {
    let (_domain, item, attempt) = domain_seed(AcceptanceContractMode::PolicyFinal);
    let mut plane = EvaluationPlane::new();
    let contract = test_contract(AcceptanceContractMode::PolicyFinal, true, vec![], false);
    let cid = plane.register_contract(contract).unwrap();
    let mut o = obs(
        item,
        attempt,
        EvaluatorClass::ScmCiStatus,
        CriterionResult::Satisfied,
    );
    o.target.commit_identity = 999; // wrong commit
    assert!(!scm_ci_commit_matches(&o, 100));
    let oid = plane.submit_observation(o);
    let eid = plane.evaluate(cid, &[oid], Some(100), None).unwrap();
    assert!(!plane.policy_final_may_accept(eid).unwrap());
    assert_eq!(
        plane.recommended_disposition(eid).unwrap(),
        AttemptDisposition::Rejected
    );
}

#[test]
fn spec019_17_09_reviewer_independence_violation() {
    let (_domain, item, attempt) = domain_seed(AcceptanceContractMode::PolicyFinal);
    let mut plane = EvaluationPlane::new();
    let contract = test_contract(AcceptanceContractMode::PolicyFinal, true, vec![], true);
    let cid = plane.register_contract(contract).unwrap();
    let mut o = obs(
        item,
        attempt,
        EvaluatorClass::IndependentModelReview,
        CriterionResult::Satisfied,
    );
    o.independence = Some(IndependenceEvidence {
        shared_provider_continuation: true,
        shared_mutable_worktree: false,
        shared_hidden_working_state: false,
        shared_context_input_generation: 0,
        distinct_model_name_only: true,
    });
    assert!(!o.independence.unwrap().is_independent());
    let oid = plane.submit_observation(o);
    let eid = plane.evaluate(cid, &[oid], None, None).unwrap();
    assert!(!plane.policy_final_may_accept(eid).unwrap());
}

#[test]
fn spec019_17_10_attention_wait_not_human_labor() {
    let (_domain, item, attempt) = domain_seed(AcceptanceContractMode::HumanFinal);
    let mut plane = EvaluationPlane::new();
    let time = TimeEvidence {
        work_item_id: item,
        attempt_id: attempt,
        elapsed_work_millis: AccountingValue::Observed(60_000),
        attention_wait_millis: AccountingValue::Observed(45_000),
        human_active_millis: AccountingValue::Observed(5_000),
        model_tool_compute_millis: AccountingValue::Observed(10_000),
    };
    assert!(time.attention_is_not_human_labor());
    assert_eq!(
        time.active_human_labor_millis(),
        AccountingValue::Observed(5_000)
    );
    assert_ne!(time.active_human_labor_millis(), time.attention_wait_millis);
    plane.record_time(time);
    assert_eq!(
        plane.time(attempt).unwrap().active_human_labor_millis(),
        AccountingValue::Observed(5_000)
    );
}

#[test]
fn spec019_17_11_missing_usage_unknown() {
    let (_domain, item, attempt) = domain_seed(AcceptanceContractMode::HumanFinal);
    let mut plane = EvaluationPlane::new();
    let usage = UsageObservation::missing(item, attempt);
    assert!(usage.any_unknown());
    plane.record_usage(usage);
    let cost = plane.derive_cost(attempt, None);
    assert_eq!(cost, CostEvidence::unknown(attempt));
    assert!(cost.cost.is_unknown());
    assert_ne!(cost.cost, AccountingValue::Observed(0));

    let pricing = PricingAssumption {
        id: PricingAssumptionId::new(),
        source_ref: 1,
        version: 1,
        effective_at_millis: 1,
        rate_micro: 100,
    };
    plane.register_pricing(pricing);
    let still = plane.derive_cost(attempt, Some(pricing.id));
    assert!(still.cost.is_unknown());
}

#[test]
fn spec019_17_12_same_task_multi_route_router_evidence() {
    let (_domain, item, attempt) = domain_seed(AcceptanceContractMode::HumanFinal);
    let mut plane = EvaluationPlane::new();
    let o1 = RoutingQualityObservation {
        id: RoutingQualityObservationId::new(),
        work_item_id: item,
        attempt_id: attempt,
        agent_run_id: None,
        routing_decision_ref: 1,
        eligible_candidate_set_ref: 10,
        selection_policy_version: 1,
        task_profile_feature_snapshot_ref: 1,
        route_attribution_ref: 1,
        model_attribution_ref: 1,
        harness_attribution_ref: 1,
        provider_attribution_ref: 1,
        context_compiler_generation: 0, // Unknown until Context Engine exists
        fallback_position: 0,
        acceptance_criterion_provenance: 1,
        evaluation_observation_ids: vec![],
        human_intervention: false,
        outcome_cost: AccountingValue::Observed(5),
        outcome_latency_millis: AccountingValue::Observed(100),
        missing_or_cancelled: false,
        environment_class_ref: 1,
        sample_count: 1,
        time_window_millis: 1,
        uncertainty_ref: 0,
        source_lineage_ref: 1,
        policy_generation: 1,
        purpose: PurposeEligibility::audit_only(),
        selection_support: SelectionSupport::DeterministicZeroSupport,
        comparison_bias: RouteComparisonBias::ExplicitAbstain,
        quality_attribution: QualityAttribution::ModelOrRoute,
        derived_feature_ids: vec![],
        revoked: false,
    };
    let mut o2 = o1.clone();
    o2.id = RoutingQualityObservationId::new();
    o2.routing_decision_ref = 2;
    o2.route_attribution_ref = 2;
    o2.fallback_position = 1;
    plane.retain_routing_quality(o1.clone());
    plane.retain_routing_quality(o2.clone());
    assert_ne!(
        plane.routing_quality(o1.id).unwrap().route_attribution_ref,
        plane.routing_quality(o2.id).unwrap().route_attribution_ref
    );
}

#[test]
fn spec019_17_13_audit_ok_learning_export_disabled() {
    let purpose = PurposeEligibility::audit_only();
    assert!(EvaluationPlane::purpose_audit_ok_learning_disabled(purpose));
    assert!(purpose.operational_audit);
    assert!(!purpose.local_adaptation);
    assert!(!purpose.export_training);

    let (_domain, item, attempt) = domain_seed(AcceptanceContractMode::HumanFinal);
    let mut plane = EvaluationPlane::new();
    let obs = RoutingQualityObservation {
        id: RoutingQualityObservationId::new(),
        work_item_id: item,
        attempt_id: attempt,
        agent_run_id: None,
        routing_decision_ref: 1,
        eligible_candidate_set_ref: 1,
        selection_policy_version: 1,
        task_profile_feature_snapshot_ref: 1,
        route_attribution_ref: 1,
        model_attribution_ref: 1,
        harness_attribution_ref: 1,
        provider_attribution_ref: 1,
        context_compiler_generation: 0,
        fallback_position: 0,
        acceptance_criterion_provenance: 1,
        evaluation_observation_ids: vec![],
        human_intervention: false,
        outcome_cost: AccountingValue::Unknown,
        outcome_latency_millis: AccountingValue::Unknown,
        missing_or_cancelled: false,
        environment_class_ref: 1,
        sample_count: 1,
        time_window_millis: 1,
        uncertainty_ref: 0,
        source_lineage_ref: 1,
        policy_generation: 1,
        purpose,
        selection_support: SelectionSupport::DeterministicZeroSupport,
        comparison_bias: RouteComparisonBias::ExplicitAbstain,
        quality_attribution: QualityAttribution::MixedOrUnknown,
        derived_feature_ids: vec![],
        revoked: false,
    };
    plane.retain_routing_quality(obs.clone());
    assert!(!plane
        .routing_quality(obs.id)
        .unwrap()
        .eligible_for_local_adaptation());
    assert!(!plane
        .routing_quality(obs.id)
        .unwrap()
        .eligible_for_export_training());
    assert!(
        plane
            .routing_quality(obs.id)
            .unwrap()
            .purpose
            .operational_audit
    );
}

#[test]
fn spec019_17_14_revocation_invalidates_derived_features() {
    let (_domain, item, attempt) = domain_seed(AcceptanceContractMode::HumanFinal);
    let mut plane = EvaluationPlane::new();
    let rq_id = RoutingQualityObservationId::new();
    let feature_id = DerivedFeatureId::new();
    let cal_id = CalibrationArtifactId::new();
    plane.retain_routing_quality(RoutingQualityObservation {
        id: rq_id,
        work_item_id: item,
        attempt_id: attempt,
        agent_run_id: None,
        routing_decision_ref: 1,
        eligible_candidate_set_ref: 1,
        selection_policy_version: 1,
        task_profile_feature_snapshot_ref: 1,
        route_attribution_ref: 1,
        model_attribution_ref: 1,
        harness_attribution_ref: 1,
        provider_attribution_ref: 1,
        context_compiler_generation: 0,
        fallback_position: 0,
        acceptance_criterion_provenance: 1,
        evaluation_observation_ids: vec![],
        human_intervention: false,
        outcome_cost: AccountingValue::Observed(1),
        outcome_latency_millis: AccountingValue::Observed(1),
        missing_or_cancelled: false,
        environment_class_ref: 1,
        sample_count: 1,
        time_window_millis: 1,
        uncertainty_ref: 0,
        source_lineage_ref: 1,
        policy_generation: 1,
        purpose: PurposeEligibility {
            operational_audit: true,
            local_adaptation: true,
            export_training: true,
        },
        selection_support: SelectionSupport::Randomized {
            propensity_millis: 500,
        },
        comparison_bias: RouteComparisonBias::RandomizedSupported,
        quality_attribution: QualityAttribution::ModelOrRoute,
        derived_feature_ids: vec![feature_id],
        revoked: false,
    });
    plane.register_derived_feature(DerivedRoutingFeature {
        id: feature_id,
        source_observation_id: rq_id,
        state: DerivedFeatureState::Eligible,
    });
    plane.register_calibration(LocalCalibrationArtifact {
        id: cal_id,
        depends_on: vec![feature_id],
        state: CalibrationState::Valid,
    });
    plane.revoke_routing_observation(rq_id).unwrap();
    assert_eq!(
        plane.derived_feature_state(feature_id),
        Some(DerivedFeatureState::IneligibleRevoked)
    );
    assert_eq!(plane.calibration_reusable(cal_id), Some(false));
    assert!(!plane
        .routing_quality(rq_id)
        .unwrap()
        .eligible_for_export_training());
    assert!(
        plane
            .routing_quality(rq_id)
            .unwrap()
            .purpose
            .operational_audit
    );
}

#[test]
fn spec019_17_15_no_unbiased_easy_vs_hard_fallback() {
    assert!(!unbiased_comparison_allowed(
        RouteComparisonBias::EasyFirstVsHardFallback
    ));
    assert!(unbiased_comparison_allowed(
        RouteComparisonBias::MatchedIsolatedStart
    ));
    let obs = RoutingQualityObservation {
        id: RoutingQualityObservationId::new(),
        work_item_id: seyal_agent_core::WorkItemId::new(),
        attempt_id: seyal_agent_core::AttemptId::new(),
        agent_run_id: None,
        routing_decision_ref: 1,
        eligible_candidate_set_ref: 1,
        selection_policy_version: 1,
        task_profile_feature_snapshot_ref: 1,
        route_attribution_ref: 1,
        model_attribution_ref: 1,
        harness_attribution_ref: 1,
        provider_attribution_ref: 1,
        context_compiler_generation: 0,
        fallback_position: 1,
        acceptance_criterion_provenance: 1,
        evaluation_observation_ids: vec![],
        human_intervention: false,
        outcome_cost: AccountingValue::Observed(1),
        outcome_latency_millis: AccountingValue::Observed(1),
        missing_or_cancelled: false,
        environment_class_ref: 1,
        sample_count: 1,
        time_window_millis: 1,
        uncertainty_ref: 0,
        source_lineage_ref: 1,
        policy_generation: 1,
        purpose: PurposeEligibility::audit_only(),
        selection_support: SelectionSupport::DeterministicZeroSupport,
        comparison_bias: RouteComparisonBias::EasyFirstVsHardFallback,
        quality_attribution: QualityAttribution::ModelOrRoute,
        derived_feature_ids: vec![],
        revoked: false,
    };
    assert!(!obs.allows_unbiased_route_comparison());
}

#[test]
fn spec019_17_16_compiler_vs_model_attribution() {
    assert!(attributions_distinct(
        QualityAttribution::ContextCompiler,
        QualityAttribution::ModelOrRoute
    ));
    assert!(!attributions_distinct(
        QualityAttribution::ModelOrRoute,
        QualityAttribution::ModelOrRoute
    ));
}

#[test]
fn spec019_17_17_experiment_propensity_vs_deterministic_zero_support() {
    let randomized = SelectionSupport::Randomized {
        propensity_millis: 250,
    };
    assert!(randomized.can_support_counterfactual());
    let deterministic = SelectionSupport::DeterministicZeroSupport;
    assert!(!deterministic.can_support_counterfactual());

    let mut obs = RoutingQualityObservation {
        id: RoutingQualityObservationId::new(),
        work_item_id: seyal_agent_core::WorkItemId::new(),
        attempt_id: seyal_agent_core::AttemptId::new(),
        agent_run_id: None,
        routing_decision_ref: 1,
        eligible_candidate_set_ref: 1,
        selection_policy_version: 1,
        task_profile_feature_snapshot_ref: 1,
        route_attribution_ref: 1,
        model_attribution_ref: 1,
        harness_attribution_ref: 1,
        provider_attribution_ref: 1,
        context_compiler_generation: 0,
        fallback_position: 0,
        acceptance_criterion_provenance: 1,
        evaluation_observation_ids: vec![],
        human_intervention: false,
        outcome_cost: AccountingValue::Observed(1),
        outcome_latency_millis: AccountingValue::Observed(1),
        missing_or_cancelled: false,
        environment_class_ref: 1,
        sample_count: 1,
        time_window_millis: 1,
        uncertainty_ref: 0,
        source_lineage_ref: 1,
        policy_generation: 1,
        purpose: PurposeEligibility::audit_only(),
        selection_support: deterministic,
        comparison_bias: RouteComparisonBias::MatchedIsolatedStart,
        quality_attribution: QualityAttribution::ModelOrRoute,
        derived_feature_ids: vec![],
        revoked: false,
    };
    // Deterministic zero-support must not fabricate counterfactual support even
    // when comparison bias would otherwise allow a claim.
    assert!(!obs.selection_support.can_support_counterfactual());
    assert!(!obs.allows_unbiased_route_comparison());
    assert!(!obs.counterfactual_support_fabricated());

    obs.selection_support = randomized;
    obs.comparison_bias = RouteComparisonBias::RandomizedSupported;
    assert!(obs.allows_unbiased_route_comparison());
}

#[test]
fn criterion_unknown_never_means_pass_and_contract_versioning() {
    assert!(!CriterionResult::Inconclusive.is_pass());
    assert!(!CriterionResult::NotRun.is_pass());
    let c = test_contract(AcceptanceContractMode::Hybrid, false, vec![], false);
    assert!(c.clone().with_version_bump(2).is_ok());
    assert!(c.with_version_bump(1).is_err());
    assert_eq!(required_metric_defs().len(), 7);
    assert!(parse_id_bytes(&[0; 8]).is_none());
}

#[test]
fn aggregation_never_synchronously_gates_terminal() {
    let mut plane = EvaluationPlane::new();
    plane
        .schedule_aggregation(AggregationKind::CohortMetrics, 128)
        .unwrap();
    assert!(!plane.pending_aggregation()[0].may_gate_terminal_progress());
}

#[test]
fn eligibility_and_ai_cost_helpers() {
    let c = test_contract(AcceptanceContractMode::PolicyFinal, true, vec![1], false);
    let id = c.criteria[0].id;
    assert!(check_criterion_eligibility(
        &c,
        id,
        EvaluatorClass::DeterministicTest,
        true,
        Some(TestIntegrityClass::TrustedExisting),
        false
    )
    .is_ok());
    assert!(check_criterion_eligibility(
        &c,
        id,
        EvaluatorClass::HarnessReport,
        true,
        Some(TestIntegrityClass::TrustedExisting),
        false
    )
    .is_err());
    let _ = ai_cost_per_accepted(&[]);
}
