//! Global command palette apply paths.

use super::*;
use crate::navigation::{
    navigate, EmptyExecutionInventory, NavigationPrincipal, NavigationRejection, ResourceAddress,
};
use crate::palette::{PaletteAction, PaletteCommand, PaletteRunTarget};

impl ApplicationRoot {
    pub(super) fn open_palette(&mut self, fence: AppFence) -> Result<(), AppError> {
        self.require_fence(fence)?;
        // Mutually exclusive with goto; one overlay surface.
        self.goto.close();
        self.palette
            .apply(PaletteAction::Open, 0)
            .map_err(palette_error)?;
        self.rebuild_palette();
        Ok(())
    }

    pub(super) fn set_palette_query(
        &mut self,
        fence: AppFence,
        query: String,
    ) -> Result<(), AppError> {
        // Overlay reuse: query keystrokes hit the open surface.
        if self.goto.is_open() {
            return self.set_goto_query(fence, query);
        }
        self.require_fence(fence)?;
        self.palette
            .apply(PaletteAction::SetQuery(query), 0)
            .map_err(palette_error)?;
        self.rebuild_palette();
        Ok(())
    }

    pub(super) fn move_palette_selection(
        &mut self,
        fence: AppFence,
        delta: i32,
    ) -> Result<(), AppError> {
        if self.goto.is_open() {
            return self.move_goto_selection(fence, delta);
        }
        self.require_fence(fence)?;
        let row_count = self.palette.snapshot().rows.len();
        self.palette
            .apply(PaletteAction::MoveSelection(delta), row_count)
            .map_err(palette_error)
    }

    pub(super) fn close_palette(&mut self, fence: AppFence) -> Result<(), AppError> {
        if self.goto.is_open() {
            return self.close_goto(fence);
        }
        self.require_fence(fence)?;
        self.palette
            .apply(PaletteAction::Close, 0)
            .map_err(palette_error)
    }

    fn rebuild_palette(&mut self) {
        let shell = self.shell.snapshot();
        let chrome = self.chrome.snapshot(&shell, &self.focused_blocks());
        self.palette.rebuild(
            &shell,
            &chrome,
            self.shell.allows_tab_creation(),
            self.shell.allows_pane_splitting(),
        );
    }

    /// Run the selected palette row. When `address` is provided (host echo of
    /// a navigation row), Navigate that address. Otherwise use the frozen
    /// selected target. Never rebuilds and rebinds by ordinal.
    pub(super) fn run_palette(
        &mut self,
        fence: AppFence,
        address: Option<ResourceAddress>,
    ) -> Result<(), AppError> {
        if self.goto.is_open() {
            return self.run_goto(fence, address);
        }
        self.require_fence(fence)?;
        let target = match address {
            Some(address) => PaletteRunTarget::Navigate(address),
            None => self
                .palette
                .selected_target()
                .ok_or(palette_error(PaletteError::NoSelection))?,
        };
        match target {
            PaletteRunTarget::Navigate(address) => {
                // Rejected Navigate leaves focus and palette open (R4.2 / R8.2).
                self.navigate_address(address)?;
                self.palette.close();
                Ok(())
            }
            PaletteRunTarget::Command(command) => {
                self.palette.close();
                self.run_command(fence, command)
            }
        }
    }

    pub(super) fn navigate_address(&mut self, address: ResourceAddress) -> Result<(), AppError> {
        let prior_window = self.shell.active_window_id();
        navigate(
            address,
            &mut self.shell,
            &EmptyExecutionInventory,
            NavigationPrincipal::local_user(),
        )
        .map_err(navigation_error)?;
        let after_window = self.shell.active_window_id();
        self.drain_shell_effects();
        if after_window != prior_window {
            // Navigate emitted exactly one WindowActivation for Rust's placement.
            self.begin_window_activation_episode(after_window);
        } else {
            self.clear_window_activation_episode();
        }
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn run_command(
        &mut self,
        fence: AppFence,
        command: PaletteCommand,
    ) -> Result<(), AppError> {
        match command {
            PaletteCommand::CreateTab => self.create_tab(),
            PaletteCommand::SplitFocused(axis) => self.split_focused(axis),
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

pub(super) fn navigation_error(error: NavigationRejection) -> AppError {
    match error {
        NavigationRejection::UnsupportedKind => AppError::NavigationUnsupportedKind,
        NavigationRejection::NavigationDenied => AppError::NavigationDenied,
        NavigationRejection::UnknownWorkspace => AppError::NavigationUnknownWorkspace,
        NavigationRejection::UnknownTab => AppError::NavigationUnknownTab,
        NavigationRejection::UnknownPane => AppError::NavigationUnknownPane,
        NavigationRejection::UnknownExecution => AppError::NavigationUnknownExecution,
        NavigationRejection::NotComposed => AppError::NavigationNotComposed,
        NavigationRejection::TargetTerminated => AppError::NavigationTargetTerminated,
        NavigationRejection::TargetUnbound => AppError::NavigationTargetUnbound,
        NavigationRejection::AmbiguousTarget => AppError::NavigationAmbiguousTarget,
    }
}
