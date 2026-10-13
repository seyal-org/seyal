//! ADR-018 §2.4 native effects queued for the thin host (W3 / W6).

use seyal_core::WindowId;

use crate::shell::ShellNativeEffect;

/// Relative quit-cleanup deadline in milliseconds. See ADR-018 §4.
pub const QUIT_CLEANUP_DEADLINE_MS: u64 = 500;

/// Typed native effect returned to the host after a committed product transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeEffect {
    BoundedDetachThenTerminate { deadline_ms: u64 },
    RealizeWindow { window: WindowId },
    DestroyWindowRealization { window: WindowId },
    OrderFrontMakeKey { window: WindowId },
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
            Self::QuitCleanupComplete => 7,
        }
    }

    pub fn window(self) -> Option<WindowId> {
        match self {
            Self::BoundedDetachThenTerminate { .. } | Self::QuitCleanupComplete => None,
            Self::RealizeWindow { window }
            | Self::DestroyWindowRealization { window }
            | Self::OrderFrontMakeKey { window } => Some(window),
        }
    }

    pub fn deadline_ms(self) -> u64 {
        match self {
            Self::BoundedDetachThenTerminate { deadline_ms } => deadline_ms,
            _ => 0,
        }
    }
}
