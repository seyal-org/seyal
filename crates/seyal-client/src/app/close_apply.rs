//! ApplicationRoot close coordination, preserving presentation and execution lifecycles.

use super::provisioning_apply::WireDispatchContext;
use super::{
    close_pane_error, close_tab_error, close_window_error, AppAction, AppError, ApplicationRoot,
};
use crate::chrome::ChromeAction;
use crate::provisioning::ProvisioningEffect;
use crate::shell::ShellAction;
use seyal_core::{PaneId, TabId, WindowId, WorkspaceId};

impl ApplicationRoot {
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
        for pane in panes {
            let released = self.provisioning.recorded_execution(pane);
            let pane_effects = self.provisioning.on_bound_pane_closed(pane);
            debug_assert!(
                !pane_effects
                    .iter()
                    .any(|effect| matches!(effect, ProvisioningEffect::SendTerminate { .. })),
                "presentation removal must not terminate a bound execution"
            );
            effects.extend(pane_effects);
            self.clear_authority_for_pane(pane);
            if let Some(execution) = released {
                self.record_unpresented(execution, workspace)?;
            }
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
