//! ADR-018 §2.4 native effects produced by shell commits (W3).

use seyal_core::WindowId;

/// Typed native effect for unavoidable `NSApplication`/`NSWindow` operations.
///
/// Emitted in commit order by successful shell actions. The host realizes them;
/// Rust remains the sole product authority (ADR-015 / ADR-018 §2.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellNativeEffect {
    RealizeWindow {
        window: WindowId,
    },
    DestroyWindowRealization {
        window: WindowId,
    },
    OrderFrontMakeKey {
        window: WindowId,
    },
    /// Navigate named this window; host realizes key/front only (SPEC-022 §5).
    WindowActivation {
        window: WindowId,
    },
}
