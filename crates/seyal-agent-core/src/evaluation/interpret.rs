//! Immutable Evaluation interpretation and PolicyFinal guardrails (SPEC-019 §6 / §10).

use crate::lifecycle::{AcceptanceContractMode, AttemptDisposition, WorkItemOutcome};

use super::contract::{AcceptanceContract, CriterionOutcome};
use super::ids::{EvaluationId, EvaluationObservationId};
use super::integrity::{detect_integrity_violation, IntegrityViolation};
use super::observation::EvaluationObservation;
use super::types::{CriterionResult, EvaluatorClass};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Evaluation {
    pub id: EvaluationId,
    pub contract_id: super::ids::AcceptanceContractId,
    pub contract_version: u32,
    pub observation_ids: Vec<EvaluationObservationId>,
    pub criterion_outcomes: Vec<CriterionOutcome>,
    pub supersedes: Option<EvaluationId>,
    pub recommended_disposition: AttemptDisposition,
    pub recommended_outcome: Option<WorkItemOutcome>,
    pub policy_final_eligible: bool,
    pub blocking_reason: Option<PolicyFinalBlock>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyFinalBlock {
    ModeNotPolicyFinal,
    AutoAcceptNotPermitted,
    RequiredNotSatisfied,
    IneligibleEvaluator,
    InconclusiveOrNonPass,
    IntegrityViolation(IntegrityViolation),
    SelfReportCannotGrantAcceptance,
    IndependenceViolation,
    ScmCiCommitMismatch,
    UnresolvedEvidence,
}

/// Interpret observations against a contract. Never lets self-report imply Accepted.
pub fn interpret_evaluation(
    contract: &AcceptanceContract,
    observations: &[&EvaluationObservation],
    expected_commit: Option<u64>,
) -> Evaluation {
    let mut outcomes = Vec::new();
    let mut blocking = None;
    let mut self_report_done = false;

    for obs in observations {
        if obs.self_report_done || obs.evaluator_class.is_self_report() {
            self_report_done = true;
        }
        if let Some(expected) = expected_commit
            && obs.evaluator_class == EvaluatorClass::ScmCiStatus
            && (obs.target.commit_identity == 0 || obs.target.commit_identity != expected)
        {
            blocking = Some(PolicyFinalBlock::ScmCiCommitMismatch);
            if let Some(first) = contract.criteria.first() {
                outcomes.push(CriterionOutcome {
                    criterion_id: first.id,
                    result: CriterionResult::NotSatisfied,
                    evaluator_class: obs.evaluator_class,
                    observation_bound: true,
                });
            }
            continue;
        }

        let criterion_id = contract
            .criteria
            .iter()
            .find(|c| {
                obs.criterion_ref.is_some_and(|r| r == c.predicate_ref)
                    || c.eligible_classes.contains(&obs.evaluator_class)
            })
            .map(|c| c.id);

        if let Some(cid) = criterion_id {
            let criterion = contract.criterion(cid).expect("criterion present");
            if !criterion.eligible_classes.contains(&obs.evaluator_class) {
                blocking = Some(PolicyFinalBlock::IneligibleEvaluator);
            }
            if criterion.requires_independence
                && !obs.independence.is_some_and(|i| i.is_independent())
            {
                blocking = Some(PolicyFinalBlock::IndependenceViolation);
            }
            if let Some(v) = detect_integrity_violation(obs, &criterion.forbidden_path_refs) {
                blocking = Some(PolicyFinalBlock::IntegrityViolation(v));
            }
            let mut result = obs.result;
            if blocking.is_some() && result.is_pass() {
                result = CriterionResult::NotSatisfied;
            }
            if result.blocks_policy_final() && criterion.required {
                if matches!(
                    result,
                    CriterionResult::Inconclusive
                        | CriterionResult::Error
                        | CriterionResult::NotRun
                        | CriterionResult::Stale
                ) {
                    blocking = Some(PolicyFinalBlock::InconclusiveOrNonPass);
                } else if blocking.is_none() {
                    blocking = Some(PolicyFinalBlock::RequiredNotSatisfied);
                }
            }
            outcomes.push(CriterionOutcome {
                criterion_id: cid,
                result,
                evaluator_class: obs.evaluator_class,
                observation_bound: true,
            });
        } else if obs.result == CriterionResult::NotRun
            || obs.result == CriterionResult::Inconclusive
        {
            blocking = Some(PolicyFinalBlock::UnresolvedEvidence);
        }
    }

    // Required criteria with no observations remain NotRun.
    for criterion in contract.required_criteria() {
        if !outcomes.iter().any(|o| o.criterion_id == criterion.id) {
            outcomes.push(CriterionOutcome {
                criterion_id: criterion.id,
                result: CriterionResult::NotRun,
                evaluator_class: EvaluatorClass::DeterministicTest,
                observation_bound: false,
            });
            blocking = Some(PolicyFinalBlock::InconclusiveOrNonPass);
        }
    }

    if self_report_done
        && outcomes.iter().any(|o| {
            !o.result.is_pass()
                || matches!(
                    o.evaluator_class,
                    EvaluatorClass::HarnessReport | EvaluatorClass::ProviderReport
                )
        })
    {
        // Self-report never grants acceptance even when claimed done.
        if blocking.is_none() {
            blocking = Some(PolicyFinalBlock::SelfReportCannotGrantAcceptance);
        }
    }
    if self_report_done {
        // Explicit: agent done alone never yields Accepted.
        let only_self = observations.iter().all(|o| o.is_self_report());
        if only_self {
            blocking = Some(PolicyFinalBlock::SelfReportCannotGrantAcceptance);
        }
    }

    let all_required_pass = contract.required_criteria().all(|c| {
        outcomes
            .iter()
            .find(|o| o.criterion_id == c.id)
            .is_some_and(|o| o.result.is_pass())
    });
    let any_unresolved = outcomes.iter().any(|o| {
        matches!(
            o.result,
            CriterionResult::Inconclusive
                | CriterionResult::NotRun
                | CriterionResult::Stale
                | CriterionResult::Error
        )
    });

    // Disposition recommendation ignores PolicyFinal mode — mode only gates auto-accept.
    let evidence_blocking = blocking;
    let recommended_disposition = if all_required_pass && evidence_blocking.is_none() {
        AttemptDisposition::CandidateAccepted
    } else if any_unresolved {
        AttemptDisposition::Inconclusive
    } else {
        AttemptDisposition::Rejected
    };

    let policy_final_eligible = match contract.mode {
        AcceptanceContractMode::PolicyFinal if contract.permits_policy_final_auto_accept => {
            evidence_blocking.is_none()
                && all_required_pass
                && !observations
                    .iter()
                    .any(|o| o.is_self_report() && o.self_report_done)
                && !observations.iter().all(|o| o.is_self_report())
        }
        AcceptanceContractMode::PolicyFinal => false,
        _ => false,
    };

    let blocking_reason = if policy_final_eligible {
        None
    } else if let Some(reason) = evidence_blocking {
        Some(reason)
    } else if matches!(contract.mode, AcceptanceContractMode::PolicyFinal)
        && !contract.permits_policy_final_auto_accept
    {
        Some(PolicyFinalBlock::AutoAcceptNotPermitted)
    } else if !matches!(contract.mode, AcceptanceContractMode::PolicyFinal) {
        Some(PolicyFinalBlock::ModeNotPolicyFinal)
    } else {
        Some(PolicyFinalBlock::RequiredNotSatisfied)
    };

    let recommended_outcome = if policy_final_eligible {
        Some(WorkItemOutcome::Accepted)
    } else if any_unresolved {
        Some(WorkItemOutcome::Unresolved)
    } else if matches!(recommended_disposition, AttemptDisposition::Rejected) {
        Some(WorkItemOutcome::Rejected)
    } else {
        // CandidateAccepted under HumanFinal/Hybrid still needs human/authority finalize.
        None
    };

    Evaluation {
        id: EvaluationId::new(),
        contract_id: contract.id,
        contract_version: contract.version,
        observation_ids: observations.iter().map(|o| o.id).collect(),
        criterion_outcomes: outcomes,
        supersedes: None,
        recommended_disposition,
        recommended_outcome,
        policy_final_eligible,
        blocking_reason,
    }
}

/// Self-report alone must never produce WorkItem Accepted.
pub fn self_report_implies_accepted(evaluation: &Evaluation) -> bool {
    evaluation.policy_final_eligible
        && evaluation.recommended_outcome == Some(WorkItemOutcome::Accepted)
        && evaluation
            .criterion_outcomes
            .iter()
            .all(|o| o.evaluator_class.is_self_report())
}
