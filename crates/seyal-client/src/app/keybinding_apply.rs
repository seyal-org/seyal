//! SPEC-024 K3/K7: dispatch matched WorkspaceCommands from the routing gate.

use std::sync::OnceLock;

use crate::composer::ComposerAction;
use crate::keybinding::{
    load_keybinding_table_from_path, resolve_tab_ordinal, route_context_set, route_keystroke,
    validate_workspace_command, BindingContext, InvokeError, KeybindingTable, NormalizedStroke,
    RouteOutcome, WorkspaceCommand, WorkspaceCommandId,
};
use crate::presentation::{PresentationAction, PresentationMode};
use crate::shell::{directional_neighbor, FocusDirection, MoveSide, ShellAction, ShellError, SplitAxis};

use super::{AppError, ApplicationRoot};

fn process_keybinding_table() -> &'static KeybindingTable {
    static TABLE: OnceLock<KeybindingTable> = OnceLock::new();
    TABLE.get_or_init(|| {
        load_keybinding_table_from_path(crate::keybinding::keybinding_config_path().as_deref())
    })
}

impl ApplicationRoot {
    /// Current §6.1 route context set from palette / presentation / composer focus.
    pub fn keybinding_route_context(&self, composer_first_responder: bool) -> BindingContext {
        route_context_set(
            self.palette.is_open(),
            self.presentation.snapshot().mode,
            composer_first_responder,
        )
    }

    /// Route one already-normalized stroke (§6.2). Matched commands are applied
    /// here so ApplicationCommand paths write zero PTY bytes.
    pub fn route_normalized_keystroke(
        &mut self,
        stroke: &NormalizedStroke,
        composer_first_responder: bool,
        composition_active: bool,
    ) -> Result<RouteOutcome, AppError> {
        let table = process_keybinding_table();
        let route = self.keybinding_route_context(composer_first_responder);
        let outcome = route_keystroke(table, stroke, route, composition_active);
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
        &self,
        command: WorkspaceCommand,
        composer_first_responder: bool,
    ) -> Result<(), AppError> {
        let table = process_keybinding_table();
        let route = self.keybinding_route_context(composer_first_responder);
        validate_workspace_command(table, command, route).map_err(invoke_error)
    }

    pub(super) fn invoke_workspace_command(
        &mut self,
        command: WorkspaceCommand,
        route: BindingContext,
    ) -> Result<(), AppError> {
        let table = process_keybinding_table();
        validate_workspace_command(table, command, route).map_err(invoke_error)?;
        self.dispatch_workspace_command(command)
    }

    /// Apply a catalog command after route/binding validation (or from tests that
    /// exercise catalog-only ids such as `pane.swap_*` / `pane.move_*`).
    pub(super) fn dispatch_workspace_command(
        &mut self,
        command: WorkspaceCommand,
    ) -> Result<(), AppError> {
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
            WorkspaceCommandId::PaneFocusLeft => self.focus_direction(FocusDirection::Left),
            WorkspaceCommandId::PaneFocusRight => self.focus_direction(FocusDirection::Right),
            WorkspaceCommandId::PaneFocusUp => self.focus_direction(FocusDirection::Up),
            WorkspaceCommandId::PaneFocusDown => self.focus_direction(FocusDirection::Down),
            WorkspaceCommandId::PaneZoomToggle => self.zoom_toggle_focused(),
            WorkspaceCommandId::PaneSwapLeft => self.swap_focused_neighbor(FocusDirection::Left),
            WorkspaceCommandId::PaneSwapRight => self.swap_focused_neighbor(FocusDirection::Right),
            WorkspaceCommandId::PaneSwapUp => self.swap_focused_neighbor(FocusDirection::Up),
            WorkspaceCommandId::PaneSwapDown => self.swap_focused_neighbor(FocusDirection::Down),
            WorkspaceCommandId::PaneMoveLeft => {
                self.move_focused_beside(FocusDirection::Left, MoveSide::Left)
            }
            WorkspaceCommandId::PaneMoveRight => {
                self.move_focused_beside(FocusDirection::Right, MoveSide::Right)
            }
            WorkspaceCommandId::PaneMoveUp => {
                self.move_focused_beside(FocusDirection::Up, MoveSide::Above)
            }
            WorkspaceCommandId::PaneMoveDown => {
                self.move_focused_beside(FocusDirection::Down, MoveSide::Below)
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
        Ok(())
    }

    /// SPEC-024 §5.1 / §10.2: focus-relative directional neighbor via FocusDirection.
    fn focus_direction(&mut self, direction: FocusDirection) -> Result<(), AppError> {
        self.apply_shell(ShellAction::FocusDirection { direction })
            .map_err(focus_direction_error)
    }


    /// SPEC-024 §5.1: Unzoom when zoomed, else ZoomPane of the focused leaf.
    fn zoom_toggle_focused(&mut self) -> Result<(), AppError> {
        let snap = self.shell.snapshot();
        let action = if snap.zoomed.is_some() {
            ShellAction::Unzoom
        } else {
            ShellAction::ZoomPane {
                id: snap.focused_pane,
            }
        };
        self.apply_shell(action).map_err(pane_verb_error)
    }

    fn swap_focused_neighbor(&mut self, direction: FocusDirection) -> Result<(), AppError> {
        let snap = self.shell.snapshot();
        let neighbor = directional_neighbor(&snap.tree, snap.focused_pane, direction)
            .ok_or(AppError::NoDirectionalNeighbor)?;
        self.apply_shell(ShellAction::SwapPanes {
            a: snap.focused_pane,
            b: neighbor,
        })
        .map_err(pane_verb_error)
    }

    fn move_focused_beside(
        &mut self,
        direction: FocusDirection,
        side: MoveSide,
    ) -> Result<(), AppError> {
        let snap = self.shell.snapshot();
        let neighbor = directional_neighbor(&snap.tree, snap.focused_pane, direction)
            .ok_or(AppError::NoDirectionalNeighbor)?;
        self.apply_shell(ShellAction::MovePaneBeside {
            pane: snap.focused_pane,
            neighbor,
            side,
        })
        .map_err(pane_verb_error)
    }

    /// Gate menu-originated CreateTab when the palette owns focus (R6.4.1).
    pub(super) fn require_workspace_command_for_menu(
        &self,
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

fn focus_direction_error(error: ShellError) -> AppError {
    match error {
        ShellError::NoDirectionalNeighbor => AppError::NoDirectionalNeighbor,
        ShellError::UnknownPane => AppError::UnknownPane,
        _ => AppError::ActionUnavailable,
    }
}

fn pane_verb_error(error: ShellError) -> AppError {
    match error {
        ShellError::NoDirectionalNeighbor => AppError::NoDirectionalNeighbor,
        ShellError::UnknownPane => AppError::UnknownPane,
        ShellError::NotZoomed | ShellError::InvalidMoveTarget => AppError::ActionUnavailable,
        _ => AppError::ActionUnavailable,
    }
}
