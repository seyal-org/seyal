//! Composer draft/submit/history/Block projection apply paths.

use seyal_core::BlockId;

use super::*;
use crate::chrome::ChromeAction;
use crate::composer::{ComposerAction, RuntimeBlockRecord, RuntimeComposerEligibility};

impl ApplicationRoot {
    pub(super) fn composer_eligible_for(&self, eligibility: PresentationEligibility) -> bool {
        let pane = self.shell.snapshot().focused_pane;
        // #1379 Defect B: shell-exit is sticky per Pane. A global
        // RecoveryStage::ExecutionEnded must not disable a newly focused
        // leaf that never ended (CreateTab / Split / New shell recovery).
        eligibility == PresentationEligibility::Flow
            && !self.frozen
            && !self.composer.execution_ended(pane)
    }

    pub(super) fn sync_composer_presentation(&mut self) {
        let pane = self.shell.snapshot().focused_pane;
        let _ = self.composer.apply(ComposerAction::EnsurePane { pane });
        let (mode, input_route) = if self.authority.is_none() || self.frozen {
            (PresentationMode::Flow, InputRoute::Frozen)
        } else {
            let snap = self.presentation.snapshot();
            (snap.mode, snap.input_route)
        };
        let _ = self.composer.apply(ComposerAction::ApplyPresentation {
            pane,
            mode,
            input_route,
        });
        // Do not rebroadcast global ExecutionEnded onto the focused Pane.
        // SetExecutionEnded is latched in complete_recovery for the pane that
        // ended, and cleared when that pane binds a live execution again.
    }

    /// An ended execution is not still running a Block. Keep an observed exit
    /// status; a block the timeline left `Running` becomes Unknown (ADR-009
    /// mechanism 5) instead of `/ running`.
    ///
    /// Clamp a missing end to `start_line` so projection hands off to History
    /// instead of FailClosed (which collapses the body and leaves stale
    /// live-tail paint after clear → exit).
    pub(super) fn settle_running_blocks_after_exit(&mut self, pane: seyal_core::PaneId) {
        let Ok(snapshot) = self.composer.snapshot(pane) else {
            return;
        };
        if !snapshot
            .blocks
            .iter()
            .any(|block| block.state == crate::composer::BlockPresentationState::Running)
        {
            return;
        }
        let records = snapshot
            .blocks
            .into_iter()
            .map(|block| {
                let ended_without_status =
                    block.state == crate::composer::BlockPresentationState::Running;
                let end_line = block
                    .end_line
                    .or_else(|| (block.start_line > 0).then_some(block.start_line));
                RuntimeBlockRecord {
                    id: block.id,
                    command: block.command,
                    start_line: block.start_line,
                    end_line,
                    running: false,
                    exit_status: if ended_without_status {
                        None
                    } else {
                        block.exit_status
                    },
                }
            })
            .collect();
        let _ = self
            .composer
            .apply(ComposerAction::ApplyRuntimeBlocks { pane, records });
    }

    /// Latch shell-exit on one Pane. Callers pass the ended Pane, never the
    /// focused sibling (SPEC-009 §8.2.2).
    pub(super) fn latch_execution_ended_on_pane(&mut self, pane: seyal_core::PaneId) {
        let _ = self.composer.apply(ComposerAction::EnsurePane { pane });
        let _ = self
            .composer
            .apply(ComposerAction::SetExecutionEnded { pane, ended: true });
    }

    /// Clear the shell-exit latch after a Pane binds a live execution again.
    pub(super) fn clear_execution_ended_for_pane(&mut self, pane: seyal_core::PaneId) {
        let _ = self.composer.apply(ComposerAction::EnsurePane { pane });
        let _ = self
            .composer
            .apply(ComposerAction::SetExecutionEnded { pane, ended: false });
    }

    pub(super) fn set_composer_draft(
        &mut self,
        fence: AppFence,
        text: String,
        composer_epoch: u64,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        self.composer
            .apply(ComposerAction::SetDraft {
                pane: fence.pane,
                text,
                epoch: composer_epoch,
            })
            .map(|_| ())
            .map_err(composer_error)
    }

    pub(super) fn submit_composer(
        &mut self,
        fence: AppFence,
        composer_epoch: u64,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        self.composer
            .apply(ComposerAction::Submit {
                pane: fence.pane,
                epoch: composer_epoch,
            })
            .map(|_| ())
            .map_err(composer_error)
    }

    pub(super) fn apply_composer_result(
        &mut self,
        fence: AppFence,
        request_id: u64,
        accepted: bool,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        self.composer
            .apply(ComposerAction::ApplyResult {
                pane: fence.pane,
                request_id,
                accepted,
            })
            .map(|_| ())
            .map_err(composer_error)
    }

    pub(super) fn focused_blocks(&self) -> Vec<crate::composer::BlockProjection> {
        let pane = self.shell.snapshot().focused_pane;
        self.composer
            .snapshot(pane)
            .map(|composer| composer.blocks)
            .unwrap_or_default()
    }

    pub(super) fn select_block(&mut self, fence: AppFence, id: BlockId) -> Result<(), AppError> {
        self.require_fence(fence)?;
        let shell = self.shell.snapshot();
        let blocks = self.focused_blocks();
        self.chrome
            .apply(ChromeAction::SelectBlock { id, blocks }, &shell)
            .map(|_| ())
            .map_err(chrome_error)
    }

    /// History recall is fenced like every other pane-sensitive composer
    /// action: stale Pane/execution/attachment identity fails closed.
    pub(super) fn composer_history(
        &mut self,
        fence: AppFence,
        action: ComposerAction,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        self.composer
            .apply(action)
            .map(|_| ())
            .map_err(composer_error)
    }

    pub(super) fn apply_runtime_blocks(
        &mut self,
        fence: AppFence,
        records: Vec<RuntimeBlockRecord>,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        self.composer
            .apply(ComposerAction::ApplyRuntimeBlocks {
                pane: fence.pane,
                records,
            })
            .map(|_| ())
            .map_err(composer_error)?;
        // A selected Block that Runtime no longer lists is not inspectable.
        if self.chrome.selected_block_is_stale(&self.focused_blocks()) {
            let shell = self.shell.snapshot();
            let _ = self.chrome.apply(ChromeAction::ClearBlockSelection, &shell);
        }
        Ok(())
    }

    pub(super) fn apply_runtime_composer_status(
        &mut self,
        fence: AppFence,
        eligibility: Option<RuntimeComposerEligibility>,
        revision: u64,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        self.composer
            .apply(ComposerAction::ApplyRuntimeEligibility {
                pane: fence.pane,
                eligibility,
                revision,
            })
            .map(|_| ())
            .map_err(composer_error)?;
        if let Some(eligibility) = eligibility {
            self.integration_unsupported = eligibility == RuntimeComposerEligibility::Unsupported;
            self.recompute_resting();
            self.derive_presentation(self.alternate_screen)?;
        }
        Ok(())
    }
}
