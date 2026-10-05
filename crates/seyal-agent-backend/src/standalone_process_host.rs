//! First real ExecutionHost: supervised standalone process (SPEC-018 §2,
//! SPEC-027 §9.5).
//!
//! Owns no PTY master or TerminalState. `start` spawns the child and a
//! background reader thread, then returns immediately; `observe` only
//! drains what that thread has already buffered (never blocks on child
//! I/O); `signal_cancel` is a best-effort kill; `reap` is a bounded wait for
//! exit evidence. Composed into the production binary by #1224 (the "#679
//! child" SPEC-027 §12 names); program/argv/env/cwd come from the
//! per-dispatch `LaunchDescriptor` the Agent Backend resolves (§5/§6),
//! never from host construction-time config.

use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use seyal_agent_core::{AgentRunId, BindingGeneration, LaunchDescriptor};

use seyal_agent_core::ExecutionHostKind;

use crate::execution_host::{
    HostExitEvidence, HostExitKind, HostHandle, HostNotStartedReason, HostObservation,
    HostObservationKind, HostStartOutcome, SessionExecutionHost,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostError {
    ZeroOutputChunk,
    SpawnFailed,
    Io,
    OrdinalExhausted,
    UnknownHandle,
}

/// Host-level supervision tuning, shared across every child this host
/// spawns. Per-run spawn input (program/argv/env/cwd) is never part of this
/// — it arrives fresh with each `start` call as a [`LaunchDescriptor`].
#[derive(Clone, Debug)]
pub struct StandaloneProcessConfig {
    max_output_chunk: usize,
    /// Bound how long we wait for exit after stdout EOF while the child is live.
    disconnect_grace: Duration,
    /// SPEC-027 §9.5: TTY-required offerings stay `ExecutionTargetUnavailable`
    /// on this host until a shared PTY primitive exists. Fixed per host
    /// instance since this is a pipe-safe-only host, not per-offering.
    requires_tty: bool,
}

impl StandaloneProcessConfig {
    pub fn new(max_output_chunk: usize) -> Result<Self, HostError> {
        if max_output_chunk == 0 {
            return Err(HostError::ZeroOutputChunk);
        }
        Ok(Self {
            max_output_chunk,
            disconnect_grace: Duration::from_millis(50),
            requires_tty: false,
        })
    }

    pub fn with_disconnect_grace(mut self, grace: Duration) -> Self {
        self.disconnect_grace = grace;
        self
    }

    pub fn with_tty_required(mut self, requires_tty: bool) -> Self {
        self.requires_tty = requires_tty;
        self
    }

    pub fn requires_tty(&self) -> bool {
        self.requires_tty
    }
}

struct SupervisedChild {
    child: Arc<Mutex<Child>>,
    observations: Arc<Mutex<VecDeque<HostObservation>>>,
    exit_evidence: Arc<Mutex<Option<HostExitEvidence>>>,
}

/// Supervises standalone OS processes and emits HostObservations.
///
/// One instance can supervise multiple concurrently-live children, each
/// addressed by its own [`HostHandle`]; `start`/`observe`/`signal_cancel`/
/// `reap` never block on another handle's child I/O.
pub struct StandaloneProcessHost {
    config: StandaloneProcessConfig,
    children: HashMap<u64, SupervisedChild>,
    next_handle: AtomicU64,
}

impl StandaloneProcessHost {
    pub fn new(config: StandaloneProcessConfig) -> Self {
        Self {
            config,
            children: HashMap::new(),
            next_handle: AtomicU64::new(1),
        }
    }

    pub fn config(&self) -> &StandaloneProcessConfig {
        &self.config
    }

    fn classify_exit(status: ExitStatus) -> HostExitKind {
        if status.success() {
            HostExitKind::Completed
        } else if status.code().is_some() {
            HostExitKind::Failed
        } else {
            // Signal death (e.g. SIGKILL) is crash evidence, not a clean terminate.
            HostExitKind::Crashed
        }
    }

    /// Spawn the child and a background reader thread that pushes
    /// observations into `observations` and the final exit evidence into
    /// `exit_evidence` once the pipe closes and the child has been waited
    /// on. Returns immediately after spawn (SPEC-027 §9.1 `start` contract);
    /// all blocking I/O happens on the background thread, never on the
    /// caller's thread.
    fn spawn(
        &self,
        run_id: AgentRunId,
        binding_generation: BindingGeneration,
        descriptor: &LaunchDescriptor,
    ) -> Result<SupervisedChild, HostError> {
        let mut command = Command::new(&descriptor.program);
        command
            .args(&descriptor.argv)
            .current_dir(&descriptor.cwd)
            .env_clear()
            .envs(descriptor.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn().map_err(|_| HostError::SpawnFailed)?;
        let stdout = child.stdout.take().ok_or(HostError::SpawnFailed)?;

        let observations = Arc::new(Mutex::new(VecDeque::new()));
        observations.lock().unwrap().push_back(HostObservation {
            run_id,
            binding_generation,
            ordinal: 1,
            kind: HostObservationKind::Started,
        });

        let child = Arc::new(Mutex::new(child));
        let exit_evidence: Arc<Mutex<Option<HostExitEvidence>>> = Arc::new(Mutex::new(None));

        let thread_child = Arc::clone(&child);
        let thread_observations = Arc::clone(&observations);
        let thread_exit_evidence = Arc::clone(&exit_evidence);
        let max_output_chunk = self.config.max_output_chunk;
        let disconnect_grace = self.config.disconnect_grace;

        std::thread::spawn(move || {
            Self::drive_reader(
                run_id,
                binding_generation,
                stdout,
                max_output_chunk,
                disconnect_grace,
                &thread_child,
                &thread_observations,
                &thread_exit_evidence,
            );
        });

        Ok(SupervisedChild {
            child,
            observations,
            exit_evidence,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn drive_reader(
        run_id: AgentRunId,
        binding_generation: BindingGeneration,
        mut stdout: impl Read,
        max_output_chunk: usize,
        disconnect_grace: Duration,
        child: &Arc<Mutex<Child>>,
        observations: &Arc<Mutex<VecDeque<HostObservation>>>,
        exit_evidence: &Arc<Mutex<Option<HostExitEvidence>>>,
    ) {
        let mut next_ordinal = 2_u64;
        let mut buf = vec![0_u8; max_output_chunk];
        let push = |kind: HostObservationKind, ordinal: &mut u64| {
            observations.lock().unwrap().push_back(HostObservation {
                run_id,
                binding_generation,
                ordinal: *ordinal,
                kind,
            });
            *ordinal = ordinal.saturating_add(1);
        };
        loop {
            match stdout.read(&mut buf) {
                Ok(0) => {
                    let resolved = {
                        let mut guard = child.lock().unwrap();
                        match guard.try_wait() {
                            Ok(Some(status)) => Some(Self::classify_exit(status)),
                            Ok(None) => match Self::wait_for_exit(&mut guard, disconnect_grace) {
                                Ok(Some(status)) => Some(Self::classify_exit(status)),
                                Ok(None) => None,
                                Err(_) => Some(HostExitKind::Unknown),
                            },
                            Err(_) => Some(HostExitKind::Unknown),
                        }
                    };
                    match resolved {
                        Some(kind) => {
                            push(
                                match kind {
                                    HostExitKind::Completed => HostObservationKind::KnownSuccess,
                                    HostExitKind::Failed => HostObservationKind::KnownFailure,
                                    HostExitKind::Crashed => HostObservationKind::HarnessCrashed,
                                    HostExitKind::Unknown => HostObservationKind::UnknownLiveness,
                                },
                                &mut next_ordinal,
                            );
                            *exit_evidence.lock().unwrap() = Some(HostExitEvidence { kind });
                        }
                        None => {
                            // Child still alive with closed observation pipe.
                            // Honest disconnect — never fabricate KnownTerminated.
                            push(
                                HostObservationKind::ObservationDisconnected,
                                &mut next_ordinal,
                            );
                        }
                    }
                    break;
                }
                Ok(n) => {
                    push(
                        HostObservationKind::Output(buf[..n].to_vec()),
                        &mut next_ordinal,
                    );
                }
                Err(_) => {
                    push(
                        HostObservationKind::ObservationDisconnected,
                        &mut next_ordinal,
                    );
                    let mut guard = child.lock().unwrap();
                    let _ = guard.kill();
                    let _ = guard.wait();
                    break;
                }
            }
        }
    }

    fn wait_for_exit(child: &mut Child, grace: Duration) -> Result<Option<ExitStatus>, HostError> {
        let deadline = Instant::now() + grace;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return Ok(Some(status)),
                Ok(None) if Instant::now() >= deadline => return Ok(None),
                Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                Err(_) => return Err(HostError::Io),
            }
        }
    }
}

impl SessionExecutionHost for StandaloneProcessHost {
    fn kind(&self) -> ExecutionHostKind {
        ExecutionHostKind::StandaloneProcess
    }

    fn start(
        &mut self,
        run_id: AgentRunId,
        binding_generation: BindingGeneration,
        descriptor: LaunchDescriptor,
    ) -> HostStartOutcome {
        if self.config.requires_tty {
            return HostStartOutcome::NotStarted(HostNotStartedReason::TtyRequired);
        }
        match self.spawn(run_id, binding_generation, &descriptor) {
            Ok(supervised) => {
                let raw = self.next_handle.fetch_add(1, Ordering::Relaxed);
                self.children.insert(raw, supervised);
                HostStartOutcome::Started(HostHandle::new(raw))
            }
            Err(_) => HostStartOutcome::NotStarted(HostNotStartedReason::SpawnFailed),
        }
    }

    fn observe(&mut self, handle: HostHandle) -> Result<Vec<HostObservation>, ()> {
        let supervised = self.children.get(&handle.get()).ok_or(())?;
        let mut queue = supervised.observations.lock().unwrap();
        Ok(queue.drain(..).collect())
    }

    fn signal_cancel(&mut self, handle: HostHandle) -> Result<(), ()> {
        let supervised = self.children.get(&handle.get()).ok_or(())?;
        let mut child = supervised.child.lock().unwrap();
        child.kill().map_err(|_| ())
    }

    fn reap(&mut self, handle: HostHandle) -> Result<HostExitEvidence, ()> {
        let deadline = Instant::now() + self.config.disconnect_grace.max(Duration::from_secs(2));
        let evidence = loop {
            let supervised = self.children.get(&handle.get()).ok_or(())?;
            let current = *supervised.exit_evidence.lock().unwrap();
            if let Some(evidence) = current {
                break evidence;
            }
            if Instant::now() >= deadline {
                // Bounded: force resolution rather than waiting indefinitely
                // (AGENTS.md termination invariant).
                let mut child = supervised.child.lock().unwrap();
                let _ = child.kill();
                let evidence = match child.wait() {
                    Ok(status) => HostExitEvidence {
                        kind: Self::classify_exit(status),
                    },
                    Err(_) => HostExitEvidence {
                        kind: HostExitKind::Unknown,
                    },
                };
                drop(child);
                break evidence;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        self.children.remove(&handle.get());
        Ok(evidence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ObservationAuthority;
    use seyal_agent_core::{AgentDomain, WorkScopeKind};

    fn apply_all(authority: &mut ObservationAuthority, observations: Vec<HostObservation>) {
        for observation in observations {
            authority.apply(observation).unwrap();
        }
    }

    fn seeded_run() -> (ObservationAuthority, AgentRunId, BindingGeneration) {
        let mut domain = AgentDomain::new();
        let scope = domain.create_work_scope(WorkScopeKind::Repository);
        let item = domain.create_work_item(scope).unwrap();
        let attempt = domain.create_attempt(item).unwrap();
        let run = domain.create_agent_run(attempt).unwrap();
        let generation = domain.agent_run(run).unwrap().binding_generation();
        (ObservationAuthority::new(domain), run, generation)
    }

    fn descriptor(
        program: impl Into<String>,
        argv: impl IntoIterator<Item = impl Into<String>>,
    ) -> LaunchDescriptor {
        LaunchDescriptor {
            program: program.into(),
            argv: argv.into_iter().map(Into::into).collect(),
            env: Vec::new(),
            cwd: std::env::temp_dir(),
        }
    }

    /// Bounded poll for `observe` to see the terminal observation, modeling
    /// the caller re-polling off-lock rather than the host blocking.
    fn observe_until_terminal(
        host: &mut StandaloneProcessHost,
        handle: HostHandle,
        deadline: Duration,
    ) -> Vec<HostObservation> {
        let start = Instant::now();
        let mut collected = Vec::new();
        loop {
            collected.extend(host.observe(handle).unwrap());
            if collected.iter().any(|observation| {
                matches!(
                    observation.kind,
                    HostObservationKind::KnownSuccess
                        | HostObservationKind::KnownFailure
                        | HostObservationKind::HarnessCrashed
                        | HostObservationKind::ObservationDisconnected
                )
            }) {
                return collected;
            }
            if Instant::now().duration_since(start) >= deadline {
                return collected;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn successful_echo_terminates_with_known_success() {
        let (mut authority, run, generation) = seeded_run();
        let config = StandaloneProcessConfig::new(64).unwrap();
        let mut host = StandaloneProcessHost::new(config);

        let HostStartOutcome::Started(handle) =
            host.start(run, generation, descriptor("/bin/echo", ["host-ok"]))
        else {
            panic!("expected spawn evidence");
        };
        let observations = observe_until_terminal(&mut host, handle, Duration::from_secs(2));

        assert!(matches!(
            observations.first().map(|o| &o.kind),
            Some(HostObservationKind::Started)
        ));
        assert!(observations.iter().any(|o| matches!(
            &o.kind,
            HostObservationKind::Output(bytes) if bytes.windows(7).any(|w| w == b"host-ok")
        )));
        assert!(matches!(
            observations.last().map(|o| &o.kind),
            Some(HostObservationKind::KnownSuccess)
        ));

        let evidence = host.reap(handle).unwrap();
        assert_eq!(evidence.kind, HostExitKind::Completed);

        apply_all(&mut authority, observations);
        assert_eq!(authority.liveness(run), crate::RunLiveness::KnownTerminated);
    }

    #[test]
    fn signal_death_is_crash_not_fabricated_termination() {
        let (mut authority, run, generation) = seeded_run();
        let config = StandaloneProcessConfig::new(32).unwrap();
        let mut host = StandaloneProcessHost::new(config);

        let HostStartOutcome::Started(handle) =
            host.start(run, generation, descriptor("/bin/sh", ["-c", "kill -9 $$"]))
        else {
            panic!("expected spawn evidence");
        };
        let observations = observe_until_terminal(&mut host, handle, Duration::from_secs(2));

        assert!(observations
            .iter()
            .any(|o| matches!(o.kind, HostObservationKind::HarnessCrashed)));
        assert!(!observations.iter().any(|o| matches!(
            o.kind,
            HostObservationKind::KnownSuccess | HostObservationKind::KnownFailure
        )));

        apply_all(&mut authority, observations);
        assert_eq!(
            authority.liveness(run),
            crate::RunLiveness::UnknownAfterCrash
        );
    }

    #[test]
    fn stdout_eof_while_child_alive_is_disconnect_not_termination() {
        let (mut authority, run, generation) = seeded_run();

        // Close stdout, then keep the process alive long enough for EOF-with-live-child.
        let config = StandaloneProcessConfig::new(32)
            .unwrap()
            .with_disconnect_grace(Duration::from_millis(20));
        let mut host = StandaloneProcessHost::new(config);
        let HostStartOutcome::Started(handle) = host.start(
            run,
            generation,
            descriptor("/bin/sh", ["-c", "exec 1>&-; sleep 30"]),
        ) else {
            panic!("expected spawn evidence");
        };
        let observations = observe_until_terminal(&mut host, handle, Duration::from_secs(2));

        assert!(observations
            .iter()
            .any(|o| matches!(o.kind, HostObservationKind::ObservationDisconnected)));
        assert!(!observations.iter().any(|o| matches!(
            o.kind,
            HostObservationKind::KnownSuccess | HostObservationKind::KnownFailure
        )));

        apply_all(&mut authority, observations);
        assert_eq!(authority.liveness(run), crate::RunLiveness::ObservationLost);

        // The child is still alive (disconnect only). Clean up out-of-band.
        host.signal_cancel(handle).ok();
        host.reap(handle).ok();
    }

    #[test]
    fn tty_required_offering_reports_typed_not_started_without_spawning() {
        let (_, run, generation) = seeded_run();
        let config = StandaloneProcessConfig::new(16)
            .unwrap()
            .with_tty_required(true);
        let mut host = StandaloneProcessHost::new(config);
        assert_eq!(
            host.start(run, generation, descriptor("/bin/echo", ["unused"])),
            HostStartOutcome::NotStarted(HostNotStartedReason::TtyRequired)
        );
        assert!(host.children.is_empty());
    }

    /// SPEC-027 §9.4 / fixture 13: `signal_cancel` on a live child followed by
    /// a bounded `reap` must terminate without the test itself blocking
    /// indefinitely. We don't fabricate `Cancelled` at the host layer — a
    /// signalled child surfaces as `Crashed` exit evidence; it is the
    /// domain's cancel-intent bookkeeping (SPEC-026 §9.2), not the host,
    /// that interprets a crash-after-cancel as `Terminated(Cancelled)`.
    #[test]
    fn cancel_of_live_child_signals_and_reaps_without_fabricating_completion() {
        let (_, run, generation) = seeded_run();
        let config = StandaloneProcessConfig::new(32).unwrap();
        let mut host = StandaloneProcessHost::new(config);
        let HostStartOutcome::Started(handle) =
            host.start(run, generation, descriptor("/bin/sh", ["-c", "sleep 30"]))
        else {
            panic!("expected spawn evidence");
        };

        // Give the reader thread a moment to observe `Started` before cancel.
        std::thread::sleep(Duration::from_millis(20));
        host.signal_cancel(handle).unwrap();

        let started = Instant::now();
        let evidence = host.reap(handle).unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "reap must be bounded, not an indefinite wait"
        );
        assert_ne!(evidence.kind, HostExitKind::Completed);
    }

    #[test]
    fn observe_on_unknown_handle_fails_closed() {
        let config = StandaloneProcessConfig::new(16).unwrap();
        let mut host = StandaloneProcessHost::new(config);
        assert_eq!(host.observe(HostHandle::new(999)), Err(()));
        assert_eq!(host.signal_cancel(HostHandle::new(999)), Err(()));
        assert_eq!(host.reap(HostHandle::new(999)), Err(()));
    }

    #[test]
    fn standalone_host_high_volume_output_is_segment_bounded_with_refs() {
        use seyal_agent_store::{
            decode_output_ref, AgentStore, AggregateId, FingerprintRef, RetentionPolicyRef,
            OUTPUT_SEGMENT_LEN,
        };
        use std::sync::atomic::{AtomicU64 as GlobalAtomicU64, Ordering as GlobalOrdering};

        static NEXT: GlobalAtomicU64 = GlobalAtomicU64::new(1);
        let dir = std::env::temp_dir().join(format!(
            "seyal-ab15-host-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, GlobalOrdering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("agent.db");
        let store = AgentStore::open(&db).unwrap();

        let (_, run, generation) = seeded_run();

        // ~48 KiB of stdout in small host chunks → multiple append batches.
        let config = StandaloneProcessConfig::new(700).unwrap();
        let mut host = StandaloneProcessHost::new(config);
        let HostStartOutcome::Started(handle) = host.start(
            run,
            generation,
            descriptor(
                "/usr/bin/python3",
                ["-c", "import sys; sys.stdout.write('x' * (48 * 1024))"],
            ),
        ) else {
            panic!("expected spawn evidence");
        };
        let observations = observe_until_terminal(&mut host, handle, Duration::from_secs(5));
        let mut output_batches = 0_u64;
        for observation in &observations {
            if let HostObservationKind::Output(bytes) = &observation.kind {
                store
                    .append_output_event(run, 2, bytes, observation.ordinal, observation.ordinal)
                    .unwrap();
                output_batches += 1;
            }
        }
        assert!(output_batches > 1, "cross-batch streaming required");
        let segments = store.output_segment_count(run).unwrap();
        assert_eq!(segments, (48 * 1024) / OUTPUT_SEGMENT_LEN as u64);
        assert!(segments < output_batches, "no one-row-per-token regression");

        let events = store
            .replay_after(AggregateId::AgentRun(run), None)
            .unwrap();
        let mut rebuilt = Vec::new();
        for event in &events {
            let output = decode_output_ref(&event.payload).unwrap();
            assert_eq!(
                output.retention_policy_ref,
                RetentionPolicyRef::retained_stream()
            );
            assert!(matches!(
                output.fingerprint_ref,
                FingerprintRef::PublicContentDigest(_)
            ));
            rebuilt.extend(store.materialize_output_ref(run, &output).unwrap());
        }
        assert_eq!(rebuilt.len(), 48 * 1024);
        assert!(rebuilt.iter().all(|b| *b == b'x'));
        let _ = std::fs::remove_dir_all(dir);
    }
}
