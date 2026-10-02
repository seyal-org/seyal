//! Composer draft/submit/history/Block projection apply paths.

use seyal_core::BlockId;

use super::*;
use crate::chrome::ChromeAction;
use crate::composer::{ComposerAction, RuntimeBlockRecord, RuntimeComposerEligibility};

impl ApplicationRoot {
    pub(super) fn composer_eligible_for(&self, eligibility: PresentationEligibility) -> bool {
        eligibility == PresentationEligibility::Flow
            && !self.frozen
            && self.recovery.state().stage != crate::recovery::RecoveryStage::ExecutionEnded
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
        let _ = self.composer.apply(ComposerAction::SetExecutionEnded {
            pane,
            ended: self.recovery.state().stage == crate::recovery::RecoveryStage::ExecutionEnded,
        });
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

    /// Rerun (#1010): load a finished focused-Pane Block's Runtime-published
    /// command as the draft. Fails closed, leaving the draft untouched, unless
    /// the composer is Available (the same gate `block_actions` projects) and
    /// the draft is empty.
    pub(super) fn rerun_block(
        &mut self,
        fence: AppFence,
        id: BlockId,
        composer_epoch: u64,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        let block = self
            .focused_blocks()
            .into_iter()
            .find(|block| block.id == id)
            .ok_or(AppError::UnknownBlock)?;
        if block.state == crate::composer::BlockPresentationState::Running {
            return Err(AppError::BlockRunning);
        }
        let snap = self
            .composer
            .snapshot(fence.pane)
            .map_err(|_| AppError::ComposerUnavailable)?;
        if snap.mode != crate::composer::ComposerMode::Available {
            return Err(AppError::ComposerUnavailable);
        }
        if !snap.draft.is_empty() {
            return Err(AppError::ComposerDraftOccupied);
        }
        self.composer
            .apply(ComposerAction::SetDraft {
                pane: fence.pane,
                text: block.command,
                epoch: composer_epoch,
            })
            .map(|_| ())
            .map_err(composer_error)
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
            .map_err(composer_error)
    }
}
