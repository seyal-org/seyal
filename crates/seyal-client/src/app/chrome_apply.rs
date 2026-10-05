//! Shell tab/pane and chrome inspector apply paths.

use seyal_core::{PaneId, TabId, WindowId, WorkspaceId};

use super::*;
use crate::chrome::{AgentId, AttentionId, ChromeAction, InspectorMode, LeftPanelMode};
use crate::navigation::ResourceAddress;
use crate::pane_layout::{self, SplitPosition};
use crate::shell::{ShellAction, ShellError, SplitAxis};

impl ApplicationRoot {
    pub(crate) fn apply_shell(&mut self, action: ShellAction) -> Result<(), ShellError> {
        let result = self.shell.apply(action);
        if result.is_ok() {
            self.drain_shell_effects();
        }
        result
    }

    pub(super) fn split_focused(&mut self, axis: SplitAxis) -> Result<(), AppError> {
        self.apply_shell(ShellAction::SplitFocused { axis })
            .map_err(|_| AppError::PaneSplitUnavailable)?;
        // Split focuses the new leaf; record as a user-initiated commit (R6.3).
        self.record_focused_pane_commit();
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn close_pane(&mut self, id: PaneId) -> Result<(), AppError> {
        self.close_pane_with_disposition(id)
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
                .apply_activate_workspace(workspace)
                .map_err(|_| AppError::UnknownChromeWorkspace)?;
            self.drain_shell_effects();
        }
        if let Some(tab) = effect.select_tab {
            self.apply_shell(ShellAction::SelectTab { id: tab })
                .map_err(|_| AppError::UnknownChromeTab)?;
        }
        if effect.select_workspace.is_some() || effect.select_tab.is_some() {
            self.record_focused_pane_commit();
        }
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn select_workspace(&mut self, id: WorkspaceId) -> Result<(), AppError> {
        self.shell
            .apply_activate_workspace(id)
            .map_err(|_| AppError::UnknownChromeWorkspace)?;
        self.drain_shell_effects();
        self.record_focused_pane_commit();
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn select_tab(&mut self, id: TabId) -> Result<(), AppError> {
        self.apply_shell(ShellAction::SelectTab { id })
            .map_err(|_| AppError::UnknownChromeTab)?;
        #[cfg(target_os = "macos")]
        self.activate_focused_pane_authority();
        self.record_focused_pane_commit();
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn focus_pane(&mut self, id: PaneId) -> Result<(), AppError> {
        self.apply_shell(ShellAction::FocusPane { id })
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

    /// Active Tab's Split dividers (#928), pre-order.
    pub fn pane_dividers(&self) -> Vec<pane_layout::PaneDivider> {
        pane_layout::dividers(&self.shell.snapshot().tree)
    }

    /// Resize the Split whose divider `pane` leads (#928). The host sends the
    /// raw pointer position; Rust derives and clamps the ratio from the
    /// divider's own area, so no host inverts the layout.
    pub(super) fn move_split_divider(
        &mut self,
        pane: PaneId,
        position: SplitPosition,
    ) -> Result<(), AppError> {
        let shell = self.shell.snapshot();
        let Some(divider) = pane_layout::dividers(&shell.tree)
            .into_iter()
            .find(|divider| divider.leading == pane)
        else {
            return Err(if shell.panes.iter().any(|row| row.id == pane) {
                AppError::NoSplitDivider
            } else {
                AppError::UnknownPane
            });
        };
        let ratio = divider.ratio_at(position).ok_or(AppError::NoSplitDivider)?;
        self.apply_shell(ShellAction::SetSplitRatio { pane, ratio })
            .map_err(|error| match error {
                ShellError::NoSplitDivider => AppError::NoSplitDivider,
                _ => AppError::UnknownPane,
            })
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
    pub(super) fn select_window(&mut self, id: WindowId) -> Result<(), AppError> {
        self.apply_shell(ShellAction::SelectWindow { id })
            .map_err(|_| AppError::UnknownWindow)?;
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn cycle_window(
        &mut self,
        direction: crate::shell::CycleDirection,
    ) -> Result<(), AppError> {
        self.apply_shell(ShellAction::CycleWindow { direction })
            .map_err(|_| AppError::UnknownWindow)?;
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn create_window(&mut self) -> Result<(), AppError> {
        // ADR-018 §2.2 / §3.3a: host sends target-free New Window; Rust picks
        // the product-active Window's Workspace when any Window exists,
        // otherwise `last_active_workspace` (zero-window re-entry / W4b).
        let snap = self.shell.snapshot();
        let workspace = if snap.windows.is_empty() {
            snap.last_active_workspace
        } else {
            snap.active_workspace
        };
        let generation = snap.containment_generation;
        self.apply_shell(ShellAction::CreateWindow {
            workspace,
            containment_generation: generation,
        })
        .map_err(|error| match error {
            ShellError::WindowCreationUnavailable => AppError::WindowCreationUnavailable,
            _ => AppError::UnknownChromeWorkspace,
        })?;
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn report_window_event(
        &mut self,
        window: WindowId,
        event: WindowNativeEvent,
    ) -> Result<(), AppError> {
        if !self
            .shell
            .snapshot()
            .windows
            .iter()
            .any(|entry| entry.id == window)
        {
            return Err(AppError::UnknownWindow);
        }
        // Disposable presentation input only — never mutates window/tab/pane product state.
        self.last_window_event = Some((window, event));
        if event == WindowNativeEvent::ActivationFailed {
            return Err(AppError::WindowActivationFailed);
        }
        Ok(())
    }

    /// Last forwarded window event (W4a tests / host observability).
    pub fn last_window_event(&self) -> Option<(WindowId, WindowNativeEvent)> {
        self.last_window_event
    }
}
