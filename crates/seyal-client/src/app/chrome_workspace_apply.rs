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
        self.shell
            .create_workspace(id, root_path, self.shell.containment_generation())
            .map_err(|error| match error {
                ShellError::InvalidWorkspaceRoot => AppError::InvalidPayload,
                ShellError::DuplicateWorkspace => AppError::UnknownChromeWorkspace,
                _ => AppError::UnknownChromeWorkspace,
            })?;
        self.stage_workspace_spawn_cwd(id);
        self.drain_shell_effects();
        // Keep the open sidebar row even when the first surface provision fails.
        // cmux / ADR-029: CreateWorkspace must appear in the host window's open
        // list; unbound recovery owns the terminal surface, not RemoveWorkspace.
        if let Err(error) = self.begin_provisioning_for_focused_unbound_terminal() {
            self.last_error = Some(error);
        }
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
        self.shell
            .set_workspace_root(id, root_path)
            .map_err(|error| match error {
                ShellError::InvalidWorkspaceRoot => AppError::InvalidPayload,
                _ => AppError::UnknownChromeWorkspace,
            })?;
        self.stage_workspace_spawn_cwd(id);
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    /// Stage the bound ADR-028 root so Runtime interactive create can spawn
    /// with cwd_override (no typed `cd` in the PTY).
    fn stage_workspace_spawn_cwd(&self, id: WorkspaceId) {
        #[cfg(target_os = "macos")]
        {
            let Some(root) = self.shell.workspace_root(id) else {
                return;
            };
            let _ = seyal_protocol::workspace_spawn_cwd::stage(id, root);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = id;
        }
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
        // ADR-028 §2.5 Ready cold path first. Blocking mint on the live
        // Attached Controller runs poll_prepare + sleep on the UI thread and
        // can leave a fatal Error frame that makes the terminal unresponsive
        // after CreateWorkspace fails. Ready uses a dedicated short-lived
        // control connection and never touches the terminal's read buffer.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        match crate::local::LocalDisplayClient::mint_workspace_via_ready_connection(deadline) {
            Ok(id) => return Ok(id),
            Err(error) => {
                let ready = super::provisioning_apply::client_error(error);
                // Only fall back to Attached when Ready could not connect at
                // all. Capacity / protocol refusals must not poke the live
                // terminal socket (MAX_CONNECTIONS is Ready's problem).
                if !matches!(ready, AppError::NoLiveClient) {
                    return Err(ready);
                }
            }
        }

        // Cold-start / Discovery miss: try provisioning-capable Attached
        // Controllers with a short deadline so the UI cannot freeze for 5s.
        let attached_deadline = std::time::Instant::now() + std::time::Duration::from_millis(400);
        let mut last_error = Some(AppError::NoLiveClient);

        if let Some(client) = self.wire_client.as_mut()
            && client.execution_provisioning_negotiated()
        {
            match client.mint_workspace_blocking(attached_deadline) {
                Ok(id) => return Ok(id),
                Err(error) => last_error = Some(super::provisioning_apply::client_error(error)),
            }
        }

        for handle in self.attached_mint_client_raws() {
            match crate::ffi::with_client_mut(handle, |client| {
                if !client.execution_provisioning_negotiated() {
                    return None;
                }
                Some(client.mint_workspace_blocking(attached_deadline))
            }) {
                Some(Some(Ok(id))) => return Ok(id),
                Some(Some(Err(error))) => {
                    last_error = Some(super::provisioning_apply::client_error(error))
                }
                Some(None) | None => {}
            }
        }

        Err(last_error.unwrap_or(AppError::NoLiveClient))
    }

    /// Attached Controllers that can admit MintWorkspace (ADR-028 §2.5).
    /// Focused / authority pane clients are tried before other registry entries.
    #[cfg(target_os = "macos")]
    fn attached_mint_client_raws(&self) -> Vec<u64> {
        let mut handles = Vec::new();
        let mut push = |raw: u64| {
            if !handles.contains(&raw) {
                handles.push(raw);
            }
        };
        if let Some(raw) = self
            .client_handle
            .as_ref()
            .map(crate::ffi::ClientRegistryHandle::raw)
        {
            push(raw);
        }
        let focused = self.shell.snapshot().focused_pane;
        if let Some(raw) = self.pane_client_raws.get(&focused).copied() {
            push(raw);
        }
        if let Some(authority_pane) = self.authority.map(|authority| authority.pane)
            && let Some(raw) = self.pane_client_raws.get(&authority_pane).copied()
        {
            push(raw);
        }
        for raw in self.pane_client_raws.values().copied() {
            push(raw);
        }
        for raw in self.terminal_client_raws.values().copied() {
            push(raw);
        }
        handles
    }

    #[cfg(all(test, target_os = "macos"))]
    pub(crate) fn attached_mint_client_raws_for_test(&self) -> Vec<u64> {
        self.attached_mint_client_raws()
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
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }
}
