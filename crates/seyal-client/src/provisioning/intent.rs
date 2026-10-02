//! Provisioning intent records and bootstrap geometry (ADR-017 §5.2).

use seyal_core::{AttachmentId, ExecutionId, PaneId};
use seyal_protocol::framing::ErrorCode;

use super::{ConnectionOwner, ProvisioningFailure};

/// Documented bootstrap geometry when a Pane has no laid-out cell size yet.
pub const BOOTSTRAP_ROWS: u16 = 24;
pub const BOOTSTRAP_COLUMNS: u16 = 80;

const MAX_ROWS: u16 = 256;
const MAX_COLUMNS: u16 = 512;
const MAX_CELLS: u32 = 131_072;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaneGeometry {
    pub rows: u16,
    pub columns: u16,
}

impl PaneGeometry {
    pub fn validate(self) -> Result<(), ProvisioningFailure> {
        if self.rows == 0 || self.columns == 0 {
            return Err(ProvisioningFailure::CreateRejected(
                ErrorCode::InvalidGeometry,
            ));
        }
        if self.rows > MAX_ROWS || self.columns > MAX_COLUMNS {
            return Err(ProvisioningFailure::CreateRejected(
                ErrorCode::InvalidGeometry,
            ));
        }
        let cells = u32::from(self.rows).saturating_mul(u32::from(self.columns));
        if cells > MAX_CELLS {
            return Err(ProvisioningFailure::CreateRejected(
                ErrorCode::InvalidGeometry,
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntentPhase {
    AwaitingCreate,
    Created {
        execution: ExecutionId,
    },
    Attaching {
        execution: ExecutionId,
    },
    Attached {
        execution: ExecutionId,
        attachment: AttachmentId,
    },
    /// Controller attach solely to dispose, or terminate-in-flight.
    Disposing {
        execution: ExecutionId,
        attached: bool,
    },
    Bound {
        execution: ExecutionId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingIntent {
    pub pane: PaneId,
    pub owner: ConnectionOwner,
    pub request_id: u64,
    pub geometry: PaneGeometry,
    /// True when create used bootstrap geometry and a resize is owed after bind.
    pub needs_bootstrap_resize: bool,
    pub intent_alive: bool,
    pub phase: IntentPhase,
    pub attachment: Option<AttachmentId>,
}
