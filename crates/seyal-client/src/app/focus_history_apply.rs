//! Focus-history Back/Forward apply paths (SPEC-022 §6).

use super::*;
use crate::chrome::ChromeAction;
use crate::navigation::{
    history_back, history_forward, EmptyExecutionInventory, FocusSeq, NavigationPrincipal,
};
use crate::navigation::{matches_destroyed_pane, matches_destroyed_tab, ResourceAddress};
use seyal_core::{PaneId, TabId};

impl ApplicationRoot {
    /// Remove destroyed resources from focus history and, when shell focus
    /// changed, record the shell-selected successor as the new user focus.
    pub(super) fn record_destroyed_tab_focus(&mut self, id: TabId, successor_focused: bool) {
        let successor = successor_focused.then(|| self.focused_history_address());
        self.focus_history
            .on_destroy(|address| matches_destroyed_tab(address, id), successor);
    }

    pub(super) fn record_destroyed_pane_focus(&mut self, id: PaneId, successor_focused: bool) {
        let successor = successor_focused.then(|| self.focused_history_address());
        self.focus_history
            .on_destroy(|address| matches_destroyed_pane(address, id), successor);
    }

    fn focused_history_address(&self) -> ResourceAddress {
        let focus = self.shell.focus_checkpoint();
        ResourceAddress::Pane {
            workspace: focus.active_workspace,
            tab: focus.active_tab,
            pane: focus.focused_pane,
        }
    }

    pub(super) fn history_back(&mut self, observed: FocusSeq) -> Result<(), AppError> {
        history_back(
            observed,
            &mut self.focus_history,
            &mut self.shell,
            &EmptyExecutionInventory,
            NavigationPrincipal::local_user(),
        )
        .map_err(super::palette_apply::navigation_error)?;
        self.after_history_focus_applied();
        Ok(())
    }

    pub(super) fn history_forward(&mut self, observed: FocusSeq) -> Result<(), AppError> {
        history_forward(
            observed,
            &mut self.focus_history,
            &mut self.shell,
            &EmptyExecutionInventory,
            NavigationPrincipal::local_user(),
        )
        .map_err(super::palette_apply::navigation_error)?;
        self.after_history_focus_applied();
        Ok(())
    }

    fn after_history_focus_applied(&mut self) {
        self.activate_focused_pane_authority();
        // Authority activation can be a no-op when traversal lands on the
        // already-active binding; ensure the destination Pane's composer and
        // presentation projection in every traversal case.
        self.sync_composer_presentation();
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
    }
}
