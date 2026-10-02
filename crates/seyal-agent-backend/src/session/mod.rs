//! One backend-owned session over the accepted V1 command catalog.
//!
//! The socket is not the authority. Disconnect leaves the domain, store, and
//! `ClientSession` in place. A new process mints a new `BackendInstanceId` and
//! therefore rejects every session opened by the previous process.

mod frame_io;
mod recovery;
mod wire;

#[cfg(test)]
mod tests;

pub(crate) use frame_io::{read_session_frame, SessionRead};

use std::path::PathBuf;

use seyal_agent_core::{
    AgentRunId, AttemptId, BindingGeneration, ControlGeneration, DomainError, WorkItemId,
    WorkScopeId, WorkScopeKind,
};
use seyal_agent_protocol::{
    decode_command, encode_result, AggregateRef, Command, CommandError, CommandResult, Frame,
    FrameKind, SnapshotView, ABSOLUTE_MAX_FRAME_SIZE, MAX_EVENT_WINDOW, REPLAY_EVENT_OVERHEAD,
    REPLAY_RESULT_OVERHEAD,
};
use seyal_agent_store::{AgentStore, AggregateId, AggregateSequence, StoreError, OUTPUT_REF_LEN};

use crate::{
    AuthorizationRepository, ClientScope, FakeExecutionHost, HostObservation, HostObservationKind,
    ObservationAuthority, ObserveError, PrincipalKind, RunLiveness, ScriptStep,
};

use recovery::restore_identities;
use wire::{
    fit_replay_events, liveness_code, map_auth, map_domain, observation_payload, snapshot_payload,
    to_aggregate, ReplayPage,
};

const OUTPUT_CHUNK: usize = 1024;
const EVENT_OBSERVATION: u16 = 2;

#[derive(Clone, Debug)]
pub struct IntegrationConfig {
    pub store_path: PathBuf,
    pub script: Vec<ScriptStep>,
}

pub struct IntegrationService {
    instance_id: seyal_agent_protocol::BackendInstanceId,
    principal_id: seyal_agent_core::ClientPrincipalId,
    auth: AuthorizationRepository,
    authority: ObservationAuthority,
    store: AgentStore,
    host: FakeExecutionHost,
    script: Vec<ScriptStep>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceError {
    Failed,
}

impl IntegrationService {
    pub fn open(
        instance_id: seyal_agent_protocol::BackendInstanceId,
        config: &IntegrationConfig,
    ) -> Result<Self, ServiceError> {
        if config.script.is_empty() {
            return Err(ServiceError::Failed);
        }
        let host = FakeExecutionHost::new(OUTPUT_CHUNK).map_err(|_| ServiceError::Failed)?;
        let store = AgentStore::open(&config.store_path).map_err(|_| ServiceError::Failed)?;
        let mut auth = AuthorizationRepository::default();
        let principal_id = auth.register_principal(
            PrincipalKind::FirstPartyCli,
            [
                ClientScope::RunsCreate,
                ClientScope::RunsObserve,
                ClientScope::RunsControl,
            ],
        );
        let mut authority = ObservationAuthority::new(seyal_agent_core::AgentDomain::new());
        restore_identities(&store, &mut authority, &mut auth, principal_id)?;
        Ok(Self {
            instance_id,
            principal_id,
            auth,
            authority,
            store,
            host,
            script: config.script.clone(),
        })
    }

    #[cfg(feature = "test-fault-injection")]
    pub(crate) fn fail_after_writes(&self, allowed: u64) {
        self.store.fail_after_writes(allowed);
    }

    pub fn handle(
        &mut self,
        frame: Frame,
        max_frame_size: u32,
        event_window: u32,
    ) -> Result<Vec<u8>, ServiceError> {
        let result = if frame.kind != FrameKind::Command {
            CommandResult::Error(CommandError::Malformed)
        } else {
            match decode_command(&frame.body) {
                Ok(command) => self.dispatch(command, event_window, max_frame_size),
                Err(_) => CommandResult::Error(CommandError::Malformed),
            }
        };
        encode_result(&result, max_frame_size).map_err(|_| ServiceError::Failed)
    }

    fn dispatch(
        &mut self,
        command: Command,
        event_window: u32,
        max_frame_size: u32,
    ) -> CommandResult {
        match command {
            Command::OpenSession { scopes } => self.open_session(scopes),
            Command::ResumeSession { session_id } => self.resume_session(session_id),
            Command::CreateWorkScope { session_id, kind } => {
                self.create_work_scope(session_id, kind)
            }
            Command::CreateWorkItem {
                session_id,
                work_scope_id,
            } => self.create_work_item(session_id, work_scope_id),
            Command::CreateAttempt {
                session_id,
                work_item_id,
            } => self.create_attempt(session_id, work_item_id),
            Command::StartAgentRun {
                session_id,
                attempt_id,
            } => self.start_agent_run(session_id, attempt_id),
            Command::GetSnapshot {
                session_id,
                aggregate,
            } => self.snapshot(session_id, aggregate),
            Command::Subscribe {
                session_id,
                aggregate,
                after,
            } => self.subscribe(session_id, aggregate, after, event_window, max_frame_size),
            Command::CheckGeneration {
                session_id,
                run_id,
                binding_generation,
                control_generation,
            } => self.check_generation(session_id, run_id, binding_generation, control_generation),
            Command::ReadRun { session_id, run_id } => self.read_run(session_id, run_id),
        }
    }

    fn open_session(&mut self, scopes: Vec<u8>) -> CommandResult {
        let mut decoded = Vec::with_capacity(scopes.len());
        for scope in scopes {
            match ClientScope::decode(scope) {
                Ok(scope) => decoded.push(scope),
                Err(_) => return CommandResult::Error(CommandError::Malformed),
            }
        }
        match self
            .auth
            .open_session(self.principal_id, self.instance_id, decoded)
        {
            Ok(session_id) => CommandResult::Opened { session_id },
            Err(error) => CommandResult::Error(map_auth(error)),
        }
    }

    fn resume_session(&mut self, session_id: seyal_agent_core::ClientSessionId) -> CommandResult {
        match self.auth.resume_session(session_id, self.instance_id) {
            Ok(()) => CommandResult::Resumed,
            Err(error) => CommandResult::Error(map_auth(error)),
        }
    }

    fn create_work_scope(
        &mut self,
        session_id: seyal_agent_core::ClientSessionId,
        kind: WorkScopeKind,
    ) -> CommandResult {
        if let Err(error) =
            self.auth
                .authorize_session(session_id, self.instance_id, ClientScope::RunsCreate)
        {
            return CommandResult::Error(map_auth(error));
        }
        let id = WorkScopeId::new();
        if self
            .store
            .commit_work_scope(id, kind.code())
            .and_then(|_| {
                self.authority
                    .restore_work_scope(id, kind)
                    .map_err(|_| StoreError::Corrupt)
            })
            .is_err()
        {
            return CommandResult::Error(CommandError::Failed);
        }
        CommandResult::WorkScope { id }
    }

    fn create_work_item(
        &mut self,
        session_id: seyal_agent_core::ClientSessionId,
        work_scope_id: WorkScopeId,
    ) -> CommandResult {
        if let Err(error) =
            self.auth
                .authorize_session(session_id, self.instance_id, ClientScope::RunsCreate)
        {
            return CommandResult::Error(map_auth(error));
        }
        if self.authority.domain().work_scope(work_scope_id).is_none() {
            return CommandResult::Error(CommandError::NotFound);
        }
        let id = WorkItemId::new();
        if self
            .store
            .commit_work_item(id, work_scope_id)
            .and_then(|_| {
                self.authority
                    .restore_work_item(id, work_scope_id)
                    .map_err(|_| StoreError::Corrupt)
            })
            .is_err()
        {
            return CommandResult::Error(CommandError::Failed);
        }
        CommandResult::WorkItem { id }
    }

    fn create_attempt(
        &mut self,
        session_id: seyal_agent_core::ClientSessionId,
        work_item_id: WorkItemId,
    ) -> CommandResult {
        if let Err(error) =
            self.auth
                .authorize_session(session_id, self.instance_id, ClientScope::RunsCreate)
        {
            return CommandResult::Error(map_auth(error));
        }
        if self.authority.domain().work_item(work_item_id).is_none() {
            return CommandResult::Error(CommandError::NotFound);
        }
        let id = AttemptId::new();
        if self
            .store
            .commit_attempt(id, work_item_id)
            .and_then(|_| {
                self.authority
                    .restore_attempt(id, work_item_id)
                    .map_err(|_| StoreError::Corrupt)
            })
            .is_err()
        {
            return CommandResult::Error(CommandError::Failed);
        }
        CommandResult::Attempt { id }
    }

    fn start_agent_run(
        &mut self,
        session_id: seyal_agent_core::ClientSessionId,
        attempt_id: AttemptId,
    ) -> CommandResult {
        let principal =
            match self
                .auth
                .authorize_session(session_id, self.instance_id, ClientScope::RunsCreate)
            {
                Ok(principal) => principal,
                Err(error) => return CommandResult::Error(map_auth(error)),
            };
        if self.authority.domain().attempt(attempt_id).is_none() {
            return CommandResult::Error(CommandError::NotFound);
        }
        let run_id = AgentRunId::new();
        let binding = BindingGeneration::FIRST;
        let control = ControlGeneration::FIRST;
        // Persist the run before host observations. A later allow_run/host
        // failure returns Failed even though the row remains for recovery
        // (false failure, not false success).
        if self
            .store
            .mutate_agent_run_and_append(
                run_id,
                attempt_id,
                binding.get(),
                control.get(),
                1,
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
        if self.auth.allow_run(principal, run_id).is_err() {
            return CommandResult::Error(CommandError::Failed);
        }

        let observations = match self.host.execute(run_id, binding, &self.script) {
            Ok(observations) => observations,
            Err(_) => return CommandResult::Error(CommandError::Failed),
        };
        if let Err(error) = self.commit_observation_batch(observations) {
            return CommandResult::Error(error);
        }
        let aggregate = AggregateId::AgentRun(run_id);
        let events = match self.store.replay_after(aggregate, None) {
            Ok(events) => events,
            Err(_) => return CommandResult::Error(CommandError::Failed),
        };
        let Some(last) = events.last() else {
            return CommandResult::Error(CommandError::Failed);
        };
        let payload = snapshot_payload(&self.authority, run_id);
        if self
            .store
            .snapshot(aggregate, last.sequence, &payload)
            .is_err()
        {
            return CommandResult::Error(CommandError::Failed);
        }
        CommandResult::Started {
            run_id,
            binding_generation: binding.get(),
            control_generation: control.get(),
            event_count: events.len() as u64,
        }
    }

    fn commit_observation_batch(
        &mut self,
        observations: Vec<HostObservation>,
    ) -> Result<(), CommandError> {
        let mut index = 0;
        while index < observations.len() {
            if matches!(observations[index].kind, HostObservationKind::Output(_)) {
                let run_id = observations[index].run_id;
                let mut end = index + 1;
                while end < observations.len()
                    && observations[end].run_id == run_id
                    && matches!(observations[end].kind, HostObservationKind::Output(_))
                {
                    end += 1;
                }
                self.commit_output_group(&observations[index..end])?;
                index = end;
            } else {
                self.commit_observation(observations[index].clone())?;
                index += 1;
            }
        }
        Ok(())
    }

    fn commit_output_group(&mut self, group: &[HostObservation]) -> Result<(), CommandError> {
        let mut applied = Vec::new();
        let mut joined = Vec::new();
        let mut first_ordinal = None;
        let mut last_ordinal = None;
        for observation in group {
            let previous_liveness = self.authority.recorded_liveness(observation.run_id);
            let previous_effects = self.authority.effects_performed();
            let before = self.authority.applied_count();
            if let Err(error) = self.authority.apply(observation.clone()) {
                // Mid-group apply failure must not leave earlier Outputs sticky
                // in memory (same class as the append-fault undo path).
                Self::undo_applied_group(&mut self.authority, applied);
                return Err(map_observe(error));
            }
            if self.authority.applied_count() == before {
                continue;
            }
            if let HostObservationKind::Output(bytes) = &observation.kind {
                joined.extend_from_slice(bytes);
                if first_ordinal.is_none() {
                    first_ordinal = Some(observation.ordinal);
                }
                last_ordinal = Some(observation.ordinal);
            }
            applied.push((observation.clone(), previous_liveness, previous_effects));
        }
        if applied.is_empty() {
            return Ok(());
        }
        let run_id = applied[0].0.run_id;
        let first = first_ordinal.unwrap_or(applied[0].0.ordinal);
        let last = last_ordinal.unwrap_or(applied[0].0.ordinal);
        if OUTPUT_REF_LEN
            > (ABSOLUTE_MAX_FRAME_SIZE as usize)
                .saturating_sub(REPLAY_RESULT_OVERHEAD)
                .saturating_sub(REPLAY_EVENT_OVERHEAD)
        {
            Self::undo_applied_group(&mut self.authority, applied);
            return Err(CommandError::Failed);
        }
        if self
            .store
            .append_output_event(run_id, EVENT_OBSERVATION, &joined, first, last)
            .is_err()
        {
            Self::undo_applied_group(&mut self.authority, applied);
            return Err(CommandError::Failed);
        }
        Ok(())
    }

    fn undo_applied_group(
        authority: &mut ObservationAuthority,
        applied: Vec<(HostObservation, Option<RunLiveness>, u64)>,
    ) {
        for (observation, previous_liveness, previous_effects) in applied.into_iter().rev() {
            authority.undo_apply(&observation, previous_liveness, previous_effects);
        }
    }

    fn commit_observation(&mut self, observation: HostObservation) -> Result<(), CommandError> {
        let payload = observation_payload(&observation);
        let max_payload = (ABSOLUTE_MAX_FRAME_SIZE as usize)
            .saturating_sub(REPLAY_RESULT_OVERHEAD)
            .saturating_sub(REPLAY_EVENT_OVERHEAD);
        if payload.len() > max_payload {
            return Err(CommandError::Failed);
        }
        let before = self.authority.applied_count();
        let previous_liveness = self.authority.recorded_liveness(observation.run_id);
        let previous_effects = self.authority.effects_performed();
        self.authority
            .apply(observation.clone())
            .map_err(map_observe)?;
        if self.authority.applied_count() == before {
            return Ok(());
        }
        if self
            .store
            .append_event(
                AggregateId::AgentRun(observation.run_id),
                EVENT_OBSERVATION,
                &payload,
            )
            .is_err()
        {
            self.authority
                .undo_apply(&observation, previous_liveness, previous_effects);
            return Err(CommandError::Failed);
        }
        Ok(())
    }

    fn snapshot(
        &mut self,
        session_id: seyal_agent_core::ClientSessionId,
        aggregate: AggregateRef,
    ) -> CommandResult {
        if let Err(error) = self.authorize_observe(session_id, aggregate) {
            return CommandResult::Error(error);
        }
        match self.store.get_snapshot(to_aggregate(aggregate)) {
            Ok(None) => CommandResult::Snapshot { view: None },
            Ok(Some((position, payload))) => CommandResult::Snapshot {
                view: Some(SnapshotView {
                    incorporated_through: position.incorporated_through.get(),
                    payload,
                }),
            },
            Err(_) => CommandResult::Error(CommandError::Failed),
        }
    }

    fn subscribe(
        &mut self,
        session_id: seyal_agent_core::ClientSessionId,
        aggregate: AggregateRef,
        after: Option<u64>,
        event_window: u32,
        max_frame_size: u32,
    ) -> CommandResult {
        if let Err(error) = self.authorize_observe(session_id, aggregate) {
            return CommandResult::Error(error);
        }
        let after = match after {
            Some(sequence) => match AggregateSequence::from_raw(sequence) {
                Some(sequence) => Some(sequence),
                None => return CommandResult::Error(CommandError::Malformed),
            },
            None => None,
        };
        let window = event_window.clamp(1, MAX_EVENT_WINDOW) as usize;
        match self
            .store
            .replay_page(to_aggregate(aggregate), after, window)
        {
            Ok(events) => match fit_replay_events(events, window, max_frame_size) {
                ReplayPage::Events(events) => CommandResult::Replay { events },
                ReplayPage::NextEventTooLarge => CommandResult::Error(CommandError::Failed),
            },
            Err(StoreError::History(gap)) => CommandResult::Gap {
                requested_after: gap.requested_after.get(),
                earliest_available: gap.earliest_available.get(),
                current_snapshot_sequence: gap
                    .current_snapshot_sequence
                    .map(|sequence| sequence.get()),
            },
            Err(_) => CommandResult::Error(CommandError::Failed),
        }
    }

    fn check_generation(
        &mut self,
        session_id: seyal_agent_core::ClientSessionId,
        run_id: AgentRunId,
        binding_generation: u64,
        control_generation: u64,
    ) -> CommandResult {
        if let Err(error) = self.auth.authorize_run(
            session_id,
            self.instance_id,
            ClientScope::RunsControl,
            run_id,
        ) {
            return CommandResult::Error(map_auth(error));
        }
        let Some(binding) = BindingGeneration::from_raw(binding_generation) else {
            return CommandResult::Error(CommandError::Malformed);
        };
        let Some(control) = ControlGeneration::from_raw(control_generation) else {
            return CommandResult::Error(CommandError::Malformed);
        };
        if let Err(error) = self
            .authority
            .domain()
            .validate_binding_generation(run_id, binding)
        {
            return CommandResult::Error(map_domain(error));
        }
        if let Err(error) = self
            .authority
            .domain()
            .validate_control_generation(run_id, control)
        {
            return CommandResult::Error(map_domain(error));
        }
        CommandResult::GenerationOk
    }

    fn read_run(
        &mut self,
        session_id: seyal_agent_core::ClientSessionId,
        run_id: AgentRunId,
    ) -> CommandResult {
        if let Err(error) = self.auth.authorize_run(
            session_id,
            self.instance_id,
            ClientScope::RunsObserve,
            run_id,
        ) {
            return CommandResult::Error(map_auth(error));
        }
        let Some(run) = self.authority.domain().agent_run(run_id) else {
            return CommandResult::Error(CommandError::NotFound);
        };
        CommandResult::Run {
            binding_generation: run.binding_generation().get(),
            control_generation: run.control_generation().get(),
            liveness: liveness_code(self.authority.liveness(run_id)),
        }
    }

    /// Run snapshots and replays use the same target gate as `ReadRun`.
    fn authorize_observe(
        &self,
        session_id: seyal_agent_core::ClientSessionId,
        aggregate: AggregateRef,
    ) -> Result<(), CommandError> {
        let decision = if let AggregateRef::AgentRun(run_id) = aggregate {
            self.auth.authorize_run(
                session_id,
                self.instance_id,
                ClientScope::RunsObserve,
                run_id,
            )
        } else {
            self.auth
                .authorize_session(session_id, self.instance_id, ClientScope::RunsObserve)
                .map(|_| ())
        };
        decision.map_err(map_auth)
    }
}

fn map_observe(error: ObserveError) -> CommandError {
    match error {
        ObserveError::StaleGeneration => CommandError::StaleBinding,
        ObserveError::Domain(DomainError::StaleControlGeneration { .. }) => {
            CommandError::StaleControl
        }
        ObserveError::Domain(DomainError::UnknownAgentRun(_)) => CommandError::NotFound,
        _ => CommandError::Failed,
    }
}
