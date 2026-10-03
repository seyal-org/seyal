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
    /// Per-run next expected ordinal. Production next-lookup must not scan
    /// `applied` payloads (AB-1.6 / AB-0 shortcut removal).
    next_ordinal: HashMap<AgentRunId, u64>,
    liveness: HashMap<AgentRunId, RunLiveness>,
    effects_performed: u64,
}

impl ObservationAuthority {
    pub fn new(domain: AgentDomain) -> Self {
        Self {
            domain,
            applied: HashMap::new(),
            next_ordinal: HashMap::new(),
            liveness: HashMap::new(),
            effects_performed: 0,
        }
    }

    pub fn domain(&self) -> &AgentDomain {
        &self.domain
    }

    pub fn domain_mut(&mut self) -> &mut AgentDomain {
        &mut self.domain
    }

    pub fn restore_work_scope(
        &mut self,
        id: seyal_agent_core::WorkScopeId,
        kind: seyal_agent_core::WorkScopeKind,
    ) -> Result<(), DomainError> {
        self.domain.restore_work_scope(id, kind)
    }

    pub fn restore_work_item(
        &mut self,
        id: seyal_agent_core::WorkItemId,
        work_scope_id: seyal_agent_core::WorkScopeId,
    ) -> Result<(), DomainError> {
        self.domain.restore_work_item(id, work_scope_id)
    }

    pub fn restore_attempt(
        &mut self,
        id: seyal_agent_core::AttemptId,
        work_item_id: seyal_agent_core::WorkItemId,
    ) -> Result<(), DomainError> {
        self.domain.restore_attempt(id, work_item_id)
    }

    pub fn restore_agent_run(
        &mut self,
        id: AgentRunId,
        attempt_id: seyal_agent_core::AttemptId,
        binding_generation: seyal_agent_core::BindingGeneration,
        control_generation: seyal_agent_core::ControlGeneration,
    ) -> Result<(), DomainError> {
        self.domain
            .restore_agent_run(id, attempt_id, binding_generation, control_generation)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore_agent_run_state(
        &mut self,
        id: AgentRunId,
        attempt_id: seyal_agent_core::AttemptId,
        binding_generation: seyal_agent_core::BindingGeneration,
        control_generation: seyal_agent_core::ControlGeneration,
        lifecycle: seyal_agent_core::AgentRunLifecycle,
        execution_liveness: seyal_agent_core::ExecutionLiveness,
        observation: seyal_agent_core::ObservationFact,
        resumability: seyal_agent_core::ResumabilityFact,
        run_revision: u64,
    ) -> Result<(), DomainError> {
        self.domain.restore_agent_run_state(
            id,
            attempt_id,
            binding_generation,
            control_generation,
            lifecycle,
            execution_liveness,
            observation,
            resumability,
            run_revision,
            None,
            None,
            None,
        )
    }

    pub fn restore_orphaned_agent_run(
        &mut self,
        id: AgentRunId,
        attempt_id: seyal_agent_core::AttemptId,
        binding_generation: seyal_agent_core::BindingGeneration,
        control_generation: seyal_agent_core::ControlGeneration,
    ) -> Result<(), DomainError> {
        self.domain.restore_orphaned_agent_run(
            id,
            attempt_id,
            binding_generation,
            control_generation,
        )
    }

    /// Record durable terminal evidence before crash recovery classification.
    ///
    /// `mark_recovered` must not erase KnownTerminated once this is set
    /// (`set_liveness` keeps KnownTerminated sticky).
    pub fn note_committed_terminal(&mut self, run_id: AgentRunId) {
        self.liveness.insert(run_id, RunLiveness::KnownTerminated);
    }

    /// Crash recovery default: unknown liveness unless durable terminal
    /// observations were already noted.
    pub fn mark_recovered(&mut self, run_id: AgentRunId) {
        self.set_liveness(run_id, RunLiveness::UnknownAfterCrash);
    }

    /// Next ordinal the authority will accept for `run_id` (1 when none yet).
    pub fn next_expected_ordinal(&self, run_id: AgentRunId) -> u64 {
        self.next_ordinal.get(&run_id).copied().unwrap_or(1)
    }

    /// Fence a recovered run so pre-crash binding/control presentations fail.
    pub fn advance_binding_generation(
        &mut self,
        run_id: AgentRunId,
        presented: seyal_agent_core::BindingGeneration,
    ) -> Result<seyal_agent_core::BindingGeneration, DomainError> {
        self.domain.advance_binding_generation(run_id, presented)
    }

    pub fn advance_control_generation(
        &mut self,
        run_id: AgentRunId,
        presented: seyal_agent_core::ControlGeneration,
    ) -> Result<seyal_agent_core::ControlGeneration, DomainError> {
        self.domain.advance_control_generation(run_id, presented)
    }

    pub fn liveness(&self, run_id: AgentRunId) -> RunLiveness {
        self.recorded_liveness(run_id)
            .unwrap_or(RunLiveness::ScriptedLive)
    }

    pub fn recorded_liveness(&self, run_id: AgentRunId) -> Option<RunLiveness> {
        self.liveness.get(&run_id).copied()
    }

    /// Drop one accepted observation when its event did not commit.
    pub fn undo_apply(
        &mut self,
        observation: &HostObservation,
        previous_liveness: Option<RunLiveness>,
        previous_effects: u64,
    ) {
        self.applied
            .remove(&(observation.run_id, observation.ordinal));
        let expected_next = self.next_expected_ordinal(observation.run_id);
        if observation
            .ordinal
            .checked_add(1)
            .is_some_and(|tip| tip == expected_next)
        {
            if observation.ordinal <= 1 {
                self.next_ordinal.remove(&observation.run_id);
            } else {
                self.next_ordinal
                    .insert(observation.run_id, observation.ordinal);
            }
        }
        match previous_liveness {
            Some(liveness) => {
                self.liveness.insert(observation.run_id, liveness);
            }
            None => {
                self.liveness.remove(&observation.run_id);
            }
        }
        self.effects_performed = previous_effects;
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
                DomainError::StaleBinding { .. } => ObserveError::StaleGeneration,
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
        let expected = self.next_expected_ordinal(observation.run_id);
        if observation.ordinal != expected {
            return Err(ObserveError::OutOfOrder);
        }
        let next = expected.checked_add(1).ok_or(ObserveError::OutOfOrder)?;

        match &observation.kind {
            HostObservationKind::ObservationDisconnected => {
                self.set_liveness(observation.run_id, RunLiveness::ObservationLost);
            }
            HostObservationKind::ObservationReconnected => {
                if self.liveness(observation.run_id) == RunLiveness::ObservationLost {
                    self.set_liveness(observation.run_id, RunLiveness::ScriptedLive);
                }
            }
            HostObservationKind::HarnessCrashed | HostObservationKind::UnknownLiveness => {
                self.set_liveness(observation.run_id, RunLiveness::UnknownAfterCrash);
            }
            HostObservationKind::Delayed { .. } => {
                if !self.liveness.contains_key(&observation.run_id) {
                    self.set_liveness(observation.run_id, RunLiveness::ScriptedLive);
                }
            }
            HostObservationKind::KnownSuccess | HostObservationKind::KnownFailure => {
                self.set_liveness(observation.run_id, RunLiveness::KnownTerminated);
            }
            HostObservationKind::Result(_) | HostObservationKind::Output(_) => {
                self.effects_performed += 1;
                if !self.liveness.contains_key(&observation.run_id) {
                    self.set_liveness(observation.run_id, RunLiveness::ScriptedLive);
                }
            }
            HostObservationKind::EffectUnknown => {
                // Deliberately does not increment effects_performed: the
                // unknown-effect path must prove zero side effects.
            }
            _ => {
                if !self.liveness.contains_key(&observation.run_id) {
                    self.set_liveness(observation.run_id, RunLiveness::ScriptedLive);
                }
            }
        }
        self.applied.insert(key, observation.kind);
        self.next_ordinal.insert(observation.run_id, next);
        Ok(())
    }

    /// KnownTerminated is sticky against every later liveness. UnknownAfterCrash
    /// is sticky against disconnect/reconnect so crash ≠ live (Issue #1028).
    fn set_liveness(&mut self, run_id: AgentRunId, next: RunLiveness) {
        match self.liveness(run_id) {
            RunLiveness::KnownTerminated => return,
            RunLiveness::UnknownAfterCrash
                if matches!(
                    next,
                    RunLiveness::ObservationLost | RunLiveness::ScriptedLive
                ) =>
            {
                return;
            }
            _ => {}
        }
        self.liveness.insert(run_id, next);
    }
}

#[cfg(all(test, feature = "fixture-host"))]
mod tests {
    use super::*;
    use crate::{parse_script, FakeExecutionHost, ScriptStep};
    use seyal_agent_core::{BindingGeneration, WorkScopeKind};
    use std::time::Instant;

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
    fn known_terminated_liveness_is_sticky_against_disconnect_and_crash() {
        let (mut authority, run, generation) = authority_with_run();
        authority
            .apply(HostObservation {
                run_id: run,
                binding_generation: generation,
                ordinal: 1,
                kind: HostObservationKind::KnownSuccess,
            })
            .unwrap();
        assert_eq!(authority.liveness(run), RunLiveness::KnownTerminated);
        for (ordinal, kind) in [
            (2, HostObservationKind::ObservationDisconnected),
            (3, HostObservationKind::HarnessCrashed),
            (4, HostObservationKind::UnknownLiveness),
        ] {
            authority
                .apply(HostObservation {
                    run_id: run,
                    binding_generation: generation,
                    ordinal,
                    kind,
                })
                .unwrap();
            assert_eq!(authority.liveness(run), RunLiveness::KnownTerminated);
        }
    }

    #[test]
    fn unknown_after_crash_is_sticky_against_disconnect_and_reconnect() {
        let (mut authority, run, generation) = authority_with_run();
        authority
            .apply(HostObservation {
                run_id: run,
                binding_generation: generation,
                ordinal: 1,
                kind: HostObservationKind::HarnessCrashed,
            })
            .unwrap();
        assert_eq!(authority.liveness(run), RunLiveness::UnknownAfterCrash);
        authority
            .apply(HostObservation {
                run_id: run,
                binding_generation: generation,
                ordinal: 2,
                kind: HostObservationKind::ObservationDisconnected,
            })
            .unwrap();
        assert_eq!(authority.liveness(run), RunLiveness::UnknownAfterCrash);
        authority
            .apply(HostObservation {
                run_id: run,
                binding_generation: generation,
                ordinal: 3,
                kind: HostObservationKind::ObservationReconnected,
            })
            .unwrap();
        assert_eq!(authority.liveness(run), RunLiveness::UnknownAfterCrash);
    }

    #[test]
    fn effect_unknown_proves_zero_side_effects_while_result_increments() {
        let (mut authority, run, generation) = authority_with_run();
        authority
            .apply(HostObservation {
                run_id: run,
                binding_generation: generation,
                ordinal: 1,
                kind: HostObservationKind::EffectUnknown,
            })
            .unwrap();
        assert_eq!(authority.effects_performed(), 0);
        authority
            .apply(HostObservation {
                run_id: run,
                binding_generation: generation,
                ordinal: 2,
                kind: HostObservationKind::Result(b"ok".to_vec()),
            })
            .unwrap();
        assert_eq!(authority.effects_performed(), 1);
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
        // Minimized libFuzzer crash: even byte length, odd char boundary.
        assert!(parse_script("emit result e\u{00c2}e").is_err());
        assert!(parse_script("emit output e\u{00c2}e").is_err());
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

    #[test]
    fn next_ordinal_cursor_stays_constant_time_under_large_retained_payloads() {
        // Scale fixture: many large payloads must not make next-ordinal lookup
        // scan applied entries. A linear scan of 8_000 × 2 KiB keys would blow
        // this bound; the per-run cursor must stay O(1).
        const EVENTS: u64 = 8_000;
        const PAYLOAD: usize = 2 * 1024;
        let (mut authority, run, generation) = authority_with_run();
        let payload = vec![0xAB; PAYLOAD];
        let started = Instant::now();
        for ordinal in 1..=EVENTS {
            authority
                .apply(HostObservation {
                    run_id: run,
                    binding_generation: generation,
                    ordinal,
                    kind: HostObservationKind::Result(payload.clone()),
                })
                .unwrap();
        }
        let fill = started.elapsed();
        assert_eq!(authority.next_expected_ordinal(run), EVENTS + 1);
        assert_eq!(authority.applied_count(), EVENTS as usize);

        let probe = Instant::now();
        for _ in 0..2_048 {
            assert_eq!(
                authority.apply(HostObservation {
                    run_id: run,
                    binding_generation: generation,
                    ordinal: EVENTS + 2,
                    kind: HostObservationKind::KnownSuccess,
                }),
                Err(ObserveError::OutOfOrder)
            );
            assert_eq!(authority.next_expected_ordinal(run), EVENTS + 1);
        }
        let probe_elapsed = probe.elapsed();
        // Cursor lookup is O(1). A returned linear key scan over 8k entries for
        // each probe would dominate; keep a generous absolute ceiling for CI.
        assert!(
            probe_elapsed.as_millis() < 500,
            "next-ordinal probes must stay bounded with large retained history: fill={fill:?} probe={probe_elapsed:?}"
        );

        authority
            .apply(HostObservation {
                run_id: run,
                binding_generation: generation,
                ordinal: EVENTS + 1,
                kind: HostObservationKind::KnownSuccess,
            })
            .unwrap();
        assert_eq!(authority.liveness(run), RunLiveness::KnownTerminated);
        assert_eq!(authority.next_expected_ordinal(run), EVENTS + 2);
    }

    #[test]
    fn mark_recovered_honors_committed_terminal_and_unknown_otherwise() {
        let (mut authority, run, generation) = authority_with_run();
        authority
            .apply(HostObservation {
                run_id: run,
                binding_generation: generation,
                ordinal: 1,
                kind: HostObservationKind::KnownSuccess,
            })
            .unwrap();
        assert_eq!(authority.liveness(run), RunLiveness::KnownTerminated);
        authority.mark_recovered(run);
        assert_eq!(
            authority.liveness(run),
            RunLiveness::KnownTerminated,
            "committed terminal must survive recovery classification"
        );

        let (mut live, live_run, live_generation) = authority_with_run();
        live.apply(HostObservation {
            run_id: live_run,
            binding_generation: live_generation,
            ordinal: 1,
            kind: HostObservationKind::Started,
        })
        .unwrap();
        live.mark_recovered(live_run);
        assert_eq!(live.liveness(live_run), RunLiveness::UnknownAfterCrash);

        let (mut noted, noted_run, _) = authority_with_run();
        noted.note_committed_terminal(noted_run);
        noted.mark_recovered(noted_run);
        assert_eq!(noted.liveness(noted_run), RunLiveness::KnownTerminated);
    }

    #[test]
    fn undo_apply_rewinds_next_ordinal_cursor() {
        let (mut authority, run, generation) = authority_with_run();
        let observation = HostObservation {
            run_id: run,
            binding_generation: generation,
            ordinal: 1,
            kind: HostObservationKind::Output(vec![1, 2, 3, 4]),
        };
        let previous_liveness = authority.recorded_liveness(run);
        let previous_effects = authority.effects_performed();
        authority.apply(observation.clone()).unwrap();
        assert_eq!(authority.next_expected_ordinal(run), 2);
        authority.undo_apply(&observation, previous_liveness, previous_effects);
        assert_eq!(authority.next_expected_ordinal(run), 1);
        assert_eq!(authority.applied_count(), 0);
        authority.apply(observation).unwrap();
        assert_eq!(authority.next_expected_ordinal(run), 2);
    }
}
