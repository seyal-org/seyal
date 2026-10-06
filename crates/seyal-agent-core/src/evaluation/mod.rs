//! SPEC-019 evaluation, acceptance, outcome, cost and routing-quality export plane.
//!
//! Permanent production path for M005 evaluation evidence. Does not implement
//! SPEC-020 ranking, Context Engine, or MemoryStore.

mod aggregation;
mod cohort;
mod contract;
mod cost;
mod ids;
mod integrity;
mod interpret;
mod observation;
mod plane;
mod routing_export;
mod types;

pub use aggregation::{AggregationError, AggregationKind, BackgroundAggregation};
pub use cohort::{
    ai_cost_per_accepted, attributed_costs_for_work_item, required_metric_defs, sum_attempt_costs,
    CohortAttemptRow, CohortMetricDef, CohortMetricValue, MissingDataTreatment,
    AI_COST_PER_ACCEPTED, AI_COST_PER_ALL, ATTEMPTS_PER_ACCEPTED, ELAPSED_PER_ACCEPTED,
    FIRST_ATTEMPT_ACCEPTANCE, OUTCOME_RATE_ACCEPTED, RETRY_FALLBACK_RATE,
};
pub use contract::{
    check_criterion_eligibility, AcceptanceContract, ContractError, CriterionOutcome, CriterionSpec,
};
pub use cost::{CostEvidence, PricingAssumption, TimeEvidence, UsageObservation};
pub use ids::{
    parse_id_bytes, AcceptanceContractId, CalibrationArtifactId, CriterionId, DerivedFeatureId,
    EvaluationId, EvaluationObservationId, PricingAssumptionId, RoutingQualityObservationId,
};
pub use integrity::{detect_integrity_violation, integrity_blocks_acceptance, IntegrityViolation};
pub use interpret::{
    interpret_evaluation, self_report_implies_accepted, Evaluation, PolicyFinalBlock,
};
pub use observation::{
    scm_ci_commit_matches, EvaluationObservation, IndependenceEvidence, TargetRefs,
};
pub use plane::{EvaluationError, EvaluationPlane};
pub use routing_export::{
    attributions_distinct, revoke_derived_features, unbiased_comparison_allowed, CalibrationState,
    DerivedFeatureState, DerivedRoutingFeature, LocalCalibrationArtifact,
    RoutingQualityObservation,
};
pub use types::{
    CriterionResult, EvaluatorClass, PurposeEligibility, QualityAttribution, RouteComparisonBias,
    SelectionSupport, TestIntegrityClass,
};
