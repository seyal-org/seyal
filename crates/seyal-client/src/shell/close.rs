//! ADR-018 §3.1 / §3.2 / §3.3a presentation removal (W2b).
//!
//! ClosePane / CloseTab / CloseWindow unbind and detach presentation only.
//! Bound executions become live-unpresented; none of these paths emit
//! [`ShellNativeEffect::TerminateExecution`].

use seyal_core::{ExecutionId, PaneId, TabId, WindowId, WorkspaceId};

use super::{ShellError, ShellNativeEffect, ShellState};

impl ShellState {
    pub(super) fn close_window(
        &mut self,
        id: WindowId,
        containment_generation: u64,
    ) -> Result<(), ShellError> {
        self.require_containment_generation(containment_generation)?;
        let (workspace_id, was_product_active) = {
            let (workspace_index, workspace) =
                self.find_window(id).ok_or(ShellError::UnknownWindow)?;
            let workspace_id = workspace.id;
            let was_product_active = self.product_active_window_id() == Some(id);
            let _ = workspace_index;
            (workspace_id, was_product_active)
        };

        let released = self.take_window_executions(id)?;
        self.remove_window_record(id)?;
        self.remove_from_mru(id);
        self.push_effect(ShellNativeEffect::DestroyWindowRealization { window: id });

        for (execution, workspace) in released {
            self.unpresented.insert(execution, workspace);
        }

        if was_product_active {
            if let Some(successor) = self.successor_after_close(workspace_id) {
                self.activate_window(successor)?;
            }
            // Zero-Window: last_active_workspace stays unchanged (ADR-018 §3.2 / §3.3a).
        } else {
            self.repair_workspace_active_window(workspace_id);
        }

        self.bump_containment_generation();
        Ok(())
    }

    pub(super) fn close_tab(
        &mut self,
        id: TabId,
        containment_generation: u64,
    ) -> Result<(), ShellError> {
        self.require_containment_generation(containment_generation)?;
        let (workspace_id, window_id, tab_index) =
            self.find_tab_location(id).ok_or(ShellError::UnknownTab)?;
        let only_tab = self
            .workspace(workspace_id)?
            .window(window_id)
            .ok_or(ShellError::UnknownWindow)?
            .tabs
            .len()
            == 1;
        if only_tab {
            return self.close_window(window_id, containment_generation);
        }

        let removed = {
            let window = self
                .workspace_mut(workspace_id)?
                .window_mut(window_id)
                .ok_or(ShellError::UnknownWindow)?;
            let removed = window.tabs.remove(tab_index);
            if window.active_tab == id {
                // Follower in prior order, or the new last Tab if it was last.
                let replacement = tab_index.min(window.tabs.len() - 1);
                window.active_tab = window.tabs[replacement].id;
            }
            removed
        };
        for pane in removed.panes.values() {
            if let Some(execution) = pane.execution {
                self.unpresented.insert(execution, workspace_id);
            }
        }

        self.bump_containment_generation();
        Ok(())
    }

    pub(super) fn close_pane(
        &mut self,
        pane_id: PaneId,
        containment_generation: u64,
    ) -> Result<(), ShellError> {
        self.require_containment_generation(containment_generation)?;
        let (workspace_id, _window_id, tab_id) = self
            .find_pane_location(pane_id)
            .ok_or(ShellError::UnknownPane)?;
        let only_pane = {
            let tab = self
                .workspace(workspace_id)?
                .tab(tab_id)
                .ok_or(ShellError::UnknownTab)?;
            tab.panes.len() == 1
        };
        if only_pane {
            return self.close_tab(tab_id, containment_generation);
        }

        let released = {
            let tab = self.tab_mut(workspace_id, tab_id)?;
            let Some(pane) = tab.panes.get_mut(&pane_id) else {
                return Err(ShellError::UnknownPane);
            };
            let released = pane.execution.take();
            let Some(root) = tab.root.removing(pane_id) else {
                return Err(ShellError::UnknownPane);
            };
            tab.root = root;
            tab.panes.remove(&pane_id);
            if tab.focused == pane_id || !tab.panes.contains_key(&tab.focused) {
                // Surviving sibling subtree's pre-order first leaf (ADR-021 / §3.2).
                tab.focused = tab
                    .root
                    .first_pane()
                    .expect("remaining Pane tree must contain a Pane");
            }
            released
        };
        if let Some(execution) = released {
            self.unpresented.insert(execution, workspace_id);
        }

        self.bump_containment_generation();
        Ok(())
    }

    fn find_pane_location(&self, pane: PaneId) -> Option<(WorkspaceId, WindowId, TabId)> {
        for workspace in &self.workspaces {
            for window in &workspace.windows {
                for tab in &window.tabs {
                    if tab.panes.contains_key(&pane) {
                        return Some((workspace.id, window.id, tab.id));
                    }
                }
            }
        }
        None
    }

    fn tab_mut(
        &mut self,
        workspace: WorkspaceId,
        tab: TabId,
    ) -> Result<&mut super::workspace::Tab, ShellError> {
        let workspace = self.workspace_mut(workspace)?;
        for window in &mut workspace.windows {
            if let Some(found) = window.tabs.iter_mut().find(|item| item.id == tab) {
                return Ok(found);
            }
        }
        Err(ShellError::UnknownTab)
    }

    fn take_window_executions(
        &mut self,
        window: WindowId,
    ) -> Result<Vec<(ExecutionId, WorkspaceId)>, ShellError> {
        let (workspace_id, workspace) = self
            .find_window(window)
            .map(|(_, workspace)| (workspace.id, workspace))
            .ok_or(ShellError::UnknownWindow)?;
        let window_ref = workspace.window(window).ok_or(ShellError::UnknownWindow)?;
        let mut released = Vec::new();
        for tab in &window_ref.tabs {
            for pane in tab.panes.values() {
                if let Some(execution) = pane.execution {
                    released.push((execution, workspace_id));
                }
            }
        }
        Ok(released)
    }

    fn remove_window_record(&mut self, window: WindowId) -> Result<(), ShellError> {
        let workspace_id = self
            .find_window(window)
            .map(|(_, workspace)| workspace.id)
            .ok_or(ShellError::UnknownWindow)?;
        let workspace = self.workspace_mut(workspace_id)?;
        let index = workspace
            .window_index(window)
            .ok_or(ShellError::UnknownWindow)?;
        workspace.windows.remove(index);
        // Leave product-active unset so `activate_window(successor)` observes a
        // real change and emits OrderFrontMakeKey (ADR-018 §3.2).
        if workspace.active_window == Some(window) {
            workspace.active_window = None;
        }
        Ok(())
    }

    fn successor_after_close(&self, closed_workspace: WorkspaceId) -> Option<WindowId> {
        self.window_mru
            .iter()
            .copied()
            .find(|window| {
                self.find_window(*window)
                    .is_some_and(|(_, workspace)| workspace.id == closed_workspace)
            })
            .or_else(|| self.window_mru.first().copied())
    }

    fn repair_workspace_active_window(&mut self, workspace: WorkspaceId) {
        if let Ok(workspace) = self.workspace_mut(workspace)
            && let Some(active) = workspace.active_window
            && workspace.window(active).is_none()
        {
            workspace.active_window = workspace.windows.first().map(|item| item.id);
        }
    }

    pub(super) fn product_active_window_id(&self) -> Option<WindowId> {
        self.workspace(self.active_workspace)
            .ok()
            .and_then(|workspace| workspace.active_window)
            .filter(|window| self.find_window(*window).is_some())
    }

    pub(super) fn dispatch_close(&mut self, action: super::ShellAction) -> Result<(), ShellError> {
        match action {
            super::ShellAction::CloseWindow {
                id,
                containment_generation,
            } => self.close_window(id, containment_generation),
            super::ShellAction::CloseTab {
                id,
                containment_generation,
            } => self.close_tab(id, containment_generation),
            super::ShellAction::ClosePane {
                id,
                containment_generation,
            } => self.close_pane(id, containment_generation),
            _ => unreachable!("non-close action routed to dispatch_close"),
        }
    }
}
