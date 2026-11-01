//! ApplicationRoot leave-offer observe / Yes / No (ADR-028).

use super::leave_offer::{evaluate_leave_offer, LeaveOfferKind};
use super::{AppError, ApplicationRoot};

impl ApplicationRoot {
    pub fn leave_offer(&self) -> Option<&super::LeaveOffer> {
        self.leave_offer.as_ref()
    }

    pub(super) fn observe_trusted_foreground_cwd(&mut self, path: String) -> Result<(), AppError> {
        // Empty observation clears a stale offer without inventing one.
        if path.trim().is_empty() {
            self.leave_offer = None;
            return Ok(());
        }
        self.leave_offer = evaluate_leave_offer(&self.shell, &path);
        Ok(())
    }

    pub(super) fn confirm_leave_offer(&mut self) -> Result<(), AppError> {
        let Some(offer) = self.leave_offer.take() else {
            return Ok(());
        };
        match offer.kind {
            LeaveOfferKind::Create => self.create_workspace(offer.path),
            LeaveOfferKind::Jump { workspace } => self.select_workspace(workspace),
        }
    }

    pub(super) fn dismiss_leave_offer(&mut self) -> Result<(), AppError> {
        self.leave_offer = None;
        Ok(())
    }
}
