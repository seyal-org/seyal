//! Portable Runtime lifecycle recovery policy.
//!
//! Owns retry delays, attempt budget, episode deadline, stage transitions, and
//! continuity-identity fencing. Does not own PTY, attachment, bundled-helper
//! launch APIs, or AppKit. Hosts inject clock/scheduler/attempt/launch.

use std::time::Duration;

pub const RETRY_DELAYS: [Duration; 6] = [
    Duration::from_millis(10),
    Duration::from_millis(20),
    Duration::from_millis(40),
    Duration::from_millis(80),
    Duration::from_millis(160),
    Duration::from_millis(250),
];

pub const MAXIMUM_ATTEMPTS: u32 = RETRY_DELAYS.len() as u32 + 1;
pub const EPISODE_DEADLINE: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RecoveryStage {
    #[default]
    Disconnected,
    Discovering,
    StartingRuntime,
    WaitingForController,
    Reconstructing,
    RestoringInteraction,
    Usable,
    Exhausted,
    Blocked,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecoveryState {
    pub stage: RecoveryStage,
    pub generation: u64,
}

impl RecoveryState {
    fn begin(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.stage = RecoveryStage::Discovering;
    }

    fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.stage = RecoveryStage::Disconnected;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContinuityIdentity {
    pub low: u64,
    pub high: u64,
}

impl ContinuityIdentity {
    pub const NONE: Self = Self { low: 0, high: 0 };

    pub fn is_valid(self) -> bool {
        self != Self::NONE
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReconstructionStage {
    #[default]
    Disconnected,
    AwaitingAuthoritativeSnapshot,
    Usable,
    BlockedIdentityMismatch,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReconstructionState {
    pub stage: ReconstructionStage,
    expected_runtime: Option<ContinuityIdentity>,
    expected_execution: Option<ContinuityIdentity>,
    last_attachment: Option<ContinuityIdentity>,
}

impl ReconstructionState {
    pub fn can_mutate(&self) -> bool {
        self.stage == ReconstructionStage::Usable
    }

    pub fn begin_attempt(&mut self) {
        self.stage = ReconstructionStage::AwaitingAuthoritativeSnapshot;
    }

    pub fn disconnect(&mut self) {
        self.stage = ReconstructionStage::Disconnected;
    }

    pub fn commit(
        &mut self,
        runtime: ContinuityIdentity,
        execution: ContinuityIdentity,
        attachment: ContinuityIdentity,
        controller_authority_committed: bool,
        authoritative_snapshot_committed: bool,
    ) -> bool {
        let identity_ok = runtime.is_valid()
            && execution.is_valid()
            && attachment.is_valid()
            && self
                .expected_runtime
                .map(|expected| expected == runtime)
                .unwrap_or(true)
            && self
                .expected_execution
                .map(|expected| expected == execution)
                .unwrap_or(true)
            && self
                .last_attachment
                .map(|previous| previous != attachment)
                .unwrap_or(true);
        if !identity_ok {
            self.stage = ReconstructionStage::BlockedIdentityMismatch;
            return false;
        }
        if !controller_authority_committed || !authoritative_snapshot_committed {
            self.stage = ReconstructionStage::AwaitingAuthoritativeSnapshot;
            return false;
        }
        self.expected_runtime = Some(runtime);
        self.expected_execution = Some(execution);
        self.last_attachment = Some(attachment);
        self.stage = ReconstructionStage::Usable;
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttemptOutcome {
    Connected,
    Opened { handle: u64, adopted: bool },
    EndpointMissing,
    Retryable,
    ControllerBusy,
    Blocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchResult {
    Started,
    HelperMissing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryEffect {
    PerformAttempt {
        generation: u64,
        remaining: Duration,
    },
    Schedule {
        generation: u64,
        delay: Duration,
    },
    /// Hosts must drop this effect when `generation` is not the current episode.
    LaunchHelper {
        generation: u64,
    },
    DisposeHandle(u64),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecoveryCoordinator {
    state: RecoveryState,
    deadline: Option<Duration>,
    launch_claimed: bool,
    attempt_count: u32,
    scheduled: bool,
    blocked_launch: bool,
}

impl RecoveryCoordinator {
    pub fn state(&self) -> RecoveryState {
        self.state
    }

    pub fn attempt_count(&self) -> u32 {
        self.attempt_count
    }

    pub fn has_scheduled_attempt(&self) -> bool {
        self.scheduled
    }

    pub fn is_active(&self) -> bool {
        self.deadline.is_some()
    }

    pub fn blocked_launch(&self) -> bool {
        self.blocked_launch
    }

    pub fn begin_episode(&mut self, now: Duration) -> Vec<RecoveryEffect> {
        self.replace_generation(RecoveryStage::Discovering);
        self.deadline = Some(now.saturating_add(EPISODE_DEADLINE));
        self.launch_claimed = false;
        self.blocked_launch = false;
        self.attempt_count = 0;
        self.perform_attempt(self.state.generation, now)
    }

    pub fn cancel(&mut self) {
        self.scheduled = false;
        self.deadline = None;
        self.launch_claimed = false;
        self.blocked_launch = false;
        self.attempt_count = 0;
        self.state.cancel();
    }

    /// Host reports presentation progress after connect (SPEC-009 §10).
    /// Only RestoringInteraction and Usable are accepted, and only after
    /// Reconstructing (or RestoringInteraction → Usable).
    pub fn advance_presentation_stage(&mut self, stage: RecoveryStage) -> bool {
        match (self.state.stage, stage) {
            (RecoveryStage::Reconstructing, RecoveryStage::RestoringInteraction)
            | (RecoveryStage::Reconstructing, RecoveryStage::Usable)
            | (RecoveryStage::RestoringInteraction, RecoveryStage::Usable) => {
                self.state.stage = stage;
                true
            }
            _ => false,
        }
    }

    pub fn scheduled_fire(&mut self, generation: u64, now: Duration) -> Vec<RecoveryEffect> {
        if generation != self.state.generation {
            return Vec::new();
        }
        self.scheduled = false;
        self.perform_attempt(generation, now)
    }

    pub fn complete_attempt(
        &mut self,
        generation: u64,
        outcome: AttemptOutcome,
        now: Duration,
        launch: Option<LaunchResult>,
    ) -> Vec<RecoveryEffect> {
        if generation != self.state.generation {
            return self.dispose_opened(outcome);
        }
        let Some(deadline) = self.deadline else {
            return self.dispose_opened(outcome);
        };
        if now >= deadline {
            let mut effects = self.dispose_opened(outcome);
            effects.extend(self.exhaust(generation));
            return effects;
        }
        match outcome {
            AttemptOutcome::Connected => self.finish_connected(generation),
            AttemptOutcome::Opened { handle, adopted } => {
                if !adopted {
                    self.deadline = None;
                    self.scheduled = false;
                    self.state.stage = RecoveryStage::Blocked;
                    vec![RecoveryEffect::DisposeHandle(handle)]
                } else {
                    self.finish_connected(generation)
                }
            }
            AttemptOutcome::EndpointMissing => {
                self.state.stage = RecoveryStage::StartingRuntime;
                let mut effects = Vec::new();
                if !self.launch_claimed {
                    self.launch_claimed = true;
                    effects.push(RecoveryEffect::LaunchHelper { generation });
                    if launch == Some(LaunchResult::HelperMissing) {
                        self.scheduled = false;
                        self.deadline = None;
                        self.blocked_launch = true;
                        self.state.stage = RecoveryStage::Blocked;
                        return effects;
                    }
                }
                effects.extend(self.schedule_retry(generation, now));
                effects
            }
            AttemptOutcome::ControllerBusy => {
                self.state.stage = RecoveryStage::WaitingForController;
                self.schedule_retry(generation, now)
            }
            AttemptOutcome::Retryable => {
                self.state.stage = RecoveryStage::Discovering;
                self.schedule_retry(generation, now)
            }
            AttemptOutcome::Blocked => {
                self.scheduled = false;
                self.deadline = None;
                self.state.stage = RecoveryStage::Blocked;
                Vec::new()
            }
        }
    }

    fn replace_generation(&mut self, stage: RecoveryStage) {
        self.scheduled = false;
        self.deadline = None;
        self.state.begin();
        self.state.stage = stage;
    }

    fn perform_attempt(&mut self, generation: u64, now: Duration) -> Vec<RecoveryEffect> {
        if generation != self.state.generation {
            return Vec::new();
        }
        let Some(deadline) = self.deadline else {
            return Vec::new();
        };
        self.scheduled = false;
        if now >= deadline || self.attempt_count >= MAXIMUM_ATTEMPTS {
            return self.exhaust(generation);
        }
        self.attempt_count += 1;
        let remaining = deadline.saturating_sub(now);
        vec![RecoveryEffect::PerformAttempt {
            generation,
            remaining,
        }]
    }

    fn schedule_retry(&mut self, generation: u64, now: Duration) -> Vec<RecoveryEffect> {
        if generation != self.state.generation {
            return Vec::new();
        }
        let Some(deadline) = self.deadline else {
            return Vec::new();
        };
        let retry_index = self.attempt_count.saturating_sub(1) as usize;
        if retry_index >= RETRY_DELAYS.len() {
            return self.exhaust(generation);
        }
        let delay = RETRY_DELAYS[retry_index];
        if now.saturating_add(delay) >= deadline {
            return self.exhaust(generation);
        }
        self.scheduled = true;
        vec![RecoveryEffect::Schedule { generation, delay }]
    }

    fn finish_connected(&mut self, generation: u64) -> Vec<RecoveryEffect> {
        if generation != self.state.generation {
            return Vec::new();
        }
        self.scheduled = false;
        self.deadline = None;
        self.state.stage = RecoveryStage::Reconstructing;
        Vec::new()
    }

    fn exhaust(&mut self, generation: u64) -> Vec<RecoveryEffect> {
        if generation != self.state.generation {
            return Vec::new();
        }
        self.scheduled = false;
        self.deadline = None;
        self.state.stage = RecoveryStage::Exhausted;
        Vec::new()
    }

    fn dispose_opened(&self, outcome: AttemptOutcome) -> Vec<RecoveryEffect> {
        match outcome {
            AttemptOutcome::Opened { handle, .. } => vec![RecoveryEffect::DisposeHandle(handle)],
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drive_retryable(coordinator: &mut RecoveryCoordinator) -> Vec<Duration> {
        let mut now = Duration::ZERO;
        let mut delays = Vec::new();
        let mut effects = coordinator.begin_episode(now);
        let mut attempts = 0;
        loop {
            let next = match effects.as_slice() {
                [RecoveryEffect::PerformAttempt { generation, .. }] => {
                    let generation = *generation;
                    attempts += 1;
                    coordinator.complete_attempt(generation, AttemptOutcome::Retryable, now, None)
                }
                [RecoveryEffect::Schedule { generation, delay }] => {
                    let generation = *generation;
                    let delay = *delay;
                    delays.push(delay);
                    now += delay;
                    coordinator.scheduled_fire(generation, now)
                }
                [] => break,
                other => panic!("unexpected effects {other:?}"),
            };
            effects = next;
            if attempts > 20 {
                panic!("did not converge");
            }
        }
        assert_eq!(attempts, MAXIMUM_ATTEMPTS);
        delays
    }

    #[test]
    fn seven_attempt_schedule_exhausts() {
        let mut coordinator = RecoveryCoordinator::default();
        let delays = drive_retryable(&mut coordinator);
        assert_eq!(delays.as_slice(), &RETRY_DELAYS);
        assert_eq!(coordinator.attempt_count(), MAXIMUM_ATTEMPTS);
        assert_eq!(coordinator.state().stage, RecoveryStage::Exhausted);
        assert!(!coordinator.has_scheduled_attempt());
    }

    #[test]
    fn endpoint_missing_launches_once_controller_busy_never_launches() {
        let mut missing = RecoveryCoordinator::default();
        let mut now = Duration::ZERO;
        let mut launches = 0;
        let mut effects = missing.begin_episode(now);
        for _ in 0..20 {
            let next = match effects.as_slice() {
                [RecoveryEffect::PerformAttempt { generation, .. }] => {
                    let generation = *generation;
                    missing.complete_attempt(
                        generation,
                        AttemptOutcome::EndpointMissing,
                        now,
                        Some(LaunchResult::Started),
                    )
                }
                [RecoveryEffect::LaunchHelper { .. }, RecoveryEffect::Schedule { generation, delay }] =>
                {
                    let generation = *generation;
                    let delay = *delay;
                    launches += 1;
                    now += delay;
                    missing.scheduled_fire(generation, now)
                }
                [RecoveryEffect::Schedule { generation, delay }] => {
                    let generation = *generation;
                    let delay = *delay;
                    now += delay;
                    missing.scheduled_fire(generation, now)
                }
                [] => break,
                other => panic!("unexpected {other:?}"),
            };
            effects = next;
        }
        assert_eq!(launches, 1);
        assert_eq!(missing.state().stage, RecoveryStage::Exhausted);

        let mut busy = RecoveryCoordinator::default();
        now = Duration::ZERO;
        effects = busy.begin_episode(now);
        let RecoveryEffect::PerformAttempt {
            generation: busy_generation,
            ..
        } = effects[0]
        else {
            panic!("expected attempt");
        };
        effects = busy.complete_attempt(busy_generation, AttemptOutcome::ControllerBusy, now, None);
        assert_eq!(busy.state().stage, RecoveryStage::WaitingForController);
        while let Some((generation, delay)) = match effects.as_slice() {
            [RecoveryEffect::Schedule { generation, delay }] => Some((*generation, *delay)),
            _ => None,
        } {
            now += delay;
            effects = busy.scheduled_fire(generation, now);
            if let [RecoveryEffect::PerformAttempt { generation, .. }] = effects.as_slice() {
                let generation = *generation;
                effects =
                    busy.complete_attempt(generation, AttemptOutcome::ControllerBusy, now, None);
            }
        }
        assert_eq!(launches, 1);
        assert_eq!(busy.attempt_count(), MAXIMUM_ATTEMPTS);
        assert_eq!(busy.state().stage, RecoveryStage::Exhausted);
    }

    #[test]
    fn begin_episode_replaces_generation_and_cancels_prior_schedule() {
        let mut coordinator = RecoveryCoordinator::default();
        let first = coordinator.begin_episode(Duration::ZERO);
        let RecoveryEffect::PerformAttempt {
            generation: first_generation,
            ..
        } = first[0]
        else {
            panic!("expected attempt");
        };
        coordinator.complete_attempt(
            first_generation,
            AttemptOutcome::Retryable,
            Duration::ZERO,
            None,
        );
        assert!(coordinator.has_scheduled_attempt());
        let second = coordinator.begin_episode(Duration::ZERO);
        let RecoveryEffect::PerformAttempt {
            generation: second_generation,
            ..
        } = second[0]
        else {
            panic!("expected attempt");
        };
        assert!(second_generation > first_generation);
        assert_eq!(coordinator.attempt_count(), 1);
        assert!(!coordinator.has_scheduled_attempt());
        assert!(coordinator
            .scheduled_fire(first_generation, Duration::from_millis(10))
            .is_empty());
        let stale = coordinator.complete_attempt(
            first_generation,
            AttemptOutcome::Connected,
            Duration::from_millis(10),
            None,
        );
        assert!(stale.is_empty());
        assert_eq!(coordinator.state().generation, second_generation);
        assert_eq!(coordinator.state().stage, RecoveryStage::Discovering);

        let opened = coordinator.complete_attempt(
            first_generation,
            AttemptOutcome::Opened {
                handle: 77,
                adopted: true,
            },
            Duration::from_millis(10),
            None,
        );
        assert_eq!(opened, vec![RecoveryEffect::DisposeHandle(77)]);
        assert_eq!(coordinator.state().stage, RecoveryStage::Discovering);
    }

    #[test]
    fn rejected_opened_handle_is_disposed_and_blocked() {
        let mut coordinator = RecoveryCoordinator::default();
        let started = coordinator.begin_episode(Duration::ZERO);
        let RecoveryEffect::PerformAttempt { generation, .. } = started[0] else {
            panic!("expected attempt");
        };
        let effects = coordinator.complete_attempt(
            generation,
            AttemptOutcome::Opened {
                handle: 103,
                adopted: false,
            },
            Duration::ZERO,
            None,
        );
        assert_eq!(effects, vec![RecoveryEffect::DisposeHandle(103)]);
        assert_eq!(coordinator.state().stage, RecoveryStage::Blocked);
        assert!(!coordinator.has_scheduled_attempt());
    }

    #[test]
    fn helper_missing_blocks_without_scheduling() {
        let mut coordinator = RecoveryCoordinator::default();
        let started = coordinator.begin_episode(Duration::ZERO);
        let RecoveryEffect::PerformAttempt { generation, .. } = started[0] else {
            panic!("expected attempt");
        };
        let effects = coordinator.complete_attempt(
            generation,
            AttemptOutcome::EndpointMissing,
            Duration::ZERO,
            Some(LaunchResult::HelperMissing),
        );
        assert_eq!(effects, vec![RecoveryEffect::LaunchHelper { generation }]);
        assert_eq!(coordinator.state().stage, RecoveryStage::Blocked);
        assert!(coordinator.blocked_launch());
        assert!(!coordinator.has_scheduled_attempt());
    }

    #[test]
    fn stale_launch_helper_carries_superseded_generation() {
        let mut coordinator = RecoveryCoordinator::default();
        let started = coordinator.begin_episode(Duration::ZERO);
        let RecoveryEffect::PerformAttempt {
            generation: first_generation,
            ..
        } = started[0]
        else {
            panic!("expected attempt");
        };
        let launched = coordinator.complete_attempt(
            first_generation,
            AttemptOutcome::EndpointMissing,
            Duration::ZERO,
            Some(LaunchResult::Started),
        );
        assert!(launched.contains(&RecoveryEffect::LaunchHelper {
            generation: first_generation
        }));
        let second = coordinator.begin_episode(Duration::from_millis(10));
        let RecoveryEffect::PerformAttempt {
            generation: second_generation,
            ..
        } = second[0]
        else {
            panic!("expected attempt");
        };
        assert_ne!(first_generation, second_generation);
        assert!(!launched.iter().any(|effect| matches!(
            effect,
            RecoveryEffect::LaunchHelper { generation } if *generation == second_generation
        )));
    }

    #[test]
    fn reconstruction_pins_runtime_execution_and_requires_fresh_attachment() {
        let runtime = ContinuityIdentity { low: 1, high: 2 };
        let execution = ContinuityIdentity { low: 3, high: 4 };
        let first = ContinuityIdentity { low: 5, high: 6 };
        let second = ContinuityIdentity { low: 7, high: 8 };
        let mut state = ReconstructionState::default();
        state.begin_attempt();
        assert!(!state.can_mutate());
        assert!(state.commit(runtime, execution, first, true, true));
        assert!(state.can_mutate());
        state.disconnect();
        state.begin_attempt();
        assert!(state.commit(runtime, execution, second, true, true));
        state.disconnect();
        state.begin_attempt();
        assert!(!state.commit(
            ContinuityIdentity { low: 99, high: 2 },
            execution,
            ContinuityIdentity { low: 9, high: 10 },
            true,
            true
        ));
        assert_eq!(state.stage, ReconstructionStage::BlockedIdentityMismatch);
        assert!(!state.can_mutate());
    }

    #[test]
    fn reconstruction_rejects_interrupted_snapshot_and_old_attachment() {
        let runtime = ContinuityIdentity { low: 1, high: 2 };
        let execution = ContinuityIdentity { low: 3, high: 4 };
        let attachment = ContinuityIdentity { low: 5, high: 6 };
        let mut state = ReconstructionState::default();
        state.begin_attempt();
        assert!(!state.commit(runtime, execution, attachment, true, false));
        assert_eq!(
            state.stage,
            ReconstructionStage::AwaitingAuthoritativeSnapshot
        );
        assert!(state.commit(runtime, execution, attachment, true, true));
        state.disconnect();
        state.begin_attempt();
        assert!(!state.commit(runtime, execution, attachment, true, true));
        assert_eq!(state.stage, ReconstructionStage::BlockedIdentityMismatch);
    }


    #[test]
    fn advance_presentation_stage_after_connect() {
        let mut c = RecoveryCoordinator::default();
        let now = Duration::ZERO;
        let _ = c.begin_episode(now);
        let generation = c.state().generation;
        let _ = c.complete_attempt(generation, AttemptOutcome::Connected, now, None);
        assert_eq!(c.state().stage, RecoveryStage::Reconstructing);
        assert!(c.advance_presentation_stage(RecoveryStage::RestoringInteraction));
        assert_eq!(c.state().stage, RecoveryStage::RestoringInteraction);
        assert!(c.advance_presentation_stage(RecoveryStage::Usable));
        assert_eq!(c.state().stage, RecoveryStage::Usable);
        assert!(!c.advance_presentation_stage(RecoveryStage::Discovering));
    }

    #[test]
    fn cancel_clears_active_episode() {
        let mut c = RecoveryCoordinator::default();
        let _ = c.begin_episode(Duration::ZERO);
        assert!(c.is_active());
        c.cancel();
        assert!(!c.is_active());
        assert_eq!(c.state().stage, RecoveryStage::Disconnected);
    }

}
