//! WorkItem / Attempt / AgentRun lifecycle and orthogonal run facts (SPEC-026).
//!
//! Consumes SPEC-014 resumability classifications and SPEC-019 disposition /
//! outcome values without restating their evaluation semantics.

use crate::execution_host::ExecutionHostKind;
use crate::{AdapterId, AgentRunId, AttemptId, RouteOfferingId};

/// WorkItem lifecycle (SPEC-026 §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkItemLifecycle {
    Open,
    Finalized,
}

/// AcceptanceContract mode stub (SPEC-019 §4). Detection-created work uses
/// `HumanFinal` per SPEC-026 O3.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcceptanceContractMode {
    HumanFinal,
    PolicyFinal,
    Hybrid,
}

/// WorkItem outcome (SPEC-019 §9). Owned only by WorkItem finalization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkItemOutcome {
    Accepted,
    Rejected,
    Unresolved,
    Abandoned,
}

/// Attempt origin (SPEC-026 §5). Set at creation; never changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttemptOrigin {
    Initial,
    RetryOf(AttemptId),
    ForkOf(AttemptId),
    ParallelCandidateOf(AttemptId),
    StrategyChangeFrom(AttemptId),
}

impl AttemptOrigin {
    pub const fn is_retry(self) -> bool {
        matches!(self, Self::RetryOf(_))
    }

    pub const fn parent(self) -> Option<AttemptId> {
        match self {
            Self::Initial => None,
            Self::RetryOf(id)
            | Self::ForkOf(id)
            | Self::ParallelCandidateOf(id)
            | Self::StrategyChangeFrom(id) => Some(id),
        }
    }
}

/// Attempt lifecycle (SPEC-026 §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttemptLifecycle {
    Created,
    Active,
    Closing,
    Closed,
}

/// Attempt disposition (SPEC-019 §8). Immutable once Closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttemptDisposition {
    CandidateAccepted,
    Rejected,
    Inconclusive,
    Cancelled,
    Interrupted,
    Superseded,
}

/// AgentRun lifecycle (SPEC-026 §6.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentRunLifecycle {
    Created,
    Prepared,
    Dispatching,
    Active,
    Terminating,
    Terminated,
}

impl AgentRunLifecycle {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Terminated)
    }

    pub const fn is_non_terminal(self) -> bool {
        !self.is_terminal()
    }
}

/// Orthogonal execution liveness (SPEC-026 §6.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionLiveness {
    NotStarted,
    Alive,
    Exited,
    Unknown,
}

/// Orthogonal observation connectivity (SPEC-026 §6.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservationFact {
    Connected,
    Degraded,
    Disconnected,
}

/// Behavioral resumability (SPEC-014 §9) plus `NotEvaluated` (SPEC-026 §6.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResumabilityFact {
    NotEvaluated,
    BehavioralResumeAvailable,
    ReconciliationRequired,
    ResumeUnavailable,
}

/// Run termination record (SPEC-026 §6.3). Never implies WorkItem outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunTermination {
    pub kind: TerminationKind,
    pub source: TerminationSource,
    pub reason_code: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminationKind {
    Completed,
    Failed,
    Cancelled,
    Interrupted,
    Lost,
    Superseded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminationSource {
    Backend,
    Harness,
    Provider,
    Execution,
    User,
    Policy,
}

/// AgentRun lineage for forks (SPEC-026 §9.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentRunLineage {
    ForkOf(AgentRunId),
}

/// Opaque routing-decision reference set when Prepared (SPEC-020).
///
/// Content/generation pointer into [`crate::AgentDomain`]'s immutable
/// `RoutingDecision` history (SPEC-027 §4.2). Never a second router.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RoutingDecisionRef(u64);

impl RoutingDecisionRef {
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// How the bound RouteOffering was selected (SPEC-027 §4.2–§4.3).
///
/// `RouterV1` is accepted as a future discriminant but MUST be unreachable
/// until SPEC-020 V1 ranking (#681) exists. No code in this repository may
/// construct it; [`crate::resolve_execution_target`] never returns it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionKind {
    /// Client presented `route_offering_id` and it resolved to an enabled,
    /// hard-constraint-satisfying offering (SPEC-027 §4.3 step 1).
    Pinned,
    /// No pin; exactly one enabled, hard-constraint-satisfying offering
    /// existed (SPEC-027 §4.3 step 2).
    Singleton,
    /// Forbidden before #681. See module doc.
    RouterV1,
}

/// Content-address placeholder for the resolved, manifest-owned launch
/// descriptor frozen at Prepared (SPEC-027 §4.2 / §5.1).
///
/// Opaque until a real content-addressed descriptor store exists; equality
/// is what fencing relies on, not a specific byte encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LaunchDescriptorRef(u64);

impl LaunchDescriptorRef {
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Immutable execution-target binding recorded at `Created → Prepared`
/// (SPEC-027 §4.2). Never mutated once recorded; retried/forked/fallback
/// runs mint a new [`RoutingDecision`] rather than editing this one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoutingDecision {
    pub adapter_id: AdapterId,
    pub adapter_manifest_generation: u64,
    pub route_offering_id: RouteOfferingId,
    pub execution_host_kind: ExecutionHostKind,
    pub launch_descriptor_ref: LaunchDescriptorRef,
    pub selection_kind: SelectionKind,
}

/// Opaque execution-host reference for binding / detection (SPEC-018).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ExecutionRef(u64);

impl ExecutionRef {
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Durable binding evidence for external detection idempotency (SPEC-026 §9.9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ExternalIdentityKey {
    pub execution_ref: ExecutionRef,
    pub external_identity: u64,
}

/// Opaque adapter-scoped upstream session/thread/conversation reference
/// (ADR-012). Never a Seyal `WorkItem` / `Attempt` / `AgentRun` identity.
///
/// Adapters may store vendor thread/session IDs here as resumability metadata
/// only. Core lifecycle authorities remain Seyal-owned.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HarnessSessionRef {
    /// First-party adapter label (e.g. `codex-cli`), not a vendor product ID.
    pub adapter_label: String,
    /// Opaque upstream token (Codex thread id, Claude session id, …).
    pub upstream_ref: String,
}

impl HarnessSessionRef {
    pub fn new(adapter_label: impl Into<String>, upstream_ref: impl Into<String>) -> Self {
        Self {
            adapter_label: adapter_label.into(),
            upstream_ref: upstream_ref.into(),
        }
    }
}

/// Ephemeral client attachment access (SPEC-026 §8). Not durable identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachmentAccess {
    Observe,
    Interact,
    Control,
}

/// Usage/cost accounting: missing is unknown, never zero (SPEC-026 §11 / fixture 28).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountingValue {
    Unknown,
    Observed(u64),
}

impl AccountingValue {
    pub const fn is_unknown(self) -> bool {
        matches!(self, Self::Unknown)
    }
}

/// Stable store/wire codes for durable lifecycle columns (fail-closed on unknown).
pub mod codes {
    use super::*;

    pub const fn work_item_lifecycle(value: WorkItemLifecycle) -> u8 {
        match value {
            WorkItemLifecycle::Open => 1,
            WorkItemLifecycle::Finalized => 2,
        }
    }

    pub const fn work_item_lifecycle_from(code: u8) -> Option<WorkItemLifecycle> {
        match code {
            1 => Some(WorkItemLifecycle::Open),
            2 => Some(WorkItemLifecycle::Finalized),
            _ => None,
        }
    }

    pub const fn attempt_lifecycle(value: AttemptLifecycle) -> u8 {
        match value {
            AttemptLifecycle::Created => 1,
            AttemptLifecycle::Active => 2,
            AttemptLifecycle::Closing => 3,
            AttemptLifecycle::Closed => 4,
        }
    }

    pub const fn attempt_lifecycle_from(code: u8) -> Option<AttemptLifecycle> {
        match code {
            1 => Some(AttemptLifecycle::Created),
            2 => Some(AttemptLifecycle::Active),
            3 => Some(AttemptLifecycle::Closing),
            4 => Some(AttemptLifecycle::Closed),
            _ => None,
        }
    }

    pub const fn attempt_origin_kind(value: AttemptOrigin) -> u8 {
        match value {
            AttemptOrigin::Initial => 1,
            AttemptOrigin::RetryOf(_) => 2,
            AttemptOrigin::ForkOf(_) => 3,
            AttemptOrigin::ParallelCandidateOf(_) => 4,
            AttemptOrigin::StrategyChangeFrom(_) => 5,
        }
    }

    pub const fn agent_run_lifecycle(value: AgentRunLifecycle) -> u8 {
        match value {
            AgentRunLifecycle::Created => 1,
            AgentRunLifecycle::Prepared => 2,
            AgentRunLifecycle::Dispatching => 3,
            AgentRunLifecycle::Active => 4,
            AgentRunLifecycle::Terminating => 5,
            AgentRunLifecycle::Terminated => 6,
        }
    }

    pub const fn agent_run_lifecycle_from(code: u8) -> Option<AgentRunLifecycle> {
        match code {
            1 => Some(AgentRunLifecycle::Created),
            2 => Some(AgentRunLifecycle::Prepared),
            3 => Some(AgentRunLifecycle::Dispatching),
            4 => Some(AgentRunLifecycle::Active),
            5 => Some(AgentRunLifecycle::Terminating),
            6 => Some(AgentRunLifecycle::Terminated),
            _ => None,
        }
    }

    pub const fn execution_liveness(value: ExecutionLiveness) -> u8 {
        match value {
            ExecutionLiveness::NotStarted => 1,
            ExecutionLiveness::Alive => 2,
            ExecutionLiveness::Exited => 3,
            ExecutionLiveness::Unknown => 4,
        }
    }

    pub const fn execution_liveness_from(code: u8) -> Option<ExecutionLiveness> {
        match code {
            1 => Some(ExecutionLiveness::NotStarted),
            2 => Some(ExecutionLiveness::Alive),
            3 => Some(ExecutionLiveness::Exited),
            4 => Some(ExecutionLiveness::Unknown),
            _ => None,
        }
    }

    pub const fn observation(value: ObservationFact) -> u8 {
        match value {
            ObservationFact::Connected => 1,
            ObservationFact::Degraded => 2,
            ObservationFact::Disconnected => 3,
        }
    }

    pub const fn observation_from(code: u8) -> Option<ObservationFact> {
        match code {
            1 => Some(ObservationFact::Connected),
            2 => Some(ObservationFact::Degraded),
            3 => Some(ObservationFact::Disconnected),
            _ => None,
        }
    }

    pub const fn resumability(value: ResumabilityFact) -> u8 {
        match value {
            ResumabilityFact::NotEvaluated => 1,
            ResumabilityFact::BehavioralResumeAvailable => 2,
            ResumabilityFact::ReconciliationRequired => 3,
            ResumabilityFact::ResumeUnavailable => 4,
        }
    }

    pub const fn resumability_from(code: u8) -> Option<ResumabilityFact> {
        match code {
            1 => Some(ResumabilityFact::NotEvaluated),
            2 => Some(ResumabilityFact::BehavioralResumeAvailable),
            3 => Some(ResumabilityFact::ReconciliationRequired),
            4 => Some(ResumabilityFact::ResumeUnavailable),
            _ => None,
        }
    }

    pub const fn attempt_disposition(value: AttemptDisposition) -> u8 {
        match value {
            AttemptDisposition::CandidateAccepted => 1,
            AttemptDisposition::Rejected => 2,
            AttemptDisposition::Inconclusive => 3,
            AttemptDisposition::Cancelled => 4,
            AttemptDisposition::Interrupted => 5,
            AttemptDisposition::Superseded => 6,
        }
    }

    pub const fn attempt_disposition_from(code: u8) -> Option<AttemptDisposition> {
        match code {
            1 => Some(AttemptDisposition::CandidateAccepted),
            2 => Some(AttemptDisposition::Rejected),
            3 => Some(AttemptDisposition::Inconclusive),
            4 => Some(AttemptDisposition::Cancelled),
            5 => Some(AttemptDisposition::Interrupted),
            6 => Some(AttemptDisposition::Superseded),
            _ => None,
        }
    }

    pub const fn work_item_outcome(value: WorkItemOutcome) -> u8 {
        match value {
            WorkItemOutcome::Accepted => 1,
            WorkItemOutcome::Rejected => 2,
            WorkItemOutcome::Unresolved => 3,
            WorkItemOutcome::Abandoned => 4,
        }
    }

    pub const fn selection_kind(value: SelectionKind) -> u8 {
        match value {
            SelectionKind::Pinned => 1,
            SelectionKind::Singleton => 2,
            SelectionKind::RouterV1 => 3,
        }
    }

    pub const fn selection_kind_from(code: u8) -> Option<SelectionKind> {
        match code {
            1 => Some(SelectionKind::Pinned),
            2 => Some(SelectionKind::Singleton),
            3 => Some(SelectionKind::RouterV1),
            _ => None,
        }
    }

    pub const fn work_item_outcome_from(code: u8) -> Option<WorkItemOutcome> {
        match code {
            1 => Some(WorkItemOutcome::Accepted),
            2 => Some(WorkItemOutcome::Rejected),
            3 => Some(WorkItemOutcome::Unresolved),
            4 => Some(WorkItemOutcome::Abandoned),
            _ => None,
        }
    }
}
