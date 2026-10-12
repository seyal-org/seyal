//! Apply structural close actions while preserving the execution lifecycle.

use super::provisioning_apply::WireDispatchContext;
use super::{close_pane_error, close_tab_error, AppAction, AppError, ApplicationRoot};
use crate::chrome::ChromeAction;
use crate::provisioning::ProvisioningEffect;
use crate::shell::{ShellAction, ShellError};
use seyal_core::{PaneId, TabId, WindowId};

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
        self.shell
            .apply(ShellAction::CloseWindow {
                id,
                containment_generation,
            })
            .map_err(|error| match error {
                ShellError::StaleContainment => AppError::StalePane,
                _ => AppError::UnknownChromeTab,
            })?;
        self.drain_shell_effects();
        let removed = self.shell.take_removed_tab_panes();
        let mut effects = Vec::new();
        for pane in removed {
            let pane_effects = self.provisioning.on_bound_pane_closed(pane);
            debug_assert!(
                !pane_effects
                    .iter()
                    .any(|effect| matches!(effect, ProvisioningEffect::SendTerminate { .. })),
                "removing a window must not terminate a bound execution"
            );
            effects.extend(pane_effects);
            self.clear_authority_for_pane(pane);
        }
        let _ = self.dispatch_wire_effects(
            effects,
            WireDispatchContext {
                workspace_id: 0,
                launch_profile: 0,
            },
        );
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
        self.shell
            .apply(ShellAction::CloseTab {
                id,
                containment_generation,
            })
            .map_err(close_tab_error)?;
        self.drain_shell_effects();
        let removed = self.shell.take_removed_tab_panes();
        let workspace = self.shell.snapshot().active_workspace;
        let mut effects = Vec::new();
        for pane in removed {
            let released = self.provisioning.recorded_execution(pane);
            let pane_effects = self.provisioning.on_bound_pane_closed(pane);
            debug_assert!(
                !pane_effects
                    .iter()
                    .any(|effect| matches!(effect, ProvisioningEffect::SendTerminate { .. })),
                "removing a tab must not terminate a bound execution"
            );
            effects.extend(pane_effects);
            self.clear_authority_for_pane(pane);
            if let Some(execution) = released {
                let _ = self.record_unpresented(execution, workspace);
            }
        }
        let _ = self.dispatch_wire_effects(
            effects,
            WireDispatchContext {
                workspace_id: 0,
                launch_profile: 0,
            },
        );
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    /// Close a Pane without terminating its bound execution.
    pub(super) fn close_pane_with_disposition(
        &mut self,
        id: PaneId,
        containment_generation: u64,
    ) -> Result<(), AppError> {
        self.shell
            .apply(ShellAction::ClosePane {
                id,
                containment_generation,
            })
            .map_err(close_pane_error)?;
        self.drain_shell_effects();
        let workspace = self.shell.snapshot().active_workspace;
        let removed = self.shell.take_removed_tab_panes();
        if !removed.is_empty() {
            // Hierarchical sole-pane close cascaded through tab/window removal.
            let mut effects = Vec::new();
            for pane in removed {
                let pane_effects = self.provisioning.on_bound_pane_closed(pane);
                debug_assert!(
                    !pane_effects
                        .iter()
                        .any(|effect| matches!(effect, ProvisioningEffect::SendTerminate { .. })),
                    "removing presentation must not terminate a bound execution"
                );
                effects.extend(pane_effects);
                self.clear_authority_for_pane(pane);
            }
            if let Some((_, execution)) = self.shell.take_released_execution() {
                let _ = self.record_unpresented(execution, workspace);
            }
            let _ = self.dispatch_wire_effects(
                effects,
                WireDispatchContext {
                    workspace_id: 0,
                    launch_profile: 0,
                },
            );
        } else if let Some((pane, execution)) = self.shell.take_released_execution() {
            let effects = self.provisioning.on_bound_pane_closed(pane);
            debug_assert!(
                self.provisioning.is_unreferenced(execution),
                "bound close must retain an unreferenced live record"
            );
            debug_assert!(
                !effects
                    .iter()
                    .any(|effect| matches!(effect, ProvisioningEffect::SendTerminate { .. })),
                "closing a pane must not terminate a bound execution"
            );
            self.clear_authority_for_pane(pane);
            let _ = self.record_unpresented(execution, workspace);
            let _ = self.dispatch_wire_effects(
                effects,
                WireDispatchContext {
                    workspace_id: 0,
                    launch_profile: 0,
                },
            );
        } else {
            // Outstanding create for this pane: keep the request until the
            // result arrives, then §6.3 disposition.
            self.provisioning.mark_intent_dead(id);
        }
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }
}
