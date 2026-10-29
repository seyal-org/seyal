//! ApplicationRoot close coordination, preserving presentation and execution lifecycles.

use super::provisioning_apply::WireDispatchContext;
use super::{
    close_pane_error, close_tab_error, close_window_error, AppAction, AppError, ApplicationRoot,
};
use crate::chrome::ChromeAction;
use crate::provisioning::ProvisioningEffect;
use crate::shell::ShellAction;
use seyal_core::{PaneId, TabId, TerminalTabId, WindowId, WorkspaceId};

impl ApplicationRoot {
    fn close_terminal_tab(
        &mut self,
        pane: PaneId,
        terminal_tab: TerminalTabId,
        containment_generation: u64,
    ) -> Result<(), AppError> {
        let execution = self
            .shell
            .terminal_execution(pane, terminal_tab)
            .map_err(|_| AppError::UnknownPane)?;
        if let Some(execution) = execution {
            let authority = self
                .terminal_authorities
                .get(&terminal_tab)
                .copied()
                .filter(|authority| authority.execution == execution && authority.controller)
                .ok_or(AppError::UnboundUnauthorized)?;
            let effects = self
                .provisioning
                .begin_terminal_terminate(pane, terminal_tab, authority.attachment)
                .map_err(|_| AppError::ProvisioningRejected)?;
            self.dispatch_wire_effects(
                effects,
                WireDispatchContext {
                    workspace_id: 0,
                    launch_profile: 0,
                },
            )?;
            return Ok(());
        }

        self.provisioning.mark_terminal_intent_dead(terminal_tab);
        self.shell
            .apply(ShellAction::CloseTerminalTab {
                pane,
                terminal_tab,
                containment_generation,
            })
            .map_err(close_pane_error)?;
        self.finish_terminal_tab_close(pane, terminal_tab);
        Ok(())
    }

    pub(super) fn finish_terminal_tab_close(&mut self, pane: PaneId, terminal_tab: TerminalTabId) {
        #[cfg(target_os = "macos")]
        self.unregister_extra_terminal_client(terminal_tab);
        self.terminal_authorities.remove(&terminal_tab);
        if let Ok(Some(selected)) = self.shell.selected_terminal_tab(pane) {
            let _ = self.provisioning.select_terminal_binding(pane, selected);
            if let Some(authority) = self.terminal_authorities.get(&selected).copied() {
                self.pane_authorities.insert(pane, authority);
            } else {
                self.pane_authorities.remove(&pane);
            }
            #[cfg(target_os = "macos")]
            if let Some(raw) = self.terminal_client_raws.get(&selected).copied() {
                self.pane_client_raws.insert(pane, raw);
            } else {
                self.pane_client_raws.remove(&pane);
            }
        } else {
            self.pane_authorities.remove(&pane);
            #[cfg(target_os = "macos")]
            self.pane_client_raws.remove(&pane);
        }
        if self.shell.snapshot().focused_pane == pane {
            self.activate_focused_pane_authority();
        }
    }

    pub(super) fn apply_close(&mut self, action: AppAction) -> Result<(), AppError> {
        match action {
            AppAction::CloseWindow {
                id,
                containment_generation,
            } => self.close_window(id, containment_generation),
            AppAction::CloseTab {
                id,
                containment_generation,
            } => self.close_tab(id, containment_generation),
            AppAction::ClosePane {
                id,
                containment_generation,
            } => self.close_pane(id, containment_generation),
            AppAction::CloseTerminalTab {
                pane,
                terminal_tab,
                containment_generation,
            } => self.close_terminal_tab(pane, terminal_tab, containment_generation),
            _ => unreachable!("apply_close receives only structural close actions"),
        }
    }

    /// Remove a Window's chrome. Bound panes detach only; executions stay live
    /// and enumerable. Outstanding create intents are marked dead for §6.3.
    pub(super) fn close_window(
        &mut self,
        id: WindowId,
        containment_generation: u64,
    ) -> Result<(), AppError> {
        let before = self.shell.snapshot();
        let removed_window = before
            .windows
            .iter()
            .find(|window| window.id == id)
            .ok_or(AppError::UnknownWindow)?;
        let removed_tabs: Vec<TabId> = removed_window.tabs.iter().map(|tab| tab.id).collect();
        let removed_workspace = removed_window.workspace;
        let removed_active_tab = removed_window.active_tab;
        let was_active = before.active_window == Some(id);
        self.shell
            .apply(ShellAction::CloseWindow {
                id,
                containment_generation,
            })
            .map_err(close_window_error)?;
        self.drain_shell_effects();
        let removed = self.shell.take_removed_tab_panes();
        self.dispose_removed_panes(removed, removed_workspace)?;
        let after = self.shell.snapshot();
        for tab in removed_tabs {
            self.record_destroyed_tab_focus(
                tab,
                was_active && tab == removed_active_tab && after.active_window.is_some(),
            );
        }
        if was_active && after.active_window.is_some() {
            self.activate_focused_pane_authority();
        }
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    /// Remove a Tab's chrome. Bound panes detach only; executions stay live
    /// and enumerable. Outstanding create intents are marked dead for §6.3.
    pub(super) fn close_tab(
        &mut self,
        id: TabId,
        containment_generation: u64,
    ) -> Result<(), AppError> {
        let before = self.shell.snapshot();
        let (window_id, workspace, active_tab) = before
            .windows
            .iter()
            .find_map(|window| {
                window.tabs.iter().any(|tab| tab.id == id).then_some((
                    window.id,
                    window.workspace,
                    window.active_tab,
                ))
            })
            .ok_or(AppError::UnknownChromeTab)?;
        let was_active = before.active_window == Some(window_id) && active_tab == id;
        self.shell
            .apply(ShellAction::CloseTab {
                id,
                containment_generation,
            })
            .map_err(close_tab_error)?;
        self.drain_shell_effects();
        let removed = self.shell.take_removed_tab_panes();
        self.dispose_removed_panes(removed, workspace)?;
        let after = self.shell.snapshot();
        self.record_destroyed_tab_focus(id, was_active && after.active_window.is_some());
        if was_active && after.active_window.is_some() {
            self.activate_focused_pane_authority();
        }
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    fn dispose_removed_panes(
        &mut self,
        panes: Vec<PaneId>,
        workspace: WorkspaceId,
    ) -> Result<(), AppError> {
        let mut effects = Vec::new();
        let released = self.shell.take_released_executions();
        for pane in panes {
            let mut released_any_terminal = false;
            for (_, terminal_tab, execution) in released
                .iter()
                .filter(|(owner_pane, _, _)| *owner_pane == pane)
            {
                released_any_terminal = true;
                effects.extend(
                    self.provisioning
                        .on_bound_terminal_pane_closed(pane, *terminal_tab),
                );
                #[cfg(target_os = "macos")]
                self.release_terminal_presentation_controller(*terminal_tab, *execution);
                self.record_unpresented(*execution, workspace)?;
            }
            self.provisioning.mark_terminal_pane_intents_dead(pane);
            if !released_any_terminal {
                let execution = self.provisioning.recorded_execution(pane);
                let pane_effects = self.provisioning.on_bound_pane_closed(pane);
                debug_assert!(
                    !pane_effects
                        .iter()
                        .any(|effect| matches!(effect, ProvisioningEffect::SendTerminate { .. })),
                    "presentation removal must not terminate a bound execution"
                );
                effects.extend(pane_effects);
                #[cfg(target_os = "macos")]
                self.release_presentation_controller(pane, execution);
                if let Some(execution) = execution {
                    self.record_unpresented(execution, workspace)?;
                }
            }
            self.clear_authority_for_pane(pane);
        }
        self.dispatch_wire_effects(
            effects,
            WireDispatchContext {
                workspace_id: 0,
                launch_profile: 0,
            },
        )?;
        Ok(())
    }

    /// Close a Pane. A previously bound execution is released as an
    /// unreferenced live record (ADR-017 §6.1 detach-only); it is never
    /// terminated as a side effect of presentation close.
    pub(super) fn close_pane_with_disposition(
        &mut self,
        id: PaneId,
        containment_generation: u64,
    ) -> Result<(), AppError> {
        let before = self.shell.snapshot();
        let (window, tab) = before
            .windows
            .iter()
            .find_map(|window| {
                window.tabs.iter().find_map(|tab| {
                    tab.panes
                        .iter()
                        .any(|pane| pane.id == id)
                        .then_some((window, tab))
                })
            })
            .ok_or(AppError::UnknownPane)?;
        let workspace = window.workspace;
        let tab_id = tab.id;
        let was_focused = before.active_window == Some(window.id)
            && window.active_tab == tab.id
            && tab.focused_pane == id;
        self.shell
            .apply(ShellAction::ClosePane {
                id,
                containment_generation,
            })
            .map_err(close_pane_error)?;
        self.drain_shell_effects();
        let removed = self.shell.take_removed_tab_panes();
        if removed.is_empty() {
            if let Some((pane, execution)) = self.shell.take_released_execution() {
                self.dispose_removed_panes(vec![pane], workspace)?;
                debug_assert!(
                    self.provisioning.is_unreferenced(execution),
                    "bound close must retain an unreferenced live record"
                );
            } else {
                // Outstanding create for this pane: keep the request until the
                // result arrives, then apply §6.3 disposition.
                self.provisioning.mark_intent_dead(id);
            }
            self.record_destroyed_pane_focus(id, was_focused);
            if was_focused && self.shell.snapshot().active_window.is_some() {
                self.activate_focused_pane_authority();
            }
        } else {
            let _ = self.shell.take_released_execution();
            self.dispose_removed_panes(removed, workspace)?;
            let after = self.shell.snapshot();
            self.record_destroyed_tab_focus(tab_id, was_focused && after.active_window.is_some());
            if was_focused && after.active_window.is_some() {
                self.activate_focused_pane_authority();
            }
        }
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }
}
