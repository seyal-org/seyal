//! Recovery coordinator apply paths for ApplicationRoot.

use std::time::Duration;

use super::*;
use crate::recovery::{AttemptOutcome, ContinuityIdentity, LaunchResult};

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
        // Keep DisposeHandle effects so a cancelled episode still drops any
        // opened-but-not-adopted client; hosts must not rely on drain order.
        self.pending_recovery
            .retain(|effect| matches!(effect, RecoveryEffect::DisposeHandle(_)));
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

    pub(super) fn begin_reconstruction_attempt(&mut self) -> Result<(), AppError> {
        self.reconstruction.begin_attempt();
        Ok(())
    }

    pub(super) fn commit_reconstruction(
        &mut self,
        runtime: ContinuityIdentity,
        execution: ContinuityIdentity,
        attachment: ContinuityIdentity,
        controller_authority_committed: bool,
        authoritative_snapshot_committed: bool,
    ) -> Result<(), AppError> {
        if self.reconstruction.commit(
            runtime,
            execution,
            attachment,
            controller_authority_committed,
            authoritative_snapshot_committed,
        ) {
            Ok(())
        } else {
            Err(AppError::InvalidPayload)
        }
    }

    pub(super) fn disconnect_reconstruction(&mut self) -> Result<(), AppError> {
        self.reconstruction.disconnect();
        Ok(())
    }
}
