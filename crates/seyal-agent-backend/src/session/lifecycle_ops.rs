//! SPEC-026 §9.1 StartAgentRun lifecycle drive through the domain writer.

use seyal_agent_core::{
    codes, AgentRunId, AgentRunLifecycle, AttemptId, BindingGeneration, ClientPrincipalId,
    ClientSessionId, ControlGeneration, RoutingDecisionRef,
};
use seyal_agent_protocol::{CommandError, CommandResult};
use seyal_agent_store::{AggregateId, AggregateSequence, StoreError};

use crate::ClientScope;

use super::wire::snapshot_payload;
use super::IntegrationService;

const EVENT_RUN_CREATED: u16 = 1;

impl IntegrationService {
    pub(super) fn start_agent_run(
        &mut self,
        principal_id: ClientPrincipalId,
        session_id: ClientSessionId,
        attempt_id: AttemptId,
    ) -> CommandResult {
        let principal =
            match self.authorized_session(principal_id, session_id, ClientScope::RunsCreate) {
                Ok(principal) => principal,
                Err(error) => return CommandResult::Error(error),
            };
        if self.authority.domain().attempt(attempt_id).is_none() {
            return CommandResult::Error(CommandError::NotFound);
        }
        if self
            .authority
            .domain()
            .run_for_attempt(attempt_id)
            .is_some()
        {
            return CommandResult::Error(CommandError::Failed);
        }
        // AB-1.9: hostless production fails closed before any AgentRun mint.
        if self.host.is_none() {
            return CommandResult::Error(CommandError::Failed);
        }

        let run_id = AgentRunId::new();
        let binding = BindingGeneration::FIRST;
        let control = ControlGeneration::FIRST;
        if self
            .store
            .mutate_agent_run_and_append(
                run_id,
                attempt_id,
                binding.get(),
                control.get(),
                EVENT_RUN_CREATED,
                &attempt_id.to_bytes(),
            )
            .and_then(|_| {
                self.authority
                    .restore_agent_run(run_id, attempt_id, binding, control)
                    .map_err(|_| StoreError::Corrupt)
            })
            .is_err()
        {
            return CommandResult::Error(CommandError::Failed);
        }

        // Grant auth before further durable writes so a mid-start persistence
        // fault still leaves the run observable (AB-1 fault-injection contract).
        if self.auth.allow_run(principal, run_id).is_err() {
            return CommandResult::Error(CommandError::Failed);
        }
        self.auth.allow_run_for_observers(run_id);

        // §9.1: Created → Prepared → Dispatching before host invocation.
        let routing = RoutingDecisionRef::new(1);
        if self
            .authority
            .domain_mut()
            .start_prepare_and_dispatch(run_id, routing)
            .is_err()
        {
            return CommandResult::Error(CommandError::Failed);
        }
        if persist_run_lifecycle(&self.store, &self.authority, run_id).is_err() {
            return CommandResult::Error(CommandError::Failed);
        }

        let host = self.host.as_mut().expect("host checked present");
        let observations = match host.collect_observations(run_id, binding) {
            Ok(observations) => observations,
            Err(()) => return CommandResult::Error(CommandError::Failed),
        };
        if let Err(error) = self.commit_observation_batch(observations) {
            return CommandResult::Error(error);
        }

        // Host confirmation → Active when still Dispatching (scripted start).
        if self
            .authority
            .domain()
            .agent_run(run_id)
            .is_some_and(|run| run.lifecycle() == AgentRunLifecycle::Dispatching)
            && self
                .authority
                .domain_mut()
                .activate_agent_run(run_id)
                .is_ok()
        {
            let _ = persist_run_lifecycle(&self.store, &self.authority, run_id);
        }

        let aggregate = AggregateId::AgentRun(run_id);
        let event_count = match self.store.high_water(aggregate) {
            Ok(count) if count > 0 => count,
            _ => return CommandResult::Error(CommandError::Failed),
        };
        let Some(last) = AggregateSequence::from_raw(event_count) else {
            return CommandResult::Error(CommandError::Failed);
        };
        let payload = snapshot_payload(&self.authority, run_id);
        if self.store.snapshot(aggregate, last, &payload).is_err() {
            return CommandResult::Error(CommandError::Failed);
        }
        CommandResult::Started {
            run_id,
            binding_generation: binding.get(),
            control_generation: control.get(),
            event_count,
        }
    }
}

fn persist_run_lifecycle(
    store: &seyal_agent_store::AgentStore,
    authority: &crate::ObservationAuthority,
    run_id: AgentRunId,
) -> Result<(), StoreError> {
    let run = authority
        .domain()
        .agent_run(run_id)
        .ok_or(StoreError::Corrupt)?;
    // Column-only: StartAgentRun already appends the create outbox event and
    // host observations. Lifecycle facts are durable on the identity row; later
    // control transitions append Critical outbox events when they mutate.
    store.set_agent_run_lifecycle_columns(
        run_id,
        codes::agent_run_lifecycle(run.lifecycle()),
        codes::execution_liveness(run.execution_liveness()),
        codes::observation(run.observation()),
        codes::resumability(run.resumability()),
        run.run_revision(),
    )
}
