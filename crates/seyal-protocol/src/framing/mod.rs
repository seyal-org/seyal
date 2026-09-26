//! SPEC-004 Candidate-D binary wire framing.
//!
//! The 24-byte envelope is shared by control and presentation messages. Rust
//! memory layout is never wire format and all client-controlled lengths are
//! validated before allocation/use.

mod envelope;
mod message;
mod payload;

#[cfg(test)]
mod tests;

pub use crate::pass7::{
    BlockTimeline, CommandBlock, CommandBlockState, ComposerCommandRef, ComposerEligibility,
    ComposerResult, ComposerResultCode, ComposerStatus, HistoryCell, HistoryRangeRequest,
    HistoryRangeSnapshot, HistoryRangeStatus, HistoryRow, HistorySourceCell, ResizeRequest,
    ResizeResult, ResizeResultCode, TerminalKey, TerminalKeyKind, TerminalKeyModifiers,
    TerminalKeyV2, TerminalKeyV2Event, TerminalKeyV2Kind, TerminalKeyV2Modifiers,
    CAP_CORRELATED_RESIZE, CAP_EXTENDED_TERMINAL_KEY, CAP_SEMANTIC_TERMINAL_KEY,
    HISTORY_CELL_CONTINUATION_FLAG, HISTORY_CELL_SIDECAR_FLAG, HISTORY_CELL_WIDTH_MASK,
    HISTORY_CELL_WIDTH_SHIFT, MAX_HISTORY_GRAPHEME_BYTES, MAX_HISTORY_RANGE_BYTES,
    MAX_HISTORY_RANGE_CELLS, MAX_HISTORY_RANGE_LINES, MAX_HISTORY_SIDECAR_BYTES,
};

pub use envelope::{
    ErrorCode, FrameHeader, FramingError, CAP_BINARY_DISPLAY, CAP_COMMAND_BLOCKS,
    CAP_GRAPHEME_DISPLAY, CAP_OBSERVER, HEADER_LEN, MAGIC, MAJOR, MAX_EXECUTION_LIST_ENTRIES,
    MAX_FRAME_PAYLOAD, MAX_INPUT_BYTES, MINOR,
};

pub use message::{decode_message, encode_frame, Message, MessageType};

pub use payload::{
    Attach, Attached, ClientHello, Detach, Detached, ErrorMessage, ExecutionList,
    ExecutionListEntry, HostSearch, HostSelection, HostSelectionAction, InputRef, Lifecycle,
    LifecycleMessage, Resize, Resync, Role, ServerHello, TerminalMouse, TerminalMouseKind,
};
