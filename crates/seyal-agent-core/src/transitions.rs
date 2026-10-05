//! SPEC-026 §9 transition APIs on [`AgentDomain`].
//!
//! `ControlGeneration` is the client control epoch (SPEC-026 O1 / §8.3).

use crate::domain::{AgentDomain, AgentRun, Attempt, DomainError, WorkItem};
use crate::lifecycle::{
    AcceptanceContractMode, AccountingValue, AgentRunLifecycle, AgentRunLineage,
    AttemptDisposition, AttemptLifecycle, AttemptOrigin, ExecutionLiveness, ExecutionRef,
    ExternalIdentityKey, ObservationFact, ResumabilityFact, RoutingDecision, RunTermination,
    TerminationKind, TerminationSource, WorkItemLifecycle, WorkItemOutcome,
};
use crate::{AgentRunId, AttemptId, BindingGeneration, ControlGeneration, WorkItemId, WorkScopeId};

/// Result of an identity-preserving or identity-minting transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransitionIds {
    pub work_item_id: WorkItemId,
    pub attempt_id: AttemptId,
    pub agent_run_id: AgentRunId,
}

impl AgentDomain {
    /// Finalize a WorkItem via the authorized outcome path (SPEC-019 / SPEC-026 §4).
    pub fn finalize_work_item(
        &mut self,
        work_item_id: WorkItemId,
        outcome: WorkItemOutcome,
        authorized: bool,
    ) -> Result<(), DomainError> {
        if !authorized {
            return Err(DomainError::NotAuthorized);
        }
        let item = self
            .work_items
            .get_mut(&work_item_id)
            .ok_or(DomainError::UnknownWorkItem(work_item_id))?;
        if item.lifecycle == WorkItemLifecycle::Finalized {
            return Err(DomainError::WorkItemFinalized);
        }
        item.lifecycle = WorkItemLifecycle::Finalized;
        item.outcome = Some(outcome);
        Ok(())
    }

    /// Reject mutation of a finalized WorkItem; regression creates a related Open item.
    pub fn create_related_work_item(
        &mut self,
        prior: WorkItemId,
    ) -> Result<WorkItemId, DomainError> {
        let prior_item = *self
            .work_items
            .get(&prior)
            .ok_or(DomainError::UnknownWorkItem(prior))?;
        if prior_item.lifecycle != WorkItemLifecycle::Finalized {
            return Err(DomainError::InvalidTransition);
        }
        let id = WorkItemId::new();
        self.work_items.insert(
            id,
            WorkItem {
                id,
                work_scope_id: prior_item.work_scope_id,
                lifecycle: WorkItemLifecycle::Open,
                outcome: None,
                acceptance_mode: prior_item.acceptance_mode,
                related_to: Some(prior),
            },
        );
        Ok(id)
    }

    pub fn create_attempt_with_origin(
        &mut self,
        work_item_id: WorkItemId,
        origin: AttemptOrigin,
    ) -> Result<AttemptId, DomainError> {
        let item = self
            .work_items
            .get(&work_item_id)
            .ok_or(DomainError::UnknownWorkItem(work_item_id))?;
        if item.lifecycle == WorkItemLifecycle::Finalized {
            return Err(DomainError::WorkItemFinalized);
        }
        if let Some(parent) = origin.parent() {
            let parent_attempt = self
                .attempts
                .get(&parent)
                .ok_or(DomainError::UnknownAttempt(parent))?;
            if parent_attempt.work_item_id != work_item_id {
                return Err(DomainError::InvalidTransition);
            }
        }
        let id = AttemptId::new();
        self.attempts.insert(
            id,
            Attempt {
                id,
                work_item_id,
                origin,
                lifecycle: AttemptLifecycle::Created,
                disposition: None,
                usage: AccountingValue::Unknown,
                cost: AccountingValue::Unknown,
            },
        );
        Ok(id)
    }

    /// Prepare: bind route/context/permission prerequisites (Created → Prepared).
    ///
    /// `routing` is committed atomically with the lifecycle transition
    /// (SPEC-027 §4.2); this mints and stores the immutable history entry.
    pub fn prepare_agent_run(
        &mut self,
        agent_run_id: AgentRunId,
        routing: RoutingDecision,
    ) -> Result<(), DomainError> {
        Self::require_lifecycle(self.run_mut(agent_run_id)?.lifecycle, &[AgentRunLifecycle::Created])?;
        let reference = self.record_routing_decision(routing);
        let run = self.run_mut(agent_run_id)?;
        run.lifecycle = AgentRunLifecycle::Prepared;
        run.routing_decision_ref = Some(reference);
        run.bump_revision();
        Ok(())
    }

    /// Dispatch commits Dispatching **before** the ExecutionHost is invoked (§9.1).
    pub fn dispatch_agent_run(&mut self, agent_run_id: AgentRunId) -> Result<(), DomainError> {
        let attempt_id = {
            let run = self.run_mut(agent_run_id)?;
            Self::require_lifecycle(run.lifecycle, &[AgentRunLifecycle::Prepared])?;
            run.lifecycle = AgentRunLifecycle::Dispatching;
            run.bump_revision();
            run.attempt_id
        };
        self.activate_attempt_on_first_dispatch(attempt_id)?;
        Ok(())
    }

    /// Host confirms execution started (Dispatching → Active, liveness Alive).
    pub fn activate_agent_run(&mut self, agent_run_id: AgentRunId) -> Result<(), DomainError> {
        let run = self.run_mut(agent_run_id)?;
        Self::require_lifecycle(run.lifecycle, &[AgentRunLifecycle::Dispatching])?;
        run.lifecycle = AgentRunLifecycle::Active;
        run.execution_liveness = ExecutionLiveness::Alive;
        run.observation = ObservationFact::Connected;
        run.bump_revision();
        Ok(())
    }

    /// Full §9.1 start sequence after Created: Prepared → Dispatching.
    pub fn start_prepare_and_dispatch(
        &mut self,
        agent_run_id: AgentRunId,
        routing: RoutingDecision,
    ) -> Result<(), DomainError> {
        self.prepare_agent_run(agent_run_id, routing)?;
        self.dispatch_agent_run(agent_run_id)
    }

    /// Cancel intent (SPEC-026 §9.2). Returns whether the run is already Terminated.
    pub fn cancel_agent_run(
        &mut self,
        agent_run_id: AgentRunId,
        control: ControlGeneration,
        expected_revision: Option<u64>,
        ambiguous_effect: bool,
    ) -> Result<(), DomainError> {
        self.validate_control_generation(agent_run_id, control)?;
        let run = self.run_mut(agent_run_id)?;
        if let Some(expected) = expected_revision
            && run.run_revision != expected
        {
            return Err(DomainError::StaleRevision {
                expected,
                current: run.run_revision,
            });
        }
        if run.lifecycle.is_terminal() {
            return Err(DomainError::InvalidTransition);
        }
        if ambiguous_effect {
            run.resumability = ResumabilityFact::ReconciliationRequired;
            run.bump_revision();
            return Err(DomainError::ReconciliationRequired);
        }
        match run.lifecycle {
            AgentRunLifecycle::Created | AgentRunLifecycle::Prepared => {
                run.lifecycle = AgentRunLifecycle::Terminated;
                run.termination = Some(RunTermination {
                    kind: TerminationKind::Cancelled,
                    source: TerminationSource::User,
                    reason_code: None,
                });
                run.execution_liveness = ExecutionLiveness::NotStarted;
                run.bump_revision();
            }
            AgentRunLifecycle::Dispatching | AgentRunLifecycle::Active => {
                run.lifecycle = AgentRunLifecycle::Terminating;
                run.bump_revision();
            }
            AgentRunLifecycle::Terminating | AgentRunLifecycle::Terminated => {
                return Err(DomainError::InvalidTransition);
            }
        }
        Ok(())
    }

    /// Evidence completes a cancel that was in Terminating.
    pub fn confirm_cancel_termination(
        &mut self,
        agent_run_id: AgentRunId,
    ) -> Result<(), DomainError> {
        let run = self.run_mut(agent_run_id)?;
        Self::require_lifecycle(run.lifecycle, &[AgentRunLifecycle::Terminating])?;
        run.lifecycle = AgentRunLifecycle::Terminated;
        run.termination = Some(RunTermination {
            kind: TerminationKind::Cancelled,
            source: TerminationSource::User,
            reason_code: None,
        });
        run.execution_liveness = ExecutionLiveness::Exited;
        run.bump_revision();
        Ok(())
    }

    /// Harness/execution completion — never implies WorkItem outcome (§6.3).
    pub fn terminate_completed(
        &mut self,
        agent_run_id: AgentRunId,
        source: TerminationSource,
    ) -> Result<(), DomainError> {
        let run = self.run_mut(agent_run_id)?;
        if run.lifecycle.is_terminal() {
            return Err(DomainError::InvalidTransition);
        }
        run.lifecycle = AgentRunLifecycle::Terminated;
        run.termination = Some(RunTermination {
            kind: TerminationKind::Completed,
            source,
            reason_code: None,
        });
        run.execution_liveness = ExecutionLiveness::Exited;
        run.bump_revision();
        Ok(())
    }

    pub fn terminate_failed(
        &mut self,
        agent_run_id: AgentRunId,
        source: TerminationSource,
    ) -> Result<(), DomainError> {
        let run = self.run_mut(agent_run_id)?;
        if run.lifecycle.is_terminal() {
            return Err(DomainError::InvalidTransition);
        }
        run.lifecycle = AgentRunLifecycle::Terminated;
        run.termination = Some(RunTermination {
            kind: TerminationKind::Failed,
            source,
            reason_code: None,
        });
        run.execution_liveness = ExecutionLiveness::Exited;
        run.bump_revision();
        Ok(())
    }

    /// Close Attempt with disposition when every AgentRun is terminal (§5).
    pub fn close_attempt(
        &mut self,
        attempt_id: AttemptId,
        disposition: AttemptDisposition,
    ) -> Result<(), DomainError> {
        if self
            .agent_runs
            .values()
            .any(|run| run.attempt_id == attempt_id && run.lifecycle.is_non_terminal())
        {
            return Err(DomainError::AttemptNotClosable);
        }
        let attempt = self
            .attempts
            .get_mut(&attempt_id)
            .ok_or(DomainError::UnknownAttempt(attempt_id))?;
        if attempt.lifecycle == AttemptLifecycle::Closed {
            return Err(DomainError::InvalidTransition);
        }
        attempt.lifecycle = AttemptLifecycle::Closed;
        attempt.disposition = Some(disposition);
        Ok(())
    }

    /// Fresh retry from scratch: new Attempt(RetryOf) + new AgentRun (§9.3).
    pub fn fresh_retry(
        &mut self,
        prior_attempt_id: AttemptId,
        prior_disposition: AttemptDisposition,
    ) -> Result<TransitionIds, DomainError> {
        let prior = *self
            .attempts
            .get(&prior_attempt_id)
            .ok_or(DomainError::UnknownAttempt(prior_attempt_id))?;
        if prior.lifecycle != AttemptLifecycle::Closed {
            self.close_attempt(prior_attempt_id, prior_disposition)?;
        } else {
            match prior.disposition {
                Some(existing) if existing == prior_disposition => {}
                Some(_) | None => return Err(DomainError::InvalidTransition),
            }
        }
        let work_item_id = prior.work_item_id;
        let attempt_id = self
            .create_attempt_with_origin(work_item_id, AttemptOrigin::RetryOf(prior_attempt_id))?;
        // Preserve prior accounting evidence by leaving prior Attempt untouched.
        let agent_run_id = self.create_agent_run(attempt_id)?;
        Ok(TransitionIds {
            work_item_id,
            attempt_id,
            agent_run_id,
        })
    }

    /// Fork: new Attempt(ForkOf) + new AgentRun with lineage; no inherited control (§9.4).
    pub fn fork_run(&mut self, parent_run_id: AgentRunId) -> Result<TransitionIds, DomainError> {
        let parent = *self
            .agent_runs
            .get(&parent_run_id)
            .ok_or(DomainError::UnknownAgentRun(parent_run_id))?;
        let attempt_id = self.create_attempt_with_origin(
            parent.work_item_id,
            AttemptOrigin::ForkOf(parent.attempt_id),
        )?;
        let agent_run_id = self.create_agent_run_with_lineage(
            attempt_id,
            Some(AgentRunLineage::ForkOf(parent_run_id)),
        )?;
        Ok(TransitionIds {
            work_item_id: parent.work_item_id,
            attempt_id,
            agent_run_id,
        })
    }

    /// Parallel competing candidate (§9.4).
    pub fn parallel_candidate(
        &mut self,
        parent_attempt_id: AttemptId,
    ) -> Result<TransitionIds, DomainError> {
        let parent = *self
            .attempts
            .get(&parent_attempt_id)
            .ok_or(DomainError::UnknownAttempt(parent_attempt_id))?;
        let attempt_id = self.create_attempt_with_origin(
            parent.work_item_id,
            AttemptOrigin::ParallelCandidateOf(parent_attempt_id),
        )?;
        let agent_run_id = self.create_agent_run(attempt_id)?;
        Ok(TransitionIds {
            work_item_id: parent.work_item_id,
            attempt_id,
            agent_run_id,
        })
    }

    /// Strategy/model change as a new candidate (§9.4).
    pub fn strategy_change_candidate(
        &mut self,
        parent_attempt_id: AttemptId,
    ) -> Result<TransitionIds, DomainError> {
        let parent = *self
            .attempts
            .get(&parent_attempt_id)
            .ok_or(DomainError::UnknownAttempt(parent_attempt_id))?;
        let attempt_id = self.create_attempt_with_origin(
            parent.work_item_id,
            AttemptOrigin::StrategyChangeFrom(parent_attempt_id),
        )?;
        let agent_run_id = self.create_agent_run(attempt_id)?;
        Ok(TransitionIds {
            work_item_id: parent.work_item_id,
            attempt_id,
            agent_run_id,
        })
    }

    /// Resume same AgentRun when BehavioralResumeAvailable (§9.5). No retry budget.
    pub fn resume_agent_run(&mut self, agent_run_id: AgentRunId) -> Result<(), DomainError> {
        let run = self
            .agent_runs
            .get(&agent_run_id)
            .ok_or(DomainError::UnknownAgentRun(agent_run_id))?;
        if run.lifecycle.is_terminal() {
            return Err(DomainError::InvalidTransition);
        }
        if run.resumability == ResumabilityFact::ReconciliationRequired {
            return Err(DomainError::ReconciliationRequired);
        }
        if run.resumability != ResumabilityFact::BehavioralResumeAvailable {
            return Err(DomainError::ResumeNotAvailable {
                reason: run.resumability,
            });
        }
        let run = self.run_mut(agent_run_id)?;
        run.observation = ObservationFact::Connected;
        run.bump_revision();
        Ok(())
    }

    /// Adapter/worker replacement: same run, new binding generation (§9.5 / §7).
    pub fn rebind_agent_run(
        &mut self,
        agent_run_id: AgentRunId,
        presented: BindingGeneration,
    ) -> Result<BindingGeneration, DomainError> {
        match self.advance_binding_generation(agent_run_id, presented) {
            Ok(next) => {
                let run = self.run_mut(agent_run_id)?;
                run.observation = ObservationFact::Disconnected;
                run.bump_revision();
                Ok(next)
            }
            Err(DomainError::GenerationExhausted) => {
                let run = self.run_mut(agent_run_id)?;
                run.resumability = ResumabilityFact::ReconciliationRequired;
                run.bump_revision();
                Err(DomainError::ReconciliationRequired)
            }
            Err(other) => Err(other),
        }
    }

    /// Pre-start route fallback on typed not-started evidence (§9.6 / O2).
    pub fn pre_start_fallback(
        &mut self,
        agent_run_id: AgentRunId,
        new_routing: RoutingDecision,
        proof_not_started: bool,
    ) -> Result<(), DomainError> {
        {
            let run = self.run_mut(agent_run_id)?;
            Self::require_lifecycle(run.lifecycle, &[AgentRunLifecycle::Dispatching])?;
            if run.execution_liveness != ExecutionLiveness::NotStarted {
                return Err(DomainError::InvalidTransition);
            }
            if !proof_not_started {
                run.resumability = ResumabilityFact::ReconciliationRequired;
                run.bump_revision();
                return Err(DomainError::ReconciliationRequired);
            }
        }
        let reference = self.record_routing_decision(new_routing);
        let run = self.run_mut(agent_run_id)?;
        run.lifecycle = AgentRunLifecycle::Prepared;
        run.routing_decision_ref = Some(reference);
        run.bump_revision();
        Ok(())
    }

    /// Agent Backend restart recovery fence before accepting new control (§9.7).
    pub fn backend_restart_recovery(
        &mut self,
        agent_run_id: AgentRunId,
    ) -> Result<(BindingGeneration, ControlGeneration), DomainError> {
        let (binding, control) = {
            let run = self
                .agent_runs
                .get(&agent_run_id)
                .ok_or(DomainError::UnknownAgentRun(agent_run_id))?;
            if run.lifecycle.is_terminal() {
                return Ok((run.binding_generation, run.control_generation));
            }
            (run.binding_generation, run.control_generation)
        };
        let next_binding = match binding.next() {
            Some(next) => next,
            None => {
                let run = self.run_mut(agent_run_id)?;
                run.execution_liveness = ExecutionLiveness::Unknown;
                run.observation = ObservationFact::Disconnected;
                run.resumability = ResumabilityFact::ReconciliationRequired;
                run.bump_revision();
                return Err(DomainError::ReconciliationRequired);
            }
        };
        let next_control = control.next().ok_or(DomainError::ReconciliationRequired)?;
        // Drop all client attachments (pre-restart sessions rejected).
        self.attachments
            .retain(|_, (run, _, _, _)| *run != agent_run_id);
        let run = self.run_mut(agent_run_id)?;
        run.binding_generation = next_binding;
        run.control_generation = next_control;
        run.execution_liveness = ExecutionLiveness::Unknown;
        run.observation = ObservationFact::Disconnected;
        run.resumability = ResumabilityFact::ReconciliationRequired;
        run.bump_revision();
        Ok((next_binding, next_control))
    }

    /// Terminal Runtime replacement execution: new ExecutionId only (§9.8).
    pub fn replace_execution(
        &mut self,
        agent_run_id: AgentRunId,
        new_execution: ExecutionRef,
        old_execution: ExecutionRef,
    ) -> Result<(), DomainError> {
        if new_execution == old_execution {
            return Err(DomainError::InvalidTransition);
        }
        if self.retired_executions.contains(&old_execution) {
            return Err(DomainError::InvalidTransition);
        }
        self.retired_executions.insert(old_execution);
        let run = self.run_mut(agent_run_id)?;
        if run.lifecycle.is_terminal() {
            return Err(DomainError::InvalidTransition);
        }
        run.current_execution = Some(new_execution);
        run.resumability = ResumabilityFact::ReconciliationRequired;
        run.execution_liveness = ExecutionLiveness::Unknown;
        run.bump_revision();
        Ok(())
    }

    /// External detection bind — idempotent on (execution_ref, external identity) (§9.9).
    pub fn detect_and_bind(
        &mut self,
        work_scope_id: WorkScopeId,
        key: ExternalIdentityKey,
    ) -> Result<(TransitionIds, bool), DomainError> {
        if let Some(existing) = self.detection_bindings.get(&key).copied() {
            let run = self
                .agent_runs
                .get(&existing)
                .ok_or(DomainError::UnknownAgentRun(existing))?;
            return Ok((
                TransitionIds {
                    work_item_id: run.work_item_id,
                    attempt_id: run.attempt_id,
                    agent_run_id: existing,
                },
                true,
            ));
        }
        if !self.work_scopes.contains_key(&work_scope_id) {
            return Err(DomainError::UnknownWorkScope(work_scope_id));
        }
        let work_item_id = {
            let id = WorkItemId::new();
            self.work_items.insert(
                id,
                WorkItem {
                    id,
                    work_scope_id,
                    lifecycle: WorkItemLifecycle::Open,
                    outcome: None,
                    acceptance_mode: AcceptanceContractMode::HumanFinal,
                    related_to: None,
                },
            );
            id
        };
        let attempt_id = self.create_attempt_with_origin(work_item_id, AttemptOrigin::Initial)?;
        let agent_run_id = self.create_agent_run(attempt_id)?;
        {
            let run = self.run_mut(agent_run_id)?;
            run.current_execution = Some(key.execution_ref);
        }
        self.detection_bindings.insert(key, agent_run_id);
        Ok((
            TransitionIds {
                work_item_id,
                attempt_id,
                agent_run_id,
            },
            false,
        ))
    }

    fn activate_attempt_on_first_dispatch(
        &mut self,
        attempt_id: AttemptId,
    ) -> Result<(), DomainError> {
        let attempt = self
            .attempts
            .get_mut(&attempt_id)
            .ok_or(DomainError::UnknownAttempt(attempt_id))?;
        if attempt.lifecycle == AttemptLifecycle::Created {
            attempt.lifecycle = AttemptLifecycle::Active;
        }
        Ok(())
    }

    pub(crate) fn run_mut(
        &mut self,
        agent_run_id: AgentRunId,
    ) -> Result<&mut AgentRun, DomainError> {
        self.agent_runs
            .get_mut(&agent_run_id)
            .ok_or(DomainError::UnknownAgentRun(agent_run_id))
    }

    fn require_lifecycle(
        current: AgentRunLifecycle,
        allowed: &[AgentRunLifecycle],
    ) -> Result<(), DomainError> {
        if allowed.contains(&current) {
            Ok(())
        } else {
            Err(DomainError::InvalidTransition)
        }
    }
}
