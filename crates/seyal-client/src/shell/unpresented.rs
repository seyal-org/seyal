//! ADR-018 §3.3 live-unpresented inventory, adopt, and explicit terminate.
//!
//! Enumeration is deterministic (`BTreeMap` by `ExecutionId`) and never
//! auto-selects (SPEC-009 §8.2). Adoption rebinds the same `ExecutionId` into a
//! Pane leaf; Runtime supplies a fresh `AttachmentId` on the attach path.
//! `TerminateExecution` queues the existing ADR-005 termination effect. The
//! unpresented catalog entry stays until the runtime request has been made.

use std::collections::BTreeMap;

use seyal_core::{ExecutionId, PaneId, WorkspaceId};

use super::{ShellAction, ShellError, ShellNativeEffect, ShellState};

impl ShellState {
    /// Deterministic live-unpresented ids for one Workspace (ADR-018 §3.3).
    ///
    /// Order is `ExecutionId` ascending. Callers must not treat the first entry
    /// as an implicit selection.
    pub fn live_unpresented(&self, workspace: WorkspaceId) -> Vec<ExecutionId> {
        self.unpresented
            .iter()
            .filter(|(_, owned)| **owned == workspace)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Replace the headed-session live-unpresented catalog from Runtime facts.
    ///
    /// Bound executions are omitted. Entries absent from `live` are dropped
    /// (retired/finalized). Does not allocate attachments or touch PTYs.
    pub fn replace_live_unpresented(
        &mut self,
        live: impl IntoIterator<Item = (ExecutionId, WorkspaceId)>,
    ) {
        let mut next = BTreeMap::new();
        for (execution, workspace) in live {
            if !self.execution_is_bound(execution) {
                next.insert(execution, workspace);
            }
        }
        self.unpresented = next;
    }

    pub(super) fn record_unpresented(
        &mut self,
        execution: ExecutionId,
        workspace: WorkspaceId,
    ) -> Result<(), ShellError> {
        if self.execution_is_bound(execution) {
            return Err(ShellError::ExecutionAlreadyBound);
        }
        if let Some(existing) = self.unpresented.get(&execution) {
            if *existing != workspace {
                return Err(ShellError::CrossWorkspaceAdopt);
            }
            return Ok(());
        }
        self.unpresented.insert(execution, workspace);
        Ok(())
    }

    pub(super) fn forget_unpresented(&mut self, execution: ExecutionId) -> Result<(), ShellError> {
        if self.unpresented.remove(&execution).is_none() {
            return Err(ShellError::ExecutionNotUnpresented);
        }
        Ok(())
    }

    /// Fail-closed adopt predicates without mutating shell bindings.
    pub(crate) fn validate_adopt_execution(
        &self,
        pane_id: PaneId,
        execution: ExecutionId,
    ) -> Result<(), ShellError> {
        if self.execution_is_bound(execution) {
            return Err(ShellError::ExecutionAlreadyBound);
        }
        let Some(execution_workspace) = self.unpresented.get(&execution).copied() else {
            return Err(ShellError::ExecutionNotUnpresented);
        };
        let pane_workspace = self.pane_workspace(pane_id)?;
        if pane_workspace != execution_workspace {
            return Err(ShellError::CrossWorkspaceAdopt);
        }
        let pane = self.pane(pane_id)?;
        if pane.execution.is_some() {
            return Err(ShellError::ExecutionAlreadyBound);
        }
        Ok(())
    }

    pub(super) fn adopt_execution(
        &mut self,
        pane_id: PaneId,
        execution: ExecutionId,
    ) -> Result<(), ShellError> {
        self.validate_adopt_execution(pane_id, execution)?;
        let pane = self.pane_mut(pane_id)?;
        pane.execution = Some(execution);
        self.unpresented.remove(&execution);
        Ok(())
    }

    /// Fail-closed terminate predicates without queuing an effect or dropping
    /// the catalog entry.
    pub(crate) fn validate_terminate_execution(
        &self,
        execution: ExecutionId,
    ) -> Result<(), ShellError> {
        if self.execution_is_bound(execution) {
            return Err(ShellError::ExecutionAlreadyBound);
        }
        if !self.unpresented.contains_key(&execution) {
            return Err(ShellError::ExecutionNotUnpresented);
        }
        Ok(())
    }

    pub(super) fn terminate_execution(&mut self, execution: ExecutionId) -> Result<(), ShellError> {
        self.validate_terminate_execution(execution)?;
        self.push_effect(ShellNativeEffect::TerminateExecution { execution });
        Ok(())
    }

    pub(super) fn pane_workspace(&self, pane: PaneId) -> Result<WorkspaceId, ShellError> {
        for workspace in &self.workspaces {
            for tab in workspace.tabs() {
                if tab.panes.contains_key(&pane) {
                    return Ok(workspace.id);
                }
            }
        }
        Err(ShellError::UnknownPane)
    }

    pub(super) fn dispatch_unpresented(&mut self, action: ShellAction) -> Result<(), ShellError> {
        match action {
            ShellAction::RecordUnpresented {
                execution,
                workspace,
            } => self.record_unpresented(execution, workspace),
            ShellAction::ForgetUnpresented { execution } => self.forget_unpresented(execution),
            ShellAction::AdoptExecution { pane, execution } => {
                self.adopt_execution(pane, execution)
            }
            ShellAction::TerminateExecution { execution } => self.terminate_execution(execution),
            _ => unreachable!("dispatch_unpresented only handles §3.3 actions"),
        }
    }
}

/// Short non-secret palette label for one unpresented execution.
pub fn unpresented_palette_label(execution: ExecutionId) -> String {
    let bytes = execution.to_bytes();
    format!(
        "{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3]
    )
}
