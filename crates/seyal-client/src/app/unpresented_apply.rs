//! ADR-018 §3.3 adopt / terminate / live-unpresented sync apply paths.

use super::*;
use crate::shell::ShellAction;

impl ApplicationRoot {
    /// Deterministic live-unpresented ids for the active Workspace (ADR-018 §3.3).
    pub fn live_unpresented(&self) -> Vec<ExecutionId> {
        let workspace = self.shell.snapshot().active_workspace;
        self.shell.live_unpresented(workspace)
    }

    pub(super) fn sync_live_unpresented(
        &mut self,
        entries: Vec<(ExecutionId, WorkspaceId)>,
    ) -> Result<(), AppError> {
        self.shell.replace_live_unpresented(entries);
        Ok(())
    }

    pub(super) fn record_unpresented(
        &mut self,
        execution: ExecutionId,
        workspace: WorkspaceId,
    ) -> Result<(), AppError> {
        self.apply_shell(ShellAction::RecordUnpresented {
            execution,
            workspace,
        })
        .map_err(unpresented_shell_error)
    }

    /// Adopt with fresh Runtime attachment evidence (no new PTY / ExecutionId).
    pub(super) fn adopt(
        &mut self,
        fence: AppFence,
        evidence: BindingEvidence,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        if evidence.pty_generation == 0 {
            return Err(AppError::ZeroPtyGeneration);
        }
        self.apply_shell(ShellAction::AdoptExecution {
            pane: fence.pane,
            execution: evidence.execution,
        })
        .map_err(unpresented_shell_error)?;
        if self.authority.is_none() {
            let identity = PresentationIdentity::new(evidence.execution, evidence.pty_generation)
                .ok_or(AppError::ZeroPtyGeneration)?;
            self.presentation
                .apply(PresentationAction::BindIdentity(identity))
                .map_err(|_| AppError::AlreadyBound)?;
            self.authority = Some(PaneAuthority {
                pane: fence.pane,
                execution: evidence.execution,
                attachment: evidence.attachment,
                controller: evidence.controller,
                pty_generation: evidence.pty_generation,
            });
            self.derive_presentation(evidence.alternate_screen)?;
            self.sync_composer_presentation();
        }
        Ok(())
    }

    /// Palette and ABI adopt entry.
    ///
    /// Rejects already-bound, cross-workspace, and retired or missing ids
    /// before any attach or bind. A successful call runs the existing Runtime
    /// `Attach` handshake (fresh `AttachmentId`, same `ExecutionId`, no new
    /// PTY) and removes the catalog entry only after the pane bind commits.
    pub(super) fn adopt_unpresented_command(
        &mut self,
        execution: ExecutionId,
    ) -> Result<(), AppError> {
        let pane = self.shell.snapshot().focused_pane;
        self.shell
            .validate_adopt_execution(pane, execution)
            .map_err(unpresented_shell_error)?;
        self.attach_and_bind_unpresented(pane, execution)
    }

    pub(super) fn terminate_execution(&mut self, execution: ExecutionId) -> Result<(), AppError> {
        self.shell
            .validate_terminate_execution(execution)
            .map_err(unpresented_shell_error)?;
        // The catalog and the terminate effect stay untouched until Runtime
        // has accepted this one attempt. A failed write or `InvalidState`
        // leaves the id in place so a later apply can request again.
        if self.request_runtime_termination(execution).is_err() {
            return Err(AppError::TerminationNotRequested);
        }
        self.apply_shell(ShellAction::TerminateExecution { execution })
            .map_err(unpresented_shell_error)?;
        self.apply_shell(ShellAction::ForgetUnpresented { execution })
            .map_err(unpresented_shell_error)
    }

    fn attach_and_bind_unpresented(
        &mut self,
        pane: seyal_core::PaneId,
        execution: ExecutionId,
    ) -> Result<(), AppError> {
        #[cfg(target_os = "macos")]
        {
            let client = crate::LocalDisplayClient::connect_execution_id(
                execution,
                seyal_runtime::local_ipc::framing::Role::Controller,
            )
            .map_err(|_| AppError::NoLiveClient)?;
            if client.execution_id() != execution {
                return Err(AppError::ExecutionNotUnpresented);
            }
            let evidence = BindingEvidence {
                execution: client.execution_id(),
                attachment: client.attachment_id(),
                controller: matches!(
                    client.role(),
                    seyal_runtime::local_ipc::framing::Role::Controller
                ),
                pty_generation: client.cache().generation.max(1),
                alternate_screen: client.cache().alternate_screen,
            };
            let registered =
                crate::ffi::register_app_client(client).map_err(|_| AppError::AlreadyBound)?;
            let fence = self.fence();
            if fence.pane != pane {
                let _ = crate::ffi::unregister_client(registered.raw());
                return Err(AppError::StalePane);
            }
            if let Err(error) = self.adopt(fence, evidence) {
                let _ = crate::ffi::unregister_client(registered.raw());
                return Err(error);
            }
            if let Some(previous) = self.client_handle.take()
                && previous.raw() != registered.raw()
            {
                let _ = crate::ffi::unregister_client(previous.raw());
            }
            self.client_handle = Some(registered);
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (self, pane, execution);
            Err(AppError::NoLiveClient)
        }
    }

    /// One IPC attempt on the live local client. `Err` means the runtime was
    /// not asked to reap; the catalog entry must stay.
    fn request_runtime_termination(&mut self, execution: ExecutionId) -> Result<(), ()> {
        #[cfg(target_os = "macos")]
        {
            let Some(handle) = self
                .client_handle
                .as_ref()
                .map(crate::ffi::ClientRegistryHandle::raw)
            else {
                return Err(());
            };
            match crate::ffi::with_client_mut(handle, |client| {
                client.send_terminate_execution(execution)
            }) {
                Some(Ok(())) => Ok(()),
                Some(Err(_)) => Err(()),
                None => {
                    self.client_handle = None;
                    Err(())
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (self, execution);
            Err(())
        }
    }
}

pub(super) fn unpresented_shell_error(error: ShellError) -> AppError {
    match error {
        ShellError::ExecutionAlreadyBound => AppError::AlreadyBound,
        ShellError::CrossWorkspaceAdopt => AppError::CrossWorkspaceAdopt,
        ShellError::ExecutionNotUnpresented => AppError::ExecutionNotUnpresented,
        ShellError::UnknownPane => AppError::UnknownPane,
        _ => AppError::ExecutionNotUnpresented,
    }
}
