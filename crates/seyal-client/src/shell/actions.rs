//! ADR-018 §2.2 / §6 window and tab actions (W2a create/move/select).

use seyal_core::{TabId, TerminalTabId, WindowId, WorkspaceId};

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
        let previous_product = self.product_active_window().map(|window| window.id);
        self.activate_window_from(window, previous_product, true)
    }

    pub(super) fn observe_window_became_key(&mut self, window: WindowId) -> Result<(), ShellError> {
        let previous_product = self.product_active_window().map(|window| window.id);
        self.activate_window_from(window, previous_product, false)
    }

    fn activate_window_from(
        &mut self,
        window: WindowId,
        previous_product: Option<WindowId>,
        request_native_activation: bool,
    ) -> Result<(), ShellError> {
        let (workspace_id, realized) = {
            let workspace = self.find_window(window).ok_or(ShellError::UnknownWindow)?;
            let item = workspace.window(window).ok_or(ShellError::UnknownWindow)?;
            (workspace.id, item.realized)
        };
        let workspace = self.workspace_mut(workspace_id)?;
        workspace.active_window = Some(window);
        self.last_active_workspace = workspace_id;
        // Unrealized pane-tree storage is not a native projection host. Keep
        // sidebar_host on the realized presentation window (#1490).
        if realized {
            self.sidebar_host = Some(window);
            self.record_window_focus(window)?;
            if request_native_activation && previous_product != Some(window) {
                self.push_effect(ShellNativeEffect::OrderFrontMakeKey { window });
            }
        } else {
            self.record_window_focus(window)?;
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

    pub(super) fn window_of_pane(&self, pane: seyal_core::PaneId) -> Option<WindowId> {
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
            .and_then(|item| item.active_window)
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
        self.workspace_mut(workspace)?.presenting_window = Some(window_id);
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
        // Product CreateTab still mints an outer Tab leaf for the live
        // create→attach→bind path (C2b). ADR-029 removes Tab from
        // ResourceAddress; migration (§3.5) collapses multi-tab windows to
        // one pane tree. Do not reintroduce Tab into user-facing addresses.
        self.require_containment_generation(containment_generation)?;
        if !self.allows_tab_creation {
            return Err(ShellError::TabCreationUnavailable);
        }
        let workspace_id = self
            .find_window(window)
            .map(|workspace| workspace.id)
            .ok_or(ShellError::UnknownWindow)?;
        let tab = self.new_tab_record(false);
        self.workspace_mut(workspace_id)?
            .push_tab_on_window(window, tab)?;
        self.activate_window(window)?;
        self.bump_containment_generation();
        Ok(())
    }

    pub(super) fn select_window(&mut self, id: WindowId) -> Result<(), ShellError> {
        self.activate_window(id)
    }

    pub(super) fn native_window_became_key(&mut self, id: WindowId) -> Result<(), ShellError> {
        let workspace_id = self
            .find_window(id)
            .map(|workspace| workspace.id)
            .ok_or(ShellError::UnknownWindow)?;
        let workspace = self.workspace_mut(workspace_id)?;
        if workspace.window(id).is_none() {
            return Err(ShellError::UnknownWindow);
        }
        workspace.active_window = Some(id);
        self.last_active_workspace = workspace_id;
        self.sidebar_host = Some(id);
        self.record_window_focus(id)
    }

    pub(super) fn select_tab_identity(&mut self, id: TabId) -> Result<(), ShellError> {
        let (workspace_id, window_id, _) =
            self.find_tab_location(id).ok_or(ShellError::UnknownTab)?;
        let previous_product = self.product_active_window().map(|window| window.id);
        self.workspace_mut(workspace_id)?.select_tab(id)?;
        self.activate_window_from(window_id, previous_product, true)
    }

    pub(super) fn cycle_window(&mut self, direction: CycleDirection) -> Result<(), ShellError> {
        let workspace = self.workspace(self.active_workspace_id())?;
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
        let workspace = self.workspace(self.active_workspace_id())?;
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
            if let Some(host) = self.sidebar_host {
                if self.find_window(host).is_some() {
                    return self.open_workspace_in_sidebar(host, workspace, containment_generation);
                }
            }
            let target = self
                .derived_mru_window(workspace)
                .ok_or(ShellError::UnknownWindow)?;
            return self.activate_window(target);
        }
        if let Some(host) = self.sidebar_host {
            if self.find_window(host).is_some() {
                self.require_containment_generation(containment_generation)?;
                self.attach_unrealized_pane_window(workspace)?;
                self.present_workspace_on_host(host, workspace)?;
                self.bump_containment_generation();
                self.last_active_workspace = workspace;
                if let Ok(pane) = self.focused_pane_id() {
                    self.focus_history.record(pane);
                }
                return Ok(());
            }
        }
        self.create_window(workspace, containment_generation)
    }

    fn attach_unrealized_pane_window(
        &mut self,
        workspace: WorkspaceId,
    ) -> Result<WindowId, ShellError> {
        if let Some(existing) = self.workspace(workspace)?.active_window {
            return Ok(existing);
        }
        let mut window = self.new_window_record(workspace)?;
        window.realized = false;
        let window_id = window.id;
        {
            let item = self.workspace_mut(workspace)?;
            item.push_window(window);
            // Snapshot content keys off active_window(); without this the
            // sidebar selects the workspace while projecting an empty tree.
            item.active_window = Some(window_id);
        }
        Ok(window_id)
    }

    fn open_workspace_in_sidebar(
        &mut self,
        host: WindowId,
        workspace: WorkspaceId,
        containment_generation: u64,
    ) -> Result<(), ShellError> {
        self.workspace(workspace)?;
        let host_owner = self
            .find_window(host)
            .map(|owner| owner.id)
            .ok_or(ShellError::UnknownWindow)?;
        let (already_open, already_selected) = {
            let window = self
                .workspace(host_owner)?
                .window(host)
                .ok_or(ShellError::UnknownWindow)?;
            (
                window.open_workspaces.contains(&workspace),
                window.selected_workspace == workspace,
            )
        };
        if already_selected {
            return Ok(());
        }
        // Already listed on this host: select in place. Do not raise a foreign
        // solitary home that still holds a stale presenting_window pointer.
        if already_open {
            self.present_workspace_on_host(host, workspace)?;
            self.last_active_workspace = workspace;
            if let Ok(pane) = self.focused_pane_id() {
                self.focus_history.record(pane);
            }
            return Ok(());
        }
        if let Some(presenter) = self.foreign_presenter(workspace, host) {
            self.present_workspace_on_host(presenter, workspace)?;
            self.last_active_workspace = workspace;
            return self.activate_window(presenter);
        }
        self.require_containment_generation(containment_generation)?;
        self.demote_prior_product_presentation(workspace, host)?;
        self.present_workspace_on_host(host, workspace)?;
        self.bump_containment_generation();
        self.last_active_workspace = workspace;
        if let Ok(pane) = self.focused_pane_id() {
            self.focus_history.record(pane);
        }
        Ok(())
    }

    fn foreign_presenter(&self, workspace: WorkspaceId, host: WindowId) -> Option<WindowId> {
        let presenter = self
            .workspace(workspace)
            .ok()
            .and_then(|item| item.presenting_window)
            .filter(|window| *window != host)?;
        let window = self
            .workspaces
            .iter()
            .flat_map(|item| &item.windows)
            .find(|item| item.id == presenter)?;
        // Unrealized storage is not a native projection; open on the host.
        // Any realized presenter — including a solitary home window — raises
        // per #1490 acceptance 6 (already-open workspace orders its window front).
        if !window.realized {
            return None;
        }
        Some(presenter)
    }

    fn demote_prior_product_presentation(
        &mut self,
        workspace: WorkspaceId,
        keep: WindowId,
    ) -> Result<(), ShellError> {
        let victims: Vec<WindowId> = self
            .workspaces
            .iter()
            .flat_map(|item| &item.windows)
            .filter(|window| {
                window.realized
                    && window.id != keep
                    && window.open_workspaces.contains(&workspace)
                    // AC6: never demote a solitary realized home presentation.
                    && !(window.workspace_id == workspace && window.open_workspaces.len() == 1)
            })
            .map(|window| window.id)
            .collect();
        for window_id in victims {
            let owner = self
                .find_window(window_id)
                .map(|item| item.id)
                .ok_or(ShellError::UnknownWindow)?;
            let only_member = {
                let window = self
                    .workspace(owner)?
                    .window(window_id)
                    .ok_or(ShellError::UnknownWindow)?;
                window.open_workspaces.len() == 1
            };
            if only_member {
                self.close_window(window_id, self.containment_generation)?;
                continue;
            }
            let window = self
                .workspace_mut(owner)?
                .window_mut(window_id)
                .ok_or(ShellError::UnknownWindow)?;
            if let Some(index) = window
                .open_workspaces
                .iter()
                .position(|id| *id == workspace)
            {
                let was_selected = window.selected_workspace == workspace;
                window.open_workspaces.remove(index);
                if was_selected {
                    let next = index.min(window.open_workspaces.len() - 1);
                    window.selected_workspace = window.open_workspaces[next];
                }
            }
        }
        Ok(())
    }

    pub(super) fn remove_workspace_from_window(
        &mut self,
        window: WindowId,
        workspace: WorkspaceId,
        containment_generation: u64,
    ) -> Result<(), ShellError> {
        self.require_containment_generation(containment_generation)?;
        let owner = self
            .find_window(window)
            .map(|item| item.id)
            .ok_or(ShellError::UnknownWindow)?;
        let (index, selected, remaining) = {
            let item = self
                .workspace(owner)?
                .window(window)
                .ok_or(ShellError::UnknownWindow)?;
            let index = item
                .open_workspaces
                .iter()
                .position(|id| *id == workspace)
                .ok_or(ShellError::UnknownWorkspace)?;
            (
                index,
                item.selected_workspace == workspace,
                item.open_workspaces.len() - 1,
            )
        };
        if remaining == 0 {
            return self.close_window(window, containment_generation);
        }
        {
            let item = self
                .workspace_mut(owner)?
                .window_mut(window)
                .ok_or(ShellError::UnknownWindow)?;
            item.open_workspaces.remove(index);
            if selected {
                let next = index.min(item.open_workspaces.len() - 1);
                item.selected_workspace = item.open_workspaces[next];
            }
        }
        if self.workspace(workspace)?.presenting_window == Some(window) {
            self.workspace_mut(workspace)?.presenting_window = None;
        }
        let selected_now = self
            .workspace(owner)?
            .window(window)
            .map(|item| item.selected_workspace)
            .ok_or(ShellError::UnknownWindow)?;
        self.last_active_workspace = selected_now;
        if let Ok(pane) = self.focused_pane_id() {
            self.focus_history.record(pane);
        }
        self.bump_containment_generation();
        Ok(())
    }

    pub(super) fn present_workspace_on_host(
        &mut self,
        host: WindowId,
        workspace: WorkspaceId,
    ) -> Result<(), ShellError> {
        self.workspace(workspace)?;
        let host_owner = self
            .find_window(host)
            .map(|owner| owner.id)
            .ok_or(ShellError::UnknownWindow)?;
        let window = self
            .workspace_mut(host_owner)?
            .window_mut(host)
            .ok_or(ShellError::UnknownWindow)?;
        if !window.open_workspaces.contains(&workspace) {
            window.open_workspaces.push(workspace);
        }
        window.selected_workspace = workspace;
        self.workspace_mut(workspace)?.presenting_window = Some(host);
        Ok(())
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

        let previous_product = self.product_active_window().map(|active| active.id);
        let (removed, destroyed) = self.workspace_mut(source_workspace)?.take_tab(tab)?;
        if let Some(destroyed_id) = destroyed {
            // The Tab and its Pane identities remain live after a move, so
            // preserve pane-addressed focus history while releasing the empty
            // source Window realization in the host.
            self.push_effect(ShellNativeEffect::DestroyWindowRealization {
                window: destroyed_id,
            });
        }
        self.workspace_mut(source_workspace)?
            .insert_tab_before(window, removed, before)?;
        self.activate_window_from(window, previous_product, true)?;
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
        let previous_product = self.product_active_window().map(|active| active.id);
        let new_id = WindowId::new();
        let (removed, destroyed) = self.workspace_mut(workspace_id)?.take_tab(tab)?;
        debug_assert!(destroyed.is_none());
        let window = Window::try_new(new_id, workspace_id, vec![removed], tab)
            .expect("moved tab forms a valid window");
        self.workspace_mut(workspace_id)?.push_window(window);
        self.push_effect(ShellNativeEffect::RealizeWindow { window: new_id });
        self.activate_window_from(new_id, previous_product, true)?;
        self.bump_containment_generation();
        Ok(())
    }

    fn new_window_record(&mut self, workspace: WorkspaceId) -> Result<Window, ShellError> {
        // New workspace / NewWindow first pane mirrors M001: allow implicit
        // bootstrap so CreateWorkspace still gets a live surface if the
        // explicit provision intent races or fails closed.
        let tab = self.new_tab_record(true);
        let tab_id = tab.id;
        Window::try_new(WindowId::new(), workspace, vec![tab], tab_id)
    }

    fn new_tab_record(&mut self, _allows_implicit_execution_bootstrap: bool) -> Tab {
        let ordinal = self.next_tab_ordinal;
        self.next_tab_ordinal = self.next_tab_ordinal.saturating_add(1);
        let terminal_id = TerminalTabId::new();
        let pane = Pane {
            id: seyal_core::PaneId::new(),
            title: "Pane 1".to_owned(),
            terminal_tabs: vec![super::workspace::TerminalTab {
                id: terminal_id,
                title: "Pane 1".to_owned(),
                execution: None,
            }],
            selected_terminal_tab: Some(terminal_id),
            // New tabs never inherit implicit bootstrap; CreateWorkspace /
            // CreateWindow seed the first leaf via new_window_record separately.
            allows_implicit_execution_bootstrap: false,
            // Resting Raw only — never mint Flow routes from tab creation (#1490).
            presentation_mode: crate::presentation::PresentationMode::Raw,
            resting_presentation_mode: crate::presentation::PresentationMode::Raw,
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
            ShellAction::SelectWindow { id, .. } => self.select_window(id),
            ShellAction::NativeWindowBecameKey { id, .. } => self.native_window_became_key(id),
            ShellAction::ObserveWindowBecameKey { id } => self.observe_window_became_key(id),
            ShellAction::SelectTab { id, .. } => self.select_tab_identity(id),
            ShellAction::CycleWindow { direction, .. } => self.cycle_window(direction),
            ShellAction::CycleTab { direction, .. } => self.cycle_tab(direction),
            ShellAction::ActivateWorkspace {
                workspace,
                containment_generation,
            } => self.activate_workspace(workspace, containment_generation),
            ShellAction::RemoveWorkspace {
                window,
                workspace,
                containment_generation,
            } => self.remove_workspace_from_window(window, workspace, containment_generation),
            _ => unreachable!("non-W2a action routed to dispatch_w2a"),
        }
    }
}
