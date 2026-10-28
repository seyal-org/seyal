//! Recovery coordinator apply paths for ApplicationRoot.

use std::time::Duration;

use seyal_core::{AttachmentId, ExecutionId, PaneId};

use super::*;
use crate::presentation::PresentationAction;
use crate::recovery::{AttemptOutcome, ContinuityIdentity, LaunchResult};
use crate::shell::ShellAction;

impl ApplicationRoot {
    pub(super) fn begin_recovery(&mut self, now: Duration) -> Result<(), AppError> {
        self.supersede_pane_recovery_for_primary();
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
        let had_outstanding = self.recovery.has_outstanding_attempt();
        let effects = self
            .recovery
            .complete_attempt(generation, outcome, now, launch);
        if stale || !had_outstanding {
            // Stale-generation or duplicate/late current-generation completion:
            // dispose only. Never replace a queued Schedule/PerformAttempt the
            // coordinator still expects the host to drain (ADR-015).
            let current = std::mem::take(&mut self.pending_recovery);
            self.pending_recovery = effects;
            self.pending_recovery.extend(current);
            if stale {
                return Err(AppError::StaleRecoveryGeneration);
            }
            return Ok(());
        }
        if outcome == AttemptOutcome::ExecutionEnded && self.pane_recovery.active().is_none() {
            // Latch the primary Pane, not whoever is focused. A sibling can
            // hold focus while the primary shell exits (SPEC-009 §8.2.2).
            // Keep shell bind and Flow authority so completed Block history
            // still projects. New shell releases that bind explicitly.
            let ended_pane = self
                .pane_recovery
                .primary_pane
                .unwrap_or(self.shell.snapshot().focused_pane);
            self.latch_execution_ended_on_pane(ended_pane);
            self.settle_running_blocks_after_exit(ended_pane);
            self.sync_composer_presentation();
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

    /// Host open budget: pending `PerformAttempt` remaining, or the claimed
    /// budget after Ack drained that effect.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn recovery_open_budget_millis(&self) -> u64 {
        match self.pending_recovery.first() {
            Some(RecoveryEffect::PerformAttempt { remaining, .. }) => {
                return remaining.as_millis() as u64;
            }
            Some(RecoveryEffect::Schedule { delay, .. }) => return delay.as_millis() as u64,
            Some(RecoveryEffect::DisposeHandle(handle)) => return *handle,
            Some(RecoveryEffect::LaunchHelper { generation }) => return *generation,
            None => {}
        }
        self.recovery
            .claimed_attempt_remaining()
            .map(|remaining| remaining.as_millis() as u64)
            .unwrap_or(0)
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

    pub(super) fn clear_reconstruction_continuity(&mut self) -> Result<(), AppError> {
        self.reconstruction.clear_continuity_pins();
        Ok(())
    }

    pub(crate) fn replacement_bootstrap_pending(&self) -> bool {
        self.replacement_bootstrap.is_some()
    }

    /// SPEC-004 §18 Create+Bind for the ended Pane. Prepare releases only that
    /// Pane. The bind phase uses the created ExecutionId, never `open_first`.
    pub(super) fn create_replacement_execution(
        &mut self,
        execution: Option<ExecutionId>,
        attachment: Option<AttachmentId>,
        pty_generation: u64,
    ) -> Result<(), AppError> {
        let pane = self.replacement_target_pane()?;
        match execution {
            None => self.prepare_replacement(pane),
            Some(execution) => {
                let attachment = attachment.ok_or(AppError::InvalidPayload)?;
                self.finish_replacement_bind(pane, execution, attachment, pty_generation)
            }
        }
    }

    fn replacement_target_pane(&self) -> Result<PaneId, AppError> {
        // The focused Pane is the one whose composer is hidden. A focused
        // sibling must not be released when the primary has ended.
        let focused = self.shell.snapshot().focused_pane;
        if self.composer.execution_ended(focused) {
            return Ok(focused);
        }
        Err(AppError::InvalidPayload)
    }

    fn prepare_replacement(&mut self, pane: PaneId) -> Result<(), AppError> {
        if !self.composer.execution_ended(pane) {
            return Err(AppError::InvalidPayload);
        }
        let _ = self.shell.release_execution(pane);
        self.provisioning.release_dead_binding(pane);
        self.pane_authorities.remove(&pane);
        if self
            .authority
            .is_some_and(|authority| authority.pane == pane)
        {
            self.authority = None;
            let _ = self.presentation.apply(PresentationAction::ClearIdentity);
            self.sync_composer_presentation();
        }
        self.reconstruction.clear_continuity_pins();
        self.replacement_bootstrap = Some(pane);
        Ok(())
    }

    fn finish_replacement_bind(
        &mut self,
        pane: PaneId,
        execution: ExecutionId,
        attachment: AttachmentId,
        pty_generation: u64,
    ) -> Result<(), AppError> {
        if self.replacement_bootstrap != Some(pane) {
            return Err(AppError::StalePane);
        }
        if pty_generation == 0 {
            return Err(AppError::ZeroPtyGeneration);
        }
        self.shell
            .apply(ShellAction::BindExecution { pane, execution })
            .map_err(|_| AppError::AlreadyBound)?;
        self.provisioning.record_adopted_binding(pane, execution);
        let evidence = BindingEvidence {
            execution,
            attachment,
            controller: true,
            pty_generation,
            alternate_screen: false,
        };
        self.install_pane_authority(pane, evidence)?;
        self.clear_execution_ended_for_pane(pane);
        self.replacement_bootstrap = None;
        if self.shell.snapshot().focused_pane == pane {
            self.recovery.mark_usable();
            self.sync_composer_presentation();
        }
        Ok(())
    }
}
