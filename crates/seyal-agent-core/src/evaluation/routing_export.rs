//! Routing-quality observation export schema without ranking (SPEC-019 §15).

use crate::{AgentRunId, AttemptId, WorkItemId};

use super::ids::{
    CalibrationArtifactId, DerivedFeatureId, EvaluationObservationId, RoutingQualityObservationId,
};
use super::types::{PurposeEligibility, QualityAttribution, RouteComparisonBias, SelectionSupport};

/// Export schema for routing-quality observations (SPEC-019 §15).
///
/// This is an evidence export plane only — it does not implement SPEC-020 ranking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutingQualityObservation {
    pub id: RoutingQualityObservationId,
    pub work_item_id: WorkItemId,
    pub attempt_id: AttemptId,
    pub agent_run_id: Option<AgentRunId>,
    pub routing_decision_ref: u64,
    pub eligible_candidate_set_ref: u64,
    pub selection_policy_version: u32,
    pub task_profile_feature_snapshot_ref: u64,
    pub route_attribution_ref: u64,
    pub model_attribution_ref: u64,
    pub harness_attribution_ref: u64,
    pub provider_attribution_ref: u64,
    /// ContextDeliveryPlan / request-compiler / context generation, or Unknown (0).
    pub context_compiler_generation: u64,
    pub fallback_position: u32,
    pub acceptance_criterion_provenance: u64,
    pub evaluation_observation_ids: Vec<EvaluationObservationId>,
    pub human_intervention: bool,
    pub outcome_cost: super::super::lifecycle::AccountingValue,
    pub outcome_latency_millis: super::super::lifecycle::AccountingValue,
    pub missing_or_cancelled: bool,
    pub environment_class_ref: u64,
    pub sample_count: u64,
    pub time_window_millis: u64,
    pub uncertainty_ref: u64,
    pub source_lineage_ref: u64,
    pub policy_generation: u64,
    pub purpose: PurposeEligibility,
    pub selection_support: SelectionSupport,
    pub comparison_bias: RouteComparisonBias,
    pub quality_attribution: QualityAttribution,
    pub derived_feature_ids: Vec<DerivedFeatureId>,
    pub revoked: bool,
}

impl RoutingQualityObservation {
    pub fn eligible_for_local_adaptation(&self) -> bool {
        self.purpose.local_adaptation && !self.revoked
    }

    pub fn eligible_for_export_training(&self) -> bool {
        self.purpose.export_training && !self.revoked
    }

    pub fn allows_unbiased_route_comparison(&self) -> bool {
        self.comparison_bias.allows_unbiased_claim()
            && self.selection_support.can_support_counterfactual()
    }

    /// Deterministic zero-support cannot fabricate counterfactual support.
    pub fn counterfactual_support_fabricated(&self) -> bool {
        matches!(
            self.selection_support,
            SelectionSupport::DeterministicZeroSupport
        ) && self.allows_unbiased_route_comparison()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DerivedFeatureState {
    Eligible,
    IneligibleRevoked,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DerivedRoutingFeature {
    pub id: DerivedFeatureId,
    pub source_observation_id: RoutingQualityObservationId,
    pub state: DerivedFeatureState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalibrationState {
    Valid,
    Invalidated,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalCalibrationArtifact {
    pub id: CalibrationArtifactId,
    pub depends_on: Vec<DerivedFeatureId>,
    pub state: CalibrationState,
}

impl LocalCalibrationArtifact {
    pub fn invalidate_if_depends_on(&mut self, feature: DerivedFeatureId) -> bool {
        if self.depends_on.contains(&feature) {
            self.state = CalibrationState::Invalidated;
            true
        } else {
            false
        }
    }

    pub fn reusable(&self) -> bool {
        self.state == CalibrationState::Valid
    }
}

/// Revocation makes derived routing-training features ineligible and invalidates
/// dependent local calibration before reuse (SPEC-019 §15 / fixture 14).
pub fn revoke_derived_features(
    features: &mut [DerivedRoutingFeature],
    calibrations: &mut [LocalCalibrationArtifact],
    observation_id: RoutingQualityObservationId,
) {
    let mut revoked_ids = Vec::new();
    for feature in features.iter_mut() {
        if feature.source_observation_id == observation_id {
            feature.state = DerivedFeatureState::IneligibleRevoked;
            revoked_ids.push(feature.id);
        }
    }
    for id in revoked_ids {
        for cal in calibrations.iter_mut() {
            cal.invalidate_if_depends_on(id);
        }
    }
}

/// Easy-first vs hard-fallback must not be reported as unbiased.
pub fn unbiased_comparison_allowed(bias: RouteComparisonBias) -> bool {
    bias.allows_unbiased_claim()
}

/// Context-compiler change vs model/route change remain separately attributable.
pub fn attributions_distinct(a: QualityAttribution, b: QualityAttribution) -> bool {
    a != b
        && matches!(
            (a, b),
            (
                QualityAttribution::ContextCompiler,
                QualityAttribution::ModelOrRoute
            ) | (
                QualityAttribution::ModelOrRoute,
                QualityAttribution::ContextCompiler
            )
        )
}
