//! RunWorkingSet retention and resume classification (SPEC-014).

use crate::{
    AgentRunId, AttemptId, ContextBundleId, ContinuationPlanId, MemoryId, PlanGeneration,
    RunWorkingSetId, WorkItemId, WorkingSetGeneration,
};

use super::types::Sensitivity;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WorkingSetEntryClass {
    UserInstructionOrCorrection = 1,
    ModelVisibleAssistantMessage = 2,
    ContextBundleDependencyRef = 3,
    MemoryRecordDependencyRef = 4,
    AgentRunEventOrEvidenceRef = 5,
    PendingActionOrResultRef = 6,
    ArtifactRef = 7,
    PlanOrTaskState = 8,
    ToolOrCapabilityObservation = 9,
    ProviderContinuationRef = 10,
    DerivedSummaryOrCompaction = 11,
}

impl WorkingSetEntryClass {
    pub const fn code(self) -> u8 {
        self as u8
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::UserInstructionOrCorrection),
            2 => Some(Self::ModelVisibleAssistantMessage),
            3 => Some(Self::ContextBundleDependencyRef),
            4 => Some(Self::MemoryRecordDependencyRef),
            5 => Some(Self::AgentRunEventOrEvidenceRef),
            6 => Some(Self::PendingActionOrResultRef),
            7 => Some(Self::ArtifactRef),
            8 => Some(Self::PlanOrTaskState),
            9 => Some(Self::ToolOrCapabilityObservation),
            10 => Some(Self::ProviderContinuationRef),
            11 => Some(Self::DerivedSummaryOrCompaction),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RetentionAvailability {
    RetainedPayload = 1,
    ReconstructableFromCurrentAuthority = 2,
    ReferenceOnly = 3,
    Unavailable = 4,
    RevokedOrForbidden = 5,
    Expired = 6,
    Stale = 7,
}

impl RetentionAvailability {
    pub const fn code(self) -> u8 {
        self as u8
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::RetainedPayload),
            2 => Some(Self::ReconstructableFromCurrentAuthority),
            3 => Some(Self::ReferenceOnly),
            4 => Some(Self::Unavailable),
            5 => Some(Self::RevokedOrForbidden),
            6 => Some(Self::Expired),
            7 => Some(Self::Stale),
            _ => None,
        }
    }

    /// Stronger denial wins (SPEC-014 §5).
    pub fn tighten(self, other: Self) -> Self {
        const ORDER: [RetentionAvailability; 7] = [
            RetentionAvailability::RevokedOrForbidden,
            RetentionAvailability::Expired,
            RetentionAvailability::Stale,
            RetentionAvailability::Unavailable,
            RetentionAvailability::ReferenceOnly,
            RetentionAvailability::RetainedPayload,
            RetentionAvailability::ReconstructableFromCurrentAuthority,
        ];
        let rank = |a: RetentionAvailability| ORDER.iter().position(|x| *x == a).unwrap_or(99);
        if rank(self) <= rank(other) {
            self
        } else {
            other
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Requiredness {
    Required,
    Optional,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SatisfactionMode {
    PayloadRequired,
    ReferenceSufficient,
    ReconstructableAllowed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanDependency {
    pub class: WorkingSetEntryClass,
    pub identity: Vec<u8>,
    pub requiredness: Requiredness,
    pub satisfaction: SatisfactionMode,
    pub expected_generation: u64,
    pub max_sensitivity: Sensitivity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContinuationPlan {
    pub id: ContinuationPlanId,
    pub schema_version: u16,
    pub plan_generation: PlanGeneration,
    pub issuer_version: u16,
    pub work_item_id: WorkItemId,
    pub attempt_id: AttemptId,
    pub agent_run_id: AgentRunId,
    pub binding_generation: u64,
    pub consumer_contract_version: u16,
    pub policy_fence: Vec<u8>,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: Option<u64>,
    pub dependencies: Vec<PlanDependency>,
}

impl ContinuationPlan {
    pub const CURRENT_SCHEMA: u16 = 1;

    pub fn validate(
        &self,
        now_unix_ms: u64,
        expected_run: AgentRunId,
        expected_attempt: AttemptId,
        expected_binding: u64,
    ) -> Result<(), ResumeClassification> {
        if self.schema_version != Self::CURRENT_SCHEMA {
            return Err(ResumeClassification::ReconciliationRequired);
        }
        if self.agent_run_id != expected_run || self.attempt_id != expected_attempt {
            return Err(ResumeClassification::ReconciliationRequired);
        }
        if self.binding_generation != expected_binding {
            return Err(ResumeClassification::ReconciliationRequired);
        }
        if let Some(expires) = self.expires_at_unix_ms
            && now_unix_ms >= expires
        {
            return Err(ResumeClassification::ReconciliationRequired);
        }
        for dep in &self.dependencies {
            if WorkingSetEntryClass::from_code(dep.class.code()).is_none() {
                return Err(ResumeClassification::ReconciliationRequired);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResumeClassification {
    BehavioralResumeAvailable,
    ReconciliationRequired,
    ResumeUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExecutionLivenessHint {
    KnownLive,
    KnownTerminated,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkingSetEntry {
    pub entry_id: [u8; 16],
    pub class: WorkingSetEntryClass,
    pub availability: RetentionAvailability,
    pub sensitivity: Sensitivity,
    pub payload: Option<Vec<u8>>,
    pub dependency_ref: Vec<u8>,
    pub source_generation: u64,
    pub reconstructable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunWorkingSet {
    pub id: RunWorkingSetId,
    pub work_item_id: WorkItemId,
    pub attempt_id: AttemptId,
    pub agent_run_id: AgentRunId,
    pub working_set_generation: WorkingSetGeneration,
    pub policy_fence: Vec<u8>,
    pub builder_version: u16,
    pub created_at_unix_ms: u64,
    pub last_compacted_at_unix_ms: Option<u64>,
    pub provider_continuation_ref: Option<Vec<u8>>,
    pub entries: Vec<WorkingSetEntry>,
    pub degraded: bool,
}

impl RunWorkingSet {
    pub fn bound_to(&self, run: AgentRunId, attempt: AttemptId) -> bool {
        self.agent_run_id == run && self.attempt_id == attempt
    }

    pub fn byte_footprint(&self) -> u64 {
        self.entries
            .iter()
            .map(|e| e.payload.as_ref().map(|p| p.len()).unwrap_or(0) + e.dependency_ref.len() + 64)
            .sum::<usize>() as u64
    }
}

/// Deterministic SPEC-014 §9 classification.
pub fn classify_resume(
    plan: Option<&ContinuationPlan>,
    working_set: &RunWorkingSet,
    now_unix_ms: u64,
    expected_binding: u64,
    liveness: ExecutionLivenessHint,
    ambiguous_external_effect: bool,
) -> ResumeClassification {
    let Some(plan) = plan else {
        return ResumeClassification::ReconciliationRequired;
    };
    if plan
        .validate(
            now_unix_ms,
            working_set.agent_run_id,
            working_set.attempt_id,
            expected_binding,
        )
        .is_err()
    {
        return ResumeClassification::ReconciliationRequired;
    }
    if matches!(liveness, ExecutionLivenessHint::Unknown) || ambiguous_external_effect {
        return ResumeClassification::ReconciliationRequired;
    }

    let mut saw_unavailable = false;
    for dep in plan
        .dependencies
        .iter()
        .filter(|d| d.requiredness == Requiredness::Required)
    {
        let availability = resolve_dependency_availability(dep, working_set);
        match availability {
            RetentionAvailability::RevokedOrForbidden
            | RetentionAvailability::Expired
            | RetentionAvailability::Unavailable
            | RetentionAvailability::Stale => {
                saw_unavailable = true;
            }
            RetentionAvailability::ReferenceOnly => {
                if dep.satisfaction == SatisfactionMode::PayloadRequired {
                    saw_unavailable = true;
                }
            }
            RetentionAvailability::RetainedPayload => {}
            RetentionAvailability::ReconstructableFromCurrentAuthority => {
                if !matches!(
                    dep.satisfaction,
                    SatisfactionMode::ReconstructableAllowed
                        | SatisfactionMode::ReferenceSufficient
                ) && dep.satisfaction == SatisfactionMode::PayloadRequired
                {
                    // Reconstructable only counts when plan allows it.
                    if dep.satisfaction != SatisfactionMode::ReconstructableAllowed {
                        saw_unavailable = true;
                    }
                }
            }
        }
        // Unknown reconstructability for a required dep => reconciliation.
        if availability == RetentionAvailability::Stale {
            // Irrecoverably stale already counted; if we cannot establish validity, reconcile.
            // Spec: unknown validity => ReconciliationRequired takes precedence.
            return ResumeClassification::ReconciliationRequired;
        }
    }

    if saw_unavailable {
        ResumeClassification::ResumeUnavailable
    } else {
        ResumeClassification::BehavioralResumeAvailable
    }
}

fn resolve_dependency_availability(
    dep: &PlanDependency,
    working_set: &RunWorkingSet,
) -> RetentionAvailability {
    let mut best: Option<RetentionAvailability> = None;
    for entry in &working_set.entries {
        if entry.class != dep.class {
            continue;
        }
        if !dep.identity.is_empty() && entry.dependency_ref != dep.identity {
            continue;
        }
        let mut availability = entry.availability;
        if entry.source_generation != dep.expected_generation
            && dep.expected_generation != 0
            && availability != RetentionAvailability::RevokedOrForbidden
        {
            availability = availability.tighten(RetentionAvailability::Stale);
        }
        best = Some(match best {
            None => availability,
            Some(prev) => prev.tighten(availability),
        });
    }
    best.unwrap_or(RetentionAvailability::Unavailable)
}

/// Fixture ContextBundle generation fence used until #1272 lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FixtureBundleRef {
    pub bundle_id: ContextBundleId,
    pub bundle_generation: u64,
    pub revocation_vector_hash: [u8; 32],
}

/// Fixture MemoryRecord dependency for working-set tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FixtureMemoryRef {
    pub memory_id: MemoryId,
    pub record_generation: u64,
}

#[cfg(test)]
mod tests {
    use super::super::types::Sensitivity;
    use super::*;
    use crate::{
        AgentRunId, AttemptId, ContinuationPlanId, PlanGeneration, RunWorkingSetId, WorkItemId,
        WorkingSetGeneration,
    };

    fn empty_ws(run: AgentRunId, attempt: AttemptId) -> RunWorkingSet {
        RunWorkingSet {
            id: RunWorkingSetId::new(),
            work_item_id: WorkItemId::new(),
            attempt_id: attempt,
            agent_run_id: run,
            working_set_generation: WorkingSetGeneration::FIRST,
            policy_fence: Vec::new(),
            builder_version: 1,
            created_at_unix_ms: 0,
            last_compacted_at_unix_ms: None,
            provider_continuation_ref: None,
            entries: Vec::new(),
            degraded: false,
        }
    }

    #[test]
    fn missing_plan_requires_reconciliation() {
        let run = AgentRunId::new();
        let attempt = AttemptId::new();
        let ws = empty_ws(run, attempt);
        assert_eq!(
            classify_resume(
                None,
                &ws,
                0,
                1,
                ExecutionLivenessHint::KnownTerminated,
                false
            ),
            ResumeClassification::ReconciliationRequired
        );
    }

    #[test]
    fn reference_only_payload_required_is_unavailable() {
        let run = AgentRunId::new();
        let attempt = AttemptId::new();
        let mut ws = empty_ws(run, attempt);
        ws.entries.push(WorkingSetEntry {
            entry_id: [9; 16],
            class: WorkingSetEntryClass::UserInstructionOrCorrection,
            availability: RetentionAvailability::ReferenceOnly,
            sensitivity: Sensitivity::Internal,
            payload: None,
            dependency_ref: b"instr".to_vec(),
            source_generation: 1,
            reconstructable: false,
        });
        let plan = ContinuationPlan {
            id: ContinuationPlanId::new(),
            schema_version: 1,
            plan_generation: PlanGeneration::FIRST,
            issuer_version: 1,
            work_item_id: ws.work_item_id,
            attempt_id: attempt,
            agent_run_id: run,
            binding_generation: 1,
            consumer_contract_version: 1,
            policy_fence: Vec::new(),
            created_at_unix_ms: 0,
            expires_at_unix_ms: None,
            dependencies: vec![PlanDependency {
                class: WorkingSetEntryClass::UserInstructionOrCorrection,
                identity: b"instr".to_vec(),
                requiredness: Requiredness::Required,
                satisfaction: SatisfactionMode::PayloadRequired,
                expected_generation: 1,
                max_sensitivity: Sensitivity::Restricted,
            }],
        };
        assert_eq!(
            classify_resume(
                Some(&plan),
                &ws,
                0,
                1,
                ExecutionLivenessHint::KnownTerminated,
                false
            ),
            ResumeClassification::ResumeUnavailable
        );
    }
}
