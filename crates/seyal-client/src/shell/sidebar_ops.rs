//! SPEC-032 / #1490 sidebar open/select/present and ActivateWorkspace routing.

use seyal_core::{WindowId, WorkspaceId};

use super::{ShellError, ShellState};

impl ShellState {
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
            if let Some(host) = self.sidebar_host
                && self.find_window(host).is_some()
            {
                return self.open_workspace_in_sidebar(host, workspace, containment_generation);
            }
            let target = self
                .derived_mru_window(workspace)
                .ok_or(ShellError::UnknownWindow)?;
            return self.activate_window(target);
        }
        if let Some(host) = self.sidebar_host
            && self.find_window(host).is_some()
        {
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

    /// SPEC-032 §3.1: select only. Does not append, raise another window, or
    /// create a pane tree.
    pub(super) fn select_workspace_on_window(
        &mut self,
        window: WindowId,
        workspace: WorkspaceId,
        containment_generation: u64,
    ) -> Result<(), ShellError> {
        self.require_containment_generation(containment_generation)?;
        self.workspace(workspace)?;
        let host_owner = self
            .find_window(window)
            .map(|owner| owner.id)
            .ok_or(ShellError::UnknownWindow)?;
        let (listed, already_selected) = {
            let item = self
                .workspace(host_owner)?
                .window(window)
                .ok_or(ShellError::UnknownWindow)?;
            (
                item.open_workspaces.contains(&workspace),
                item.selected_workspace == workspace,
            )
        };
        if !listed {
            return Err(ShellError::UnknownWorkspace);
        }
        if already_selected {
            return Ok(());
        }
        self.present_workspace_on_host(window, workspace)?;
        self.bump_containment_generation();
        self.last_active_workspace = workspace;
        if let Ok(pane) = self.focused_pane_id() {
            self.focus_history.record(pane);
        }
        Ok(())
    }

    pub(super) fn open_workspace_in_sidebar(
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
        // SPEC-032 §§2–3: every select/open path validates containment
        // generation so stale native events no-mutate.
        self.require_containment_generation(containment_generation)?;
        if already_selected {
            return Ok(());
        }
        // Already listed on this host: select in place. Do not raise a foreign
        // solitary home that still holds a stale presenting_window pointer.
        if already_open {
            self.present_workspace_on_host(host, workspace)?;
            self.bump_containment_generation();
            self.last_active_workspace = workspace;
            if let Ok(pane) = self.focused_pane_id() {
                self.focus_history.record(pane);
            }
            return Ok(());
        }
        if let Some(presenter) = self.foreign_presenter(workspace, host) {
            self.present_workspace_on_host(presenter, workspace)?;
            self.bump_containment_generation();
            self.last_active_workspace = workspace;
            return self.activate_window(presenter);
        }
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
}
