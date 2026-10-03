//! Persist/recovery restore entry points for AgentDomain.

use crate::domain::{
    AgentDomain, AgentRun, Attempt, DomainError, WorkItem, WorkScope, WorkScopeKind,
};
use crate::lifecycle::{
    AcceptanceContractMode, AccountingValue, AgentRunLifecycle, AgentRunLineage,
    AttemptDisposition, AttemptLifecycle, AttemptOrigin, ExecutionLiveness, ObservationFact,
    ResumabilityFact, RoutingDecisionRef, RunTermination, WorkItemLifecycle, WorkItemOutcome,
};
use crate::{AgentRunId, AttemptId, BindingGeneration, ControlGeneration, WorkItemId, WorkScopeId};

impl AgentDomain {
    pub fn restore_work_scope(
        &mut self,
        id: WorkScopeId,
        kind: WorkScopeKind,
    ) -> Result<(), DomainError> {
        if let Some(existing) = self.work_scopes.get(&id) {
            return if existing.kind == kind {
                Ok(())
            } else {
                Err(DomainError::Conflict)
            };
        }
        self.work_scopes.insert(id, WorkScope { id, kind });
        Ok(())
    }

    pub fn restore_work_item(
        &mut self,
        id: WorkItemId,
        work_scope_id: WorkScopeId,
    ) -> Result<(), DomainError> {
        self.restore_work_item_state(
            id,
            work_scope_id,
            WorkItemLifecycle::Open,
            None,
            AcceptanceContractMode::HumanFinal,
            None,
        )
    }

    pub fn restore_work_item_state(
        &mut self,
        id: WorkItemId,
        work_scope_id: WorkScopeId,
        lifecycle: WorkItemLifecycle,
        outcome: Option<WorkItemOutcome>,
        acceptance_mode: AcceptanceContractMode,
        related_to: Option<WorkItemId>,
    ) -> Result<(), DomainError> {
        if !self.work_scopes.contains_key(&work_scope_id) {
            return Err(DomainError::UnknownWorkScope(work_scope_id));
        }
        let item = WorkItem {
            id,
            work_scope_id,
            lifecycle,
            outcome,
            acceptance_mode,
            related_to,
        };
        if let Some(existing) = self.work_items.get(&id) {
            return if *existing == item {
                Ok(())
            } else if existing.work_scope_id == work_scope_id
                && existing.lifecycle == WorkItemLifecycle::Open
                && lifecycle == WorkItemLifecycle::Open
            {
                Ok(())
            } else {
                Err(DomainError::Conflict)
            };
        }
        self.work_items.insert(id, item);
        Ok(())
    }

    pub fn restore_attempt(
        &mut self,
        id: AttemptId,
        work_item_id: WorkItemId,
    ) -> Result<(), DomainError> {
        self.restore_attempt_state(
            id,
            work_item_id,
            AttemptOrigin::Initial,
            AttemptLifecycle::Created,
            None,
            AccountingValue::Unknown,
            AccountingValue::Unknown,
        )
    }

    pub fn restore_attempt_state(
        &mut self,
        id: AttemptId,
        work_item_id: WorkItemId,
        origin: AttemptOrigin,
        lifecycle: AttemptLifecycle,
        disposition: Option<AttemptDisposition>,
        usage: AccountingValue,
        cost: AccountingValue,
    ) -> Result<(), DomainError> {
        if !self.work_items.contains_key(&work_item_id) {
            return Err(DomainError::UnknownWorkItem(work_item_id));
        }
        let attempt = Attempt {
            id,
            work_item_id,
            origin,
            lifecycle,
            disposition,
            usage,
            cost,
        };
        if let Some(existing) = self.attempts.get(&id) {
            return if existing.work_item_id == work_item_id {
                Ok(())
            } else {
                Err(DomainError::Conflict)
            };
        }
        self.attempts.insert(id, attempt);
        Ok(())
    }

    pub fn restore_agent_run(
        &mut self,
        id: AgentRunId,
        attempt_id: AttemptId,
        binding_generation: BindingGeneration,
        control_generation: ControlGeneration,
    ) -> Result<(), DomainError> {
        if !self.attempts.contains_key(&attempt_id) {
            return Err(DomainError::UnknownAttempt(attempt_id));
        }
        self.insert_restored_agent_run(
            id,
            attempt_id,
            binding_generation,
            control_generation,
            AgentRunLifecycle::Created,
            ExecutionLiveness::NotStarted,
            ObservationFact::Disconnected,
            ResumabilityFact::NotEvaluated,
            1,
            None,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore_agent_run_state(
        &mut self,
        id: AgentRunId,
        attempt_id: AttemptId,
        binding_generation: BindingGeneration,
        control_generation: ControlGeneration,
        lifecycle: AgentRunLifecycle,
        execution_liveness: ExecutionLiveness,
        observation: ObservationFact,
        resumability: ResumabilityFact,
        run_revision: u64,
        termination: Option<RunTermination>,
        lineage: Option<AgentRunLineage>,
        routing_decision_ref: Option<RoutingDecisionRef>,
    ) -> Result<(), DomainError> {
        if !self.attempts.contains_key(&attempt_id) {
            return Err(DomainError::UnknownAttempt(attempt_id));
        }
        self.insert_restored_agent_run(
            id,
            attempt_id,
            binding_generation,
            control_generation,
            lifecycle,
            execution_liveness,
            observation,
            resumability,
            run_revision,
            termination,
            lineage,
            routing_decision_ref,
        )
    }

    /// Recover a persisted AgentRun whose Attempt parent is absent.
    pub fn restore_orphaned_agent_run(
        &mut self,
        id: AgentRunId,
        attempt_id: AttemptId,
        binding_generation: BindingGeneration,
        control_generation: ControlGeneration,
    ) -> Result<(), DomainError> {
        if self.attempts.contains_key(&attempt_id) {
            return self.restore_agent_run(id, attempt_id, binding_generation, control_generation);
        }
        self.insert_restored_agent_run(
            id,
            attempt_id,
            binding_generation,
            control_generation,
            AgentRunLifecycle::Created,
            ExecutionLiveness::Unknown,
            ObservationFact::Disconnected,
            ResumabilityFact::ReconciliationRequired,
            1,
            None,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_restored_agent_run(
        &mut self,
        id: AgentRunId,
        attempt_id: AttemptId,
        binding_generation: BindingGeneration,
        control_generation: ControlGeneration,
        lifecycle: AgentRunLifecycle,
        execution_liveness: ExecutionLiveness,
        observation: ObservationFact,
        resumability: ResumabilityFact,
        run_revision: u64,
        termination: Option<RunTermination>,
        lineage: Option<AgentRunLineage>,
        routing_decision_ref: Option<RoutingDecisionRef>,
    ) -> Result<(), DomainError> {
        let work_item_id = self
            .attempts
            .get(&attempt_id)
            .map(|a| a.work_item_id)
            .unwrap_or_else(WorkItemId::new);
        if let Some(existing) = self.agent_runs.get(&id) {
            return if existing.attempt_id == attempt_id
                && existing.binding_generation == binding_generation
                && existing.control_generation == control_generation
            {
                Ok(())
            } else {
                Err(DomainError::Conflict)
            };
        }
        self.agent_runs.insert(
            id,
            AgentRun {
                id,
                attempt_id,
                work_item_id,
                binding_generation,
                control_generation,
                run_revision,
                lifecycle,
                execution_liveness,
                observation,
                resumability,
                termination,
                lineage,
                routing_decision_ref,
                current_execution: None,
            },
        );
        Ok(())
    }
}
