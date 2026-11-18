//! Shell tab/pane and chrome inspector apply paths.

use seyal_core::{PaneId, TabId, TerminalTabId, WindowId, WorkspaceId};

use super::*;
use crate::chrome::{AgentId, AttentionId, ChromeAction, InspectorMode, LeftPanelMode};
use crate::navigation::ResourceAddress;
use crate::pane_layout::{self, SplitPosition};
use crate::presentation::{PresentationAction, PresentationIdentity};
use crate::shell::{ShellAction, ShellError};

impl ApplicationRoot {
    pub(super) fn select_terminal_tab(
        &mut self,
        pane: PaneId,
        terminal_tab: TerminalTabId,
    ) -> Result<(), AppError> {
        self.shell
            .apply(ShellAction::SelectTerminalTab { pane, terminal_tab })
            .map_err(|_| AppError::UnknownPane)?;
        let execution = self
            .shell
            .terminal_execution(pane, terminal_tab)
            .map_err(|_| AppError::UnknownPane)?;
        if execution.is_some() {
            let _ = self
                .provisioning
                .select_terminal_binding(pane, terminal_tab);
        }
        if let Some(authority) = self.terminal_authorities.get(&terminal_tab).copied()
            && Some(authority.execution) == execution
        {
            self.pane_authorities.insert(pane, authority);
        } else {
            self.pane_authorities.remove(&pane);
        }
        #[cfg(target_os = "macos")]
        if let Some(raw) = self.terminal_client_raws.get(&terminal_tab).copied() {
            self.pane_client_raws.insert(pane, raw);
        } else {
            self.pane_client_raws.remove(&pane);
        }
        if self.shell.snapshot().focused_pane == pane {
            self.activate_focused_pane_authority();
        }
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn close_pane(
        &mut self,
        id: PaneId,
        containment_generation: u64,
    ) -> Result<(), AppError> {
        self.close_pane_with_disposition(id, containment_generation)
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
            self.drain_shell_effects();
            let _ = self
                .chrome
                .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
            return Ok(());
        }
        if let Some(workspace) = effect.select_workspace {
            self.shell
                .apply_activate_workspace(workspace)
                .map_err(|_| AppError::UnknownChromeWorkspace)?;
            self.drain_shell_effects();
        }
        if let Some(tab) = effect.select_tab {
            self.apply_shell(ShellAction::SelectTab {
                id: tab,
                containment_generation: self.shell.containment_generation(),
            })
            .map_err(|_| AppError::UnknownChromeTab)?;
        }
        if effect.select_workspace.is_some() || effect.select_tab.is_some() {
            self.activate_focused_pane_authority();
            self.record_focused_pane_commit();
        }
        self.drain_shell_effects();
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn select_workspace(
        &mut self,
        window: WindowId,
        id: WorkspaceId,
        containment_generation: u64,
    ) -> Result<(), AppError> {
        // SPEC-032: named window + generation. Listed → select-only; absent →
        // OpenWorkspace semantics on that host. Empty workspaces still need
        // #994 provisioning after open.
        let snap = self.shell.snapshot();
        let previous_selected = snap.selected_workspace;
        let previous_open = snap.open_workspaces.clone();
        let listed = snap
            .windows
            .iter()
            .find(|item| item.id == window)
            .is_some_and(|item| item.open_workspaces.contains(&id));
        let needs_bootstrap = !listed && !snap.windows.iter().any(|item| item.workspace == id);
        if listed {
            self.shell
                .apply(ShellAction::SelectWorkspace {
                    window,
                    workspace: id,
                    containment_generation,
                })
                .map_err(|error| match error {
                    crate::shell::ShellError::StaleContainment => AppError::StaleContainment,
                    _ => AppError::UnknownChromeWorkspace,
                })?;
        } else {
            self.shell
                .apply(ShellAction::ActivateWorkspace {
                    workspace: id,
                    containment_generation,
                })
                .map_err(|error| match error {
                    crate::shell::ShellError::StaleContainment => AppError::StaleContainment,
                    _ => AppError::UnknownChromeWorkspace,
                })?;
        }
        self.drain_shell_effects();
        // Re-admit spawn cwd before bootstrap create so open/select uses the
        // bound ADR-028 root (BindWorkspaceRoot / #1490). Propagate on
        // bootstrap; ignore only when no new surface is being created.
        if let Some(root) = self.shell.workspace_root(id) {
            match self.bind_workspace_spawn_root(id, root) {
                Ok(()) => {}
                Err(error) if needs_bootstrap => return Err(error),
                Err(_) => {}
            }
        }
        if needs_bootstrap
            && let Err(error) = self.begin_provisioning_for_focused_unbound_terminal()
        {
            // Rollback only the newly opened row/intent. Never CloseWindow the
            // product host — it may still present unrelated open workspaces.
            let after = self.shell.snapshot();
            if after.open_workspaces.len() > 1 && after.open_workspaces.contains(&id) {
                let _ = self
                    .shell
                    .apply(crate::shell::ShellAction::RemoveWorkspace {
                        window,
                        workspace: id,
                        containment_generation: self.shell.containment_generation(),
                    });
            }
            if previous_open.contains(&previous_selected)
                && self.shell.snapshot().selected_workspace != previous_selected
            {
                let _ = self.shell.apply(ShellAction::SelectWorkspace {
                    window,
                    workspace: previous_selected,
                    containment_generation: self.shell.containment_generation(),
                });
            }
            self.drain_shell_effects();
            self.last_error = Some(error);
            return Err(error);
        }
        self.activate_focused_pane_authority();
        self.record_focused_pane_commit();
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn rename_pane(&mut self, id: PaneId, title: String) -> Result<(), AppError> {
        self.shell
            .rename_pane(id, title)
            .map_err(|error| match error {
                crate::shell::ShellError::InvalidDisplayName => AppError::InvalidPayload,
                crate::shell::ShellError::UnknownPane => AppError::UnknownPane,
                _ => AppError::UnknownPane,
            })?;
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn select_tab(&mut self, id: TabId) -> Result<(), AppError> {
        self.shell
            .apply(ShellAction::SelectTab {
                id,
                containment_generation: self.shell.containment_generation(),
            })
            .map_err(|_| AppError::UnknownChromeTab)?;
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
        self.activate_focused_pane_authority();
        self.record_focused_pane_commit();
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    /// Record the focused Pane after a user focus transition (SPEC-022 R6.3).
    pub(super) fn record_focused_pane_commit(&mut self) {
        self.drain_shell_effects();
        let focus = self.shell.focus_checkpoint();
        self.focus_history
            .record_user_commit(ResourceAddress::Pane {
                workspace: focus.active_workspace,
                pane: focus.focused_pane,
            });
    }

    /// Switch active authority/presentation to the focused pane. An unbound
    /// target clears the active input route without deleting sibling bindings.
    pub(super) fn activate_focused_pane_authority(&mut self) {
        let focused = self.shell.snapshot().focused_pane;
        let stored_mode = self.shell.resting_presentation_mode(focused);
        // Restore the unbound Pane's mode before its first Controller bind.
        self.explicit_flow = stored_mode == Some(crate::presentation::PresentationMode::Flow);
        self.recompute_resting();
        if self.pane_recovery.expectations.contains_key(&focused) {
            self.authority = None;
            let _ = self.presentation.apply(PresentationAction::ClearIdentity);
            self.sync_composer_presentation();
            self.output_utf8.clear();
            #[cfg(target_os = "macos")]
            self.clear_focused_display_handle_if_owned();
            return;
        }
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
        // A live bind replaces any prior shell-exit latch on this Pane (#1379).
        self.clear_execution_ended_for_pane(focused);
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

    /// Active Tab's Split dividers (#928), pre-order. Empty while zoomed so
    /// the host does not keep overlay-vs-divider policy of its own.
    pub fn pane_dividers(&self) -> Vec<pane_layout::PaneDivider> {
        let shell = self.shell.snapshot();
        pane_layout::dividers_for_host(&shell.tree, shell.zoomed)
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
        self.apply_shell(ShellAction::SelectWindow {
            id,
            containment_generation: self.shell.containment_generation(),
        })
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
        self.apply_shell(ShellAction::CycleWindow {
            direction,
            containment_generation: self.shell.containment_generation(),
        })
        .map_err(|_| AppError::UnknownWindow)?;
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    pub(super) fn create_window(&mut self) -> Result<(), AppError> {
        // Zero-window re-entry keeps the retained workspace (ADR-018 §3.3a).
        // Otherwise SPEC-032 §3.3 NewWindow: one new workspace in a new window.
        let snap = self.shell.snapshot();
        let generation = snap.containment_generation;
        if snap.windows.is_empty() {
            self.apply_shell(ShellAction::CreateWindow {
                workspace: snap.last_active_workspace,
                containment_generation: generation,
            })
            .map_err(|error| match error {
                ShellError::WindowCreationUnavailable => AppError::WindowCreationUnavailable,
                _ => AppError::UnknownChromeWorkspace,
            })?;
        } else {
            let root = self
                .shell
                .workspace_root(snap.active_workspace)
                .filter(|path| !path.is_empty())
                .map(str::to_owned)
                .ok_or(AppError::InvalidPayload)?;
            let id = self.mint_workspace_id_for_new_window()?;
            self.bind_workspace_spawn_root(id, &root)?;
            self.shell
                .record_workspace(id, root)
                .map_err(|error| match error {
                    ShellError::InvalidWorkspaceRoot => AppError::InvalidPayload,
                    ShellError::DuplicateWorkspace => AppError::UnknownChromeWorkspace,
                    _ => AppError::UnknownChromeWorkspace,
                })?;
            self.apply_shell(ShellAction::CreateWindow {
                workspace: id,
                containment_generation: self.shell.containment_generation(),
            })
            .map_err(|error| match error {
                ShellError::WindowCreationUnavailable => AppError::WindowCreationUnavailable,
                _ => AppError::UnknownChromeWorkspace,
            })?;
        }
        self.drain_shell_effects();
        if let Err(error) = self.begin_provisioning_for_focused_unbound_terminal() {
            if let Ok(window) = self.shell.product_window_id() {
                let _ = self.apply_shell(ShellAction::CloseWindow {
                    id: window,
                    containment_generation: self.shell.containment_generation(),
                });
            }
            return Err(error);
        }
        self.activate_focused_pane_authority();
        self.record_focused_pane_commit();
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    #[cfg(test)]
    fn mint_workspace_id_for_new_window(&mut self) -> Result<WorkspaceId, AppError> {
        Ok(WorkspaceId::from_bytes({
            let mut bytes = [0x5a; 16];
            bytes[0] = self.shell.snapshot().workspaces.len() as u8;
            bytes[1] = (self.shell.containment_generation() & 0xff) as u8;
            bytes
        }))
    }

    #[cfg(all(not(test), target_os = "macos"))]
    fn mint_workspace_id_for_new_window(&mut self) -> Result<WorkspaceId, AppError> {
        self.mint_workspace_id()
    }

    #[cfg(all(not(test), not(target_os = "macos")))]
    fn mint_workspace_id_for_new_window(&mut self) -> Result<WorkspaceId, AppError> {
        Err(AppError::NoLiveClient)
    }

    pub(super) fn report_window_event(
        &mut self,
        window: WindowId,
        event: WindowNativeEvent,
        occluded: bool,
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
        match event {
            WindowNativeEvent::BecameKey => {
                // ADR-018 §1.3: native key activation is an input to Rust-owned
                // product selection and its single application-scoped focus
                // history. The native realization is already key, so this action
                // commits the identity-validated selection without echoing an
                // OrderFrontMakeKey effect back to AppKit.
                self.apply_shell(ShellAction::NativeWindowBecameKey {
                    id: window,
                    containment_generation: self.shell.containment_generation(),
                })
                .map_err(|_| AppError::UnknownWindow)?;
                let _ = self
                    .chrome
                    .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
            }
            WindowNativeEvent::Miniaturized => {
                self.shell
                    .set_window_miniaturized(window, true)
                    .map_err(|_| AppError::UnknownWindow)?;
            }
            WindowNativeEvent::Deminiaturized => {
                self.shell
                    .set_window_miniaturized(window, false)
                    .map_err(|_| AppError::UnknownWindow)?;
            }
            WindowNativeEvent::ActivationFailed => {
                self.last_window_event = Some((window, event));
                return Err(AppError::WindowActivationFailed);
            }
            WindowNativeEvent::OcclusionChanged => {
                self.shell
                    .set_window_occluded(window, occluded)
                    .map_err(|_| AppError::UnknownWindow)?;
            }
            _ => {}
        }
        self.last_window_event = Some((window, event));
        Ok(())
    }

    /// Last forwarded window event (W4a tests / host observability).
    pub fn last_window_event(&self) -> Option<(WindowId, WindowNativeEvent)> {
        self.last_window_event
    }
}

#[cfg(target_os = "macos")]
impl ApplicationRoot {
    pub(super) fn sync_attachment_delivery(&mut self) -> bool {
        use crate::shell::PresentationTier;
        let snap = self.shell.snapshot();
        let mut hidden = std::collections::HashMap::new();
        for window in &snap.windows {
            for tab in &window.tabs {
                for pane in &tab.panes {
                    hidden.insert(
                        pane.id,
                        matches!(
                            pane.presentation_tier,
                            PresentationTier::Hidden | PresentationTier::Unpresented
                        ),
                    );
                }
            }
        }
        let handles: Vec<_> = self
            .pane_client_raws
            .iter()
            .map(|(pane, raw)| (*pane, *raw))
            .collect();
        let mut all_admitted = true;
        for (pane, handle) in handles {
            let suspend = hidden.get(&pane).copied().unwrap_or(true);
            match crate::ffi::with_client_mut(handle, |client| {
                client.set_delivery_suspended(suspend)
            }) {
                Some(Ok(())) => {}
                Some(Err(_)) | None => all_admitted = false,
            }
        }
        if let Some(client) = self.wire_client.as_mut() {
            let execution = client.execution_id();
            let suspend = snap
                .windows
                .iter()
                .flat_map(|window| window.tabs.iter())
                .flat_map(|tab| tab.panes.iter())
                .find(|pane| pane.execution == Some(execution))
                .map(|pane| {
                    matches!(
                        pane.presentation_tier,
                        PresentationTier::Hidden | PresentationTier::Unpresented
                    )
                })
                .unwrap_or(true);
            if client.set_delivery_suspended(suspend).is_err() {
                all_admitted = false;
            }
        }
        all_admitted
    }
}
