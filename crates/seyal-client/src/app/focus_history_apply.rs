//! Focus-history Back/Forward apply paths (SPEC-022 §6).

use super::*;
use crate::chrome::ChromeAction;
use crate::navigation::{
    history_back, history_forward, EmptyExecutionInventory, FocusSeq, NavigationPrincipal,
};

impl ApplicationRoot {
    pub(super) fn history_back(&mut self, observed: FocusSeq) -> Result<(), AppError> {
        history_back(
            observed,
            &mut self.focus_history,
            &mut self.shell,
            &EmptyExecutionInventory,
            NavigationPrincipal::local_user(),
        )
        .map_err(super::palette_apply::navigation_error)?;
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
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
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }
}
