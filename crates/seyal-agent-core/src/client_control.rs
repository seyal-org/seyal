//! Client attachment, control fencing, and observation retention (SPEC-026 §8/§10).

use crate::domain::{AgentDomain, DomainError};
use crate::lifecycle::{
    AccountingValue, AgentRunLifecycle, AttachmentAccess, ExecutionLiveness, ExecutionRef,
    ObservationFact, ResumabilityFact,
};
use crate::{
    AgentRunId, AttemptId, BindingGeneration, ClientSessionId, ControlGeneration, WorkItemId,
};

impl AgentDomain {
    /// Client detach: attachment only; never cancels/terminates (§8.4 / fixture 5).
    pub fn attach_client(
        &mut self,
        agent_run_id: AgentRunId,
        session_id: ClientSessionId,
        access: AttachmentAccess,
    ) -> Result<(), DomainError> {
        let run = self
            .agent_runs
            .get(&agent_run_id)
            .ok_or(DomainError::UnknownAgentRun(agent_run_id))?;
        let control_epoch = run.control_generation;
        let issued_run_revision = run.run_revision;
        self.attachments
            .insert(session_id, (agent_run_id, access, control_epoch, issued_run_revision));
        Ok(())
    }

    pub fn detach_client(&mut self, session_id: ClientSessionId) -> Result<(), DomainError> {
        self.attachments
            .remove(&session_id)
            .ok_or(DomainError::NotAuthorized)?;
        Ok(())
    }

    pub fn client_control(
        &mut self,
        session_id: ClientSessionId,
        agent_run_id: AgentRunId,
        presented_epoch: ControlGeneration,
        expected_revision: u64,
    ) -> Result<(), DomainError> {
        let Some((attached_run, access, epoch, _)) = self.attachments.get(&session_id).copied()
        else {
            return Err(DomainError::NotAuthorized);
        };
        if attached_run != agent_run_id {
            return Err(DomainError::NotAuthorized);
        }
        if !matches!(access, AttachmentAccess::Control | AttachmentAccess::Interact) {
            return Err(DomainError::NotAuthorized);
        }
        if epoch != presented_epoch {
            return Err(DomainError::StaleControlEpoch {
                current: epoch,
                presented: presented_epoch,
            });
        }
        let run = self
            .agent_runs
            .get(&agent_run_id)
            .ok_or(DomainError::UnknownAgentRun(agent_run_id))?;
        if run.run_revision != expected_revision {
            return Err(DomainError::StaleRevision {
                expected: expected_revision,
                current: run.run_revision,
            });
        }
        if access != AttachmentAccess::Control {
            return Err(DomainError::NotAuthorized);
        }
        Ok(())
    }

    /// Record a producer observation with de-dup / late / stale handling (§10).
    pub fn record_observation(
        &mut self,
        agent_run_id: AgentRunId,
        producer_id: u64,
        binding_generation: BindingGeneration,
        producer_event_id: Option<u64>,
        kind: ObservationKind,
        attempt_closed: bool,
    ) -> Result<ObservationRecordResult, DomainError> {
        let run = self
            .agent_runs
            .get(&agent_run_id)
            .ok_or(DomainError::UnknownAgentRun(agent_run_id))?;
        let stale_binding = run.binding_generation != binding_generation;
        if stale_binding && kind.requires_current_binding() {
            return Err(DomainError::StaleBinding {
                current: run.binding_generation,
                presented: binding_generation,
            });
        }
        if let Some(event_id) = producer_event_id {
            let key = (producer_id, binding_generation.get(), event_id);
            if !self.observation_keys.insert(key) {
                return Ok(ObservationRecordResult::DuplicateAcknowledged);
            }
        }
        let late = attempt_closed;
        let stale = stale_binding;
        if !stale && !late {
            match kind {
                ObservationKind::HarnessExited => {
                    // Retained; only transitions if valid from committed state.
                    if run.lifecycle == AgentRunLifecycle::Active
                        || run.lifecycle == AgentRunLifecycle::Terminating
                        || run.lifecycle == AgentRunLifecycle::Dispatching
                    {
                        let run = self.run_mut(agent_run_id)?;
                        run.execution_liveness = ExecutionLiveness::Exited;
                        run.bump_revision();
                    }
                }
                ObservationKind::HarnessStarted => {
                    if run.lifecycle == AgentRunLifecycle::Dispatching {
                        self.activate_agent_run(agent_run_id)?;
                    }
                }
                ObservationKind::Output
                | ObservationKind::Progress
                | ObservationKind::Usage
                | ObservationKind::Warning => {}
                ObservationKind::ControlBearing => {}
            }
        }
        self.observation_log.push(LoggedObservation {
            agent_run_id,
            kind,
            late,
            stale,
        });
        Ok(if late {
            ObservationRecordResult::LateRetained
        } else if stale {
            ObservationRecordResult::StaleRetained
        } else {
            ObservationRecordResult::Accepted
        })
    }

    pub fn set_resumability(
        &mut self,
        agent_run_id: AgentRunId,
        fact: ResumabilityFact,
    ) -> Result<(), DomainError> {
        let run = self.run_mut(agent_run_id)?;
        run.resumability = fact;
        run.bump_revision();
        Ok(())
    }

    pub fn set_observation_fact(
        &mut self,
        agent_run_id: AgentRunId,
        fact: ObservationFact,
    ) -> Result<(), DomainError> {
        let run = self.run_mut(agent_run_id)?;
        run.observation = fact;
        run.bump_revision();
        Ok(())
    }

    pub fn set_attempt_usage(
        &mut self,
        attempt_id: AttemptId,
        usage: AccountingValue,
        cost: AccountingValue,
    ) -> Result<(), DomainError> {
        let attempt = self
            .attempts
            .get_mut(&attempt_id)
            .ok_or(DomainError::UnknownAttempt(attempt_id))?;
        attempt.usage = usage;
        attempt.cost = cost;
        Ok(())
    }

    pub fn retry_budget_consumed(&self, work_item_id: WorkItemId) -> u64 {
        self.attempts
            .values()
            .filter(|a| a.work_item_id == work_item_id && a.origin.is_retry())
            .count() as u64
    }

    pub fn observation_log(&self) -> &[LoggedObservation] {
        &self.observation_log
    }

    pub fn execution_retired(&self, execution: ExecutionRef) -> bool {
        self.retired_executions.contains(&execution)
    }

}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservationKind {
    HarnessStarted,
    HarnessExited,
    Output,
    Progress,
    Usage,
    Warning,
    ControlBearing,
}

impl ObservationKind {
    const fn requires_current_binding(self) -> bool {
        matches!(self, Self::ControlBearing)
    }

    pub const fn stale_tolerant(self) -> bool {
        matches!(
            self,
            Self::Output | Self::Progress | Self::Usage | Self::Warning
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservationRecordResult {
    Accepted,
    DuplicateAcknowledged,
    StaleRetained,
    LateRetained,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoggedObservation {
    pub agent_run_id: AgentRunId,
    pub kind: ObservationKind,
    pub late: bool,
    pub stale: bool,
}
