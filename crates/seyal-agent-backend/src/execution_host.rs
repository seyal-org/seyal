#[cfg(feature = "fixture-host")]
use seyal_agent_core::ExecutionHost;
use seyal_agent_core::{AgentRunId, BindingGeneration, ExecutionHostKind, LaunchDescriptor};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostObservationKind {
    Started,
    Progress { step: u64 },
    Result(Vec<u8>),
    KnownFailure,
    KnownSuccess,
    ObservationDisconnected,
    ObservationReconnected,
    HarnessCrashed,
    UnknownLiveness,
    EffectUnknown,
    Output(Vec<u8>),
    Delayed { ticks: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostObservation {
    pub run_id: AgentRunId,
    pub binding_generation: BindingGeneration,
    pub ordinal: u64,
    pub kind: HostObservationKind,
}

/// Opaque, per-host-instance identifier for one supervised child (SPEC-027
/// §9.1). Never a recycled OS pid; implementors bind it to spawn-time
/// identity internally (§9.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HostHandle(u64);

impl HostHandle {
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Why `start` produced no spawn evidence (SPEC-027 §9.3 typed not-started).
/// Distinct from a successful `start` that later crashes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostNotStartedReason {
    TtyRequired,
    SpawnFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostStartOutcome {
    Started(HostHandle),
    NotStarted(HostNotStartedReason),
}

/// Typed child-exit evidence (SPEC-027 §9.3). Never fabricated from I/O loss;
/// a closed observation pipe with a live child is `ObservationDisconnected`,
/// not an exit evidence variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostExitKind {
    Completed,
    Failed,
    Crashed,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostExitEvidence {
    pub kind: HostExitKind,
}

/// Object-safe host seam for [`crate::IntegrationService`] (AB-1.9, SPEC-027
/// §9.1).
///
/// Distinct from the associated-type [`ExecutionHost`] in `seyal-agent-core`
/// so the session path can hold `Option<Box<dyn SessionExecutionHost>>`.
///
/// Replaces the AB-1.9 blocking `collect_observations`-until-exit contract:
/// `start` MUST return as soon as the child is spawned/admitted (never
/// waiting for exit), `observe` MUST NOT block on child I/O (it drains
/// whatever a background reader has already buffered), and `reap` is a
/// bounded wait honoring the AGENTS.md termination invariant. None of these
/// methods may be called while holding the `IntegrationService` service
/// mutex across process I/O (SPEC-027 §9.2) — callers are responsible for
/// releasing that lock before invoking a host method that can touch the
/// child.
pub trait SessionExecutionHost: Send {
    /// HelloAck advertisement (SPEC-027 §8.2); not authorization.
    fn kind(&self) -> ExecutionHostKind;

    fn start(
        &mut self,
        run_id: AgentRunId,
        binding_generation: BindingGeneration,
        descriptor: LaunchDescriptor,
    ) -> HostStartOutcome;

    // Unit error keeps the object-safe seam free of host-specific error types;
    // IntegrationService maps `Err(())` to CommandError::Failed (#1196).
    #[allow(clippy::result_unit_err)]
    fn observe(&mut self, handle: HostHandle) -> Result<Vec<HostObservation>, ()>;

    /// Best-effort; not proof of termination (SPEC-027 §9.4).
    #[allow(clippy::result_unit_err)]
    fn signal_cancel(&mut self, handle: HostHandle) -> Result<(), ()>;

    /// Bounded wait for exit evidence (AGENTS.md termination invariant).
    #[allow(clippy::result_unit_err)]
    fn reap(&mut self, handle: HostHandle) -> Result<HostExitEvidence, ()>;

    /// Best-effort signal-and-reap of every live handle (daemon shutdown /
    /// `Drop`). Default is a no-op for hosts that do not own OS children.
    fn shutdown_all(&mut self) {}
}

#[cfg(feature = "fixture-host")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptStep {
    Emit(HostObservationKind),
    EmitExact {
        binding_generation: BindingGeneration,
        ordinal: u64,
        kind: HostObservationKind,
    },
    DuplicateLast,
    DelayTicks(u64),
}

#[cfg(feature = "fixture-host")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptError {
    ZeroOutputChunk,
    DuplicateWithoutObservation,
    OrdinalExhausted,
    MalformedScript,
    EmptyScript,
    ScriptTooLarge,
}

/// Deterministic provider-free ExecutionHost fixture (AB-0 / AB-1).
///
/// Available only behind `fixture-host`. Production composition never installs
/// this host; qualification and in-process tests inject it through
/// [`SessionExecutionHost`].
#[cfg(feature = "fixture-host")]
pub struct FakeExecutionHost {
    max_output_chunk: usize,
    script: Vec<ScriptStep>,
    next_handle: u64,
    /// Scripted outcome of `start`; defaults to immediate spawn evidence.
    start_outcome: Option<HostStartOutcome>,
    /// Observations already materialized for a live handle, drained in order
    /// by `observe` to model off-lock, non-blocking delivery.
    pending: std::collections::HashMap<u64, std::collections::VecDeque<HostObservation>>,
    /// Identity + next ordinal for each live handle after the initial observe
    /// drain (so `signal_cancel` can enqueue terminal evidence).
    live: std::collections::HashMap<u64, (AgentRunId, BindingGeneration, u64)>,
    /// Last descriptor passed to `start`, for tests asserting the backend
    /// resolved program/argv/env/cwd correctly before calling the host.
    last_descriptor: Option<LaunchDescriptor>,
}

#[cfg(feature = "fixture-host")]
impl FakeExecutionHost {
    pub fn new(max_output_chunk: usize) -> Result<Self, ScriptError> {
        if max_output_chunk == 0 {
            return Err(ScriptError::ZeroOutputChunk);
        }
        Ok(Self {
            max_output_chunk,
            script: Vec::new(),
            next_handle: 1,
            start_outcome: None,
            pending: std::collections::HashMap::new(),
            live: std::collections::HashMap::new(),
            last_descriptor: None,
        })
    }

    pub fn last_descriptor(&self) -> Option<&LaunchDescriptor> {
        self.last_descriptor.as_ref()
    }

    pub fn set_script(&mut self, script: Vec<ScriptStep>) {
        self.script = script;
    }

    pub fn script(&self) -> &[ScriptStep] {
        &self.script
    }

    /// Force the next `start` to report typed not-started instead of
    /// spawning (SPEC-027 §9.3 fixture support).
    pub fn force_not_started(&mut self, reason: HostNotStartedReason) {
        self.start_outcome = Some(HostStartOutcome::NotStarted(reason));
    }

    pub fn execute(
        &self,
        run_id: AgentRunId,
        binding_generation: BindingGeneration,
        script: &[ScriptStep],
    ) -> Result<Vec<HostObservation>, ScriptError> {
        let mut observations = Vec::new();
        let mut next_ordinal = 1_u64;

        for step in script {
            match step {
                ScriptStep::Emit(HostObservationKind::Output(bytes)) => {
                    for chunk in bytes.chunks(self.max_output_chunk) {
                        observations.push(HostObservation {
                            run_id,
                            binding_generation,
                            ordinal: next_ordinal,
                            kind: HostObservationKind::Output(chunk.to_vec()),
                        });
                        next_ordinal = next_ordinal
                            .checked_add(1)
                            .ok_or(ScriptError::OrdinalExhausted)?;
                    }
                }
                ScriptStep::Emit(kind) => {
                    observations.push(HostObservation {
                        run_id,
                        binding_generation,
                        ordinal: next_ordinal,
                        kind: kind.clone(),
                    });
                    next_ordinal = next_ordinal
                        .checked_add(1)
                        .ok_or(ScriptError::OrdinalExhausted)?;
                }
                ScriptStep::EmitExact {
                    binding_generation,
                    ordinal,
                    kind,
                } => observations.push(HostObservation {
                    run_id,
                    binding_generation: *binding_generation,
                    ordinal: *ordinal,
                    kind: kind.clone(),
                }),
                ScriptStep::DuplicateLast => {
                    let duplicate = observations
                        .last()
                        .cloned()
                        .ok_or(ScriptError::DuplicateWithoutObservation)?;
                    observations.push(duplicate);
                }
                ScriptStep::DelayTicks(ticks) => {
                    observations.push(HostObservation {
                        run_id,
                        binding_generation,
                        ordinal: next_ordinal,
                        kind: HostObservationKind::Delayed { ticks: *ticks },
                    });
                    next_ordinal = next_ordinal
                        .checked_add(1)
                        .ok_or(ScriptError::OrdinalExhausted)?;
                }
            }
        }

        Ok(observations)
    }

    /// Legacy synchronous accessor retained for direct-script tests that
    /// predate the start/observe split.
    pub fn collect_observations(
        &self,
        run_id: AgentRunId,
        binding_generation: BindingGeneration,
    ) -> Result<Vec<HostObservation>, ScriptError> {
        if self.script.is_empty() {
            return Err(ScriptError::EmptyScript);
        }
        self.execute(run_id, binding_generation, &self.script)
    }
}

#[cfg(feature = "fixture-host")]
impl ExecutionHost for FakeExecutionHost {
    type Observation = HostObservation;
    type Error = ScriptError;
    type Handle = HostHandle;

    fn kind(&self) -> ExecutionHostKind {
        ExecutionHostKind::Fake
    }

    fn start(
        &self,
        _run_id: AgentRunId,
        _binding_generation: BindingGeneration,
        _descriptor: seyal_agent_core::LaunchDescriptor,
    ) -> Result<Self::Handle, seyal_agent_core::HostStartFailure> {
        Ok(HostHandle::new(1))
    }

    fn observe(&self, _handle: &Self::Handle) -> Result<Vec<HostObservation>, ScriptError> {
        if self.script.is_empty() {
            return Err(ScriptError::EmptyScript);
        }
        self.execute(AgentRunId::new(), BindingGeneration::FIRST, &self.script)
    }

    fn signal_cancel(&self, _handle: &Self::Handle) -> Result<(), ScriptError> {
        Ok(())
    }

    fn reap(
        &self,
        _handle: &Self::Handle,
    ) -> Result<seyal_agent_core::HostExitEvidence, ScriptError> {
        Ok(seyal_agent_core::HostExitEvidence {
            kind: seyal_agent_core::HostExitKind::Completed,
        })
    }
}

#[cfg(feature = "fixture-host")]
impl SessionExecutionHost for FakeExecutionHost {
    fn kind(&self) -> ExecutionHostKind {
        ExecutionHostKind::Fake
    }

    fn start(
        &mut self,
        run_id: AgentRunId,
        binding_generation: BindingGeneration,
        descriptor: LaunchDescriptor,
    ) -> HostStartOutcome {
        self.last_descriptor = Some(descriptor);
        if let Some(outcome) = self.start_outcome.take() {
            return outcome;
        }
        let handle = self.next_handle;
        self.next_handle = self.next_handle.saturating_add(1);
        let observations = self
            .execute(run_id, binding_generation, &self.script)
            .unwrap_or_default();
        let next_ordinal = observations
            .last()
            .map(|observation| observation.ordinal.saturating_add(1))
            .unwrap_or(1);
        self.live
            .insert(handle, (run_id, binding_generation, next_ordinal));
        self.pending
            .insert(handle, observations.into_iter().collect());
        HostStartOutcome::Started(HostHandle::new(handle))
    }

    fn observe(&mut self, handle: HostHandle) -> Result<Vec<HostObservation>, ()> {
        let queue = self.pending.get_mut(&handle.get()).ok_or(())?;
        Ok(queue.drain(..).collect())
    }

    fn signal_cancel(&mut self, handle: HostHandle) -> Result<(), ()> {
        // Fixture model of cancel evidence: enqueue terminal failure so the
        // session cancel path can observe KnownTerminated after Terminating.
        let (run_id, binding, ordinal) = *self.live.get(&handle.get()).ok_or(())?;
        let queue = self.pending.entry(handle.get()).or_default();
        queue.push_back(HostObservation {
            run_id,
            binding_generation: binding,
            ordinal,
            kind: HostObservationKind::KnownFailure,
        });
        self.live
            .insert(handle.get(), (run_id, binding, ordinal.saturating_add(1)));
        Ok(())
    }

    fn reap(&mut self, handle: HostHandle) -> Result<HostExitEvidence, ()> {
        self.live.remove(&handle.get());
        self.pending.remove(&handle.get());
        Ok(HostExitEvidence {
            kind: HostExitKind::Failed,
        })
    }
}

#[cfg(all(test, feature = "fixture-host"))]
mod tests {
    use super::*;
    use seyal_agent_core::{AgentDomain, WorkScopeKind};

    fn run_with_generation() -> (AgentRunId, BindingGeneration) {
        let mut domain = AgentDomain::new();
        let scope = domain.create_work_scope(WorkScopeKind::Repository);
        let item = domain.create_work_item(scope).unwrap();
        let attempt = domain.create_attempt(item).unwrap();
        let run = domain.create_agent_run(attempt).unwrap();
        (run, domain.agent_run(run).unwrap().binding_generation())
    }

    fn sample_descriptor() -> LaunchDescriptor {
        LaunchDescriptor {
            program: "/bin/echo".to_string(),
            argv: vec!["test".to_string()],
            env: Vec::new(),
            cwd: std::env::temp_dir(),
        }
    }

    #[test]
    fn script_is_deterministic_and_duplicate_is_exact() {
        let (run, generation) = run_with_generation();
        let host = FakeExecutionHost::new(1024).unwrap();
        let script = [
            ScriptStep::Emit(HostObservationKind::Started),
            ScriptStep::Emit(HostObservationKind::Progress { step: 1 }),
            ScriptStep::DuplicateLast,
            ScriptStep::Emit(HostObservationKind::KnownSuccess),
        ];

        let first = host.execute(run, generation, &script).unwrap();
        let second = host.execute(run, generation, &script).unwrap();

        assert_eq!(first, second);
        assert_eq!(first[1], first[2]);
    }

    #[test]
    fn exact_emission_can_model_stale_generation_and_out_of_order_input() {
        let (run, first_generation) = run_with_generation();
        let mut domain = AgentDomain::new();
        let scope = domain.create_work_scope(WorkScopeKind::Repository);
        let item = domain.create_work_item(scope).unwrap();
        let attempt = domain.create_attempt(item).unwrap();
        let fenced_run = domain.create_agent_run(attempt).unwrap();
        let current = domain
            .advance_binding_generation(fenced_run, BindingGeneration::FIRST)
            .unwrap();

        let host = FakeExecutionHost::new(1024).unwrap();
        let observations = host
            .execute(
                run,
                current,
                &[
                    ScriptStep::Emit(HostObservationKind::Started),
                    ScriptStep::EmitExact {
                        binding_generation: first_generation,
                        ordinal: 1,
                        kind: HostObservationKind::Progress { step: 99 },
                    },
                ],
            )
            .unwrap();

        assert_ne!(
            observations[0].binding_generation,
            observations[1].binding_generation
        );
        assert_eq!(observations[1].ordinal, 1);
    }

    #[test]
    fn high_volume_output_is_chunked_to_the_configured_bound() {
        let (run, generation) = run_with_generation();
        let host = FakeExecutionHost::new(4).unwrap();
        let observations = host
            .execute(
                run,
                generation,
                &[ScriptStep::Emit(HostObservationKind::Output(
                    b"abcdefghij".to_vec(),
                ))],
            )
            .unwrap();

        assert_eq!(observations.len(), 3);
        assert!(observations
            .iter()
            .all(|observation| match &observation.kind {
                HostObservationKind::Output(bytes) => bytes.len() <= 4,
                _ => false,
            }));
    }

    #[test]
    fn fake_implements_execution_host_trait_deterministically() {
        let (run, generation) = run_with_generation();
        let host = FakeExecutionHost::new(1024).unwrap();
        assert_eq!(ExecutionHost::kind(&host), ExecutionHostKind::Fake);
        let mut host = host;
        host.set_script(vec![
            ScriptStep::Emit(HostObservationKind::Started),
            ScriptStep::Emit(HostObservationKind::KnownSuccess),
        ]);

        let first = host.collect_observations(run, generation).unwrap();
        let second = host.collect_observations(run, generation).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.len(), 2);
    }

    #[test]
    fn trait_dispatch_smoke_covers_fake_kind() {
        let host = FakeExecutionHost::new(8).unwrap();
        assert_eq!(
            <FakeExecutionHost as ExecutionHost>::kind(&host),
            ExecutionHostKind::Fake
        );
    }

    #[test]
    fn session_execution_host_start_observe_is_off_lock_shaped() {
        let (run, generation) = run_with_generation();
        let mut host = FakeExecutionHost::new(1024).unwrap();
        host.set_script(vec![
            ScriptStep::Emit(HostObservationKind::Started),
            ScriptStep::Emit(HostObservationKind::KnownSuccess),
        ]);

        let descriptor = sample_descriptor();
        let HostStartOutcome::Started(handle) =
            SessionExecutionHost::start(&mut host, run, generation, descriptor.clone())
        else {
            panic!("expected Started");
        };
        assert_eq!(host.last_descriptor(), Some(&descriptor));
        let observations = SessionExecutionHost::observe(&mut host, handle).unwrap();
        assert_eq!(observations.len(), 2);
        // A second observe drains nothing new (already delivered).
        assert_eq!(
            SessionExecutionHost::observe(&mut host, handle).unwrap(),
            Vec::new()
        );
        let evidence = SessionExecutionHost::reap(&mut host, handle).unwrap();
        // Fixture SessionExecutionHost::reap returns Failed (known-terminated
        // path shared with cancel/AC13). Success is observed via KnownSuccess
        // in the drained script, not via a Completed exit kind.
        assert_eq!(evidence.kind, HostExitKind::Failed);
    }

    #[test]
    fn session_execution_host_force_not_started_reports_typed_reason() {
        let (run, generation) = run_with_generation();
        let mut host = FakeExecutionHost::new(8).unwrap();
        host.force_not_started(HostNotStartedReason::TtyRequired);
        assert_eq!(
            SessionExecutionHost::start(&mut host, run, generation, sample_descriptor()),
            HostStartOutcome::NotStarted(HostNotStartedReason::TtyRequired)
        );
    }
}
