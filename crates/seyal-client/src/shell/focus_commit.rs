//! Atomic focus commit and owner-validated sidebar sync (#1490).

use seyal_core::{PaneId, TabId, WorkspaceId};

use super::{ShellError, ShellState};

impl ShellState {
    /// Atomically set Workspace + Tab + Pane focus. Validates composition
    /// before any write so a failure leaves prior focus unchanged.
    pub fn commit_focus(
        &mut self,
        workspace: WorkspaceId,
        tab: TabId,
        pane: PaneId,
    ) -> Result<(), ShellError> {
        if !self.contains_workspace(workspace) {
            return Err(ShellError::UnknownWorkspace);
        }
        let Some(owner) = self.workspace_of_tab(tab) else {
            return Err(ShellError::UnknownTab);
        };
        if owner != workspace {
            return Err(ShellError::UnknownTab);
        }
        let Some((pane_workspace, pane_tab)) = self.location_of_pane(pane) else {
            return Err(ShellError::UnknownPane);
        };
        if pane_workspace != workspace
            || pane_tab != tab
            || !self.tab_contains_leaf(workspace, tab, pane)
        {
            return Err(ShellError::UnknownPane);
        }
        let workspace_mut = self.workspace_mut(workspace)?;
        workspace_mut.select_tab(tab)?;
        let active_tab = workspace_mut.active_tab_mut()?;
        if active_tab.zoomed.is_some_and(|zoomed| zoomed != pane) {
            active_tab.zoomed = None;
        }
        active_tab.focused = pane;
        self.focus_history.record(pane);
        // Fail closed: sidebar/open must not soft-fail after focus mutates
        // (#1490). apply() clones first, so Err leaves prior ShellState.
        self.sync_sidebar_after_focus(workspace, tab)?;
        self.last_active_workspace = workspace;
        self.last_error = None;
        Ok(())
    }

    fn sync_sidebar_after_focus(
        &mut self,
        workspace: WorkspaceId,
        tab: TabId,
    ) -> Result<(), ShellError> {
        let Some(host) = self.sidebar_host else {
            return Ok(());
        };
        if self.find_window(host).is_none() {
            return Ok(());
        }
        let selected = self
            .product_active_window()
            .map(|window| window.selected_workspace);
        if selected == Some(workspace) {
            // Same selected workspace: raise another realized window that
            // hosts this tab. Never retarget sidebar_host to unrealized
            // pane-tree storage (foreign workspace projection, #1490).
            if let Some(target) = self.window_of_tab(tab)
                && target != host
            {
                let realized = self
                    .find_window(target)
                    .and_then(|owner| owner.window(target))
                    .is_some_and(|window| window.realized);
                if realized {
                    self.sidebar_host = Some(target);
                }
            }
            return Ok(());
        }
        // Owner-validated select path only — never bypass foreign_presenter /
        // membership generation via present_workspace_on_host (#1490).
        let generation = self.containment_generation;
        self.open_workspace_in_sidebar(host, workspace, generation)
    }
}
