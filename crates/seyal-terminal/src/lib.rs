//! Seyal-owned terminal semantics.
//!
//! This crate is the single portable authority for incremental VT parsing and
//! canonical terminal state. It owns no PTY, runtime IPC, renderer or native UI
//! behavior. RILL is historical implementation evidence only; Seyal semantics
//! are defined by the current architecture, M001 specification and tests.

mod active_grapheme;
mod cell;
mod color;
mod cursor;
mod damage;
mod error;
mod grapheme_store;
mod history;
mod line;
mod modes;
mod mouse;
mod parser;
mod presentation;
mod protocol_reply;
mod screen;
mod selection;
mod style;
mod terminal;
mod unicode_version;
mod width;

#[cfg(feature = "test-fault-injection")]
#[doc(hidden)]
pub mod test_fault;

pub use cell::{Cell, CellRole};
pub use color::Color;
pub use cursor::CursorState;
pub use damage::Damage;
pub use error::TerminalError;
pub use grapheme_store::{MAX_ACTIVE_GRAPHEME_BYTES, MAX_LIVE_VARIABLE_BYTES};
pub use history::{
    HistoryAnchor, HistoryAnchorResolution, HistoryBreakAfter, HistoryMatch, HistoryRangeError,
    HistoryUnitView, HistoryWireCell, ReflowRow, HISTORY_PER_EXECUTION_BYTE_CAP,
    HISTORY_PER_EXECUTION_DERIVED_INDEX_CAP, HISTORY_RUNTIME_AGGREGATE_BYTE_CAP,
    HISTORY_RUNTIME_DERIVED_INDEX_CAP, HISTORY_SEGMENT_PAYLOAD_TARGET, HISTORY_TAIL_PAYLOAD_LIMIT,
};
pub use line::LineId;
pub use modes::{ModeState, MouseReporting};
pub use mouse::{
    apply_host_mouse_gesture, encode_mouse_report, mouse_event_admitted, mouse_takes_host_override,
    MouseEventKind, MouseReport,
};
pub use presentation::{
    HostPresentationEvent, PresentationPayload, MAX_HOST_PRESENTATION_EVENTS,
    MAX_PRESENTATION_PAYLOAD_BYTES,
};
pub use protocol_reply::{ProtocolReply, MAX_PROTOCOL_REPLIES, MAX_PROTOCOL_REPLY_BYTES};
pub use selection::{
    encode_paste, format_history_copy, CopyMode, CopyModeMotion, PasteError, SearchSession,
    SelectionKind, SelectionSession, VisualPos, MAX_COPY_BYTES, MAX_PASTE_BYTES,
    MAX_SEARCH_MATCHES,
};
pub use style::Style;
pub use terminal::{
    Diagnostics, PreparedResize, SessionShellKind, ShellIntegrationEvent, ShellIntegrationToken,
    TerminalState, MAX_TERMINAL_COLUMNS, MAX_TERMINAL_ROWS,
};
pub use unicode_version::UNICODE_SEMANTIC_VERSION;
pub use width::AmbiguousWidthPolicy;
