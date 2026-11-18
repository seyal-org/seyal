//! ShellAction dispatch table (cold path).

use super::{ShellAction, ShellError, ShellState};

impl ShellState {
    pub(super) fn dispatch(&mut self, action: ShellAction) -> Result<(), ShellError> {
        match action {
            ShellAction::CreateWindow { .. }
            | ShellAction::ActivateWorkspace { .. }
            | ShellAction::SelectWorkspace { .. }
            | ShellAction::SelectWindow { .. }
            | ShellAction::NativeWindowBecameKey { .. }
            | ShellAction::ObserveWindowBecameKey { .. }
            | ShellAction::SelectTab { .. }
            | ShellAction::CreateTab { .. }
            | ShellAction::CycleWindow { .. }
            | ShellAction::CycleTab { .. }
            | ShellAction::MoveTabBefore { .. }
            | ShellAction::MoveTabToWindow { .. }
            | ShellAction::MoveTabToNewWindow { .. }
            | ShellAction::RemoveWorkspace { .. } => self.dispatch_w2a(action),
            ShellAction::CloseWindow { .. }
            | ShellAction::CloseTab { .. }
            | ShellAction::ClosePane { .. } => self.dispatch_close(action),
            ShellAction::SplitFocused {
                axis,
                containment_generation,
            } => self
                .require_containment_generation(containment_generation)
                .and_then(|()| {
                    let focused = self.focused_pane_id()?;
                    self.split_pane(focused, axis).map(|_| ())
                }),
            ShellAction::SplitPane {
                id,
                axis,
                containment_generation,
            } => self
                .require_containment_generation(containment_generation)
                .and_then(|()| self.split_pane(id, axis).map(|_| ())),
            ShellAction::FocusPane { id } => self.focus_pane(id),
            ShellAction::ZoomPane { id } => self.zoom_pane(id),
            ShellAction::Unzoom => self.unzoom(),
            ShellAction::SwapPanes {
                a,
                b,
                containment_generation,
            } => self
                .require_containment_generation(containment_generation)
                .and_then(|()| self.swap_panes(a, b)),
            ShellAction::MovePaneBeside {
                pane,
                neighbor,
                side,
                containment_generation,
            } => self
                .require_containment_generation(containment_generation)
                .and_then(|()| self.move_pane_beside(pane, neighbor, side)),
            ShellAction::FocusDirection { direction } => self.focus_direction(direction),
            ShellAction::SetSplitRatio { pane, ratio } => self.set_split_ratio(pane, ratio),
            ShellAction::EqualizeTab {
                containment_generation,
            } => self
                .require_containment_generation(containment_generation)
                .and_then(|()| self.equalize_tab()),
            ShellAction::EqualizeFocused {
                containment_generation,
            } => self
                .require_containment_generation(containment_generation)
                .and_then(|()| self.equalize_focused()),
            ShellAction::BindExecution { pane, execution } => self.bind_execution(pane, execution),
            ShellAction::NewTerminalTab {
                pane,
                containment_generation,
            } => self
                .require_containment_generation(containment_generation)
                .and_then(|()| self.new_terminal_tab(pane).map(|_| ())),
            ShellAction::SelectTerminalTab { pane, terminal_tab } => {
                self.select_terminal_tab(pane, terminal_tab)
            }
            ShellAction::BindTerminalExecution {
                pane,
                terminal_tab,
                execution,
            } => self.bind_terminal_execution(pane, terminal_tab, execution),
            ShellAction::UnbindTerminal(pane, terminal_tab) => self.unbind(pane, terminal_tab),
            ShellAction::CloseTerminalTab {
                pane,
                terminal_tab,
                containment_generation,
            } => self
                .require_containment_generation(containment_generation)
                .and_then(|()| self.close_terminal_tab(pane, terminal_tab)),
            ShellAction::RecordUnpresented { .. }
            | ShellAction::ForgetUnpresented { .. }
            | ShellAction::AdoptExecution { .. } => self.dispatch_unpresented(action),
        }
    }
}
