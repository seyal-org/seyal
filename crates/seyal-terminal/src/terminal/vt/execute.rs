//! C0 execute path for TerminalCore.

use super::TerminalCore;
use crate::{damage::Mutation, TerminalError};

impl TerminalCore {
    pub(super) fn execute_current(&mut self, byte: u8) -> Result<Mutation, TerminalError> {
        if let 0x08..=0x0d = byte {
            self.invalidate_active_grapheme();
        }
        if self.modes.alternate_screen
            && let Some(screen) = &mut self.alternate
        {
            return screen.execute(byte, &mut self.line_ids, &mut self.grapheme_store);
        }
        self.primary
            .execute(byte, &mut self.line_ids, &mut self.grapheme_store)
    }
}
