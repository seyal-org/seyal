//! EvaluationPlane: immutable evaluation / cost / export evidence authority.
//!
//! Evaluators submit observations here. This plane never mutates AgentRun
//! lifecycle; Agent Backend remains the sole WorkItem/Attempt/AgentRun writer
//! (ADR-016 / SPEC-019).

use std::collections::HashMap;

use crate::lifecycle::{AccountingValue, AttemptDisposition, WorkItemOutcome};
use crate::{AttemptId, WorkItemId};

use super::aggregation::{AggregationKind, BackgroundAggregation};
use super::cohort::{
    ai_cost_per_accepted, attributed_costs_for_work_item, required_metric_defs, CohortAttemptRow,
    CohortMetricDef, CohortMetricValue,
};
use super::contract::AcceptanceContract;
use super::cost::{CostEvidence, PricingAssumption, TimeEvidence, UsageObservation};
use super::ids::{
    AcceptanceContractId, CalibrationArtifactId, DerivedFeatureId, EvaluationId,
    EvaluationObservationId, RoutingQualityObservationId,
};
use super::interpret::{interpret_evaluation, Evaluation, PolicyFinalBlock};
use super::observation::EvaluationObservation;
use super::routing_export::{
    revoke_derived_features, CalibrationState, DerivedFeatureState, DerivedRoutingFeature,
    LocalCalibrationArtifact, RoutingQualityObservation,
};
use super::types::PurposeEligibility;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvaluationError {
    UnknownContract,
    UnknownObservation,
    UnknownEvaluation,
    UnknownRoutingObservation,
    ContractVersionConflict,
    AggregationWouldGateTerminal,
}

/// In-memory evaluation / outcome / cost / export evidence store.
#[derive(Debug, Default)]
pub struct EvaluationPlane {
    contracts: HashMap<AcceptanceContractId, AcceptanceContract>,
    observations: HashMap<EvaluationObservationId, EvaluationObservation>,
    evaluations: HashMap<EvaluationId, Evaluation>,
    usage: Vec<UsageObservation>,
    pricing: HashMap<super::ids::PricingAssumptionId, PricingAssumption>,
    costs: HashMap<AttemptId, CostEvidence>,
    time: HashMap<AttemptId, TimeEvidence>,
    routing_quality: HashMap<RoutingQualityObservationId, RoutingQualityObservation>,
    derived_features: Vec<DerivedRoutingFeature>,
    calibrations: Vec<LocalCalibrationArtifact>,
    pending_aggregation: Vec<BackgroundAggregation>,
}

impl EvaluationPlane {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_contract(
        &mut self,
        contract: AcceptanceContract,
    ) -> Result<AcceptanceContractId, EvaluationError> {
        let id = contract.id;
        if let Some(existing) = self.contracts.get(&id)
            && existing.version == contract.version
            && existing != &contract
        {
            return Err(EvaluationError::ContractVersionConflict);
        }
        self.contracts.insert(id, contract);
        Ok(id)
    }

    pub fn contract(&self, id: AcceptanceContractId) -> Option<&AcceptanceContract> {
        self.contracts.get(&id)
    }

    pub fn submit_observation(
        &mut self,
        observation: EvaluationObservation,
    ) -> EvaluationObservationId {
        let id = observation.id;
        self.observations.insert(id, observation);
        id
    }

    pub fn observation(&self, id: EvaluationObservationId) -> Option<&EvaluationObservation> {
        self.observations.get(&id)
    }

    pub fn observations_for_attempt(&self, attempt_id: AttemptId) -> Vec<&EvaluationObservation> {
        self.observations
            .values()
            .filter(|o| o.attempt_id == attempt_id)
            .collect()
    }

    /// Interpret observations into an immutable Evaluation (may supersede prior).
    pub fn evaluate(
        &mut self,
        contract_id: AcceptanceContractId,
        observation_ids: &[EvaluationObservationId],
        expected_commit: Option<u64>,
        supersedes: Option<EvaluationId>,
    ) -> Result<EvaluationId, EvaluationError> {
        let contract = self
            .contracts
            .get(&contract_id)
            .ok_or(EvaluationError::UnknownContract)?
            .clone();
        let mut obs_refs = Vec::with_capacity(observation_ids.len());
        for id in observation_ids {
            let obs = self
                .observations
                .get(id)
                .ok_or(EvaluationError::UnknownObservation)?;
            obs_refs.push(obs);
        }
        let mut evaluation = interpret_evaluation(&contract, &obs_refs, expected_commit);
        evaluation.supersedes = supersedes;
        let id = evaluation.id;
        self.evaluations.insert(id, evaluation);
        Ok(id)
    }

    pub fn evaluation(&self, id: EvaluationId) -> Option<&Evaluation> {
        self.evaluations.get(&id)
    }

    /// PolicyFinal auto-accept eligibility — caller must still invoke domain finalize.
    pub fn policy_final_may_accept(
        &self,
        evaluation_id: EvaluationId,
    ) -> Result<bool, EvaluationError> {
        let evaluation = self
            .evaluations
            .get(&evaluation_id)
            .ok_or(EvaluationError::UnknownEvaluation)?;
        Ok(evaluation.policy_final_eligible
            && evaluation.recommended_outcome == Some(WorkItemOutcome::Accepted)
            && evaluation.blocking_reason.is_none())
    }

    pub fn recommended_disposition(
        &self,
        evaluation_id: EvaluationId,
    ) -> Result<AttemptDisposition, EvaluationError> {
        self.evaluations
            .get(&evaluation_id)
            .map(|e| e.recommended_disposition)
            .ok_or(EvaluationError::UnknownEvaluation)
    }

    pub fn record_usage(&mut self, usage: UsageObservation) {
        self.usage.push(usage);
    }

    pub fn register_pricing(&mut self, pricing: PricingAssumption) {
        self.pricing.insert(pricing.id, pricing);
    }

    pub fn derive_cost(
        &mut self,
        attempt_id: AttemptId,
        pricing_id: Option<super::ids::PricingAssumptionId>,
    ) -> CostEvidence {
        let usage = self.usage.iter().rev().find(|u| u.attempt_id == attempt_id);
        let evidence = match (usage, pricing_id.and_then(|id| self.pricing.get(&id))) {
            (Some(u), Some(p)) => CostEvidence::from_usage(u, p),
            (Some(u), None) => match u.provider_reported_charge {
                AccountingValue::Observed(v) => CostEvidence {
                    attempt_id,
                    pricing_id: None,
                    cost: AccountingValue::Observed(v),
                },
                AccountingValue::Unknown => CostEvidence::unknown(attempt_id),
            },
            (None, _) => CostEvidence::unknown(attempt_id),
        };
        self.costs.insert(attempt_id, evidence);
        evidence
    }

    pub fn cost(&self, attempt_id: AttemptId) -> Option<&CostEvidence> {
        self.costs.get(&attempt_id)
    }

    pub fn record_time(&mut self, time: TimeEvidence) {
        self.time.insert(time.attempt_id, time);
    }

    pub fn time(&self, attempt_id: AttemptId) -> Option<&TimeEvidence> {
        self.time.get(&attempt_id)
    }

    pub fn retain_routing_quality(&mut self, obs: RoutingQualityObservation) {
        self.routing_quality.insert(obs.id, obs);
    }

    pub fn routing_quality(
        &self,
        id: RoutingQualityObservationId,
    ) -> Option<&RoutingQualityObservation> {
        self.routing_quality.get(&id)
    }

    pub fn register_derived_feature(&mut self, feature: DerivedRoutingFeature) {
        self.derived_features.push(feature);
    }

    pub fn register_calibration(&mut self, artifact: LocalCalibrationArtifact) {
        self.calibrations.push(artifact);
    }

    pub fn revoke_routing_observation(
        &mut self,
        observation_id: RoutingQualityObservationId,
    ) -> Result<(), EvaluationError> {
        let obs = self
            .routing_quality
            .get_mut(&observation_id)
            .ok_or(EvaluationError::UnknownRoutingObservation)?;
        obs.revoked = true;
        // Export/training eligibility cleared; operational audit may remain.
        obs.purpose.local_adaptation = false;
        obs.purpose.export_training = false;
        revoke_derived_features(
            &mut self.derived_features,
            &mut self.calibrations,
            observation_id,
        );
        Ok(())
    }

    pub fn derived_feature_state(&self, id: DerivedFeatureId) -> Option<DerivedFeatureState> {
        self.derived_features
            .iter()
            .find(|f| f.id == id)
            .map(|f| f.state)
    }

    pub fn calibration_reusable(&self, id: CalibrationArtifactId) -> Option<bool> {
        self.calibrations
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.reusable())
    }

    pub fn schedule_aggregation(
        &mut self,
        kind: AggregationKind,
        retained_window: u32,
    ) -> Result<(), EvaluationError> {
        let job = BackgroundAggregation::new(kind, retained_window)
            .map_err(|_| EvaluationError::AggregationWouldGateTerminal)?;
        if job.may_gate_terminal_progress() {
            return Err(EvaluationError::AggregationWouldGateTerminal);
        }
        self.pending_aggregation.push(job);
        Ok(())
    }

    pub fn pending_aggregation(&self) -> &[BackgroundAggregation] {
        &self.pending_aggregation
    }

    pub fn cohort_metric_defs(&self) -> &'static [CohortMetricDef] {
        required_metric_defs()
    }

    pub fn compute_ai_cost_per_accepted(&self, rows: &[CohortAttemptRow]) -> CohortMetricValue {
        ai_cost_per_accepted(rows)
    }

    pub fn attributed_costs(
        &self,
        rows: &[CohortAttemptRow],
        work_item_id: WorkItemId,
    ) -> AccountingValue {
        attributed_costs_for_work_item(rows, work_item_id)
    }

    /// Audit retention can remain while local learning/export is disabled.
    pub fn purpose_audit_ok_learning_disabled(purpose: PurposeEligibility) -> bool {
        purpose.operational_audit && !purpose.local_adaptation && !purpose.export_training
    }

    pub fn blocking_reason(
        &self,
        evaluation_id: EvaluationId,
    ) -> Result<Option<PolicyFinalBlock>, EvaluationError> {
        self.evaluations
            .get(&evaluation_id)
            .map(|e| e.blocking_reason)
            .ok_or(EvaluationError::UnknownEvaluation)
    }

    pub fn calibration_states(&self) -> impl Iterator<Item = CalibrationState> + '_ {
        self.calibrations.iter().map(|c| c.state)
    }
}
