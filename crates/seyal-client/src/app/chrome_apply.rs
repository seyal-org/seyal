//! Shell tab/pane and chrome inspector apply paths.

use seyal_core::{PaneId, TabId, WorkspaceId};

use super::*;
use crate::chrome::{
    AgentId, AttentionId, ChromeAction, ChromeError, InspectorMode, LeftPanelMode,
};
use crate::composer::ComposerError;
use crate::navigation::ResourceAddress;
use crate::palette::PaletteError;
use crate::pane_layout::{self, SplitPosition};
use crate::presentation::{PresentationAction, PresentationIdentity};
use crate::shell::{ShellAction, ShellError};

impl ApplicationRoot {
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
        if self.chrome.last_retain_details() {
            return Ok(());
        }
        if let Some(packed) = self.chrome.take_pending_reveal() {
            let reveal = crate::navigation::reveal_attention_target(
                &packed,
                &mut self.shell,
                &crate::navigation::EmptyExecutionInventory,
                crate::navigation::NavigationPrincipal::local_user(),
                &mut self.focus_history,
            );
            if matches!(reveal, crate::navigation::AttentionReveal::Focused(_)) {
                self.activate_focused_pane_authority();
            }
            let _ = self
                .chrome
                .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
            return Ok(());
        }
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
        if effect.select_workspace.is_some() || effect.select_tab.is_some() {
            self.activate_focused_pane_authority();
            self.record_focused_pane_commit();
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
        self.activate_focused_pane_authority();
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
        self.activate_focused_pane_authority();
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
        self.activate_focused_pane_authority();
        self.record_focused_pane_commit();
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    /// Record the focused Pane after a user focus transition (SPEC-022 R6.3).
    pub(super) fn record_focused_pane_commit(&mut self) {
        let focus = self.shell.focus_checkpoint();
        self.focus_history
            .record_user_commit(ResourceAddress::Pane {
                workspace: focus.active_workspace,
                tab: focus.active_tab,
                pane: focus.focused_pane,
            });
    }

    /// Switch active authority/presentation to the focused pane. An unbound
    /// target clears the active input route without deleting sibling bindings.
    pub(super) fn activate_focused_pane_authority(&mut self) {
        let focused = self.shell.snapshot().focused_pane;
        let Some(authority) = self.pane_authorities.get(&focused).copied() else {
            self.authority = None;
            let _ = self.presentation.apply(PresentationAction::ClearIdentity);
            self.sync_composer_presentation();
            self.output_utf8.clear();
            #[cfg(target_os = "macos")]
            self.clear_focused_display_handle_if_owned();
            return;
        };
        if self.authority == Some(authority) {
            #[cfg(target_os = "macos")]
            if let Some(raw) = self.pane_client_raws.get(&focused).copied() {
                crate::ffi::set_focused_display_handle(raw);
            }
            return;
        }
        let Some(identity) =
            PresentationIdentity::new(authority.execution, authority.pty_generation)
        else {
            return;
        };
        let _ = self.presentation.apply(PresentationAction::ClearIdentity);
        let _ = self
            .presentation
            .apply(PresentationAction::BindIdentity(identity));
        self.authority = Some(authority);
        let alternate = self.alternate_screen_for_pane(focused);
        let _ = self.derive_presentation(alternate);
        self.sync_composer_presentation();
        #[cfg(target_os = "macos")]
        self.refresh_output_from_pane_client(focused);
        #[cfg(target_os = "macos")]
        if let Some(raw) = self.pane_client_raws.get(&focused).copied() {
            crate::ffi::set_focused_display_handle(raw);
        }
    }

    fn alternate_screen_for_pane(&self, pane: PaneId) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.pane_client_raws
                .get(&pane)
                .and_then(|handle| {
                    crate::ffi::with_client(*handle, |client| client.cache().alternate_screen)
                })
                .unwrap_or(self.alternate_screen)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = pane;
            self.alternate_screen
        }
    }

    #[cfg(target_os = "macos")]
    fn clear_focused_display_handle_if_owned(&self) {
        let focused = crate::ffi::focused_registry_handle();
        if focused == 0 {
            return;
        }
        let owned = self.pane_client_raws.values().any(|raw| *raw == focused)
            || self
                .client_handle
                .as_ref()
                .is_some_and(|handle| handle.raw() == focused);
        if owned {
            crate::ffi::set_focused_display_handle(0);
        }
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
        self.shell
            .apply(ShellAction::SetSplitRatio { pane, ratio })
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
}

pub fn chrome_error(error: ChromeError) -> AppError {
    match error {
        ChromeError::UnknownAgent => AppError::UnknownAgent,
        ChromeError::UnknownAttention => AppError::UnknownAttention,
        ChromeError::UnknownWorkspace => AppError::UnknownChromeWorkspace,
        ChromeError::UnknownTab => AppError::UnknownChromeTab,
        ChromeError::UnknownBlock => AppError::UnknownBlock,
    }
}

pub fn close_tab_error(error: ShellError) -> AppError {
    match error {
        ShellError::CannotCloseLastTab => AppError::CannotCloseLastTab,
        _ => AppError::UnknownChromeTab,
    }
}

pub fn close_pane_error(error: ShellError) -> AppError {
    match error {
        ShellError::CannotCloseLastPane => AppError::CannotCloseLastPane,
        ShellError::CannotCloseBoundPane => AppError::CannotCloseBoundPane,
        _ => AppError::UnknownPane,
    }
}

pub fn palette_error(error: PaletteError) -> AppError {
    match error {
        PaletteError::NotOpen => AppError::PaletteNotOpen,
        PaletteError::NoSelection => AppError::PaletteNoSelection,
    }
}

pub fn composer_error(error: ComposerError) -> AppError {
    match error {
        ComposerError::UnknownPane => AppError::UnknownPane,
        ComposerError::EmptyDraft | ComposerError::SubmitDisabled => {
            AppError::ComposerSubmitDisabled
        }
        ComposerError::StaleRequest => AppError::StaleComposerRequest,
        ComposerError::StaleEpoch => AppError::StaleComposerEpoch,
        ComposerError::HistoryUnavailable => AppError::ComposerHistoryUnavailable,
        ComposerError::HistoryClosed => AppError::ComposerHistoryClosed,
        ComposerError::HistoryNoSelection => AppError::ComposerHistoryNoSelection,
    }
}
