//! Shell tab/pane and chrome inspector apply paths.

use seyal_core::{PaneId, TabId, WorkspaceId};

use super::*;
use crate::chrome::{AgentId, AttentionId, ChromeAction, InspectorMode, LeftPanelMode};
use crate::shell::{ShellAction, SplitAxis};

impl ApplicationRoot {
    pub(super) fn create_tab(&mut self) -> Result<(), AppError> {
        let snap = self.shell.snapshot();
        let window = snap.active_window.ok_or(AppError::UnknownChromeTab)?;
        self.apply_shell(ShellAction::CreateTab {
            window,
            containment_generation: snap.containment_generation,
        })
        .map_err(|_| AppError::TabCreationUnavailable)?;
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn split_focused(&mut self, axis: SplitAxis) -> Result<(), AppError> {
        self.apply_shell(ShellAction::SplitFocused { axis })
            .map_err(|_| AppError::PaneSplitUnavailable)?;
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn close_tab(&mut self, id: TabId) -> Result<(), AppError> {
        let generation = self.shell.containment_generation();
        self.apply_shell(ShellAction::CloseTab {
            id,
            containment_generation: generation,
        })
        .map_err(close_tab_error)?;
        self.release_authority_if_unbound();
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn close_pane(&mut self, id: PaneId) -> Result<(), AppError> {
        let generation = self.shell.containment_generation();
        self.apply_shell(ShellAction::ClosePane {
            id,
            containment_generation: generation,
        })
        .map_err(close_pane_error)?;
        self.release_authority_if_unbound();
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn close_window(&mut self, id: seyal_core::WindowId) -> Result<(), AppError> {
        let generation = self.shell.containment_generation();
        self.apply_shell(ShellAction::CloseWindow {
            id,
            containment_generation: generation,
        })
        .map_err(|_| AppError::UnknownChromeTab)?;
        self.release_authority_if_unbound();
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    fn release_authority_if_unbound(&mut self) {
        let Some(bound) = self.authority else {
            return;
        };
        if self
            .shell
            .pane_execution(bound.pane)
            .ok()
            .flatten()
            .is_some()
        {
            return;
        }
        // Presentation removal detaches; execution stays live-unpresented (ADR-018 §3.1).
        self.authority = None;
        #[cfg(target_os = "macos")]
        if let Some(handle) = self.client_handle.take() {
            let _ = crate::ffi::unregister_client(handle.raw());
        }
        self.sync_composer_presentation();
    }

    pub(crate) fn apply_shell(
        &mut self,
        action: ShellAction,
    ) -> Result<(), crate::shell::ShellError> {
        let result = self.shell.apply(action);
        if result.is_ok() {
            self.drain_shell_effects();
        }
        result
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
            let generation = self.shell.containment_generation();
            self.apply_shell(ShellAction::ActivateWorkspace {
                workspace,
                containment_generation: generation,
            })
            .map_err(|_| AppError::UnknownChromeWorkspace)?;
        }
        if let Some(tab) = effect.select_tab {
            self.apply_shell(ShellAction::SelectTab { id: tab })
                .map_err(|_| AppError::UnknownChromeTab)?;
        }
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn select_workspace(&mut self, id: WorkspaceId) -> Result<(), AppError> {
        let generation = self.shell.containment_generation();
        self.apply_shell(ShellAction::ActivateWorkspace {
            workspace: id,
            containment_generation: generation,
        })
        .map_err(|_| AppError::UnknownChromeWorkspace)?;
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn select_tab(&mut self, id: TabId) -> Result<(), AppError> {
        self.apply_shell(ShellAction::SelectTab { id })
            .map_err(|_| AppError::UnknownChromeTab)?;
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn focus_pane(&mut self, id: PaneId) -> Result<(), AppError> {
        self.apply_shell(ShellAction::FocusPane { id })
            .map_err(|_| AppError::UnknownPane)?;
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
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
