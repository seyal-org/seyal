//! Workspace create / root / mint apply paths (ADR-028 / #1490).

use seyal_core::WorkspaceId;

use super::*;
use crate::chrome::ChromeAction;
use crate::shell::ShellError;

impl ApplicationRoot {
    pub(super) fn create_workspace(&mut self, root_path: String) -> Result<(), AppError> {
        if root_path.trim().is_empty() {
            return Err(AppError::InvalidPayload);
        }
        let id = self.mint_workspace_id()?;
        // Admit spawn cwd before shell record so a failed bind cannot leave an
        // orphan Runtime root without a product workspace (or vice versa).
        self.bind_workspace_spawn_root(id, &root_path)?;
        self.shell
            .create_workspace(id, root_path, self.shell.containment_generation())
            .map_err(|error| match error {
                ShellError::InvalidWorkspaceRoot => AppError::InvalidPayload,
                ShellError::DuplicateWorkspace => AppError::UnknownChromeWorkspace,
                _ => AppError::UnknownChromeWorkspace,
            })?;
        self.drain_shell_effects();
        // Keep the open sidebar row even when the first surface provision fails.
        // cmux / ADR-029: CreateWorkspace must appear in the host window's open
        // list; unbound recovery owns the terminal surface, not RemoveWorkspace.
        // Surface the failure on AppSnapshot.last_error so chrome can show
        // recoverable UI without rolling back the workspace row (#1490).
        let provision_error = self.begin_provisioning_for_focused_unbound_terminal().err();
        self.last_error = provision_error;
        self.activate_focused_pane_authority();
        self.record_focused_pane_commit();
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn set_workspace_root(
        &mut self,
        id: WorkspaceId,
        root_path: String,
    ) -> Result<(), AppError> {
        self.bind_workspace_spawn_root(id, &root_path)?;
        self.shell
            .set_workspace_root(id, root_path)
            .map_err(|error| match error {
                ShellError::InvalidWorkspaceRoot => AppError::InvalidPayload,
                _ => AppError::UnknownChromeWorkspace,
            })?;
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    /// Runtime-admitted spawn cwd for `id` (BindWorkspaceRoot). Propagates
    /// failure — never silently ignore (#1490 NO-GO).
    ///
    /// Unit tests exercise shell/product state without a live Runtime socket
    /// (same pattern as the NewWindow mint test stub); production remains
    /// fail-closed on the Ready control path.
    #[cfg(test)]
    pub(super) fn bind_workspace_spawn_root(
        &self,
        id: WorkspaceId,
        root: &str,
    ) -> Result<(), AppError> {
        let _ = (self, id, root);
        Ok(())
    }

    #[cfg(all(not(test), target_os = "macos"))]
    pub(super) fn bind_workspace_spawn_root(
        &self,
        id: WorkspaceId,
        root: &str,
    ) -> Result<(), AppError> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        crate::local::LocalDisplayClient::bind_workspace_root_via_ready_connection(
            id, root, deadline,
        )
        .map_err(super::provisioning_apply::client_error)
    }

    #[cfg(all(not(test), not(target_os = "macos")))]
    pub(super) fn bind_workspace_spawn_root(
        &self,
        id: WorkspaceId,
        root: &str,
    ) -> Result<(), AppError> {
        let _ = (self, id, root);
        Err(AppError::NoLiveClient)
    }

    pub(super) fn rename_workspace(
        &mut self,
        id: WorkspaceId,
        name: String,
    ) -> Result<(), AppError> {
        self.shell
            .rename_workspace(id, name)
            .map_err(|error| match error {
                ShellError::InvalidDisplayName => AppError::InvalidPayload,
                ShellError::UnknownWorkspace => AppError::UnknownChromeWorkspace,
                _ => AppError::UnknownChromeWorkspace,
            })?;
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    #[cfg(target_os = "macos")]
    pub(super) fn mint_workspace_id(&mut self) -> Result<WorkspaceId, AppError> {
        // ADR-028 §2.5 Ready cold path only. Never poke the live Attached
        // Controller: poll_prepare + sleep on the UI thread can leave a fatal
        // Error frame that freezes input after CreateWorkspace (#1490).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        crate::local::LocalDisplayClient::mint_workspace_via_ready_connection(deadline)
            .map_err(super::provisioning_apply::client_error)
    }

    #[cfg(not(target_os = "macos"))]
    pub(super) fn mint_workspace_id(&mut self) -> Result<WorkspaceId, AppError> {
        Err(AppError::NoLiveClient)
    }

    #[cfg(test)]
    pub(crate) fn create_workspace_with_minted_id_for_test(
        &mut self,
        id: WorkspaceId,
        root_path: String,
    ) -> Result<(), AppError> {
        self.shell
            .create_workspace(id, root_path, self.shell.containment_generation())
            .map_err(|_| AppError::UnknownChromeWorkspace)?;
        self.drain_shell_effects();
        // Mirror production: keep the sidebar row and surface provision failure
        // on AppSnapshot.last_error for recoverable chrome (#1490).
        self.last_error = self.begin_provisioning_for_focused_unbound_terminal().err();
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }
}
