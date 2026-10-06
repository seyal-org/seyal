//! Test-integrity detection helpers (SPEC-019 §11).

use super::observation::EvaluationObservation;
use super::types::{EvaluatorClass, TestIntegrityClass};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegrityViolation {
    AgentGeneratedUnreviewed,
    TestsWeakenedOrDeleted,
    ForbiddenPathChanged,
}

/// Detect integrity violations that must block self-authorized acceptance.
pub fn detect_integrity_violation(
    observation: &EvaluationObservation,
    forbidden_path_refs: &[u64],
) -> Option<IntegrityViolation> {
    if observation.tests_weakened_or_deleted {
        return Some(IntegrityViolation::TestsWeakenedOrDeleted);
    }
    if observation
        .test_integrity
        .is_some_and(|c| c == TestIntegrityClass::AgentGeneratedUnreviewed)
        && matches!(
            observation.evaluator_class,
            EvaluatorClass::DeterministicTest | EvaluatorClass::Build | EvaluatorClass::Lint
        )
    {
        return Some(IntegrityViolation::AgentGeneratedUnreviewed);
    }
    if !forbidden_path_refs.is_empty()
        && observation
            .changed_path_refs
            .iter()
            .any(|p| forbidden_path_refs.contains(p))
    {
        return Some(IntegrityViolation::ForbiddenPathChanged);
    }
    None
}

pub fn integrity_blocks_acceptance(violation: Option<IntegrityViolation>) -> bool {
    violation.is_some()
}
