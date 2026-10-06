//! Versioned/size-tagged encode for [`ResourceAddress`] (ADR-015 / SPEC-022 R11.2).

use super::{
    decode_resource_address, NavigationRejection, ResourceAddress, RESOURCE_ADDRESS_ABI_VERSION,
    RESOURCE_ADDRESS_KIND_EXECUTION, RESOURCE_ADDRESS_KIND_PANE, RESOURCE_ADDRESS_KIND_TAB,
    RESOURCE_ADDRESS_KIND_WORKSPACE,
};

/// Maximum payload bytes for any M003 address kind (Pane = three UUIDs).
pub const RESOURCE_ADDRESS_MAX_PAYLOAD: usize = 48;

/// Encode `address` as `(version, kind, payload)` for FFI row/action records.
pub fn encode_resource_address(
    address: ResourceAddress,
) -> (u16, u16, [u8; RESOURCE_ADDRESS_MAX_PAYLOAD], u16) {
    let mut payload = [0_u8; RESOURCE_ADDRESS_MAX_PAYLOAD];
    match address {
        ResourceAddress::Workspace { workspace } => {
            payload[0..16].copy_from_slice(&workspace.to_bytes());
            (
                RESOURCE_ADDRESS_ABI_VERSION,
                RESOURCE_ADDRESS_KIND_WORKSPACE,
                payload,
                16,
            )
        }
        ResourceAddress::Tab { workspace, tab } => {
            payload[0..16].copy_from_slice(&workspace.to_bytes());
            payload[16..32].copy_from_slice(&tab.to_bytes());
            (
                RESOURCE_ADDRESS_ABI_VERSION,
                RESOURCE_ADDRESS_KIND_TAB,
                payload,
                32,
            )
        }
        ResourceAddress::Pane {
            workspace,
            tab,
            pane,
        } => {
            payload[0..16].copy_from_slice(&workspace.to_bytes());
            payload[16..32].copy_from_slice(&tab.to_bytes());
            payload[32..48].copy_from_slice(&pane.to_bytes());
            (
                RESOURCE_ADDRESS_ABI_VERSION,
                RESOURCE_ADDRESS_KIND_PANE,
                payload,
                48,
            )
        }
        ResourceAddress::Execution { execution } => {
            payload[0..16].copy_from_slice(&execution.to_bytes());
            (
                RESOURCE_ADDRESS_ABI_VERSION,
                RESOURCE_ADDRESS_KIND_EXECUTION,
                payload,
                16,
            )
        }
    }
}

/// Pack a ResourceAddress for AttentionTarget.resource_address (opaque store bytes).
pub fn pack_resource_address(address: ResourceAddress) -> Vec<u8> {
    let (version, kind, payload, len) = encode_resource_address(address);
    let mut out = Vec::with_capacity(6 + len as usize);
    out.extend_from_slice(&version.to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&payload[..len as usize]);
    out
}

/// Unpack AttentionTarget.resource_address. Unknown layout fails closed.
pub fn unpack_resource_address(bytes: &[u8]) -> Result<ResourceAddress, NavigationRejection> {
    if bytes.len() < 6 {
        return Err(NavigationRejection::UnsupportedKind);
    }
    let version = u16::from_le_bytes([bytes[0], bytes[1]]);
    let kind = u16::from_le_bytes([bytes[2], bytes[3]]);
    let len = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
    if bytes.len() != 6 + len {
        return Err(NavigationRejection::UnsupportedKind);
    }
    decode_resource_address(version, kind, &bytes[6..])
}
