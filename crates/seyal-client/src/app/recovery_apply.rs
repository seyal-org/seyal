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
        let effects = self
            .recovery
            .complete_attempt(generation, outcome, now, launch);
        if stale {
            // A late completion may only dispose its own handle; the current
            // episode's queued effects stay behind that disposal.
            let current = std::mem::take(&mut self.pending_recovery);
            self.pending_recovery = effects;
            self.pending_recovery.extend(current);
            return Err(AppError::StaleRecoveryGeneration);
        }
        self.pending_recovery = effects;
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

    pub(super) fn cancel_recovery(&mut self) -> Result<(), AppError> {
        self.recovery.cancel();
        self.pending_recovery.clear();
        Ok(())
    }

    pub(super) fn advance_recovery_stage(
        &mut self,
        stage: crate::recovery::RecoveryStage,
    ) -> Result<(), AppError> {
        if self.recovery.advance_presentation_stage(stage) {
            Ok(())
        } else {
            Err(AppError::InvalidPayload)
        }
    }
}
