//! Pass 7 capability-gated local IPC payloads (composer, blocks, history, keys, resize).
//!
//! Private module; `crate::framing` re-exports the public wire surface for callers.

mod command_blocks;
mod composer;
mod history;
mod resize;
mod terminal_key;

#[cfg(test)]
mod tests;

use crate::{framing::FramingError, AttachmentId};

pub use command_blocks::{BlockTimeline, CommandBlock, CommandBlockState};
pub use composer::{
    ComposerCommandRef, ComposerEligibility, ComposerResult, ComposerResultCode, ComposerStatus,
};
pub use history::{
    HistoryCell, HistoryRangeRequest, HistoryRangeSnapshot, HistoryRangeStatus, HistoryRow,
    HistorySourceCell, HISTORY_CELL_CONTINUATION_FLAG, HISTORY_CELL_SIDECAR_FLAG,
    HISTORY_CELL_WIDTH_MASK, HISTORY_CELL_WIDTH_SHIFT, MAX_HISTORY_GRAPHEME_BYTES,
    MAX_HISTORY_RANGE_BYTES, MAX_HISTORY_RANGE_CELLS, MAX_HISTORY_RANGE_LINES,
    MAX_HISTORY_SIDECAR_BYTES,
};
pub use resize::{ResizeRequest, ResizeResult, ResizeResultCode, CAP_CORRELATED_RESIZE};
pub use terminal_key::{
    TerminalKey, TerminalKeyKind, TerminalKeyModifiers, TerminalKeyV2, TerminalKeyV2Event,
    TerminalKeyV2Kind, TerminalKeyV2Modifiers, CAP_EXTENDED_TERMINAL_KEY,
    CAP_SEMANTIC_TERMINAL_KEY,
};

pub(in crate::pass7) fn exact_len(bytes: &[u8], expected: usize) -> Result<(), FramingError> {
    if bytes.len() != expected {
        return Err(FramingError::ExactLengthMismatch);
    }
    Ok(())
}

pub(in crate::pass7) fn attachment_id_from(bytes: &[u8]) -> AttachmentId {
    let mut raw = [0u8; 16];
    raw.copy_from_slice(&bytes[..16]);
    AttachmentId::from_bytes(raw)
}
