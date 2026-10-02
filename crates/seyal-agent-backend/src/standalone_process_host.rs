//! First real ExecutionHost: supervised standalone process (SPEC-018 §2).
//!
//! Owns no PTY master or TerminalState. Observations are submitted through the
//! existing ObservationAuthority path by callers.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use seyal_agent_core::{AgentRunId, BindingGeneration, ExecutionHost, ExecutionHostKind};

use crate::{HostObservation, HostObservationKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostError {
    ZeroOutputChunk,
    EmptyProgram,
    SpawnFailed,
    Io,
    OrdinalExhausted,
}

/// Configuration for one StandaloneProcessHost supervision turn.
#[derive(Clone, Debug)]
pub struct StandaloneProcessConfig {
    program: PathBuf,
    args: Vec<String>,
    max_output_chunk: usize,
    /// Bound how long we wait for exit after stdout EOF while the child is live.
    disconnect_grace: Duration,
}

impl StandaloneProcessConfig {
    pub fn new(
        program: impl Into<PathBuf>,
        args: impl IntoIterator<Item = impl Into<String>>,
        max_output_chunk: usize,
    ) -> Result<Self, HostError> {
        if max_output_chunk == 0 {
            return Err(HostError::ZeroOutputChunk);
        }
        let program = program.into();
        if program.as_os_str().is_empty() {
            return Err(HostError::EmptyProgram);
        }
        Ok(Self {
            program,
            args: args.into_iter().map(Into::into).collect(),
            max_output_chunk,
            disconnect_grace: Duration::from_millis(50),
        })
    }

    pub fn with_disconnect_grace(mut self, grace: Duration) -> Self {
        self.disconnect_grace = grace;
        self
    }

    pub fn program(&self) -> &Path {
        &self.program
    }
}

/// Supervises a standalone OS process and emits HostObservations.
pub struct StandaloneProcessHost {
    config: StandaloneProcessConfig,
}

impl StandaloneProcessHost {
    pub fn new(config: StandaloneProcessConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &StandaloneProcessConfig {
        &self.config
    }

    fn push(
        observations: &mut Vec<HostObservation>,
        run_id: AgentRunId,
        binding_generation: BindingGeneration,
        next_ordinal: &mut u64,
        kind: HostObservationKind,
    ) -> Result<(), HostError> {
        observations.push(HostObservation {
            run_id,
            binding_generation,
            ordinal: *next_ordinal,
            kind,
        });
        *next_ordinal = next_ordinal
            .checked_add(1)
            .ok_or(HostError::OrdinalExhausted)?;
        Ok(())
    }

    fn classify_exit(status: ExitStatus) -> HostObservationKind {
        if status.success() {
            HostObservationKind::KnownSuccess
        } else if status.code().is_some() {
            HostObservationKind::KnownFailure
        } else {
            // Signal death (e.g. SIGKILL) is crash evidence, not a clean terminate.
            HostObservationKind::HarnessCrashed
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

    fn reap_after_disconnect(child: &mut Child, grace: Duration) -> Result<(), HostError> {
        match Self::wait_for_exit(child, grace)? {
            Some(_) => Ok(()),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                Ok(())
            }
        }
    }
}

impl ExecutionHost for StandaloneProcessHost {
    type Observation = HostObservation;
    type Error = HostError;

    fn kind(&self) -> ExecutionHostKind {
        ExecutionHostKind::StandaloneProcess
    }

    fn collect_observations(
        &mut self,
        run_id: AgentRunId,
        binding_generation: BindingGeneration,
    ) -> Result<Vec<HostObservation>, HostError> {
        let mut child = Command::new(&self.config.program)
            .args(&self.config.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| HostError::SpawnFailed)?;

        let mut stdout = child.stdout.take().ok_or(HostError::SpawnFailed)?;
        let mut observations = Vec::new();
        let mut next_ordinal = 1_u64;
        Self::push(
            &mut observations,
            run_id,
            binding_generation,
            &mut next_ordinal,
            HostObservationKind::Started,
        )?;

        let mut buf = vec![0_u8; self.config.max_output_chunk];
        loop {
            match stdout.read(&mut buf) {
                Ok(0) => {
                    // Stdout EOF: prefer an immediate exit status, then a short
                    // grace for crash races (SIGKILL can close the pipe before
                    // waiters observe the zombie). Only then treat as disconnect.
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            Self::push(
                                &mut observations,
                                run_id,
                                binding_generation,
                                &mut next_ordinal,
                                Self::classify_exit(status),
                            )?;
                        }
                        Ok(None) => {
                            match Self::wait_for_exit(&mut child, self.config.disconnect_grace) {
                                Ok(Some(status)) => {
                                    Self::push(
                                        &mut observations,
                                        run_id,
                                        binding_generation,
                                        &mut next_ordinal,
                                        Self::classify_exit(status),
                                    )?;
                                }
                                Ok(None) => {
                                    // Child still alive with closed observation pipe.
                                    // Honest disconnect — never fabricate KnownTerminated.
                                    Self::push(
                                        &mut observations,
                                        run_id,
                                        binding_generation,
                                        &mut next_ordinal,
                                        HostObservationKind::ObservationDisconnected,
                                    )?;
                                    let _ = Self::reap_after_disconnect(
                                        &mut child,
                                        self.config.disconnect_grace,
                                    );
                                }
                                Err(_) => {
                                    Self::push(
                                        &mut observations,
                                        run_id,
                                        binding_generation,
                                        &mut next_ordinal,
                                        HostObservationKind::UnknownLiveness,
                                    )?;
                                    let _ = child.kill();
                                    let _ = child.wait();
                                }
                            }
                        }
                        Err(_) => {
                            Self::push(
                                &mut observations,
                                run_id,
                                binding_generation,
                                &mut next_ordinal,
                                HostObservationKind::UnknownLiveness,
                            )?;
                            let _ = child.kill();
                            let _ = child.wait();
                        }
                    }
                    break;
                }
                Ok(n) => {
                    Self::push(
                        &mut observations,
                        run_id,
                        binding_generation,
                        &mut next_ordinal,
                        HostObservationKind::Output(buf[..n].to_vec()),
                    )?;
                }
                Err(_) => {
                    Self::push(
                        &mut observations,
                        run_id,
                        binding_generation,
                        &mut next_ordinal,
                        HostObservationKind::ObservationDisconnected,
                    )?;
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
            }
        }

        Ok(observations)
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

    #[test]
    fn successful_echo_terminates_with_known_success() {
        let (mut authority, run, generation) = {
            let mut domain = AgentDomain::new();
            let scope = domain.create_work_scope(WorkScopeKind::Repository);
            let item = domain.create_work_item(scope).unwrap();
            let attempt = domain.create_attempt(item).unwrap();
            let run = domain.create_agent_run(attempt).unwrap();
            let generation = domain.agent_run(run).unwrap().binding_generation();
            (ObservationAuthority::new(domain), run, generation)
        };
        let config = StandaloneProcessConfig::new("/bin/echo", ["host-ok"], 64).unwrap();
        let mut host = StandaloneProcessHost::new(config);
        assert_eq!(host.kind(), ExecutionHostKind::StandaloneProcess);

        let observations = host.collect_observations(run, generation).unwrap();
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

        apply_all(&mut authority, observations);
        assert_eq!(authority.liveness(run), crate::RunLiveness::KnownTerminated);
    }

    #[test]
    fn signal_death_is_crash_not_fabricated_termination() {
        let (mut authority, run, generation) = {
            let mut domain = AgentDomain::new();
            let scope = domain.create_work_scope(WorkScopeKind::Repository);
            let item = domain.create_work_item(scope).unwrap();
            let attempt = domain.create_attempt(item).unwrap();
            let run = domain.create_agent_run(attempt).unwrap();
            let generation = domain.agent_run(run).unwrap().binding_generation();
            (ObservationAuthority::new(domain), run, generation)
        };

        let config = StandaloneProcessConfig::new("/bin/sh", ["-c", "kill -9 $$"], 32).unwrap();
        let mut host = StandaloneProcessHost::new(config);
        let observations = host.collect_observations(run, generation).unwrap();
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
        let (mut authority, run, generation) = {
            let mut domain = AgentDomain::new();
            let scope = domain.create_work_scope(WorkScopeKind::Repository);
            let item = domain.create_work_item(scope).unwrap();
            let attempt = domain.create_attempt(item).unwrap();
            let run = domain.create_agent_run(attempt).unwrap();
            let generation = domain.agent_run(run).unwrap().binding_generation();
            (ObservationAuthority::new(domain), run, generation)
        };

        // Close stdout, then keep the process alive long enough for EOF-with-live-child.
        let config = StandaloneProcessConfig::new("/bin/sh", ["-c", "exec 1>&-; sleep 30"], 32)
            .unwrap()
            .with_disconnect_grace(Duration::from_millis(20));
        let mut host = StandaloneProcessHost::new(config);
        let observations = host.collect_observations(run, generation).unwrap();

        assert!(observations
            .iter()
            .any(|o| matches!(o.kind, HostObservationKind::ObservationDisconnected)));
        assert!(!observations.iter().any(|o| matches!(
            o.kind,
            HostObservationKind::KnownSuccess | HostObservationKind::KnownFailure
        )));

        apply_all(&mut authority, observations);
        assert_eq!(authority.liveness(run), crate::RunLiveness::ObservationLost);
    }

    fn collect_via_trait<H: ExecutionHost>(
        host: &mut H,
        run_id: AgentRunId,
        binding_generation: BindingGeneration,
    ) -> Result<Vec<H::Observation>, H::Error> {
        host.collect_observations(run_id, binding_generation)
    }

    #[test]
    fn trait_dispatch_smoke_covers_fake_and_standalone() {
        let mut fake = crate::FakeExecutionHost::new(8).unwrap();
        fake.set_script(vec![crate::ScriptStep::Emit(HostObservationKind::Started)]);
        assert_eq!(fake.kind(), ExecutionHostKind::Fake);

        let mut domain = AgentDomain::new();
        let scope = domain.create_work_scope(WorkScopeKind::Repository);
        let item = domain.create_work_item(scope).unwrap();
        let attempt = domain.create_attempt(item).unwrap();
        let run = domain.create_agent_run(attempt).unwrap();
        let generation = domain.agent_run(run).unwrap().binding_generation();

        let fake_obs = collect_via_trait(&mut fake, run, generation).unwrap();
        assert_eq!(fake_obs.len(), 1);

        let config = StandaloneProcessConfig::new("/bin/echo", ["dispatch"], 16).unwrap();
        let mut standalone = StandaloneProcessHost::new(config);
        assert_eq!(standalone.kind(), ExecutionHostKind::StandaloneProcess);
        let host_obs = collect_via_trait(&mut standalone, run, generation).unwrap();
        assert!(matches!(
            host_obs.first().map(|o| &o.kind),
            Some(HostObservationKind::Started)
        ));
    }

    #[test]
    fn stale_binding_generation_is_denied_on_submit() {
        let mut domain = AgentDomain::new();
        let scope = domain.create_work_scope(WorkScopeKind::Repository);
        let item = domain.create_work_item(scope).unwrap();
        let attempt = domain.create_attempt(item).unwrap();
        let run = domain.create_agent_run(attempt).unwrap();
        let stale = domain.agent_run(run).unwrap().binding_generation();
        let current = domain
            .advance_binding_generation(run, BindingGeneration::FIRST)
            .unwrap();
        assert_ne!(stale, current);

        let mut authority = ObservationAuthority::new(domain);
        let config = StandaloneProcessConfig::new("/bin/echo", ["stale"], 16).unwrap();
        let mut host = StandaloneProcessHost::new(config);
        let observations = host.collect_observations(run, stale).unwrap();
        let err = authority.apply(observations[0].clone()).unwrap_err();
        assert_eq!(err, crate::ObserveError::StaleGeneration);
    }
}
