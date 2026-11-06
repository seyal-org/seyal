//! Seyal-owned local terminal execution boundary.
//!
//! `TerminalExecution` owns one internal PTY endpoint/child lifecycle and one
//! authoritative `seyal_terminal::TerminalState`. `ExecutionReactor` composes
//! readiness only: it never owns or duplicates those execution resources.

mod child;
mod command;
mod endpoint;
mod error;
mod execution;
mod platform;
mod projection;
mod reactor;
mod readiness;
#[cfg(all(target_os = "macos", feature = "test-fault-injection"))]
#[doc(hidden)]
pub mod test_fault;
#[cfg(feature = "test-fault-injection")]
#[doc(hidden)]
pub mod terminal_test_fault {
    pub use seyal_terminal::test_fault::*;
}
mod winsize;

pub use child::{ChildExit, SignalDisposition, TerminationPolicy};
pub use command::CommandSpec;
pub use endpoint::{ReadOutcome, WriteOutcome};
pub use error::ExecError;
pub use execution::TerminalExecution;
// Terminal semantic values cross the execution boundary only as borrowed
// event/color/line identities. Re-exporting them here keeps Runtime dependent
// on the PTY execution boundary instead of reaching through to the terminal
// crate directly.
pub use projection::{
    ProjectionAttributes, ProjectionCell, ProjectionColor, ProjectionDamage,
    TerminalProjectionSnapshot, TerminalProjectionUpdate,
};
pub use reactor::{
    ExecutionReactor, ReactorEvent, ReactorEventKind, ReactorWaker, RegistrationToken,
};
pub use readiness::Readiness;
pub use seyal_terminal::{
    apply_host_mouse_gesture, encode_mouse_report, mouse_event_admitted, mouse_takes_host_override,
    CellRole, Color, CopyModeMotion, HistoryRangeError, HistoryWireCell, HostPresentationEvent,
    LineId, ModeState, MouseEventKind, MouseReport, MouseReporting, PasteError,
    PresentationPayload, ProtocolReply, SessionShellKind, ShellIntegrationEvent,
    ShellIntegrationToken, VisualPos, HISTORY_PER_EXECUTION_DERIVED_INDEX_CAP,
    HISTORY_RUNTIME_AGGREGATE_BYTE_CAP, HISTORY_RUNTIME_DERIVED_INDEX_CAP,
};
pub use winsize::WindowSize;
