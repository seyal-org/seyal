use std::collections::{HashMap, HashSet};

use crate::client_control::LoggedObservation;
use crate::lifecycle::{
    AcceptanceContractMode, AccountingValue, AgentRunLifecycle, AgentRunLineage, AttachmentAccess,
    AttemptDisposition, AttemptLifecycle, AttemptOrigin, ExecutionLiveness, ExecutionRef,
    ExternalIdentityKey, ObservationFact, ResumabilityFact, RoutingDecision, RoutingDecisionRef,
    RunTermination, WorkItemLifecycle, WorkItemOutcome,
};
use crate::{
    AgentRunId, AttemptId, BindingGeneration, ClientSessionId, ControlGeneration, WorkItemId,
    WorkScopeId,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkScopeKind {
    Project,
    Repository,
    AdHoc,
    HostBound,
}

impl WorkScopeKind {
    pub const fn code(self) -> u8 {
        match self {
            Self::Project => 1,
            Self::Repository => 2,
            Self::AdHoc => 3,
            Self::HostBound => 4,
        }
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Project),
            2 => Some(Self::Repository),
            3 => Some(Self::AdHoc),
            4 => Some(Self::HostBound),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkScope {
    pub(crate) id: WorkScopeId,
    pub(crate) kind: WorkScopeKind,
}

impl WorkScope {
    pub const fn id(self) -> WorkScopeId {
        self.id
    }

    pub const fn kind(self) -> WorkScopeKind {
        self.kind
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkItem {
    pub(crate) id: WorkItemId,
    pub(crate) work_scope_id: WorkScopeId,
    pub(crate) lifecycle: WorkItemLifecycle,
    pub(crate) outcome: Option<WorkItemOutcome>,
    pub(crate) acceptance_mode: AcceptanceContractMode,
    pub(crate) related_to: Option<WorkItemId>,
}

impl WorkItem {
    pub const fn id(self) -> WorkItemId {
        self.id
    }

    pub const fn work_scope_id(self) -> WorkScopeId {
        self.work_scope_id
    }

    pub const fn lifecycle(self) -> WorkItemLifecycle {
        self.lifecycle
    }

    pub const fn outcome(self) -> Option<WorkItemOutcome> {
        self.outcome
    }

    pub const fn acceptance_mode(self) -> AcceptanceContractMode {
        self.acceptance_mode
    }

    pub const fn related_to(self) -> Option<WorkItemId> {
        self.related_to
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attempt {
    pub(crate) id: AttemptId,
    pub(crate) work_item_id: WorkItemId,
    pub(crate) origin: AttemptOrigin,
    pub(crate) lifecycle: AttemptLifecycle,
    pub(crate) disposition: Option<AttemptDisposition>,
    pub(crate) usage: AccountingValue,
    pub(crate) cost: AccountingValue,
}

impl Attempt {
    pub const fn id(self) -> AttemptId {
        self.id
    }

    pub const fn work_item_id(self) -> WorkItemId {
        self.work_item_id
    }

    pub const fn origin(self) -> AttemptOrigin {
        self.origin
    }

    pub const fn lifecycle(self) -> AttemptLifecycle {
        self.lifecycle
    }

    pub const fn disposition(self) -> Option<AttemptDisposition> {
        self.disposition
    }

    pub const fn usage(self) -> AccountingValue {
        self.usage
    }

    pub const fn cost(self) -> AccountingValue {
        self.cost
    }
}

/// Durable AgentRun aggregate (SPEC-026 §6).
///
/// `control_generation` is the client control epoch (SPEC-026 O1 / §8.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentRun {
    pub(crate) id: AgentRunId,
    pub(crate) attempt_id: AttemptId,
    pub(crate) work_item_id: WorkItemId,
    pub(crate) binding_generation: BindingGeneration,
    /// Client control epoch (SPEC-026 O1). Advances on backend restart/recovery
    /// and whenever control authority is re-established (§8.3).
    pub(crate) control_generation: ControlGeneration,
    pub(crate) run_revision: u64,
    pub(crate) lifecycle: AgentRunLifecycle,
    pub(crate) execution_liveness: ExecutionLiveness,
    pub(crate) observation: ObservationFact,
    pub(crate) resumability: ResumabilityFact,
    pub(crate) termination: Option<RunTermination>,
    pub(crate) lineage: Option<AgentRunLineage>,
    pub(crate) routing_decision_ref: Option<RoutingDecisionRef>,
    pub(crate) current_execution: Option<ExecutionRef>,
}

impl AgentRun {
    pub const fn id(self) -> AgentRunId {
        self.id
    }

    pub const fn attempt_id(self) -> AttemptId {
        self.attempt_id
    }

    pub const fn work_item_id(self) -> WorkItemId {
        self.work_item_id
    }

    pub const fn binding_generation(self) -> BindingGeneration {
        self.binding_generation
    }

    /// Client control epoch (SPEC-026 O1).
    pub const fn control_generation(self) -> ControlGeneration {
        self.control_generation
    }

    pub const fn run_revision(self) -> u64 {
        self.run_revision
    }

    pub const fn lifecycle(self) -> AgentRunLifecycle {
        self.lifecycle
    }

    pub const fn execution_liveness(self) -> ExecutionLiveness {
        self.execution_liveness
    }

    pub const fn observation(self) -> ObservationFact {
        self.observation
    }

    pub const fn resumability(self) -> ResumabilityFact {
        self.resumability
    }

    pub const fn termination(self) -> Option<RunTermination> {
        self.termination
    }

    pub const fn lineage(self) -> Option<AgentRunLineage> {
        self.lineage
    }

    pub const fn routing_decision_ref(self) -> Option<RoutingDecisionRef> {
        self.routing_decision_ref
    }

    pub const fn current_execution(self) -> Option<ExecutionRef> {
        self.current_execution
    }

    pub(crate) fn bump_revision(&mut self) {
        self.run_revision = self.run_revision.saturating_add(1);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomainError {
    UnknownWorkScope(WorkScopeId),
    UnknownWorkItem(WorkItemId),
    UnknownAttempt(AttemptId),
    UnknownAgentRun(AgentRunId),
    /// SPEC-026 §12 `StaleBinding`.
    StaleBinding {
        current: BindingGeneration,
        presented: BindingGeneration,
    },
    /// SPEC-026 §12 `StaleControlEpoch` (O1: ControlGeneration).
    StaleControlEpoch {
        current: ControlGeneration,
        presented: ControlGeneration,
    },
    GenerationExhausted,
    Conflict,
    InvalidTransition,
    StaleRevision {
        expected: u64,
        current: u64,
    },
    NotAuthorized,
    MultipleRunsNotPermitted,
    ResumeNotAvailable {
        reason: ResumabilityFact,
    },
    ReconciliationRequired,
    AttemptNotClosable,
    WorkItemFinalized,
}

/// Pure in-memory aggregate used as the sole agent-domain transition authority.
///
/// Persistence/replay layers may call this domain authority; they must not
/// create peer writers for the same WorkScope/WorkItem/Attempt/AgentRun
/// transitions (ADR-016 / SPEC-026 §3).
#[derive(Debug, Default)]
pub struct AgentDomain {
    pub(crate) work_scopes: HashMap<WorkScopeId, WorkScope>,
    pub(crate) work_items: HashMap<WorkItemId, WorkItem>,
    pub(crate) attempts: HashMap<AttemptId, Attempt>,
    pub(crate) agent_runs: HashMap<AgentRunId, AgentRun>,
    pub(crate) attachments:
        HashMap<ClientSessionId, (AgentRunId, AttachmentAccess, ControlGeneration, u64)>,
    pub(crate) detection_bindings: HashMap<ExternalIdentityKey, AgentRunId>,
    pub(crate) retired_executions: HashSet<ExecutionRef>,
    pub(crate) observation_keys: HashSet<(u64, u64, u64)>,
    pub(crate) observation_log: Vec<LoggedObservation>,
    /// Immutable RoutingDecision history (SPEC-027 §4.2). Never rewritten;
    /// pre-start fallback and retry mint a new entry on the same or a new run.
    pub(crate) routing_decisions: HashMap<RoutingDecisionRef, RoutingDecision>,
    pub(crate) next_routing_decision: u64,
}

impl AgentDomain {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_work_scope(&mut self, kind: WorkScopeKind) -> WorkScopeId {
        let id = WorkScopeId::new();
        self.work_scopes.insert(id, WorkScope { id, kind });
        id
    }

    pub fn create_work_item(
        &mut self,
        work_scope_id: WorkScopeId,
    ) -> Result<WorkItemId, DomainError> {
        self.create_work_item_with_mode(work_scope_id, AcceptanceContractMode::HumanFinal)
    }

    /// Create a WorkItem with an explicit AcceptanceContract mode (SPEC-019 §4).
    pub fn create_work_item_with_mode(
        &mut self,
        work_scope_id: WorkScopeId,
        acceptance_mode: AcceptanceContractMode,
    ) -> Result<WorkItemId, DomainError> {
        if !self.work_scopes.contains_key(&work_scope_id) {
            return Err(DomainError::UnknownWorkScope(work_scope_id));
        }
        let id = WorkItemId::new();
        self.work_items.insert(
            id,
            WorkItem {
                id,
                work_scope_id,
                lifecycle: WorkItemLifecycle::Open,
                outcome: None,
                acceptance_mode,
                related_to: None,
            },
        );
        Ok(id)
    }

    pub fn create_attempt(&mut self, work_item_id: WorkItemId) -> Result<AttemptId, DomainError> {
        self.create_attempt_with_origin(work_item_id, AttemptOrigin::Initial)
    }

    pub fn create_agent_run(&mut self, attempt_id: AttemptId) -> Result<AgentRunId, DomainError> {
        self.create_agent_run_with_lineage(attempt_id, None)
    }

    pub(crate) fn create_agent_run_with_lineage(
        &mut self,
        attempt_id: AttemptId,
        lineage: Option<AgentRunLineage>,
    ) -> Result<AgentRunId, DomainError> {
        let attempt = self
            .attempts
            .get(&attempt_id)
            .ok_or(DomainError::UnknownAttempt(attempt_id))?;
        let work_item_id = attempt.work_item_id;
        if self
            .work_items
            .get(&work_item_id)
            .is_some_and(|item| item.lifecycle == WorkItemLifecycle::Finalized)
        {
            return Err(DomainError::WorkItemFinalized);
        }
        // M005: one AgentRun per Attempt (SPEC-026 §5.2).
        if self
            .agent_runs
            .values()
            .any(|run| run.attempt_id == attempt_id)
        {
            return Err(DomainError::MultipleRunsNotPermitted);
        }
        let id = AgentRunId::new();
        self.agent_runs.insert(
            id,
            AgentRun {
                id,
                attempt_id,
                work_item_id,
                binding_generation: BindingGeneration::FIRST,
                control_generation: ControlGeneration::FIRST,
                run_revision: 1,
                lifecycle: AgentRunLifecycle::Created,
                execution_liveness: ExecutionLiveness::NotStarted,
                observation: ObservationFact::Disconnected,
                resumability: ResumabilityFact::NotEvaluated,
                termination: None,
                lineage,
                routing_decision_ref: None,
                current_execution: None,
            },
        );
        Ok(id)
    }

    pub fn work_scope(&self, id: WorkScopeId) -> Option<&WorkScope> {
        self.work_scopes.get(&id)
    }

    pub fn work_item(&self, id: WorkItemId) -> Option<&WorkItem> {
        self.work_items.get(&id)
    }

    pub fn attempt(&self, id: AttemptId) -> Option<&Attempt> {
        self.attempts.get(&id)
    }

    pub fn agent_run(&self, id: AgentRunId) -> Option<&AgentRun> {
        self.agent_runs.get(&id)
    }

    /// Append one immutable RoutingDecision and return its reference
    /// (SPEC-027 §4.2). Never overwrites or mutates an existing entry.
    pub(crate) fn record_routing_decision(
        &mut self,
        decision: RoutingDecision,
    ) -> RoutingDecisionRef {
        self.next_routing_decision = self.next_routing_decision.saturating_add(1);
        let reference = RoutingDecisionRef::new(self.next_routing_decision);
        self.routing_decisions.insert(reference, decision);
        reference
    }

    pub fn routing_decision(&self, reference: RoutingDecisionRef) -> Option<&RoutingDecision> {
        self.routing_decisions.get(&reference)
    }

    pub fn run_for_attempt(&self, attempt_id: AttemptId) -> Option<AgentRunId> {
        self.agent_runs
            .values()
            .find(|run| run.attempt_id == attempt_id)
            .map(|run| run.id)
    }

    pub fn validate_binding_generation(
        &self,
        agent_run_id: AgentRunId,
        presented: BindingGeneration,
    ) -> Result<(), DomainError> {
        let current = self
            .agent_runs
            .get(&agent_run_id)
            .ok_or(DomainError::UnknownAgentRun(agent_run_id))?
            .binding_generation;
        if current != presented {
            return Err(DomainError::StaleBinding { current, presented });
        }
        Ok(())
    }

    pub fn advance_binding_generation(
        &mut self,
        agent_run_id: AgentRunId,
        presented: BindingGeneration,
    ) -> Result<BindingGeneration, DomainError> {
        let run = self
            .agent_runs
            .get_mut(&agent_run_id)
            .ok_or(DomainError::UnknownAgentRun(agent_run_id))?;
        let current = run.binding_generation;
        if current != presented {
            return Err(DomainError::StaleBinding { current, presented });
        }
        let next = current.next().ok_or(DomainError::GenerationExhausted)?;
        run.binding_generation = next;
        run.bump_revision();
        Ok(next)
    }

    pub fn validate_control_generation(
        &self,
        agent_run_id: AgentRunId,
        presented: ControlGeneration,
    ) -> Result<(), DomainError> {
        let current = self
            .agent_runs
            .get(&agent_run_id)
            .ok_or(DomainError::UnknownAgentRun(agent_run_id))?
            .control_generation;
        if current != presented {
            return Err(DomainError::StaleControlEpoch { current, presented });
        }
        Ok(())
    }

    pub fn advance_control_generation(
        &mut self,
        agent_run_id: AgentRunId,
        presented: ControlGeneration,
    ) -> Result<ControlGeneration, DomainError> {
        let run = self
            .agent_runs
            .get_mut(&agent_run_id)
            .ok_or(DomainError::UnknownAgentRun(agent_run_id))?;
        let current = run.control_generation;
        if current != presented {
            return Err(DomainError::StaleControlEpoch { current, presented });
        }
        let next = current.next().ok_or(DomainError::GenerationExhausted)?;
        run.control_generation = next;
        run.bump_revision();
        Ok(next)
    }
}

#[cfg(test)]
#[path = "domain_tests.rs"]
mod tests;
