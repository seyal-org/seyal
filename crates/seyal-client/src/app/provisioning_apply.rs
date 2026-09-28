//! ApplicationRoot create/close/terminate coordination with portable provisioning.

use seyal_core::{PaneId, TabId};
use seyal_protocol::framing::ErrorCode;

use super::{close_pane_error, close_tab_error, AppError, AppFence, ApplicationRoot};
use crate::chrome::ChromeAction;
use crate::composer::ComposerAction;
use crate::provisioning::{ProvisioningEffect, ProvisioningFailure};
use crate::shell::ShellAction;

impl ApplicationRoot {
    /// Create a Tab whose terminal leaf begins one C1 provisioning intent.
    pub(super) fn create_tab(&mut self) -> Result<(), AppError> {
        self.shell
            .apply(ShellAction::CreateTab)
            .map_err(|_| AppError::TabCreationUnavailable)?;
        let snap = self.shell.snapshot();
        let pane = snap.focused_pane;
        let tab = snap.active_tab;
        let _ = self.composer.apply(ComposerAction::EnsurePane { pane });
        if let Err(failure) = self.provisioning.begin_intent(pane, None) {
            let _ = self.shell.apply(ShellAction::CloseTab { id: tab });
            let _ = self.shell.take_removed_tab_panes();
            self.provisioning.note_rejected_without_retry(pane, failure);
            return Err(provisioning_app_error(failure));
        }
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    /// Remove a Tab's chrome. Bound panes detach only; executions stay live
    /// and enumerable. Outstanding create intents are marked dead for §6.3.
    pub(super) fn close_tab(&mut self, id: TabId) -> Result<(), AppError> {
        self.shell
            .apply(ShellAction::CloseTab { id })
            .map_err(close_tab_error)?;
        let removed = self.shell.take_removed_tab_panes();
        for pane in removed {
            let effects = self.provisioning.on_bound_pane_closed(pane);
            debug_assert!(
                !effects
                    .iter()
                    .any(|effect| matches!(effect, ProvisioningEffect::SendTerminate { .. })),
                "removing a tab must not terminate a bound execution"
            );
            self.clear_authority_for_pane(pane);
        }
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    /// Close a Pane. A previously bound execution is released as an
    /// unreferenced live record (ADR-017 §6.1 detach-only); it is never
    /// terminated as a side effect of presentation close.
    pub(super) fn close_pane_with_disposition(&mut self, id: PaneId) -> Result<(), AppError> {
        self.shell
            .apply(ShellAction::ClosePane { id })
            .map_err(close_pane_error)?;
        if let Some((pane, execution)) = self.shell.take_released_execution() {
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

    /// Explicit terminate of the fenced Controller execution (P4). Distinct
    /// from removing Tab/Pane chrome.
    pub(super) fn terminate_execution(&mut self, fence: AppFence) -> Result<(), AppError> {
        self.require_fence(fence)?;
        let bound = self.authority.ok_or(AppError::UnboundUnauthorized)?;
        if !bound.controller {
            return Err(AppError::NotController);
        }
        if bound.pane != fence.pane {
            return Err(AppError::StalePane);
        }
        let effects = self
            .provisioning
            .begin_explicit_terminate(bound.pane, bound.attachment)
            .map_err(provisioning_app_error)?;
        debug_assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, ProvisioningEffect::SendTerminate { .. })),
            "explicit terminate must queue a P4 TerminateExecutionRequest"
        );
        let _ = self.shell.release_execution(bound.pane);
        self.clear_authority_for_pane(bound.pane);
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    fn clear_authority_for_pane(&mut self, pane: PaneId) {
        if self
            .authority
            .as_ref()
            .is_some_and(|authority| authority.pane == pane)
        {
            self.authority = None;
            #[cfg(target_os = "macos")]
            if let Some(handle) = self.client_handle.take() {
                let _ = crate::ffi::unregister_client(handle.raw());
            }
        }
    }
}

fn provisioning_app_error(failure: ProvisioningFailure) -> AppError {
    match failure {
        ProvisioningFailure::CreateRejected(ErrorCode::CapacityExceeded) => {
            AppError::ProvisioningCapacityExceeded
        }
        _ => AppError::ProvisioningRejected,
    }
}
