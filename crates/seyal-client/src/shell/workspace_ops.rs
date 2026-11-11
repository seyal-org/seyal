//! Workspace create / root-bind / leave matching (ADR-028).

use seyal_core::WorkspaceId;

use super::{workspace::Workspace, ShellError, ShellState};

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
        };
        workspace.bind_root_path(root_path);
        self.workspaces.push(workspace);
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
