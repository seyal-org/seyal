//! Terminal-tab-aware explicit execution termination.

use seyal_core::{AttachmentId, ExecutionId, PaneId};

use super::ApplicationRoot;
use crate::provisioning::{ProvisioningEffect, ProvisioningFailure};

impl ApplicationRoot {
    pub(super) fn begin_bound_terminate(
        &mut self,
        pane: PaneId,
        execution: ExecutionId,
        attachment: AttachmentId,
    ) -> Result<Vec<ProvisioningEffect>, ProvisioningFailure> {
        if let Some(terminal_tab) = self
            .shell
            .selected_terminal_tab(pane)
            .ok()
            .flatten()
            .filter(|terminal_tab| {
                self.provisioning.terminal_execution(*terminal_tab) == Some(execution)
            })
        {
            self.provisioning
                .begin_terminal_terminate(pane, terminal_tab, attachment)
        } else {
            self.provisioning.begin_explicit_terminate(pane, attachment)
        }
    }

    #[cfg(target_os = "macos")]
    pub(super) fn finish_terminal_termination(
        &mut self,
        pane: PaneId,
        terminal_tab: seyal_core::TerminalTabId,
        execution: ExecutionId,
    ) {
        let _ = self.shell.apply(crate::shell::ShellAction::UnbindTerminal(
            pane,
            terminal_tab,
        ));
        self.release_terminal_after_terminate(terminal_tab);
        self.terminal_authorities.remove(&terminal_tab);
        self.pane_authorities.remove(&pane);
        if self
            .authority
            .is_some_and(|authority| authority.pane == pane && authority.execution == execution)
        {
            self.authority = None;
            let _ = self
                .presentation
                .apply(crate::presentation::PresentationAction::ClearIdentity);
            self.sync_composer_presentation();
        }
        if self.shell.snapshot().focused_pane == pane {
            self.activate_focused_pane_authority();
        }
    }
}
