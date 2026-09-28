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

    /// Palette adopt: rebind `ExecutionId` into the focused Pane leaf.
    /// Fresh `AttachmentId` is completed via [`AppAction::Adopt`] after Runtime attach.
    pub(super) fn adopt_unpresented_command(
        &mut self,
        execution: ExecutionId,
    ) -> Result<(), AppError> {
        let pane = self.shell.snapshot().focused_pane;
        self.apply_shell(ShellAction::AdoptExecution { pane, execution })
            .map_err(unpresented_shell_error)
    }

    pub(super) fn terminate_execution(&mut self, execution: ExecutionId) -> Result<(), AppError> {
        self.apply_shell(ShellAction::TerminateExecution { execution })
            .map_err(unpresented_shell_error)
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
