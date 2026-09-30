//! SPEC-024 K3–K5: dispatch matched commands, own chord prefix wait, and read the projected table.

use crate::composer::ComposerAction;
use crate::keybinding::{
    process_keybinding_table, resolve_tab_ordinal, route_context_set, route_keystroke,
    validate_workspace_command, BindingContext, InvokeError, NormalizedStroke, RouteOutcome,
    WorkspaceCommand, WorkspaceCommandId,
};
use crate::presentation::{PresentationAction, PresentationMode};
use crate::shell::SplitAxis;
use std::time::Instant;

use super::{AppError, ApplicationRoot};

impl ApplicationRoot {
    /// Current §6.1 route context set from palette / presentation / composer focus.
    pub fn keybinding_route_context(&self, composer_first_responder: bool) -> BindingContext {
        route_context_set(
            self.palette.is_open(),
            self.presentation.snapshot().mode,
            composer_first_responder,
        )
    }

    /// Route one already-normalized stroke (§6.2 / §8). Matched commands are
    /// applied here so ApplicationCommand / prefix-wait paths write zero PTY bytes.
    pub fn route_normalized_keystroke(
        &mut self,
        stroke: &NormalizedStroke,
        composer_first_responder: bool,
        composition_active: bool,
    ) -> Result<RouteOutcome, AppError> {
        if composition_active {
            self.clear_chord_prefix();
        }
        let table = process_keybinding_table();
        let route = self.keybinding_route_context(composer_first_responder);
        let outcome = route_keystroke(
            table,
            stroke,
            route,
            composition_active,
            &mut self.chord_prefix,
            Instant::now(),
        );
        match outcome {
            RouteOutcome::Matched { command } => {
                self.invoke_workspace_command(command, route)?;
                self.last_error = None;
                self.snapshot_generation = self.snapshot_generation.saturating_add(1);
                Ok(outcome)
            }
            other => Ok(other),
        }
    }

    /// R6.4.1: re-validate a menu-invoked WorkspaceCommand against the route.
    pub fn validate_menu_workspace_command(
        &mut self,
        command: WorkspaceCommand,
        composer_first_responder: bool,
    ) -> Result<(), AppError> {
        self.clear_chord_prefix();
        let table = process_keybinding_table();
        let route = self.keybinding_route_context(composer_first_responder);
        validate_workspace_command(table, command, route).map_err(invoke_error)
    }

    /// Menu / key-equivalent path: validate against `route`, then dispatch.
    /// R8.4: menu entry clears any active chord prefix before dispatch.
    pub fn invoke_workspace_command_for_menu(
        &mut self,
        command: WorkspaceCommand,
        route: BindingContext,
    ) -> Result<(), AppError> {
        self.clear_chord_prefix();
        self.invoke_workspace_command(command, route)?;
        self.last_error = None;
        self.snapshot_generation = self.snapshot_generation.saturating_add(1);
        Ok(())
    }

    pub(super) fn invoke_workspace_command(
        &mut self,
        command: WorkspaceCommand,
        route: BindingContext,
    ) -> Result<(), AppError> {
        let table = process_keybinding_table();
        validate_workspace_command(table, command, route).map_err(invoke_error)?;
        let fence = self.fence();
        match command.id {
            WorkspaceCommandId::CommandPaletteOpen => self.open_palette(fence),
            WorkspaceCommandId::CommandPaletteClose => self.close_palette(fence),
            WorkspaceCommandId::TabCreate => self.create_tab(),
            WorkspaceCommandId::TabCloseFocused => {
                let id = self.shell.snapshot().active_tab;
                self.close_tab(id)
            }
            WorkspaceCommandId::TabSelectPrevious => self.select_tab_relative(-1),
            WorkspaceCommandId::TabSelectNext => self.select_tab_relative(1),
            WorkspaceCommandId::TabSelectOrdinal => {
                let ordinal = resolve_tab_ordinal(command, self.shell.snapshot().tabs.len())
                    .map_err(invoke_error)?;
                let tabs = self.shell.snapshot().tabs;
                let id = tabs
                    .get((ordinal as usize).saturating_sub(1))
                    .map(|tab| tab.id)
                    .ok_or(AppError::ActionUnavailable)?;
                self.select_tab(id)
            }
            WorkspaceCommandId::PaneSplitRight => self.split_focused(SplitAxis::Right),
            WorkspaceCommandId::PaneSplitDown => self.split_focused(SplitAxis::Down),
            WorkspaceCommandId::PaneCloseFocused => {
                let id = self.shell.snapshot().focused_pane;
                self.close_pane(id)
            }
            WorkspaceCommandId::PaneFocusNext | WorkspaceCommandId::PaneFocusPrevious => {
                Err(AppError::ActionUnavailable)
            }
            WorkspaceCommandId::PresentationSetFlow => {
                self.transition_presentation(PresentationMode::Flow)
            }
            WorkspaceCommandId::PresentationSetRaw => {
                self.transition_presentation(PresentationMode::Raw)
            }
            WorkspaceCommandId::PresentationSetTui => {
                self.transition_presentation(PresentationMode::Tui)
            }
            WorkspaceCommandId::PresentationToggleRaw => {
                let mode = self.presentation.snapshot().mode;
                let next = if mode == PresentationMode::Raw {
                    PresentationMode::Flow
                } else {
                    PresentationMode::Raw
                };
                self.transition_presentation(next)
            }
            WorkspaceCommandId::PresentationToggleTui => {
                let mode = self.presentation.snapshot().mode;
                let next = if mode == PresentationMode::Tui {
                    PresentationMode::Flow
                } else {
                    PresentationMode::Tui
                };
                self.transition_presentation(next)
            }
            WorkspaceCommandId::ComposerHistorySearchOpen => {
                self.composer_history(fence, ComposerAction::OpenHistory { pane: fence.pane })
            }
            WorkspaceCommandId::FocusHistoryBack => {
                // R5.5 / R6.8: FocusSeq from the same snapshot history committed.
                let observed = self
                    .snapshot()
                    .focus_history_seq
                    .ok_or(AppError::ActionUnavailable)?;
                self.history_back(observed)
            }
            WorkspaceCommandId::FocusHistoryForward => {
                let observed = self
                    .snapshot()
                    .focus_history_seq
                    .ok_or(AppError::ActionUnavailable)?;
                self.history_forward(observed)
            }
            WorkspaceCommandId::AppQuit => self.quit(),
        }
    }

    fn select_tab_relative(&mut self, delta: isize) -> Result<(), AppError> {
        let snap = self.shell.snapshot();
        let tabs = snap.tabs;
        if tabs.is_empty() {
            return Err(AppError::ActionUnavailable);
        }
        let current = tabs
            .iter()
            .position(|tab| tab.id == snap.active_tab)
            .ok_or(AppError::ActionUnavailable)?;
        let next = current as isize + delta;
        if next < 0 || next as usize >= tabs.len() {
            return Err(AppError::ActionUnavailable);
        }
        self.select_tab(tabs[next as usize].id)
    }

    fn transition_presentation(&mut self, mode: PresentationMode) -> Result<(), AppError> {
        let snap = self.presentation.snapshot();
        let Some(identity) = snap.identity else {
            return Err(AppError::ActionUnavailable);
        };
        self.presentation
            .apply(PresentationAction::Transition {
                mode,
                identity,
                explicit: true,
                epoch: snap.epoch,
            })
            .map_err(|_| AppError::StalePresentationEpoch)?;
        self.clear_chord_prefix();
        Ok(())
    }

    /// Gate menu-originated CreateTab when the palette owns focus (R6.4.1).
    pub(super) fn require_workspace_command_for_menu(
        &mut self,
        id: WorkspaceCommandId,
    ) -> Result<(), AppError> {
        self.validate_menu_workspace_command(WorkspaceCommand { id, ordinal: None }, false)
    }
}

fn invoke_error(error: InvokeError) -> AppError {
    match error {
        InvokeError::ActionUnavailable => AppError::ActionUnavailable,
    }
}
