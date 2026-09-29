//! Resting Flow/Raw selection and canonical TUI takeover (#867).
//!
//! Alternate-screen evidence selects TUI. Otherwise the Pane uses explicit
//! Raw, full-Pane Raw when Runtime reports unsupported shell integration
//! (SPEC-008), or Flow. The same ExecutionId and PTY generation are kept
//! across transitions. Leaving TUI restores that resting mode.

use super::*;

impl ApplicationRoot {
    pub(super) fn derive_presentation(&mut self, alternate_screen: bool) -> Result<(), AppError> {
        self.alternate_screen = alternate_screen;
        let Some(bound) = self.authority else {
            self.sync_composer_presentation();
            return Ok(());
        };
        let desired = if alternate_screen {
            PresentationMode::Tui
        } else {
            self.resting
        };
        let current = self.presentation.snapshot();
        if current.mode == desired {
            self.sync_composer_presentation();
            return Ok(());
        }
        let identity = PresentationIdentity::new(bound.execution, bound.pty_generation)
            .ok_or(AppError::ZeroPtyGeneration)?;
        self.presentation
            .apply(PresentationAction::Transition {
                mode: desired,
                identity,
                explicit: self.explicit_raw && desired == PresentationMode::Raw,
                epoch: current.epoch,
            })
            .map_err(|_| AppError::StalePresentationEpoch)?;
        self.sync_composer_presentation();
        Ok(())
    }

    pub(super) fn recompute_resting(&mut self) {
        self.resting = if self.explicit_raw || self.integration_unsupported {
            PresentationMode::Raw
        } else {
            PresentationMode::Flow
        };
    }

    pub(super) fn select_resting_presentation(
        &mut self,
        fence: AppFence,
        raw: bool,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        if self.authority.is_none() {
            return Err(AppError::UnboundUnauthorized);
        }
        self.explicit_raw = raw;
        self.recompute_resting();
        self.derive_presentation(self.alternate_screen)
    }

    /// `Some` only while a Pane is bound. The bool is the explicit Raw latch.
    pub(super) fn resting_palette_choice(&self) -> Option<bool> {
        self.authority.map(|_| self.explicit_raw)
    }
}
