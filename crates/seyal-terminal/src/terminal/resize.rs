//! Prepare/commit resize façade for TerminalState and TerminalCore.

use super::state::{TerminalState, MAX_TERMINAL_COLUMNS, MAX_TERMINAL_ROWS};
use super::vt::TerminalCore;
use crate::{damage::Mutation, screen::PreparedScreen, TerminalError};

/// Opaque prepared resize held until [`TerminalState::commit_resize`].
pub struct PreparedResize {
    pub(super) rows: u16,
    pub(super) primary: PreparedScreen,
    pub(super) alternate: Option<PreparedScreen>,
}

impl TerminalState {
    /// Convenience prepare+commit for VT-only consumers. Prefer
    /// [`prepare_resize`] / [`commit_resize`] when coordinating with a PTY.
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), TerminalError> {
        let prepared = self.prepare_resize(cols, rows)?;
        self.commit_resize(prepared);
        Ok(())
    }

    /// Fallible canonical resize preparation. Does not mutate live geometry or
    /// damage. Must be completed with [`commit_resize`] or dropped.
    pub fn prepare_resize(
        &mut self,
        cols: u16,
        rows: u16,
    ) -> Result<PreparedResize, TerminalError> {
        self.core.prepare_resize(cols, rows)
    }

    /// Infallible commit of a prepared resize. Damage/projection become
    /// observable only after this returns.
    pub fn commit_resize(&mut self, prepared: PreparedResize) {
        self.core.commit_resize(prepared);
    }
}

impl TerminalCore {
    fn prepare_resize(&mut self, cols: u16, rows: u16) -> Result<PreparedResize, TerminalError> {
        if let Some(error) = self.fault {
            return Err(error);
        }
        if cols == 0 || rows == 0 {
            return Err(TerminalError::InvalidSize);
        }
        if cols > MAX_TERMINAL_COLUMNS || rows > MAX_TERMINAL_ROWS {
            return Err(TerminalError::InvalidSize);
        }

        #[cfg(feature = "test-fault-injection")]
        if crate::test_fault::take(crate::test_fault::FaultPoint::ResizePrepare) {
            return Err(TerminalError::LineIdentityExhausted);
        }

        let mut required_ids = usize::from(rows.saturating_sub(self.primary.rows()));
        if let Some(screen) = &self.alternate {
            required_ids += usize::from(rows.saturating_sub(screen.rows()));
        }
        if !self.line_ids.can_allocate(required_ids) {
            return Err(TerminalError::LineIdentityExhausted);
        }

        let primary =
            self.primary
                .prepare_resize(cols, rows, &mut self.line_ids, &self.grapheme_store)?;
        let alternate = if let Some(screen) = &self.alternate {
            Some(screen.prepare_resize(cols, rows, &mut self.line_ids, &self.grapheme_store)?)
        } else {
            None
        };
        Ok(PreparedResize {
            rows,
            primary,
            alternate,
        })
    }

    fn commit_resize(&mut self, prepared: PreparedResize) {
        self.invalidate_active_grapheme();
        let primary = self
            .primary
            .commit_prepared(prepared.primary, &mut self.grapheme_store);
        let alternate = if let Some(prepared_alt) = prepared.alternate {
            if let Some(screen) = &mut self.alternate {
                screen.commit_prepared(prepared_alt, &mut self.grapheme_store)
            } else {
                Mutation::none()
            }
        } else {
            Mutation::none()
        };
        self.apply(
            primary
                .merge(alternate)
                .merge(Mutation::full(prepared.rows)),
        );
        self.damage.commit();
    }
}
