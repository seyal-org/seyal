//! One backend-owned session over the accepted V1 command catalog.
//!
//! The socket is not the authority. Disconnect leaves the domain, store, and
//! `ClientSession` in place. A new process mints a new `BackendInstanceId` and
//! therefore rejects every session opened by the previous process.

use std::path::PathBuf;

use seyal_agent_core::{
    AgentRunId, AttemptId, BindingGeneration, ControlGeneration, DomainError, WorkItemId,
    WorkScopeId, WorkScopeKind,
};
use seyal_agent_protocol::{
    accepted_body_len, decode_command, decode_frame, encode_result, AggregateRef, Command,
    CommandError, CommandResult, Frame, FrameError, FrameKind, ReplayEvent, SnapshotView,
    MAX_EVENT_WINDOW,
};
use seyal_agent_store::{AgentStore, AggregateId, AggregateSequence, StoreError};

use crate::{
    AuthorizationError, AuthorizationRepository, ClientScope, FakeExecutionHost, HostObservation,
    HostObservationKind, ObservationAuthority, PrincipalKind, RunLiveness, ScriptStep,
};

const OUTPUT_CHUNK: usize = 1024;
const EVENT_OBSERVATION: u16 = 2;

pub(crate) enum SessionRead {
    Frame(Frame),
    Disconnected,
    Oversized,
    Malformed,
    TimedOut,
    Io,
}

pub(crate) fn read_session_frame(
    stream: &mut impl std::io::Read,
    max_frame_size: u32,
) -> SessionRead {
    let mut header = [0; 10];
    match stream.read_exact(&mut header) {
        Ok(()) => {}
        Err(error) if is_disconnect(&error) => return SessionRead::Disconnected,
        Err(error) => return map_read_error(error),
    }
    let body_len = match accepted_body_len(&header, max_frame_size) {
        Ok(len) => len,
        Err(FrameError::Oversized) => return SessionRead::Oversized,
        Err(_) => return SessionRead::Malformed,
    };
    let mut body = vec![0; body_len];
    if body_len > 0
        && let Err(error) = stream.read_exact(&mut body)
    {
        return if is_disconnect(&error) {
            SessionRead::Io
        } else {
            map_read_error(error)
        };
    }
    let mut bytes = Vec::with_capacity(header.len() + body.len());
    bytes.extend_from_slice(&header);
    bytes.extend_from_slice(&body);
    match decode_frame(&bytes, max_frame_size) {
        Ok(frame) => SessionRead::Frame(frame),
        Err(_) => SessionRead::Malformed,
    }
}

fn is_disconnect(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::UnexpectedEof
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::BrokenPipe
    )
}

fn map_read_error(error: std::io::Error) -> SessionRead {
    if error.kind() == std::io::ErrorKind::TimedOut
        || error.kind() == std::io::ErrorKind::WouldBlock
    {
        SessionRead::TimedOut
    } else {
        SessionRead::Io
    }
}

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
                Ok(command) => self.dispatch(command, event_window),
                Err(_) => CommandResult::Error(CommandError::Malformed),
            }
        };
        encode_result(&result, max_frame_size).map_err(|_| ServiceError::Failed)
    }

    fn dispatch(&mut self, command: Command, event_window: u32) -> CommandResult {
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
            } => self.subscribe(session_id, aggregate, after, event_window),
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
        for observation in observations {
            if let Err(error) = self.commit_observation(observation) {
                return CommandResult::Error(error);
            }
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

    fn commit_observation(&mut self, observation: HostObservation) -> Result<(), CommandError> {
        let before = self.authority.applied_count();
        self.authority.apply(observation.clone()).map_err(|error| {
            use crate::ObserveError;
            match error {
                ObserveError::StaleGeneration => CommandError::StaleBinding,
                ObserveError::Domain(DomainError::StaleControlGeneration { .. }) => {
                    CommandError::StaleControl
                }
                ObserveError::Domain(DomainError::UnknownAgentRun(_)) => CommandError::NotFound,
                _ => CommandError::Failed,
            }
        })?;
        if self.authority.applied_count() == before {
            return Ok(());
        }
        let payload = observation_payload(&observation);
        self.store
            .append_event(
                AggregateId::AgentRun(observation.run_id),
                EVENT_OBSERVATION,
                &payload,
            )
            .map_err(|_| CommandError::Failed)?;
        Ok(())
    }

    fn snapshot(
        &mut self,
        session_id: seyal_agent_core::ClientSessionId,
        aggregate: AggregateRef,
    ) -> CommandResult {
        if let Err(error) =
            self.auth
                .authorize_session(session_id, self.instance_id, ClientScope::RunsObserve)
        {
            return CommandResult::Error(map_auth(error));
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
    ) -> CommandResult {
        if let Err(error) =
            self.auth
                .authorize_session(session_id, self.instance_id, ClientScope::RunsObserve)
        {
            return CommandResult::Error(map_auth(error));
        }
        let after = match after {
            Some(sequence) => match AggregateSequence::from_raw(sequence) {
                Some(sequence) => Some(sequence),
                None => return CommandResult::Error(CommandError::Malformed),
            },
            None => None,
        };
        let window = event_window.clamp(1, MAX_EVENT_WINDOW) as usize;
        match self.store.replay_after(to_aggregate(aggregate), after) {
            Ok(events) => {
                let events = events
                    .into_iter()
                    .take(window)
                    .map(|event| ReplayEvent {
                        sequence: event.sequence.get(),
                        kind: event.kind,
                        payload: event.payload,
                    })
                    .collect();
                CommandResult::Replay { events }
            }
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
}

fn restore_identities(
    store: &AgentStore,
    authority: &mut ObservationAuthority,
    auth: &mut AuthorizationRepository,
    principal_id: seyal_agent_core::ClientPrincipalId,
) -> Result<(), ServiceError> {
    for (id, kind) in store.work_scopes().map_err(|_| ServiceError::Failed)? {
        let kind = WorkScopeKind::from_code(kind).ok_or(ServiceError::Failed)?;
        authority
            .restore_work_scope(id, kind)
            .map_err(|_| ServiceError::Failed)?;
    }
    for (id, scope) in store.work_items().map_err(|_| ServiceError::Failed)? {
        authority
            .restore_work_item(id, scope)
            .map_err(|_| ServiceError::Failed)?;
    }
    for (id, item) in store.attempts().map_err(|_| ServiceError::Failed)? {
        authority
            .restore_attempt(id, item)
            .map_err(|_| ServiceError::Failed)?;
    }
    for (id, run) in store.agent_runs().map_err(|_| ServiceError::Failed)? {
        let binding =
            BindingGeneration::from_raw(run.binding_generation).ok_or(ServiceError::Failed)?;
        let control =
            ControlGeneration::from_raw(run.control_generation).ok_or(ServiceError::Failed)?;
        authority
            .restore_agent_run(id, run.attempt_id, binding, control)
            .map_err(|_| ServiceError::Failed)?;
        authority.mark_recovered(id);
        auth.allow_run(principal_id, id)
            .map_err(|_| ServiceError::Failed)?;
    }
    Ok(())
}

fn to_aggregate(aggregate: AggregateRef) -> AggregateId {
    match aggregate {
        AggregateRef::WorkScope(id) => AggregateId::WorkScope(id),
        AggregateRef::WorkItem(id) => AggregateId::WorkItem(id),
        AggregateRef::Attempt(id) => AggregateId::Attempt(id),
        AggregateRef::AgentRun(id) => AggregateId::AgentRun(id),
    }
}

fn snapshot_payload(authority: &ObservationAuthority, run_id: AgentRunId) -> Vec<u8> {
    let run = authority.domain().agent_run(run_id);
    let mut payload = vec![liveness_code(authority.liveness(run_id))];
    if let Some(run) = run {
        payload.extend_from_slice(&run.binding_generation().get().to_le_bytes());
        payload.extend_from_slice(&run.control_generation().get().to_le_bytes());
    }
    payload
}

fn liveness_code(liveness: RunLiveness) -> u8 {
    match liveness {
        RunLiveness::ScriptedLive => 1,
        RunLiveness::KnownTerminated => 2,
        RunLiveness::UnknownAfterCrash => 3,
        RunLiveness::ObservationLost => 4,
    }
}

fn observation_payload(observation: &HostObservation) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&observation.ordinal.to_le_bytes());
    match &observation.kind {
        HostObservationKind::Started => payload.push(1),
        HostObservationKind::Progress { step } => {
            payload.push(2);
            payload.extend_from_slice(&step.to_le_bytes());
        }
        HostObservationKind::KnownSuccess => payload.push(3),
        HostObservationKind::KnownFailure => payload.push(4),
        HostObservationKind::HarnessCrashed | HostObservationKind::UnknownLiveness => {
            payload.push(5)
        }
        HostObservationKind::ObservationDisconnected => payload.push(6),
        HostObservationKind::ObservationReconnected => payload.push(7),
        HostObservationKind::Output(bytes) => {
            payload.push(8);
            payload.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            payload.extend_from_slice(bytes);
        }
        HostObservationKind::Result(bytes) => {
            payload.push(9);
            payload.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            payload.extend_from_slice(bytes);
        }
        HostObservationKind::EffectUnknown => payload.push(10),
        HostObservationKind::Delayed { ticks } => {
            payload.push(11);
            payload.extend_from_slice(&ticks.to_le_bytes());
        }
    }
    payload
}

fn map_auth(error: AuthorizationError) -> CommandError {
    match error {
        AuthorizationError::UnknownSession | AuthorizationError::StaleBackendInstance => {
            CommandError::RejectedSession
        }
        AuthorizationError::Malformed | AuthorizationError::ReplayedRequest => {
            CommandError::Malformed
        }
        _ => CommandError::Denied,
    }
}

fn map_domain(error: DomainError) -> CommandError {
    match error {
        DomainError::StaleBindingGeneration { .. } => CommandError::StaleBinding,
        DomainError::StaleControlGeneration { .. } => CommandError::StaleControl,
        DomainError::UnknownWorkScope(_)
        | DomainError::UnknownWorkItem(_)
        | DomainError::UnknownAttempt(_)
        | DomainError::UnknownAgentRun(_) => CommandError::NotFound,
        DomainError::GenerationExhausted | DomainError::Conflict => CommandError::Failed,
    }
}
