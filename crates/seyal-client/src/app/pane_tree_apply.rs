//! Thin ApplicationRoot adapters for PT1–PT4 pane-tree verbs (PT5).
//!
//! Rust owns zoom, swap, move, directional, and equalize policy. The host only
//! names ids/directions; stale or invalid refs fail closed.

use seyal_core::PaneId;

use super::{chrome_error, AppError, ApplicationRoot};
use crate::chrome::ChromeAction;
use crate::shell::{FocusDirection, MoveSide, ShellAction, ShellError};

impl ApplicationRoot {
    pub(super) fn zoom_pane(&mut self, id: PaneId) -> Result<(), AppError> {
        self.shell
            .apply(ShellAction::ZoomPane { id })
            .map_err(pane_tree_error)?;
        self.note_context_navigated()
    }

    pub(super) fn unzoom(&mut self) -> Result<(), AppError> {
        self.shell
            .apply(ShellAction::Unzoom)
            .map_err(pane_tree_error)?;
        self.note_context_navigated()
    }

    pub(super) fn swap_panes(&mut self, a: PaneId, b: PaneId) -> Result<(), AppError> {
        let containment_generation = self.shell.containment_generation();
        self.shell
            .apply(ShellAction::SwapPanes {
                a,
                b,
                containment_generation,
            })
            .map_err(pane_tree_error)?;
        self.note_context_navigated()
    }

    pub(super) fn move_pane_beside(
        &mut self,
        pane: PaneId,
        neighbor: PaneId,
        side: MoveSide,
    ) -> Result<(), AppError> {
        let containment_generation = self.shell.containment_generation();
        self.shell
            .apply(ShellAction::MovePaneBeside {
                pane,
                neighbor,
                side,
                containment_generation,
            })
            .map_err(pane_tree_error)?;
        self.note_context_navigated()
    }

    pub(super) fn focus_direction(&mut self, direction: FocusDirection) -> Result<(), AppError> {
        self.shell
            .apply(ShellAction::FocusDirection { direction })
            .map_err(pane_tree_error)?;
        self.note_context_navigated()
    }

    pub(super) fn equalize_focused(&mut self) -> Result<(), AppError> {
        let containment_generation = self.shell.containment_generation();
        self.shell
            .apply(ShellAction::EqualizeFocused {
                containment_generation,
            })
            .map_err(pane_tree_error)?;
        self.note_context_navigated()
    }

    pub(super) fn equalize_tab(&mut self) -> Result<(), AppError> {
        let containment_generation = self.shell.containment_generation();
        self.shell
            .apply(ShellAction::EqualizeTab {
                containment_generation,
            })
            .map_err(pane_tree_error)?;
        self.note_context_navigated()
    }

    fn note_context_navigated(&mut self) -> Result<(), AppError> {
        self.chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot())
            .map(|_| ())
            .map_err(chrome_error)
    }
}

fn pane_tree_error(error: ShellError) -> AppError {
    match error {
        ShellError::UnknownPane => AppError::UnknownPane,
        ShellError::NotZoomed => AppError::NotZoomed,
        ShellError::InvalidMoveTarget => AppError::InvalidMoveTarget,
        ShellError::NoDirectionalNeighbor => AppError::NoDirectionalNeighbor,
        ShellError::StaleContainment => AppError::StaleContainment,
        _ => AppError::UnknownPane,
    }
}
