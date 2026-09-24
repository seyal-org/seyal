use std::collections::HashMap;

use seyal_agent_core::{AgentDomain, AgentRunId, DomainError};

use crate::{HostObservation, HostObservationKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunLiveness {
    ScriptedLive,
    ObservationLost,
    UnknownAfterCrash,
    KnownTerminated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObserveError {
    Domain(DomainError),
    StaleGeneration,
    OutOfOrder,
    Conflict,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkItemOutcome {
    NotCommitted,
}

/// Applies host observations without becoming a second lifecycle writer.
pub struct ObservationAuthority {
    domain: AgentDomain,
    applied: HashMap<(AgentRunId, u64), HostObservationKind>,
    liveness: HashMap<AgentRunId, RunLiveness>,
    effects_performed: u64,
}

impl ObservationAuthority {
    pub fn new(domain: AgentDomain) -> Self {
        Self {
            domain,
            applied: HashMap::new(),
            liveness: HashMap::new(),
            effects_performed: 0,
        }
    }

    pub fn domain(&self) -> &AgentDomain {
        &self.domain
    }

    pub fn liveness(&self, run_id: AgentRunId) -> RunLiveness {
        self.liveness
            .get(&run_id)
            .copied()
            .unwrap_or(RunLiveness::ScriptedLive)
    }

    pub fn work_item_outcome(&self, _run_id: AgentRunId) -> WorkItemOutcome {
        WorkItemOutcome::NotCommitted
    }

    pub fn effects_performed(&self) -> u64 {
        self.effects_performed
    }

    pub fn applied_count(&self) -> usize {
        self.applied.len()
    }

    pub fn apply(&mut self, observation: HostObservation) -> Result<(), ObserveError> {
        self.domain
            .validate_binding_generation(observation.run_id, observation.binding_generation)
            .map_err(|error| match error {
                DomainError::StaleBindingGeneration { .. } => ObserveError::StaleGeneration,
                other => ObserveError::Domain(other),
            })?;

        let key = (observation.run_id, observation.ordinal);
        if let Some(previous) = self.applied.get(&key) {
            return if previous == &observation.kind {
                Ok(())
            } else {
                Err(ObserveError::Conflict)
            };
        }
        let expected = self
            .applied
            .keys()
            .filter(|(run_id, _)| *run_id == observation.run_id)
            .map(|(_, ordinal)| *ordinal)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(ObserveError::OutOfOrder)?;
        if observation.ordinal != expected {
            return Err(ObserveError::OutOfOrder);
        }

        match &observation.kind {
            HostObservationKind::ObservationDisconnected => {
                self.liveness
                    .insert(observation.run_id, RunLiveness::ObservationLost);
            }
            HostObservationKind::ObservationReconnected => {
                if self.liveness(observation.run_id) == RunLiveness::ObservationLost {
                    self.liveness
                        .insert(observation.run_id, RunLiveness::ScriptedLive);
                }
            }
            HostObservationKind::HarnessCrashed | HostObservationKind::UnknownLiveness => {
                self.liveness
                    .insert(observation.run_id, RunLiveness::UnknownAfterCrash);
            }
            HostObservationKind::Delayed { .. } => {
                self.liveness
                    .entry(observation.run_id)
                    .or_insert(RunLiveness::ScriptedLive);
            }
            HostObservationKind::KnownSuccess | HostObservationKind::KnownFailure => {
                self.liveness
                    .insert(observation.run_id, RunLiveness::KnownTerminated);
            }
            HostObservationKind::EffectUnknown => {}
            _ => {
                self.liveness
                    .entry(observation.run_id)
                    .or_insert(RunLiveness::ScriptedLive);
            }
        }
        self.applied.insert(key, observation.kind);
        Ok(())
    }
}

pub fn parse_script(text: &str) -> Result<Vec<crate::ScriptStep>, crate::ScriptError> {
    if text.len() > 64 * 1024 || text.lines().count() > 1024 {
        return Err(crate::ScriptError::ScriptTooLarge);
    }
    let mut steps = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        steps.push(parse_line(line)?);
    }
    if steps.is_empty() {
        return Err(crate::ScriptError::EmptyScript);
    }
    Ok(steps)
}

fn parse_line(line: &str) -> Result<crate::ScriptStep, crate::ScriptError> {
    let mut parts = line.split_whitespace();
    let verb = parts.next().ok_or(crate::ScriptError::MalformedScript)?;
    match verb {
        "duplicate" => Ok(crate::ScriptStep::DuplicateLast),
        "delay" => {
            let ticks = parts
                .next()
                .ok_or(crate::ScriptError::MalformedScript)?
                .parse()
                .map_err(|_| crate::ScriptError::MalformedScript)?;
            Ok(crate::ScriptStep::DelayTicks(ticks))
        }
        "emit" => {
            let kind = parts.next().ok_or(crate::ScriptError::MalformedScript)?;
            let kind = match kind {
                "started" => HostObservationKind::Started,
                "progress" => {
                    let step = parts
                        .next()
                        .ok_or(crate::ScriptError::MalformedScript)?
                        .parse()
                        .map_err(|_| crate::ScriptError::MalformedScript)?;
                    HostObservationKind::Progress { step }
                }
                "disconnect" => HostObservationKind::ObservationDisconnected,
                "reconnect" => HostObservationKind::ObservationReconnected,
                "success" => HostObservationKind::KnownSuccess,
                "failure" => HostObservationKind::KnownFailure,
                "crash" => HostObservationKind::HarnessCrashed,
                "unknown-liveness" => HostObservationKind::UnknownLiveness,
                "effect-unknown" => HostObservationKind::EffectUnknown,
                "result" | "output" => {
                    let hex = parts.next().ok_or(crate::ScriptError::MalformedScript)?;
                    let bytes = decode_hex(hex)?;
                    if kind == "result" {
                        HostObservationKind::Result(bytes)
                    } else {
                        HostObservationKind::Output(bytes)
                    }
                }
                _ => return Err(crate::ScriptError::MalformedScript),
            };
            if parts.next().is_some() {
                return Err(crate::ScriptError::MalformedScript);
            }
            Ok(crate::ScriptStep::Emit(kind))
        }
        _ => Err(crate::ScriptError::MalformedScript),
    }
}

fn decode_hex(text: &str) -> Result<Vec<u8>, crate::ScriptError> {
    if !text.len().is_multiple_of(2) || text.len() > 8192 {
        return Err(crate::ScriptError::MalformedScript);
    }
    (0..text.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&text[index..index + 2], 16)
                .map_err(|_| crate::ScriptError::MalformedScript)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FakeExecutionHost, ScriptStep};
    use seyal_agent_core::{BindingGeneration, WorkScopeKind};

    const NORMAL_SUCCESS: &str = include_str!("../conformance/normal-success.script");
    const DISCONNECT: &str = include_str!("../conformance/disconnect-reconnect.script");
    const CRASH: &str = include_str!("../conformance/crash.script");
    const KNOWN_FAILURE: &str = include_str!("../conformance/known-failure.script");
    const RESUMABLE: &str = include_str!("../conformance/resumable-continuation.script");
    const HIGH_VOLUME: &str = include_str!("../conformance/high-volume.script");
    const UNKNOWN_LIVENESS: &str = include_str!("../conformance/unknown-liveness.script");

    fn authority_with_run() -> (ObservationAuthority, AgentRunId, BindingGeneration) {
        let mut domain = AgentDomain::new();
        let scope = domain.create_work_scope(WorkScopeKind::Repository);
        let item = domain.create_work_item(scope).unwrap();
        let attempt = domain.create_attempt(item).unwrap();
        let run = domain.create_agent_run(attempt).unwrap();
        let generation = domain.agent_run(run).unwrap().binding_generation();
        (ObservationAuthority::new(domain), run, generation)
    }

    fn apply_script(script: &str) -> (ObservationAuthority, AgentRunId) {
        let (mut authority, run, generation) = authority_with_run();
        let host = FakeExecutionHost::new(4).unwrap();
        let steps = parse_script(script).unwrap();
        let observations = host.execute(run, generation, &steps).unwrap();
        for observation in observations {
            authority.apply(observation).unwrap();
        }
        (authority, run)
    }

    #[test]
    fn conformance_scripts_do_not_commit_a_work_item_outcome() {
        let (success, run) = apply_script(NORMAL_SUCCESS);
        assert_eq!(success.liveness(run), RunLiveness::KnownTerminated);
        assert_eq!(
            success.work_item_outcome(run),
            WorkItemOutcome::NotCommitted
        );
        assert!(
            success
                .domain()
                .agent_run(run)
                .unwrap()
                .binding_generation()
                .get()
                >= 1
        );

        let (disconnected, run) = apply_script(DISCONNECT);
        assert_eq!(disconnected.liveness(run), RunLiveness::ScriptedLive);
        assert_eq!(
            disconnected.work_item_outcome(run),
            WorkItemOutcome::NotCommitted
        );

        let (crashed, run) = apply_script(CRASH);
        assert_eq!(crashed.liveness(run), RunLiveness::UnknownAfterCrash);
        assert_eq!(
            crashed.work_item_outcome(run),
            WorkItemOutcome::NotCommitted
        );
        assert_eq!(crashed.effects_performed(), 0);

        let (failed, run) = apply_script(KNOWN_FAILURE);
        assert_eq!(failed.liveness(run), RunLiveness::KnownTerminated);

        let (resumed, run) = apply_script(RESUMABLE);
        assert_eq!(resumed.liveness(run), RunLiveness::KnownTerminated);

        let (volume, run) = apply_script(HIGH_VOLUME);
        assert_eq!(volume.liveness(run), RunLiveness::KnownTerminated);
        assert!(volume.applied_count() > 2);

        let (unknown, run) = apply_script(UNKNOWN_LIVENESS);
        assert_eq!(unknown.liveness(run), RunLiveness::UnknownAfterCrash);
    }

    #[test]
    fn delay_ticks_emit_a_visible_delayed_observation() {
        let (mut authority, run, generation) = authority_with_run();
        let host = FakeExecutionHost::new(8).unwrap();
        let observations = host
            .execute(
                run,
                generation,
                &[
                    ScriptStep::Emit(HostObservationKind::Started),
                    ScriptStep::DelayTicks(3),
                    ScriptStep::Emit(HostObservationKind::KnownSuccess),
                ],
            )
            .unwrap();
        assert_eq!(
            observations[1].kind,
            HostObservationKind::Delayed { ticks: 3 }
        );
        for observation in observations {
            authority.apply(observation).unwrap();
        }
        assert_eq!(authority.liveness(run), RunLiveness::KnownTerminated);
    }

    #[test]
    fn records_normal_high_volume_reconnect_and_crash_resource_samples() {
        use std::time::Instant;
        let samples = [
            ("normal", NORMAL_SUCCESS),
            ("high-volume", HIGH_VOLUME),
            ("reconnect", DISCONNECT),
            ("crash", CRASH),
        ];
        for (name, script) in samples {
            let started = Instant::now();
            let (authority, run) = apply_script(script);
            let elapsed = started.elapsed();
            assert_eq!(
                authority.work_item_outcome(run),
                WorkItemOutcome::NotCommitted
            );
            eprintln!(
                "ab-0.5 measurement case={} elapsed_us={} observations={} rss_kib={:?}",
                name,
                elapsed.as_micros(),
                authority.applied_count(),
                resident_kib()
            );
        }
    }

    fn resident_kib() -> Option<u64> {
        let output = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()?;
        String::from_utf8(output.stdout).ok()?.trim().parse().ok()
    }

    #[test]
    fn duplicate_is_idempotent_and_stale_or_gapped_events_do_not_apply() {
        let (mut authority, run, generation) = authority_with_run();
        let host = FakeExecutionHost::new(8).unwrap();
        let observations = host
            .execute(
                run,
                generation,
                &[
                    ScriptStep::Emit(HostObservationKind::Started),
                    ScriptStep::DuplicateLast,
                ],
            )
            .unwrap();
        authority.apply(observations[0].clone()).unwrap();
        authority.apply(observations[1].clone()).unwrap();
        assert_eq!(authority.applied.len(), 1);

        let before = authority.applied.len();
        let presented = BindingGeneration::FIRST;
        let advanced = authority
            .domain
            .advance_binding_generation(run, presented)
            .unwrap();
        let rejected = HostObservation {
            run_id: run,
            binding_generation: presented,
            ordinal: 2,
            kind: HostObservationKind::Progress { step: 9 },
        };
        assert_eq!(
            authority.apply(rejected),
            Err(ObserveError::StaleGeneration)
        );
        assert_eq!(authority.applied.len(), before);
        assert_ne!(advanced, presented);

        let gap = HostObservation {
            run_id: run,
            binding_generation: advanced,
            ordinal: 4,
            kind: HostObservationKind::KnownSuccess,
        };
        assert_eq!(authority.apply(gap), Err(ObserveError::OutOfOrder));
        assert_eq!(
            authority.work_item_outcome(run),
            WorkItemOutcome::NotCommitted
        );
    }

    #[test]
    fn malformed_scripts_are_bounded() {
        assert!(parse_script("emit nope").is_err());
        assert!(parse_script("").is_err());
        let huge = "x".repeat(70_000);
        assert!(parse_script(&huge).is_err());
        let mut state = 9_u64;
        for _ in 0..100 {
            let mut text = String::new();
            for _ in 0..30 {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                text.push((32 + (state % 90) as u8) as char);
            }
            let _ = parse_script(&text);
        }
    }
}
