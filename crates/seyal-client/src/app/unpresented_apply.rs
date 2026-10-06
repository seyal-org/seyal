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
    ///
    /// Validates the catalog/leaf before any commit. Primary-leaf adopt uses
    /// the same bind / request-id / `record_adopted_binding` path as Bind.
    /// A non-primary leaf still gets a `pane_authorities` entry so close and
    /// P4 terminate keep the ADR-017 §6.1 ledger.
    pub(super) fn adopt(
        &mut self,
        fence: AppFence,
        evidence: BindingEvidence,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        if evidence.pty_generation == 0 {
            return Err(AppError::ZeroPtyGeneration);
        }
        self.shell
            .validate_adopt_execution(fence.pane, evidence.execution)
            .map_err(unpresented_shell_error)?;
        if self.authority.is_none() {
            return self.bind(fence, evidence);
        }
        self.apply_shell(ShellAction::AdoptExecution {
            pane: fence.pane,
            execution: evidence.execution,
        })
        .map_err(unpresented_shell_error)?;
        self.provisioning
            .record_adopted_binding(fence.pane, evidence.execution);
        self.seed_provisioning_request_floor_from_wire();
        self.pane_authorities.insert(
            fence.pane,
            PaneAuthority {
                pane: fence.pane,
                execution: evidence.execution,
                attachment: evidence.attachment,
                controller: evidence.controller,
                pty_generation: evidence.pty_generation,
            },
        );
        Ok(())
    }

    /// Palette adopt: validate and emit an attach intent only.
    ///
    /// ADR-018 §6: do not bind the leaf here. Shell binding commits only through
    /// [`AppAction::Adopt`] after Runtime attachment evidence exists.
    pub(super) fn adopt_unpresented_command(
        &mut self,
        execution: ExecutionId,
    ) -> Result<(), AppError> {
        let snap = self.shell.snapshot();
        let pane = snap.focused_pane;
        self.shell
            .validate_adopt_execution(pane, execution)
            .map_err(unpresented_shell_error)?;
        self.pending_effects
            .push(NativeEffect::RequestAdoptAttach { pane, execution });
        Ok(())
    }

    /// Explicit terminate of a live-unpresented execution (ADR-018 §3.3).
    ///
    /// Distinct from [`Self::terminate_execution`] (P4 Controller dispose of a
    /// fenced, bound attachment). Queues [`NativeEffect::TerminateExecution`]
    /// for the existing ADR-005 Runtime path; never emitted by close actions.
    pub(super) fn terminate_unpresented(&mut self, execution: ExecutionId) -> Result<(), AppError> {
        let workspace = self.shell.snapshot().active_workspace;
        if !self
            .shell
            .live_unpresented(workspace)
            .iter()
            .any(|id| *id == execution)
        {
            return Err(AppError::ExecutionNotUnpresented);
        }
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
        ShellError::StaleContainment => AppError::StalePane,
        ShellError::UnknownWorkspace => AppError::UnknownChromeWorkspace,
        ShellError::UnknownTab => AppError::UnknownChromeTab,
        ShellError::UnknownWindow => AppError::UnknownPane,
        other => close_pane_error(other),
    }
}
