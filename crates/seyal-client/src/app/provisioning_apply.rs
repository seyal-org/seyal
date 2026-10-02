//! ApplicationRoot close/bind coordination with portable provisioning.

use seyal_core::PaneId;

use super::{close_pane_error, AppError, ApplicationRoot};
use crate::chrome::ChromeAction;
use crate::shell::ShellAction;

impl ApplicationRoot {
    /// Close a Pane. A previously bound execution is released as an
    /// unreferenced live record (ADR-017 §6.1 detach-only); it is never
    /// terminated as a side effect of presentation close.
    pub(super) fn close_pane_with_disposition(&mut self, id: PaneId) -> Result<(), AppError> {
        self.shell
            .apply(ShellAction::ClosePane { id })
            .map_err(close_pane_error)?;
        if let Some((pane, execution)) = self.shell.take_released_execution() {
            let _effects = self.provisioning.on_bound_pane_closed(pane);
            debug_assert!(
                self.provisioning.is_unreferenced(execution),
                "bound close must retain an unreferenced live record"
            );
            if self
                .authority
                .as_ref()
                .is_some_and(|authority| authority.pane == pane)
            {
                self.authority = None;
                let _ = self
                    .presentation
                    .apply(crate::presentation::PresentationAction::ClearIdentity);
                self.sync_composer_presentation();
                // ADR-017 §6.1 detach-only: keep client_handle registered for
                // remaining panes on the same connection.
            }
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
