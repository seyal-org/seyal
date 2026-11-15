//! Typed local navigation addresses and versioned/size-tagged decode.

use seyal_core::{ExecutionId, PaneId, WorkspaceId};

use super::NavigationRejection;

/// Portable ABI version for the closed address set (ADR-029 drops Tab).
pub const RESOURCE_ADDRESS_ABI_VERSION: u16 = 2;

pub const RESOURCE_ADDRESS_KIND_WORKSPACE: u16 = 1;
/// Retired by ADR-029 / SPEC-032. Decode rejects this kind.
pub const RESOURCE_ADDRESS_KIND_TAB: u16 = 2;
pub const RESOURCE_ADDRESS_KIND_PANE: u16 = 3;
pub const RESOURCE_ADDRESS_KIND_EXECUTION: u16 = 4;

const WORKSPACE_PAYLOAD: usize = 16;
const PANE_PAYLOAD: usize = 32;
const EXECUTION_PAYLOAD: usize = 16;

/// Closed navigation identity. `Copy` by construction so it cannot own a
/// display string, path, ordinal, window handle, or PID (SPEC-022 R2.1).
/// Pane address is `WorkspaceId` + `PaneId` (ADR-029); `TabId` is not a
/// user-facing containment identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResourceAddress {
    Workspace {
        workspace: WorkspaceId,
    },
    Pane {
        workspace: WorkspaceId,
        pane: PaneId,
    },
    Execution {
        execution: ExecutionId,
    },
}

/// Decode a versioned/size-tagged address record without reading product state.
///
/// Unknown version, unknown kind, or mismatched payload size yields
/// [`NavigationRejection::UnsupportedKind`] before any shell or inventory read
/// (SPEC-022 R2.3).
pub fn decode_resource_address(
    version: u16,
    kind: u16,
    payload: &[u8],
) -> Result<ResourceAddress, NavigationRejection> {
    if version != RESOURCE_ADDRESS_ABI_VERSION {
        return Err(NavigationRejection::UnsupportedKind);
    }
    match kind {
        RESOURCE_ADDRESS_KIND_WORKSPACE => {
            let workspace = read_id(payload, WORKSPACE_PAYLOAD, 0)?;
            Ok(ResourceAddress::Workspace {
                workspace: WorkspaceId::from_bytes(workspace),
            })
        }
        RESOURCE_ADDRESS_KIND_TAB => Err(NavigationRejection::UnsupportedKind),
        RESOURCE_ADDRESS_KIND_PANE => {
            require_len(payload, PANE_PAYLOAD)?;
            Ok(ResourceAddress::Pane {
                workspace: WorkspaceId::from_bytes(read_exact(payload, 0)),
                pane: PaneId::from_bytes(read_exact(payload, 16)),
            })
        }
        RESOURCE_ADDRESS_KIND_EXECUTION => {
            let execution = read_id(payload, EXECUTION_PAYLOAD, 0)?;
            Ok(ResourceAddress::Execution {
                execution: ExecutionId::from_bytes(execution),
            })
        }
        _ => Err(NavigationRejection::UnsupportedKind),
    }
}

fn read_id(
    payload: &[u8],
    expected: usize,
    offset: usize,
) -> Result<[u8; 16], NavigationRejection> {
    require_len(payload, expected)?;
    Ok(read_exact(payload, offset))
}

fn require_len(payload: &[u8], expected: usize) -> Result<(), NavigationRejection> {
    if payload.len() != expected {
        return Err(NavigationRejection::UnsupportedKind);
    }
    Ok(())
}

fn read_exact(payload: &[u8], offset: usize) -> [u8; 16] {
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&payload[offset..offset + 16]);
    bytes
}
