//! ADR-018 §2.4 native effects queued for the thin host (W3/W4a).

use seyal_core::WindowId;

use crate::shell::ShellNativeEffect;

/// Relative quit-cleanup deadline (ms) carried by [`NativeEffect::BoundedDetachThenTerminate`].
///
/// Derived from SPEC-009 detach cleanup p99 (250µs) with 2000× headroom for
/// renderer/GPU release across up to 16 attachments: 250µs × 2000 = 500ms.
/// ADR-018 §4 leaves the absolute value to the implementation child under the
/// performance-gate skill; revise only with recorded measurement.
pub const QUIT_CLEANUP_DEADLINE_MS: u64 = 500;

/// Typed native effect returned to the host after a committed product transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeEffect {
    /// Quit backstop deadline. `deadline_ms` is relative from emission.
    BoundedDetachThenTerminate {
        deadline_ms: u64,
    },
    RealizeWindow {
        window: WindowId,
    },
    DestroyWindowRealization {
        window: WindowId,
    },
    OrderFrontMakeKey {
        window: WindowId,
    },
    /// SPEC-022 R5.2: host must activate this window, not another.
    WindowActivation {
        window: WindowId,
    },
    /// Rust finished quit bookkeeping after the host acked BoundedDetachThenTerminate.
    QuitCleanupComplete,
}

impl From<ShellNativeEffect> for NativeEffect {
    fn from(effect: ShellNativeEffect) -> Self {
        match effect {
            ShellNativeEffect::RealizeWindow { window } => Self::RealizeWindow { window },
            ShellNativeEffect::DestroyWindowRealization { window } => {
                Self::DestroyWindowRealization { window }
            }
            ShellNativeEffect::OrderFrontMakeKey { window } => Self::OrderFrontMakeKey { window },
            ShellNativeEffect::WindowActivation { window } => Self::WindowActivation { window },
        }
    }
}

impl NativeEffect {
    /// ABI kind code carried by `SeyalAppSnapshot.pending_effect` / effect rows.
    pub fn kind_code(self) -> u32 {
        match self {
            Self::BoundedDetachThenTerminate { .. } => 1,
            Self::RealizeWindow { .. } => 2,
            Self::DestroyWindowRealization { .. } => 3,
            Self::OrderFrontMakeKey { .. } => 4,
            Self::QuitCleanupComplete => 5,
            Self::WindowActivation { .. } => 6,
        }
    }

    pub fn window(self) -> Option<WindowId> {
        match self {
            Self::BoundedDetachThenTerminate { .. } | Self::QuitCleanupComplete => None,
            Self::RealizeWindow { window }
            | Self::DestroyWindowRealization { window }
            | Self::OrderFrontMakeKey { window }
            | Self::WindowActivation { window } => Some(window),
        }
    }

    /// Relative deadline for kind 1; otherwise 0. Encoded in `window_lo` on the ABI row.
    pub fn deadline_ms(self) -> u64 {
        match self {
            Self::BoundedDetachThenTerminate { deadline_ms } => deadline_ms,
            _ => 0,
        }
    }
}
