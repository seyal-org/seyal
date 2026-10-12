//! Immutable EvaluationObservation and independence evidence (SPEC-019 §2 / §7).

use crate::{AgentRunId, AttemptId, WorkItemId};

use super::ids::EvaluationObservationId;
use super::types::{CriterionResult, EvaluatorClass, TestIntegrityClass};

/// Fingerprint / generation refs bound to the exact evaluated state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TargetRefs {
    pub repository_generation: u64,
    pub worktree_fingerprint: u64,
    pub source_fingerprint: u64,
    pub artifact_fingerprint: Option<u64>,
    /// Exact commit identity for SCM/CI evidence (0 = unset / unknown).
    pub commit_identity: u64,
    /// Exact check identity for SCM/CI evidence (0 = unset / unknown).
    pub check_identity: u64,
}

impl TargetRefs {
    pub const fn unknown() -> Self {
        Self {
            repository_generation: 0,
            worktree_fingerprint: 0,
            source_fingerprint: 0,
            artifact_fingerprint: None,
            commit_identity: 0,
            check_identity: 0,
        }
    }
}

/// Independence evidence for required independent review (SPEC-019 §7).
///
/// A different model name alone does not prove independence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IndependenceEvidence {
    pub shared_provider_continuation: bool,
    pub shared_mutable_worktree: bool,
    pub shared_hidden_working_state: bool,
    /// ContextBundle / Memory / RunWorkingSet generation when known; 0 = Unknown.
    pub shared_context_input_generation: u64,
    pub distinct_model_name_only: bool,
}

impl IndependenceEvidence {
    pub const fn independent() -> Self {
        Self {
            shared_provider_continuation: false,
            shared_mutable_worktree: false,
            shared_hidden_working_state: false,
            shared_context_input_generation: 0,
            distinct_model_name_only: false,
        }
    }

    pub const fn is_independent(self) -> bool {
        !self.shared_provider_continuation
            && !self.shared_mutable_worktree
            && !self.shared_hidden_working_state
            && !self.distinct_model_name_only
    }
}

/// Immutable observation bound to the exact state it evaluated (SPEC-019 §2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvaluationObservation {
    pub id: EvaluationObservationId,
    pub work_item_id: WorkItemId,
    pub attempt_id: AttemptId,
    pub agent_run_id: Option<AgentRunId>,
    pub evaluator_class: EvaluatorClass,
    pub evaluator_version: u32,
    pub target: TargetRefs,
    pub result: CriterionResult,
    pub criterion_ref: Option<u64>,
    pub provenance: u64,
    pub observed_at_millis: u64,
    pub reproducibility_ref: Option<u64>,
    pub independence: Option<IndependenceEvidence>,
    pub test_integrity: Option<TestIntegrityClass>,
    /// Agent/model/harness self-reported "done" claim — never implies Accepted.
    pub self_report_done: bool,
    /// Paths changed in this observation's evaluated delta (fingerprint refs).
    pub changed_path_refs: Vec<u64>,
    /// Tests weakened or deleted in this observation's evaluated delta.
    pub tests_weakened_or_deleted: bool,
}

impl EvaluationObservation {
    pub fn is_self_report(&self) -> bool {
        self.evaluator_class.is_self_report() || self.self_report_done
    }
}

/// Reject SCM/CI evidence that is not bound to the expected commit/check.
pub fn scm_ci_commit_matches(observation: &EvaluationObservation, expected_commit: u64) -> bool {
    if observation.evaluator_class != EvaluatorClass::ScmCiStatus {
        return true;
    }
    observation.target.commit_identity != 0 && observation.target.commit_identity == expected_commit
}
