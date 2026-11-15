//! Resting Raw/Flow selection and canonical TUI takeover (#867, #1438/#1440).
//!
//! Alternate-screen evidence selects TUI. Otherwise the Pane uses grid-first
//! resting Raw by default, explicit Flow when the user opts in and integration
//! allows it, or forced Raw when Runtime reports unsupported shell
//! integration (SPEC-008). The same ExecutionId and PTY generation are kept
//! across transitions. Leaving TUI restores that resting mode. Shell-integration
//! status alone must not promote Raw → Flow while Flow is quarantined.

use super::*;

impl ApplicationRoot {
    /// Keep every pane in the Tab on the conventional Raw resting surface.
    /// TUI remains the visible takeover while this updates the stored choice.
    pub(super) fn select_split_tab_raw_resting_mode(&mut self, pane: seyal_core::PaneId) {
        let snap = self.shell.snapshot();
        if let Some(tab) = snap
            .windows
            .iter()
            .flat_map(|window| &window.tabs)
            .find(|tab| tab.panes.iter().any(|candidate| candidate.id == pane))
        {
            for candidate in &tab.panes {
                let _ = self
                    .shell
                    .set_resting_presentation_mode(candidate.id, PresentationMode::Raw);
            }
        }
    }

    /// Resolve presentation independently from composer visibility. Composer
    /// mode is Hidden for several reasons (including an unfocused pane and an
    /// ended execution); it is not a valid proxy for the pane's selected
    /// Raw/Flow/TUI mode.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn presentation_eligibility_for_pane(
        &self,
        pane: seyal_core::PaneId,
    ) -> PresentationEligibility {
        if pane == self.shell.snapshot().focused_pane {
            // A pane becomes focused before its new Controller is attached.
            // During that interval `ApplicationSnapshot.eligibility` still
            // describes the previous pane's bound execution. Do not project
            // that stale Flow/Raw state into the new pane.
            if self.pane_authorities.get(&pane).copied() != self.authority {
                return PresentationEligibility::Unbound;
            }
            return self.snapshot().eligibility;
        }
        if !self.pane_authorities.contains_key(&pane) {
            return PresentationEligibility::Unbound;
        }
        let Some(mode) = self
            .shell
            .snapshot()
            .windows
            .iter()
            .flat_map(|window| &window.tabs)
            .flat_map(|tab| &tab.panes)
            .find(|candidate| candidate.id == pane)
            .map(|candidate| candidate.presentation_mode)
        else {
            return PresentationEligibility::Unbound;
        };
        match mode {
            PresentationMode::Flow => PresentationEligibility::Flow,
            PresentationMode::Raw => PresentationEligibility::Raw,
            PresentationMode::Tui => PresentationEligibility::Tui,
        }
    }

    pub(super) fn derive_presentation(&mut self, alternate_screen: bool) -> Result<(), AppError> {
        self.alternate_screen = alternate_screen;
        let Some(bound) = self.authority else {
            self.sync_composer_presentation();
            return Ok(());
        };
        let desired = if alternate_screen {
            PresentationMode::Tui
        } else {
            PresentationMode::Raw
        };
        let current = self.presentation.snapshot();
        if current.mode == desired {
            self.sync_focused_presentation_mode();
            self.sync_composer_presentation();
            return Ok(());
        }
        let identity = PresentationIdentity::new(bound.execution, bound.pty_generation)
            .ok_or(AppError::ZeroPtyGeneration)?;
        self.presentation
            .apply(PresentationAction::Transition {
                mode: desired,
                identity,
                explicit: match desired {
                    PresentationMode::Flow => self.explicit_flow,
                    PresentationMode::Raw => !self.explicit_flow,
                    PresentationMode::Tui => false,
                },
                epoch: current.epoch,
            })
            .map_err(|_| AppError::StalePresentationEpoch)?;
        self.sync_focused_presentation_mode();
        self.clear_chord_prefix();
        self.sync_composer_presentation();
        Ok(())
    }

    pub(super) fn recompute_resting(&mut self) {
        self.resting = PresentationMode::Raw;
    }

    pub(super) fn select_resting_presentation(
        &mut self,
        fence: AppFence,
        _raw: bool,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        if self.authority.is_none() {
            return Err(AppError::UnboundUnauthorized);
        }
        // Flow/Blocks are suspended. Any stale menu or keybinding action is
        // resolved to the only supported resting presentation.
        self.explicit_flow = false;
        self.resting = PresentationMode::Raw;
        self.derive_presentation(self.alternate_screen)
    }

    /// Blocks are suspended; the palette must not offer a Flow opt-in.
    pub(super) fn resting_palette_choice(&self) -> Option<bool> {
        None
    }

    pub(super) fn sync_focused_presentation_mode(&mut self) {
        let pane = self.shell.snapshot().focused_pane;
        let mode = self.presentation.snapshot().mode;
        let _ = self.shell.set_presentation_mode(pane, mode);
        let temporary_raw = mode == PresentationMode::Raw
            && self
                .interactive_raw_target
                .is_some_and(|(target_pane, _, _, _)| target_pane == pane);
        if mode != PresentationMode::Tui && !temporary_raw {
            let _ = self.shell.set_resting_presentation_mode(pane, mode);
        }
    }
}
