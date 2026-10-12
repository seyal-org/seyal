//! AcceptanceContract and criterion eligibility (SPEC-019 §4).

use crate::lifecycle::AcceptanceContractMode;

use super::ids::{AcceptanceContractId, CriterionId};
use super::types::{CriterionResult, EvaluatorClass, TestIntegrityClass};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CriterionSpec {
    pub id: CriterionId,
    pub required: bool,
    pub eligible_classes: Vec<EvaluatorClass>,
    /// Opaque predicate identity (versioned requirement reference).
    pub predicate_ref: u64,
    /// Optional independence requirement for review criteria.
    pub requires_independence: bool,
    /// Minimum trusted integrity class when the criterion evaluates tests.
    pub min_test_integrity: Option<TestIntegrityClass>,
    /// Paths forbidden to change for this criterion (fingerprint refs).
    pub forbidden_path_refs: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptanceContract {
    pub id: AcceptanceContractId,
    pub version: u32,
    pub policy_generation: u64,
    pub mode: AcceptanceContractMode,
    pub criteria: Vec<CriterionSpec>,
    /// When true, PolicyFinal may auto-accept if all guardrails pass.
    pub permits_policy_final_auto_accept: bool,
}

impl AcceptanceContract {
    pub fn criterion(&self, id: CriterionId) -> Option<&CriterionSpec> {
        self.criteria.iter().find(|c| c.id == id)
    }

    pub fn required_criteria(&self) -> impl Iterator<Item = &CriterionSpec> {
        self.criteria.iter().filter(|c| c.required)
    }

    pub fn evaluator_eligible(&self, criterion_id: CriterionId, class: EvaluatorClass) -> bool {
        self.criterion(criterion_id)
            .is_some_and(|c| c.eligible_classes.contains(&class))
    }

    /// Changing material acceptance criteria requires an explicit version bump.
    pub fn with_version_bump(mut self, next_version: u32) -> Result<Self, ContractError> {
        if next_version <= self.version {
            return Err(ContractError::VersionMustIncrease);
        }
        self.version = next_version;
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContractError {
    VersionMustIncrease,
    UnknownCriterion,
    EvaluatorNotEligible,
    IndependenceRequired,
    TestIntegrityInsufficient,
    ForbiddenPathChanged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CriterionOutcome {
    pub criterion_id: CriterionId,
    pub result: CriterionResult,
    pub evaluator_class: EvaluatorClass,
    pub observation_bound: bool,
}

impl CriterionOutcome {
    pub const fn blocks_acceptance(self) -> bool {
        self.result.blocks_policy_final()
    }
}

/// Evaluate eligibility of one observation against a contract criterion.
pub fn check_criterion_eligibility(
    contract: &AcceptanceContract,
    criterion_id: CriterionId,
    class: EvaluatorClass,
    independence_ok: bool,
    test_integrity: Option<TestIntegrityClass>,
    forbidden_path_changed: bool,
) -> Result<(), ContractError> {
    let criterion = contract
        .criterion(criterion_id)
        .ok_or(ContractError::UnknownCriterion)?;
    if !criterion.eligible_classes.contains(&class) {
        return Err(ContractError::EvaluatorNotEligible);
    }
    if criterion.requires_independence && !independence_ok {
        return Err(ContractError::IndependenceRequired);
    }
    if let Some(min) = criterion.min_test_integrity {
        let trusted_enough = test_integrity.is_some_and(|actual| {
            actual.is_trusted_for_acceptance()
                && match min {
                    TestIntegrityClass::AgentGeneratedUnreviewed => true,
                    _ => actual != TestIntegrityClass::AgentGeneratedUnreviewed,
                }
        });
        if !trusted_enough {
            return Err(ContractError::TestIntegrityInsufficient);
        }
    }
    if forbidden_path_changed && !criterion.forbidden_path_refs.is_empty() {
        return Err(ContractError::ForbiddenPathChanged);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::AcceptanceContractMode;

    fn sample_contract() -> AcceptanceContract {
        AcceptanceContract {
            id: AcceptanceContractId::new(),
            version: 1,
            policy_generation: 1,
            mode: AcceptanceContractMode::PolicyFinal,
            criteria: vec![CriterionSpec {
                id: CriterionId::new(),
                required: true,
                eligible_classes: vec![EvaluatorClass::DeterministicTest],
                predicate_ref: 1,
                requires_independence: false,
                min_test_integrity: Some(TestIntegrityClass::TrustedExisting),
                forbidden_path_refs: vec![42],
            }],
            permits_policy_final_auto_accept: true,
        }
    }

    #[test]
    fn contract_version_must_increase() {
        let c = sample_contract();
        assert_eq!(
            c.clone().with_version_bump(1),
            Err(ContractError::VersionMustIncrease)
        );
        assert!(c.with_version_bump(2).is_ok());
    }

    #[test]
    fn harness_self_report_not_eligible_for_test_criterion() {
        let c = sample_contract();
        let id = c.criteria[0].id;
        assert_eq!(
            check_criterion_eligibility(
                &c,
                id,
                EvaluatorClass::HarnessReport,
                true,
                Some(TestIntegrityClass::TrustedExisting),
                false
            ),
            Err(ContractError::EvaluatorNotEligible)
        );
    }
}
