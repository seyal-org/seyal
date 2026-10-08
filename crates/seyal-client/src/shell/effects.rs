//! ADR-018 §2.4 native effects produced by shell commits (W3 / W6).

use seyal_core::{ExecutionId, WindowId};

/// Typed native effect for unavoidable `NSApplication`/`NSWindow` operations
/// and explicit execution disposition (ADR-018 §3.3).
///
/// Emitted in commit order by successful shell actions. The host realizes them;
/// Rust remains the sole product authority (ADR-015 / ADR-018 §2.5).
/// `TerminateExecution` is realized by the existing ADR-005 Runtime path — it
/// is never implied by tab/window destruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellNativeEffect {
    RealizeWindow { window: WindowId },
    DestroyWindowRealization { window: WindowId },
    OrderFrontMakeKey { window: WindowId },
    TerminateExecution { execution: ExecutionId },
}
