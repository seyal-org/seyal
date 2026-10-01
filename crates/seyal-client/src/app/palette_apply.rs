//! Global command palette apply paths.

use super::*;
use crate::palette::{PaletteAction, PaletteCommand, PaletteSnapshot};

impl ApplicationRoot {
    pub(super) fn open_palette(&mut self, fence: AppFence) -> Result<(), AppError> {
        self.require_fence(fence)?;
        self.palette
            .apply(PaletteAction::Open, 0)
            .map_err(palette_error)
    }

    pub(super) fn set_palette_query(
        &mut self,
        fence: AppFence,
        query: String,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        self.palette
            .apply(PaletteAction::SetQuery(query), 0)
            .map_err(palette_error)
    }

    pub(super) fn move_palette_selection(
        &mut self,
        fence: AppFence,
        delta: i32,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        let row_count = self.palette_snapshot().rows.len();
        self.palette
            .apply(PaletteAction::MoveSelection(delta), row_count)
            .map_err(palette_error)
    }

    pub(super) fn close_palette(&mut self, fence: AppFence) -> Result<(), AppError> {
        self.require_fence(fence)?;
        self.palette
            .apply(PaletteAction::Close, 0)
            .map_err(palette_error)
    }

    /// Current palette rows from a fresh Shell/Chrome pass. Used only to
    /// clamp `MoveSelection`; the same pass happens again in `snapshot()`.
    pub(super) fn palette_snapshot(&self) -> PaletteSnapshot {
        let shell = self.shell.snapshot();
        let chrome = self.chrome.snapshot(&shell, &self.focused_blocks());
        self.palette.snapshot(
            &shell,
            &chrome,
            self.shell.allows_tab_creation(),
            self.shell.allows_pane_splitting(),
        )
    }

    pub(super) fn run_palette(&mut self, fence: AppFence) -> Result<(), AppError> {
        self.require_fence(fence)?;
        let shell = self.shell.snapshot();
        let chrome = self.chrome.snapshot(&shell, &self.focused_blocks());
        let command = self.palette.resolve(
            &shell,
            &chrome,
            self.shell.allows_tab_creation(),
            self.shell.allows_pane_splitting(),
        );
        let Some(command) = command else {
            return Err(palette_error(PaletteError::NoSelection));
        };
        self.palette.close();
        self.run_command(fence, command)
    }

    pub(super) fn run_command(
        &mut self,
        fence: AppFence,
        command: PaletteCommand,
    ) -> Result<(), AppError> {
        match command {
            PaletteCommand::CreateTab => self.create_tab(),
            PaletteCommand::SplitFocused(axis) => self.split_focused(axis),
            PaletteCommand::SwitchWorkspace(id) => self.select_workspace(id),
            PaletteCommand::SwitchTab(id) => self.select_tab(id),
            PaletteCommand::FocusPane(id) => self.focus_pane(id),
            PaletteCommand::SetLeftPanel(mode) => self.set_left_panel(mode),
            PaletteCommand::SetShellVisibility {
                left,
                inspector,
                tab_strip,
            } => self.set_shell_visibility(left, inspector, tab_strip),
            PaletteCommand::SetInspectorMode(mode) => self.set_inspector_mode(mode),
            PaletteCommand::OpenAttention(id) => self.open_attention(fence, id),
            PaletteCommand::FocusAgent(id) => self.select_agent(fence, id),
        }
    }
}
