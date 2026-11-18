//! Workspace create / root-bind / leave matching (ADR-028).

use seyal_core::{PaneId, WorkspaceId};

use super::{workspace::Workspace, ShellError, ShellState};

/// Display names are labels only (ADR-029); never identities.
pub(crate) const MAX_DISPLAY_NAME_CHARS: usize = 128;

pub(crate) fn normalize_display_name(name: &str) -> Result<String, ShellError> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_DISPLAY_NAME_CHARS {
        return Err(ShellError::InvalidDisplayName);
    }
    Ok(trimmed.to_owned())
}

impl ShellState {
    /// Bind or update the root path on an existing Workspace (ADR-028 §2.2.4).
    pub fn set_workspace_root(
        &mut self,
        workspace: WorkspaceId,
        root_path: String,
    ) -> Result<(), ShellError> {
        if root_path.trim().is_empty() {
            return Err(ShellError::InvalidWorkspaceRoot);
        }
        let ws = self.workspace_mut(workspace)?;
        ws.bind_root_path(root_path);
        Ok(())
    }

    /// Rename a Workspace display label without changing `WorkspaceId` or root.
    pub fn rename_workspace(
        &mut self,
        id: WorkspaceId,
        name: impl Into<String>,
    ) -> Result<(), ShellError> {
        let name = normalize_display_name(&name.into())?;
        let workspace = self.workspace_mut(id)?;
        workspace.name = name;
        Ok(())
    }

    /// Rename the selected terminal-tab label in a Pane (and the Pane label).
    ///
    /// Sibling terminal tabs in the same Pane keep their titles. `PaneId` and
    /// bindings are unchanged (ADR-029 display labels only).
    pub fn rename_pane(&mut self, id: PaneId, title: impl Into<String>) -> Result<(), ShellError> {
        let title = normalize_display_name(&title.into())?;
        let (workspace_id, tab_id) = self.location_of_pane(id).ok_or(ShellError::UnknownPane)?;
        let workspace = self.workspace_mut(workspace_id)?;
        let tab = workspace.tab_mut(tab_id).ok_or(ShellError::UnknownTab)?;
        let pane = tab.panes.get_mut(&id).ok_or(ShellError::UnknownPane)?;
        pane.title = title.clone();
        if let Some(terminal) = pane.selected_terminal_mut() {
            terminal.title = title;
        }
        Ok(())
    }

    /// Record a Runtime-minted Workspace with a bound root. Does not activate.
    pub fn record_workspace(
        &mut self,
        id: WorkspaceId,
        root_path: String,
    ) -> Result<(), ShellError> {
        if root_path.trim().is_empty() {
            return Err(ShellError::InvalidWorkspaceRoot);
        }
        if id == WorkspaceId::m001_default() || self.workspaces.iter().any(|ws| ws.id == id) {
            return Err(ShellError::DuplicateWorkspace);
        }
        let mut workspace = Workspace {
            id,
            name: "Workspace".to_owned(),
            root_path: None,
            detail: None,
            attention: false,
            windows: Vec::new(),
            active_window: None,
            presenting_window: None,
        };
        workspace.bind_root_path(root_path);
        self.workspaces.push(workspace);
        Ok(())
    }

    /// Record a Runtime-minted Workspace with a bound root, then activate it.
    ///
    /// The caller must supply a Runtime-admitted `WorkspaceId` (ADR-028 §2.5).
    /// This method never mints identities.
    pub fn create_workspace(
        &mut self,
        id: WorkspaceId,
        root_path: String,
        containment_generation: u64,
    ) -> Result<(), ShellError> {
        // Validate generation before recording so a stale create cannot leave
        // an orphan workspace row (SPEC-032 / #1490).
        self.require_containment_generation(containment_generation)?;
        self.record_workspace(id, root_path)?;
        self.activate_workspace(id, containment_generation)
    }

    /// Exact normalized root match for leave-offer jump (ADR-028 §2.2.2).
    pub fn find_workspace_by_root(&self, root_path: &str) -> Option<WorkspaceId> {
        let needle = super::workspace::normalize_workspace_root_for_match(root_path);
        if needle.is_empty() {
            return None;
        }
        self.workspaces.iter().find_map(|ws| {
            ws.root_path
                .as_ref()
                .filter(|root| root.as_str() == needle.as_str())
                .map(|_| ws.id)
        })
    }

    pub fn workspace_root(&self, workspace: WorkspaceId) -> Option<&str> {
        self.workspace(workspace)
            .ok()
            .and_then(|ws| ws.root_path.as_deref())
    }
}
