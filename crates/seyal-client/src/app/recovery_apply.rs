//! Recovery coordinator apply paths for ApplicationRoot.

use std::time::Duration;

use super::*;
use crate::recovery::{AttemptOutcome, LaunchResult};

impl ApplicationRoot {
    pub(super) fn begin_recovery(&mut self, now: Duration) -> Result<(), AppError> {
        self.pending_recovery = self.recovery.begin_episode(now);
        Ok(())
    }

    pub(super) fn complete_recovery(
        &mut self,
        generation: u64,
        outcome: AttemptOutcome,
        now: Duration,
        launch: Option<LaunchResult>,
    ) -> Result<(), AppError> {
        let stale = generation != self.recovery.state().generation;
        self.pending_recovery = self
            .recovery
            .complete_attempt(generation, outcome, now, launch);
        if stale {
            return Err(AppError::StaleRecoveryGeneration);
        }
        Ok(())
    }

    pub(super) fn fire_scheduled_recovery(
        &mut self,
        generation: u64,
        now: Duration,
    ) -> Result<(), AppError> {
        if generation != self.recovery.state().generation {
            return self.fail(AppError::StaleRecoveryGeneration);
        }
        self.pending_recovery = self.recovery.scheduled_fire(generation, now);
        Ok(())
    }

    pub(super) fn ack_recovery_effect(&mut self) -> Result<(), AppError> {
        if !self.pending_recovery.is_empty() {
            self.pending_recovery.remove(0);
        }
        Ok(())
    }
}
