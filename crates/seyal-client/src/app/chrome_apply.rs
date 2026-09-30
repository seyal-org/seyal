//! Shell tab/pane and chrome inspector apply paths.

use seyal_core::{PaneId, TabId, WorkspaceId};

use super::*;
use crate::chrome::{AgentId, AttentionId, ChromeAction, InspectorMode, LeftPanelMode};
use crate::navigation::{matches_destroyed_pane, matches_destroyed_tab, ResourceAddress};
use crate::shell::{ShellAction, SplitAxis};

impl ApplicationRoot {
    pub(super) fn create_tab(&mut self) -> Result<(), AppError> {
        self.shell
            .apply(ShellAction::CreateTab)
            .map_err(|_| AppError::TabCreationUnavailable)?;
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn split_focused(&mut self, axis: SplitAxis) -> Result<(), AppError> {
        self.shell
            .apply(ShellAction::SplitFocused { axis })
            .map_err(|_| AppError::PaneSplitUnavailable)?;
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn close_tab(&mut self, id: TabId) -> Result<(), AppError> {
        let focus_before = self.shell.focus_checkpoint();
        let was_active = focus_before.active_tab == id;
        self.shell
            .apply(ShellAction::CloseTab { id })
            .map_err(close_tab_error)?;
        // Authoritative destroy hook (SPEC-022 R6.7 / R6.7a): one call on the
        // product close path — surfaces do not scan history themselves.
        let focus_after = self.shell.focus_checkpoint();
        let successor = if was_active {
            Some(ResourceAddress::Pane {
                workspace: focus_after.active_workspace,
                tab: focus_after.active_tab,
                pane: focus_after.focused_pane,
            })
        } else {
            None
        };
        self.focus_history
            .on_destroy(|addr| matches_destroyed_tab(addr, id), successor);
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn close_pane(&mut self, id: PaneId) -> Result<(), AppError> {
        let focus_before = self.shell.focus_checkpoint();
        let was_focused = focus_before.focused_pane == id;
        self.shell
            .apply(ShellAction::ClosePane { id })
            .map_err(close_pane_error)?;
        // Authoritative destroy hook (SPEC-022 R6.7 / R6.7a).
        let focus_after = self.shell.focus_checkpoint();
        let successor = if was_focused {
            Some(ResourceAddress::Pane {
                workspace: focus_after.active_workspace,
                tab: focus_after.active_tab,
                pane: focus_after.focused_pane,
            })
        } else {
            None
        };
        self.focus_history
            .on_destroy(|addr| matches_destroyed_pane(addr, id), successor);
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn set_left_panel(&mut self, mode: LeftPanelMode) -> Result<(), AppError> {
        let shell = self.shell.snapshot();
        self.chrome
            .apply(ChromeAction::SetLeftPanel(mode), &shell)
            .map(|_| ())
            .map_err(chrome_error)
    }

    pub(super) fn set_inspector_mode(&mut self, mode: InspectorMode) -> Result<(), AppError> {
        let shell = self.shell.snapshot();
        self.chrome
            .apply(ChromeAction::SetInspectorMode(mode), &shell)
            .map(|_| ())
            .map_err(chrome_error)
    }

    pub(super) fn set_shell_visibility(
        &mut self,
        left: bool,
        inspector: bool,
        tab_strip: bool,
    ) -> Result<(), AppError> {
        let shell = self.shell.snapshot();
        self.chrome
            .apply(
                ChromeAction::SetShellVisibility {
                    left,
                    inspector,
                    tab_strip,
                },
                &shell,
            )
            .map(|_| ())
            .map_err(chrome_error)
    }

    pub(super) fn select_agent(&mut self, fence: AppFence, id: AgentId) -> Result<(), AppError> {
        self.require_fence(fence)?;
        let shell = self.shell.snapshot();
        self.chrome
            .apply(ChromeAction::SelectAgent { id }, &shell)
            .map(|_| ())
            .map_err(chrome_error)
    }

    pub(super) fn open_attention(
        &mut self,
        fence: AppFence,
        id: AttentionId,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        let shell = self.shell.snapshot();
        let effect = self
            .chrome
            .apply(ChromeAction::OpenAttention { id }, &shell)
            .map_err(chrome_error)?;
        if let Some(workspace) = effect.select_workspace {
            self.shell
                .apply(ShellAction::SelectWorkspace { id: workspace })
                .map_err(|_| AppError::UnknownChromeWorkspace)?;
        }
        if let Some(tab) = effect.select_tab {
            self.shell
                .apply(ShellAction::SelectTab { id: tab })
                .map_err(|_| AppError::UnknownChromeTab)?;
        }
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn select_workspace(&mut self, id: WorkspaceId) -> Result<(), AppError> {
        self.shell
            .apply(ShellAction::SelectWorkspace { id })
            .map_err(|_| AppError::UnknownChromeWorkspace)?;
        self.record_focused_pane_commit();
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn select_tab(&mut self, id: TabId) -> Result<(), AppError> {
        self.shell
            .apply(ShellAction::SelectTab { id })
            .map_err(|_| AppError::UnknownChromeTab)?;
        self.record_focused_pane_commit();
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn focus_pane(&mut self, id: PaneId) -> Result<(), AppError> {
        self.shell
            .apply(ShellAction::FocusPane { id })
            .map_err(|_| AppError::UnknownPane)?;
        self.record_focused_pane_commit();
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    /// Record the focused Pane after a user focus transition (ADR-019 §6 / R6.3).
    fn record_focused_pane_commit(&mut self) {
        let focus = self.shell.focus_checkpoint();
        self.focus_history
            .record_user_commit(ResourceAddress::Pane {
                workspace: focus.active_workspace,
                tab: focus.active_tab,
                pane: focus.focused_pane,
            });
    }

    pub(super) fn replace_chrome(
        &mut self,
        fence: AppFence,
        agents: Vec<crate::chrome::AgentRecord>,
        attention: Vec<crate::chrome::AttentionItem>,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        let shell = self.shell.snapshot();
        self.chrome
            .apply(
                ChromeAction::ReplaceAgents {
                    workspace: shell.active_workspace,
                    agents,
                },
                &shell,
            )
            .map_err(chrome_error)?;
        self.chrome
            .apply(ChromeAction::ReplaceAttention { items: attention }, &shell)
            .map(|_| ())
            .map_err(chrome_error)
    }
}
