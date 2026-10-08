//! ADR-018 §2.4 native effects queued for the thin host (W3 / W6).

use seyal_core::{ExecutionId, PaneId, WindowId};

use crate::shell::ShellNativeEffect;

/// Typed native effect returned to the host after a committed product transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeEffect {
    BoundedDetachThenTerminate,
    RealizeWindow {
        window: WindowId,
    },
    DestroyWindowRealization {
        window: WindowId,
    },
    OrderFrontMakeKey {
        window: WindowId,
    },
    /// Explicit ADR-005 terminate for one live-unpresented execution (§3.3).
    TerminateExecution {
        execution: ExecutionId,
    },
    /// Palette/host intent: attach Runtime evidence, then commit via [`super::AppAction::Adopt`].
    /// Does not mutate shell bindings by itself.
    RequestAdoptAttach {
        pane: PaneId,
        execution: ExecutionId,
    },
}

impl From<ShellNativeEffect> for NativeEffect {
    fn from(effect: ShellNativeEffect) -> Self {
        match effect {
            ShellNativeEffect::RealizeWindow { window } => Self::RealizeWindow { window },
            ShellNativeEffect::DestroyWindowRealization { window } => {
                Self::DestroyWindowRealization { window }
            }
            ShellNativeEffect::OrderFrontMakeKey { window } => Self::OrderFrontMakeKey { window },
            ShellNativeEffect::TerminateExecution { execution } => {
                Self::TerminateExecution { execution }
            }
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
            Self::TerminateExecution { .. } => 5,
            Self::RequestAdoptAttach { .. } => 6,
        }
    }

    pub fn window(self) -> Option<WindowId> {
        match self {
            Self::BoundedDetachThenTerminate
            | Self::TerminateExecution { .. }
            | Self::RequestAdoptAttach { .. } => None,
            Self::RealizeWindow { window }
            | Self::DestroyWindowRealization { window }
            | Self::OrderFrontMakeKey { window } => Some(window),
        }
    }

    pub fn execution(self) -> Option<ExecutionId> {
        match self {
            Self::TerminateExecution { execution } | Self::RequestAdoptAttach { execution, .. } => {
                Some(execution)
            }
            _ => None,
        }
    }
}
