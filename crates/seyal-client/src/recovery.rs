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

    pub fn expected_execution(&self) -> Option<ContinuityIdentity> {
        self.expected_execution
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

/// Classify a bridge open failure from `SeyalRecoveryResult` failure_class /
/// retryable bits. Hosts must not reinterpret these classes in Swift.
///
/// Class 1 is a missing leaf. Class 2 is refused/disappeared: a dead
/// `control.sock` looks like an unready listener, and only a Runtime
/// singleton contender may replace it (SPEC-009). Both map to the
/// one-launch-per-episode path; Rust launch-once accounting still prevents
/// a second spawn while a just-started helper binds the canonical endpoint.
pub fn classify_open_result(failure_class: u8, retryable: bool) -> AttemptOutcome {
    if !retryable {
        return AttemptOutcome::Blocked;
    }
    match failure_class {
        1 | 2 => AttemptOutcome::EndpointMissing,
        3 => AttemptOutcome::ControllerBusy,
        _ => AttemptOutcome::Retryable,
    }
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
                }
                // Hosts may report the failed launch after executing the
                // claimed LaunchHelper effect; it still blocks the episode.
                if launch == Some(LaunchResult::HelperMissing) {
                    self.scheduled = false;
                    self.deadline = None;
                    self.blocked_launch = true;
                    self.state.stage = RecoveryStage::Blocked;
                    return effects;
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
mod tests;
