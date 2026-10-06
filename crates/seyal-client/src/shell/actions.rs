//! ADR-018 §2.2 / §6 window and tab actions (W2a: no close).

use seyal_core::{TabId, WindowId, WorkspaceId};

use super::workspace::{Pane, Tab, Window};
use super::{CycleDirection, ShellAction, ShellError, ShellNativeEffect, ShellState};

impl ShellState {
    pub(super) fn require_containment_generation(&self, carried: u64) -> Result<(), ShellError> {
        if carried != self.containment_generation {
            Err(ShellError::StaleContainment)
        } else {
            Ok(())
        }
    }

    pub(super) fn bump_containment_generation(&mut self) {
        self.containment_generation = self.containment_generation.saturating_add(1);
    }

    /// Make `window` product-active and refresh focus history.
    ///
    /// Emits [`ShellNativeEffect::OrderFrontMakeKey`] only when the
    /// product-active Window actually changes (W3: keep the host effect
    /// queue bounded under same-window selection).
    pub(super) fn activate_window(&mut self, window: WindowId) -> Result<(), ShellError> {
        let previous_product = self
            .workspace(self.active_workspace)
            .ok()
            .and_then(|workspace| workspace.active_window);
        self.activate_window_from(window, previous_product)
    }

    fn activate_window_from(
        &mut self,
        window: WindowId,
        previous_product: Option<WindowId>,
    ) -> Result<(), ShellError> {
        let workspace_id = self
            .find_window(window)
            .map(|workspace| workspace.id)
            .ok_or(ShellError::UnknownWindow)?;
        let workspace = self.workspace_mut(workspace_id)?;
        if workspace.window(window).is_none() {
            return Err(ShellError::UnknownWindow);
        }
        workspace.active_window = Some(window);
        self.active_workspace = workspace_id;
        self.last_active_workspace = workspace_id;
        self.record_window_focus(window)?;
        if previous_product != Some(window) {
            self.push_effect(ShellNativeEffect::OrderFrontMakeKey { window });
        }
        Ok(())
    }

    fn record_window_focus(&mut self, window: WindowId) -> Result<(), ShellError> {
        let pane = self
            .find_window(window)
            .and_then(|workspace| workspace.window(window))
            .and_then(|item| item.tabs.iter().find(|tab| tab.id == item.active_tab))
            .map(|tab| tab.focused)
            .ok_or(ShellError::UnknownWindow)?;
        self.focus_history.record(pane);
        Ok(())
    }

    pub(super) fn find_window(&self, window: WindowId) -> Option<&super::workspace::Workspace> {
        self.workspaces
            .iter()
            .find(|workspace| workspace.window(window).is_some())
    }

    pub(super) fn find_tab_location(&self, tab: TabId) -> Option<(WorkspaceId, WindowId, usize)> {
        for workspace in &self.workspaces {
            for window in &workspace.windows {
                if let Some(index) = window.tab_index(tab) {
                    return Some((workspace.id, window.id, index));
                }
            }
        }
        None
    }

    fn window_of_pane(&self, pane: seyal_core::PaneId) -> Option<WindowId> {
        for workspace in &self.workspaces {
            for window in &workspace.windows {
                if window.tabs.iter().any(|tab| tab.panes.contains_key(&pane)) {
                    return Some(window.id);
                }
            }
        }
        None
    }

    /// Most recently focused Window in `workspace`, derived from pane recency.
    pub(super) fn derived_mru_window(&self, workspace: WorkspaceId) -> Option<WindowId> {
        let mut seen = Vec::new();
        for pane in self.focus_history.panes() {
            let Some(window) = self.window_of_pane(*pane) else {
                continue;
            };
            let Some(owner) = self.find_window(window) else {
                continue;
            };
            if owner.id != workspace || seen.contains(&window) {
                continue;
            }
            seen.push(window);
        }
        if let Some(id) = seen.first().copied() {
            return Some(id);
        }
        self.workspace(workspace)
            .ok()
            .and_then(|item| item.windows.first().map(|window| window.id))
    }

    pub(super) fn create_window(
        &mut self,
        workspace: WorkspaceId,
        containment_generation: u64,
    ) -> Result<(), ShellError> {
        self.require_containment_generation(containment_generation)?;
        let occupied = !self.workspace(workspace)?.windows.is_empty();
        if occupied && !self.allows_window_creation {
            return Err(ShellError::WindowCreationUnavailable);
        }
        let window = self.new_window_record(workspace)?;
        let window_id = window.id;
        self.workspace_mut(workspace)?.push_window(window);
        self.push_effect(ShellNativeEffect::RealizeWindow { window: window_id });
        self.activate_window(window_id)?;
        self.bump_containment_generation();
        Ok(())
    }

    pub(super) fn create_tab_in_window(
        &mut self,
        window: WindowId,
        containment_generation: u64,
    ) -> Result<(), ShellError> {
        self.require_containment_generation(containment_generation)?;
        if !self.allows_tab_creation {
            return Err(ShellError::TabCreationUnavailable);
        }
        let workspace_id = self
            .find_window(window)
            .map(|workspace| workspace.id)
            .ok_or(ShellError::UnknownWindow)?;
        let tab = self.new_tab_record();
        self.workspace_mut(workspace_id)?
            .push_tab_on_window(window, tab)?;
        self.activate_window(window)?;
        self.bump_containment_generation();
        Ok(())
    }

    pub(super) fn select_window(&mut self, id: WindowId) -> Result<(), ShellError> {
        self.activate_window(id)
    }

    pub(super) fn select_tab_identity(&mut self, id: TabId) -> Result<(), ShellError> {
        let (workspace_id, window_id, _) =
            self.find_tab_location(id).ok_or(ShellError::UnknownTab)?;
        let previous_product = self
            .workspace(self.active_workspace)
            .ok()
            .and_then(|workspace| workspace.active_window);
        self.workspace_mut(workspace_id)?.select_tab(id)?;
        self.activate_window_from(window_id, previous_product)
    }

    pub(super) fn cycle_window(&mut self, direction: CycleDirection) -> Result<(), ShellError> {
        let workspace = self.workspace(self.active_workspace)?;
        if workspace.windows.is_empty() {
            return Err(ShellError::UnknownWindow);
        }
        let active = workspace.active_window.ok_or(ShellError::UnknownWindow)?;
        let index = workspace
            .window_index(active)
            .ok_or(ShellError::UnknownWindow)?;
        let len = workspace.windows.len();
        let next = match direction {
            CycleDirection::Next => (index + 1) % len,
            CycleDirection::Previous => index.checked_sub(1).unwrap_or(len - 1),
        };
        let target = workspace.windows[next].id;
        self.activate_window(target)
    }

    pub(super) fn cycle_tab(&mut self, direction: CycleDirection) -> Result<(), ShellError> {
        let workspace = self.workspace(self.active_workspace)?;
        let window = workspace.active_window().ok_or(ShellError::UnknownWindow)?;
        if window.tabs.is_empty() {
            return Err(ShellError::UnknownTab);
        }
        let index = window
            .tab_index(window.active_tab)
            .ok_or(ShellError::UnknownTab)?;
        let len = window.tabs.len();
        let next = match direction {
            CycleDirection::Next => (index + 1) % len,
            CycleDirection::Previous => index.checked_sub(1).unwrap_or(len - 1),
        };
        let target = window.tabs[next].id;
        self.select_tab_identity(target)
    }

    pub(super) fn activate_workspace(
        &mut self,
        workspace: WorkspaceId,
        containment_generation: u64,
    ) -> Result<(), ShellError> {
        let has_window = {
            let ws = self.workspace(workspace)?;
            !ws.windows.is_empty()
        };
        if has_window {
            let target = self
                .derived_mru_window(workspace)
                .ok_or(ShellError::UnknownWindow)?;
            return self.activate_window(target);
        }
        self.create_window(workspace, containment_generation)
    }

    pub(super) fn move_tab_before(
        &mut self,
        tab: TabId,
        before: Option<TabId>,
        window: WindowId,
        containment_generation: u64,
    ) -> Result<(), ShellError> {
        self.require_containment_generation(containment_generation)?;
        let (source_workspace, source_window, source_index) =
            self.find_tab_location(tab).ok_or(ShellError::UnknownTab)?;
        let target_workspace = self.find_window(window).ok_or(ShellError::UnknownWindow)?;
        if target_workspace.id != source_workspace {
            return Err(ShellError::CrossWorkspaceMove);
        }
        if let Some(before_id) = before {
            if before_id == tab {
                return Err(ShellError::UnknownTab);
            }
            let before_loc = self
                .find_tab_location(before_id)
                .ok_or(ShellError::UnknownTab)?;
            if before_loc.1 != window {
                return Err(ShellError::UnknownTab);
            }
        }

        if source_window == window {
            let target = self
                .workspace(source_workspace)?
                .window(window)
                .ok_or(ShellError::UnknownWindow)?;
            let desired = match before {
                None => target.tabs.len().saturating_sub(1),
                Some(before_id) => {
                    let before_index = target.tab_index(before_id).ok_or(ShellError::UnknownTab)?;
                    if source_index < before_index {
                        before_index - 1
                    } else {
                        before_index
                    }
                }
            };
            if source_index == desired {
                return Ok(());
            }
        }

        let (removed, destroyed) = self.workspace_mut(source_workspace)?.take_tab(tab)?;
        if let Some(destroyed_id) = destroyed {
            self.purge_missing_panes();
            self.push_effect(ShellNativeEffect::DestroyWindowRealization {
                window: destroyed_id,
            });
        }
        self.workspace_mut(source_workspace)?
            .insert_tab_before(window, removed, before)?;
        self.activate_window(window)?;
        self.bump_containment_generation();
        Ok(())
    }

    pub(super) fn move_tab_to_window(
        &mut self,
        tab: TabId,
        window: WindowId,
        containment_generation: u64,
    ) -> Result<(), ShellError> {
        self.move_tab_before(tab, None, window, containment_generation)
    }

    pub(super) fn move_tab_to_new_window(
        &mut self,
        tab: TabId,
        containment_generation: u64,
    ) -> Result<(), ShellError> {
        self.require_containment_generation(containment_generation)?;
        let (workspace_id, source_window, _) =
            self.find_tab_location(tab).ok_or(ShellError::UnknownTab)?;
        let only = self
            .workspace(workspace_id)?
            .window(source_window)
            .ok_or(ShellError::UnknownWindow)?
            .tabs
            .len()
            == 1;
        if only {
            return Err(ShellError::MoveWouldNotChangeContainment);
        }
        let new_id = WindowId::new();
        let (removed, destroyed) = self.workspace_mut(workspace_id)?.take_tab(tab)?;
        debug_assert!(destroyed.is_none());
        let window = Window::try_new(new_id, workspace_id, vec![removed], tab)
            .expect("moved tab forms a valid window");
        self.workspace_mut(workspace_id)?.push_window(window);
        self.push_effect(ShellNativeEffect::RealizeWindow { window: new_id });
        self.activate_window(new_id)?;
        self.bump_containment_generation();
        Ok(())
    }

    fn new_window_record(&mut self, workspace: WorkspaceId) -> Result<Window, ShellError> {
        let tab = self.new_tab_record();
        let tab_id = tab.id;
        Window::try_new(WindowId::new(), workspace, vec![tab], tab_id)
    }

    fn new_tab_record(&mut self) -> Tab {
        let ordinal = self.next_tab_ordinal;
        self.next_tab_ordinal = self.next_tab_ordinal.saturating_add(1);
        let pane = Pane {
            id: seyal_core::PaneId::new(),
            title: "Pane 1".to_owned(),
            execution: None,
            allows_implicit_execution_bootstrap: false,
        };
        Tab::with_pane(TabId::new(), format!("Terminal {ordinal}"), pane)
    }

    pub(super) fn dispatch_w2a(&mut self, action: ShellAction) -> Result<(), ShellError> {
        match action {
            ShellAction::CreateWindow {
                workspace,
                containment_generation,
            } => self.create_window(workspace, containment_generation),
            ShellAction::CreateTab {
                window,
                containment_generation,
            } => self.create_tab_in_window(window, containment_generation),
            ShellAction::MoveTabBefore {
                tab,
                before,
                window,
                containment_generation,
            } => self.move_tab_before(tab, before, window, containment_generation),
            ShellAction::MoveTabToWindow {
                tab,
                window,
                containment_generation,
            } => self.move_tab_to_window(tab, window, containment_generation),
            ShellAction::MoveTabToNewWindow {
                tab,
                containment_generation,
            } => self.move_tab_to_new_window(tab, containment_generation),
            ShellAction::SelectWindow { id } => self.select_window(id),
            ShellAction::SelectTab { id } => self.select_tab_identity(id),
            ShellAction::CycleWindow { direction } => self.cycle_window(direction),
            ShellAction::CycleTab { direction } => self.cycle_tab(direction),
            ShellAction::ActivateWorkspace {
                workspace,
                containment_generation,
            } => self.activate_workspace(workspace, containment_generation),
            _ => unreachable!("non-W2a action routed to dispatch_w2a"),
        }
    }
}
