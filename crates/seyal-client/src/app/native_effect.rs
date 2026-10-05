//! ADR-018 §2.4 native effects queued for the thin host (W3).

use seyal_core::WindowId;

use crate::shell::ShellNativeEffect;

/// Typed native effect returned to the host after a committed product transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeEffect {
    BoundedDetachThenTerminate,
    RealizeWindow { window: WindowId },
    DestroyWindowRealization { window: WindowId },
    OrderFrontMakeKey { window: WindowId },
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
            Self::BoundedDetachThenTerminate => 1,
            Self::RealizeWindow { .. } => 2,
            Self::DestroyWindowRealization { .. } => 3,
            Self::OrderFrontMakeKey { .. } => 4,
        }
    }

    pub fn window(self) -> Option<WindowId> {
        match self {
            Self::BoundedDetachThenTerminate => None,
            Self::RealizeWindow { window }
            | Self::DestroyWindowRealization { window }
            | Self::OrderFrontMakeKey { window } => Some(window),
        }
    }
}
