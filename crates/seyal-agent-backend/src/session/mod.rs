//! One backend-owned session over the accepted V1 command catalog.
//!
//! The socket is not the authority. Disconnect leaves the domain, store, and
//! `ClientSession` in place. A new process mints a new `BackendInstanceId` and
//! therefore rejects every session opened by the previous process.

mod action_ops;
mod frame_io;
mod launch_resolution;
mod lifecycle_ops;
mod recovery;
mod wire;

#[cfg(all(test, feature = "fixture-host"))]
mod fixture_support;
#[cfg(test)]
mod hostless_tests;
#[cfg(all(test, feature = "fixture-host"))]
mod spec027_catalog_tests;
#[cfg(all(test, feature = "fixture-host"))]
mod tests;

pub(crate) use frame_io::{read_session_frame, SessionRead};

use std::path::{Path, PathBuf};

use seyal_agent_core::{
    AgentRunId, AgentRunLifecycle, AttemptId, BindingGeneration, ClientPrincipalId,
    ClientSessionId, ControlGeneration, DomainError, WorkItemId, WorkScopeId, WorkScopeKind,
};
use seyal_agent_protocol::{
    decode_command, encode_result, AggregateRef, Command, CommandError, CommandResult, Frame,
    FrameKind, SnapshotView, ABSOLUTE_MAX_FRAME_SIZE, MAX_EVENT_WINDOW, REPLAY_EVENT_OVERHEAD,
    REPLAY_RESULT_OVERHEAD,
};
use seyal_agent_store::{AgentStore, AggregateId, AggregateSequence, StoreError, OUTPUT_REF_LEN};

use crate::{
    AuthorizationRepository, ClientScope, DurablePrincipal, HostObservation, HostObservationKind,
    ObservationAuthority, ObserveError, PrincipalKind, PrincipalStatus, RunLiveness,
    SessionExecutionHost,
};

use recovery::restore_identities;
use wire::{
    fit_replay_events, liveness_code, map_auth, map_domain, observation_payload, to_aggregate,
    ReplayPage,
};

const EVENT_OBSERVATION: u16 = 2;

#[derive(Clone, Debug)]
pub struct IntegrationConfig {
    pub store_path: PathBuf,
}

pub struct IntegrationService {
    instance_id: seyal_agent_protocol::BackendInstanceId,
    /// First-party CLI principal (create/observe/control). Hello evidence empty or `cli`.
    owner_principal_id: ClientPrincipalId,
    /// Distinct observe-only principal. Hello evidence `observer`.
    observer_principal_id: ClientPrincipalId,
    auth: AuthorizationRepository,
    authority: ObservationAuthority,
    store: AgentStore,
    host: Option<Box<dyn SessionExecutionHost>>,
    /// SPEC-027 §6 `AdapterWorkDir` parent: backend-owned, derived from the
    /// daemon's own private runtime directory (never `$HOME`, never a
    /// client-supplied path). Per-adapter subdirectories are created lazily
    /// at dispatch.
    adapter_work_root: PathBuf,
    /// Runs with a live host handle still worth draining (SPEC-027 §9.1/§9.4):
    /// `start_agent_run` only observes once synchronously; a real host's
    /// background reader keeps buffering after that call returns, so a run
    /// stays mapped here until a later drain sees terminal exit evidence.
    /// Removed on `KnownTerminated`/`UnknownAfterCrash` — nothing further to
    /// usefully drain from the host for either outcome.
    active_hosted_runs: std::collections::HashMap<AgentRunId, crate::HostHandle>,
    #[cfg(test)]
    last_resolved_launch: Option<seyal_agent_core::LaunchDescriptor>,
    /// Test-only: after mint/Prepared→Dispatching, bump the adapter catalog
    /// so the frozen generation is unreachable before `host.start` (SPEC-027
    /// fixture 11 / AC11). Production always leaves this false.
    #[cfg(test)]
    invalidate_frozen_manifest_after_mint: bool,
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
        let store = AgentStore::open(&config.store_path).map_err(|_| ServiceError::Failed)?;
        let mut auth = AuthorizationRepository::default();
        let (owner_principal_id, observer_principal_id) =
            load_or_seed_principals(&store, &mut auth)?;
        // SPEC-027 §7 step 5 / D3: `adapter.execute` is durable (same trusted
        // admin-tool write path as the catalog itself, §5.2) but enforced
        // in-memory; re-apply every durable grant on each open. A grant for
        // a non-first-party principal (e.g. the observer) was never written
        // by `grant_adapter_execute`'s own first-party gating, but even if
        // present it is inert here — `AuthorizationRepository::
        // grant_adapter_execute` still refuses it.
        // SPEC-027 §5.2: same posture for durable `admin.adapters`.
        for principal_id in [owner_principal_id, observer_principal_id] {
            if store.has_admin_adapters(principal_id).unwrap_or(false) {
                let _ = auth.grant_admin_adapters(principal_id);
            }
            let Ok(grants) = store.adapter_execute_grants(principal_id) else {
                continue;
            };
            for adapter_id in grants {
                let _ = auth.grant_adapter_execute(principal_id, adapter_id);
            }
        }
        let mut authority = ObservationAuthority::new(seyal_agent_core::AgentDomain::new());
        restore_identities(&store, &mut authority, &mut auth)?;
        let adapter_work_root = config
            .store_path
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("adapter-work");
        Ok(Self {
            instance_id,
            owner_principal_id,
            observer_principal_id,
            auth,
            authority,
            store,
            host: None,
            adapter_work_root,
            active_hosted_runs: std::collections::HashMap::new(),
            #[cfg(test)]
            last_resolved_launch: None,
            #[cfg(test)]
            invalidate_frozen_manifest_after_mint: false,
        })
    }

    pub fn install_execution_host(&mut self, host: Box<dyn SessionExecutionHost>) {
        self.host = Some(host);
    }

    /// Test-only: next `StartAgentRun` removes the frozen generation after
    /// mint so fixture 11 can prove post-Prepared fail-closed.
    #[cfg(test)]
    pub fn invalidate_frozen_manifest_after_mint_for_tests(&mut self) {
        self.invalidate_frozen_manifest_after_mint = true;
    }

    /// Trusted first-party bind of a WorkScope root (SPEC-027 §6). Not a
    /// client command and not authorization. Canonicalizes to an existing
    /// directory before the store write.
    pub fn bind_work_scope_root(
        &self,
        work_scope_id: WorkScopeId,
        path: &Path,
    ) -> Result<(), ServiceError> {
        let canonical = path.canonicalize().map_err(|_| ServiceError::Failed)?;
        if !canonical.is_dir() {
            return Err(ServiceError::Failed);
        }
        let text = canonical.to_str().ok_or(ServiceError::Failed)?;
        self.store
            .bind_work_scope_root(work_scope_id, text)
            .map_err(|_| ServiceError::Failed)
    }

    /// Grant durable `admin.adapters` to a first-party principal (SPEC-027
    /// §5.2). Persist then apply in-memory. Not a client Command.
    pub fn grant_admin_adapters(
        &mut self,
        principal: seyal_agent_core::ClientPrincipalId,
    ) -> Result<(), ServiceError> {
        self.auth
            .grant_admin_adapters(principal)
            .map_err(|_| ServiceError::Failed)?;
        self.store
            .grant_admin_adapters(principal)
            .map_err(|_| ServiceError::Failed)
    }

    /// First-party catalog install/update requiring `admin.adapters`
    /// (SPEC-027 §5.2 / D2). No client Command.
    pub fn install_or_update_adapter(
        &mut self,
        principal: seyal_agent_core::ClientPrincipalId,
        adapter_id: seyal_agent_core::AdapterId,
        execution_host_kind: u8,
        enabled: bool,
        launch: &seyal_agent_store::LaunchDescriptorTemplate,
    ) -> Result<u64, ServiceError> {
        self.auth
            .authorize_admin_adapters(principal)
            .map_err(|_| ServiceError::Failed)?;
        self.store
            .install_or_update_adapter(adapter_id, execution_host_kind, enabled, launch)
            .map_err(|_| ServiceError::Failed)
    }

    /// First-party set-enabled requiring `admin.adapters` (SPEC-027 §5.2).
    pub fn set_adapter_enabled(
        &mut self,
        principal: seyal_agent_core::ClientPrincipalId,
        adapter_id: seyal_agent_core::AdapterId,
        enabled: bool,
    ) -> Result<(), ServiceError> {
        self.auth
            .authorize_admin_adapters(principal)
            .map_err(|_| ServiceError::Failed)?;
        self.store
            .set_adapter_enabled(adapter_id, enabled)
            .map_err(|_| ServiceError::Failed)
    }

    /// HelloAck advertisement inputs (SPEC-027 §8.2): the composed host kind
    /// (never authorization) and the durable catalog generation. `Fake`
    /// (qualification/fixture-host builds only, never production) advertises
    /// as `StandaloneProcess` since it simulates that host's observable
    /// contract; a hostless service advertises `None`.
    pub(crate) fn server_capabilities(
        &self,
    ) -> (seyal_agent_protocol::ExecutionHostKind, Option<u64>) {
        let kind = match self.host.as_ref().map(|host| host.kind()) {
            Some(seyal_agent_core::ExecutionHostKind::StandaloneProcess)
            | Some(seyal_agent_core::ExecutionHostKind::Fake) => {
                seyal_agent_protocol::ExecutionHostKind::StandaloneProcess
            }
            None => seyal_agent_protocol::ExecutionHostKind::None,
        };
        (kind, self.store.adapter_catalog_generation().ok())
    }

    /// Install one enabled, non-TTY adapter + RouteOffering so SPEC-027
    /// §4.3 unpinned resolution has exactly one eligible target
    /// (`Singleton`), and grant `adapter.execute` to the owner principal so
    /// §7 step 5 passes. Qualification/test composition only — SPEC-027
    /// §5.2 install/enable is a first-party `admin.adapters` action, never a
    /// client-reachable command, and this helper models that trusted path.
    #[cfg(feature = "fixture-host")]
    pub fn install_default_adapter_catalog_for_tests(&mut self) -> seyal_agent_core::AdapterId {
        let adapter_id = seyal_agent_core::AdapterId::new();
        let launch = seyal_agent_store::LaunchDescriptorTemplate::new(
            "/bin/echo",
            seyal_agent_store::CwdPolicy::AdapterWorkDir,
        )
        .with_argv(["fixture-host-default"]);
        let owner = self.owner_principal_id;
        self.grant_admin_adapters(owner)
            .expect("grant admin.adapters");
        self.install_or_update_adapter(owner, adapter_id, 0, true, &launch)
            .expect("install adapter");
        self.store
            .add_route_offering(seyal_agent_core::RouteOfferingId::new(), adapter_id, false)
            .expect("add offering");
        self.auth
            .grant_adapter_execute(owner, adapter_id)
            .expect("grant adapter.execute");
        adapter_id
    }

    pub fn set_principal_status(
        &mut self,
        id: ClientPrincipalId,
        status: PrincipalStatus,
    ) -> Result<(), ServiceError> {
        self.auth
            .set_principal_status(id, status)
            .map_err(|_| ServiceError::Failed)?;
        self.store
            .set_principal_status(id, status.code())
            .map_err(|_| ServiceError::Failed)?;
        Ok(())
    }

    pub fn owner_principal_id(&self) -> ClientPrincipalId {
        self.owner_principal_id
    }

    /// Resolve Hello evidence to a principal for this connection.
    /// Socket UID admission alone never selects a privileged principal.
    /// The returned id is connection-scoped and must be passed to every `handle`.
    pub fn resolve_connection_principal(
        &self,
        evidence: &[u8],
    ) -> Result<ClientPrincipalId, ServiceError> {
        self.auth
            .principal_for_evidence(
                evidence,
                self.owner_principal_id,
                self.observer_principal_id,
            )
            .map_err(|_| ServiceError::Failed)
    }

    #[cfg(test)]
    pub fn begin_connection(&mut self, evidence: &[u8]) -> Result<ClientPrincipalId, ServiceError> {
        self.resolve_connection_principal(evidence)
    }

    #[cfg(feature = "test-fault-injection")]
    pub(crate) fn fail_after_writes(&self, allowed: u64) {
        self.store.fail_after_writes(allowed);
    }

    pub fn handle(
        &mut self,
        principal_id: ClientPrincipalId,
        frame: Frame,
        max_frame_size: u32,
        event_window: u32,
    ) -> Result<Vec<u8>, ServiceError> {
        let result = if frame.kind != FrameKind::Command {
            CommandResult::Error(CommandError::Malformed)
        } else {
            match decode_command(&frame.body) {
                Ok(command) => self.dispatch(principal_id, command, event_window, max_frame_size),
                Err(_) => CommandResult::Error(CommandError::Malformed),
            }
        };
        encode_result(&result, max_frame_size).map_err(|_| ServiceError::Failed)
    }

    fn dispatch(
        &mut self,
        principal_id: ClientPrincipalId,
        command: Command,
        event_window: u32,
        max_frame_size: u32,
    ) -> CommandResult {
        match command {
            Command::OpenSession { scopes } => self.open_session(principal_id, scopes),
            Command::ResumeSession { session_id } => self.resume_session(principal_id, session_id),
            Command::CreateWorkScope { session_id, kind } => {
                self.create_work_scope(principal_id, session_id, kind)
            }
            Command::CreateWorkItem {
                session_id,
                work_scope_id,
            } => self.create_work_item(principal_id, session_id, work_scope_id),
            Command::CreateAttempt {
                session_id,
                work_item_id,
            } => self.create_attempt(principal_id, session_id, work_item_id),
            Command::StartAgentRun {
                session_id,
                attempt_id,
                route_offering_id,
            } => self.start_agent_run(principal_id, session_id, attempt_id, route_offering_id),
            Command::GetSnapshot {
                session_id,
                aggregate,
            } => self.snapshot(principal_id, session_id, aggregate),
            Command::Subscribe {
                session_id,
                aggregate,
                after,
            } => self.subscribe(
                principal_id,
                session_id,
                aggregate,
                after,
                event_window,
                max_frame_size,
            ),
            Command::CheckGeneration {
                session_id,
                run_id,
                binding_generation,
                control_generation,
            } => self.check_generation(
                principal_id,
                session_id,
                run_id,
                binding_generation,
                control_generation,
            ),
            Command::ReadRun { session_id, run_id } => {
                self.read_run(principal_id, session_id, run_id)
            }
            Command::CancelRun {
                session_id,
                run_id,
                control_generation,
            } => {
                self.cancel_agent_run_command(principal_id, session_id, run_id, control_generation)
            }
        }
    }

    fn open_session(&mut self, principal_id: ClientPrincipalId, scopes: Vec<u8>) -> CommandResult {
        let mut decoded = Vec::with_capacity(scopes.len());
        for scope in scopes {
            match ClientScope::decode(scope) {
                Ok(scope) => decoded.push(scope),
                Err(_) => return CommandResult::Error(CommandError::Malformed),
            }
        }
        match self
            .auth
            .open_session(principal_id, self.instance_id, decoded)
        {
            Ok(session_id) => CommandResult::Opened { session_id },
            Err(error) => CommandResult::Error(map_auth(error)),
        }
    }

    fn resume_session(
        &mut self,
        principal_id: ClientPrincipalId,
        session_id: ClientSessionId,
    ) -> CommandResult {
        match self
            .auth
            .resume_session(session_id, self.instance_id, principal_id)
        {
            Ok(()) => CommandResult::Resumed,
            Err(error) => CommandResult::Error(map_auth(error)),
        }
    }

    fn create_work_scope(
        &mut self,
        principal_id: ClientPrincipalId,
        session_id: ClientSessionId,
        kind: WorkScopeKind,
    ) -> CommandResult {
        if let Err(error) =
            self.authorized_session(principal_id, session_id, ClientScope::RunsCreate)
        {
            return CommandResult::Error(error);
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
        principal_id: ClientPrincipalId,
        session_id: ClientSessionId,
        work_scope_id: WorkScopeId,
    ) -> CommandResult {
        if let Err(error) =
            self.authorized_session(principal_id, session_id, ClientScope::RunsCreate)
        {
            return CommandResult::Error(error);
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
        principal_id: ClientPrincipalId,
        session_id: ClientSessionId,
        work_item_id: WorkItemId,
    ) -> CommandResult {
        if let Err(error) =
            self.authorized_session(principal_id, session_id, ClientScope::RunsCreate)
        {
            return CommandResult::Error(error);
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
        principal_id: ClientPrincipalId,
        session_id: ClientSessionId,
        aggregate: AggregateRef,
    ) -> CommandResult {
        if let Err(error) = self.authorize_observe(principal_id, session_id, aggregate) {
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
        principal_id: ClientPrincipalId,
        session_id: ClientSessionId,
        aggregate: AggregateRef,
        after: Option<u64>,
        event_window: u32,
        max_frame_size: u32,
    ) -> CommandResult {
        if let Err(error) = self.authorize_observe(principal_id, session_id, aggregate) {
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
        principal_id: ClientPrincipalId,
        session_id: ClientSessionId,
        run_id: AgentRunId,
        binding_generation: u64,
        control_generation: u64,
    ) -> CommandResult {
        if let Err(error) =
            self.authorized_run(principal_id, session_id, ClientScope::RunsControl, run_id)
        {
            return CommandResult::Error(error);
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
        principal_id: ClientPrincipalId,
        session_id: ClientSessionId,
        run_id: AgentRunId,
    ) -> CommandResult {
        if let Err(error) =
            self.authorized_run(principal_id, session_id, ClientScope::RunsObserve, run_id)
        {
            return CommandResult::Error(error);
        }
        // §9.1/§9.4: a real host's background reader keeps buffering after
        // `start_agent_run`'s single synchronous `observe` returns; a client
        // reading run status is a natural, non-blocking point to drain
        // whatever has accumulated since (never blocks on child I/O itself —
        // `observe` only drains an already-filled queue).
        if let Err(error) = self.drain_host_observations(run_id) {
            return CommandResult::Error(error);
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

    /// Drains already-buffered host observations for `run_id` if a live host
    /// handle is still tracked for it (no-op otherwise: hostless runs,
    /// already-terminal runs, and runs with no host composed all return
    /// `Ok(())` immediately). See `active_hosted_runs` doc comment.
    fn drain_host_observations(&mut self, run_id: AgentRunId) -> Result<(), CommandError> {
        let Some(&handle) = self.active_hosted_runs.get(&run_id) else {
            return Ok(());
        };
        let Some(host) = self.host.as_mut() else {
            return Ok(());
        };
        let observations = match host.observe(handle) {
            Ok(observations) => observations,
            Err(()) => return Err(CommandError::Failed),
        };
        self.commit_observation_batch(observations)?;
        if matches!(
            self.authority.liveness(run_id),
            RunLiveness::KnownTerminated | RunLiveness::UnknownAfterCrash
        ) {
            // SPEC-026 §9.2: terminal host evidence completes a cancel that
            // was waiting in Terminating.
            if self
                .authority
                .domain()
                .agent_run(run_id)
                .is_some_and(|run| run.lifecycle() == AgentRunLifecycle::Terminating)
            {
                let _ = self
                    .authority
                    .domain_mut()
                    .confirm_cancel_termination(run_id);
            }
            if let Some(handle) = self.active_hosted_runs.remove(&run_id)
                && let Some(host) = self.host.as_mut()
            {
                let _ = host.reap(handle);
            }
        }
        Ok(())
    }

    /// Run snapshots and replays use the same target gate as `ReadRun`.
    fn authorize_observe(
        &self,
        principal_id: ClientPrincipalId,
        session_id: ClientSessionId,
        aggregate: AggregateRef,
    ) -> Result<(), CommandError> {
        if let AggregateRef::AgentRun(run_id) = aggregate {
            self.authorized_run(principal_id, session_id, ClientScope::RunsObserve, run_id)
        } else {
            self.authorized_session(principal_id, session_id, ClientScope::RunsObserve)
                .map(|_| ())
        }
    }

    fn authorized_session(
        &self,
        caller: ClientPrincipalId,
        session_id: ClientSessionId,
        scope: ClientScope,
    ) -> Result<ClientPrincipalId, CommandError> {
        self.auth
            .authorize_session(session_id, self.instance_id, scope, caller)
            .map_err(map_auth)
    }

    fn authorized_run(
        &self,
        caller: ClientPrincipalId,
        session_id: ClientSessionId,
        scope: ClientScope,
        run_id: AgentRunId,
    ) -> Result<(), CommandError> {
        self.auth
            .authorize_run(session_id, self.instance_id, scope, run_id, caller)
            .map_err(map_auth)
    }
}

fn load_or_seed_principals(
    store: &AgentStore,
    auth: &mut AuthorizationRepository,
) -> Result<(ClientPrincipalId, ClientPrincipalId), ServiceError> {
    let rows = store
        .client_principals()
        .map_err(|_| ServiceError::Failed)?;
    if rows.is_empty() {
        let owner = auth.register_principal_with_evidence(
            PrincipalKind::FirstPartyCli,
            [
                ClientScope::RunsCreate,
                ClientScope::RunsObserve,
                ClientScope::RunsControl,
            ],
            b"cli".to_vec(),
        );
        let observer = auth.register_principal_with_evidence(
            PrincipalKind::ManagedClient,
            [ClientScope::RunsObserve],
            b"observer".to_vec(),
        );
        persist_principal(store, auth, owner)?;
        persist_principal(store, auth, observer)?;
        return Ok((owner, observer));
    }
    for row in rows {
        let kind = PrincipalKind::from_code(row.kind).ok_or(ServiceError::Failed)?;
        let status = PrincipalStatus::from_code(row.status).ok_or(ServiceError::Failed)?;
        let mut scopes = std::collections::BTreeSet::new();
        for byte in row.scopes {
            scopes.insert(ClientScope::decode(byte).map_err(|_| ServiceError::Failed)?);
        }
        auth.load_durable_principal(DurablePrincipal {
            id: row.id,
            kind,
            status,
            scopes,
            evidence_key: row.evidence_key,
        })
        .map_err(|_| ServiceError::Failed)?;
    }
    let owner = auth
        .principal_by_evidence_key(b"cli")
        .ok_or(ServiceError::Failed)?;
    let observer = auth
        .principal_by_evidence_key(b"observer")
        .ok_or(ServiceError::Failed)?;
    Ok((owner, observer))
}

fn persist_principal(
    store: &AgentStore,
    auth: &AuthorizationRepository,
    id: ClientPrincipalId,
) -> Result<(), ServiceError> {
    let durable = auth
        .durable_principal(id)
        .map_err(|_| ServiceError::Failed)?;
    let mut scopes: Vec<u8> = durable.scopes.iter().map(|scope| scope.code()).collect();
    scopes.sort_unstable();
    scopes.dedup();
    store
        .upsert_principal(&seyal_agent_store::PersistedPrincipal {
            id: durable.id,
            kind: durable.kind.code(),
            status: durable.status.code(),
            scopes,
            evidence_key: durable.evidence_key,
        })
        .map_err(|_| ServiceError::Failed)?;
    Ok(())
}

fn map_observe(error: ObserveError) -> CommandError {
    match error {
        ObserveError::StaleGeneration => CommandError::StaleBinding,
        ObserveError::Domain(DomainError::StaleControlEpoch { .. }) => CommandError::StaleControl,
        ObserveError::Domain(DomainError::UnknownAgentRun(_)) => CommandError::NotFound,
        _ => CommandError::Failed,
    }
}
