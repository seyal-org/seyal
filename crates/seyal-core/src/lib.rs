//! Stable Seyal identity/value types shared across authority and protocol layers.
//!
//! This crate owns no PTY, VT, Runtime registry, renderer, transport, or UI
//! reducer. Headed composition IDs (`WindowId`, `TabId`, `PaneId`) are value
//! types only.

use std::{
    fmt,
    sync::{
        atomic::{AtomicU64, Ordering},
        OnceLock,
    },
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static PROCESS_ID_PREFIX: OnceLock<u64> = OnceLock::new();
const DEFAULT_WORKSPACE: u128 = 0x5345_5941_4c2d_4d30_3031_2d57_4f52_4b01;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RuntimeId(u128);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WorkspaceId(u128);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExecutionId(u128);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AttachmentId(u128);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ProjectionId(u128);

/// Durable-semantics Workspace metadata identity for one Block record.
///
/// This value type owns no Block lifecycle or persistence authority. Runtime /
/// Workspace composition decides when a Block exists. Generation deliberately
/// uses the same process-unique namespace source as Seyal's other fresh opaque
/// identities rather than a Runtime-local counter, so a new Runtime incarnation
/// does not intentionally reuse a prior Workspace Block identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(u128);

/// Headed composition identity for one Tab. This is not a Runtime, PTY, or VT
/// authority. Runtime `WorkspaceId` remains the execution-workspace identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabId(u128);

/// Headed composition identity for one Pane. Binding an `ExecutionId` does not
/// move PTY/VT ownership into the product shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PaneId(u128);

/// Headed composition identity for one Window. Opaque and process-unique.
/// It is not an `NSWindow` number, index, screen, or Space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowId(u128);

// Fresh authority identities intentionally have no `Default`: default
// construction would hide a stateful identity-generation side effect.
#[allow(clippy::new_without_default)]
impl RuntimeId {
    /// Create a process-local Runtime identity. Runtime remains the owner of
    /// when identities are created; this value crate only provides generation.
    pub fn new() -> Self {
        Self(unique_id(0x5255_4e54_494d_4501))
    }
}

impl WorkspaceId {
    pub const fn m001_default() -> Self {
        Self(DEFAULT_WORKSPACE)
    }
}

// See the RuntimeId rationale above: a fresh identity is not a default value.
#[allow(clippy::new_without_default)]
impl ExecutionId {
    /// Create a process-local execution identity. Runtime remains the authority
    /// that decides lifecycle/admission for the resulting identity.
    pub fn new() -> Self {
        Self(unique_id(0x4558_4543_5554_4501))
    }
}

// See the RuntimeId rationale above: a fresh identity is not a default value.
#[allow(clippy::new_without_default)]
impl AttachmentId {
    /// Create a process-local attachment identity. This does not grant any
    /// attachment authority by itself.
    pub fn new() -> Self {
        Self(unique_id(0x4154_5441_4348_0001))
    }
}

// Block identity is durable Workspace metadata semantics, not a Runtime-local
// sequence. Fresh generation therefore has no `Default` and uses a distinct
// globally namespaced domain from Runtime/Execution/Attachment identities.
#[allow(clippy::new_without_default)]
impl BlockId {
    pub fn new() -> Self {
        Self(unique_id(0x424c_4f43_4b00_0001))
    }
}

#[allow(clippy::new_without_default)]
impl TabId {
    pub fn new() -> Self {
        Self(unique_id(0x5441_4200_0000_0001))
    }
}

#[allow(clippy::new_without_default)]
impl PaneId {
    pub fn new() -> Self {
        Self(unique_id(0x5041_4e45_0000_0001))
    }
}

#[allow(clippy::new_without_default)]
impl WindowId {
    pub fn new() -> Self {
        Self(unique_id(0x5749_4e44_4f57_0001))
    }
}

macro_rules! impl_id_wire_bytes {
    ($type:ty) => {
        impl $type {
            pub fn to_bytes(self) -> [u8; 16] {
                self.0.to_le_bytes()
            }

            pub fn from_bytes(bytes: [u8; 16]) -> Self {
                Self(u128::from_le_bytes(bytes))
            }
        }
    };
}

impl_id_wire_bytes!(RuntimeId);
impl_id_wire_bytes!(WorkspaceId);
impl_id_wire_bytes!(ExecutionId);
impl_id_wire_bytes!(AttachmentId);
impl_id_wire_bytes!(ProjectionId);
impl_id_wire_bytes!(BlockId);
impl_id_wire_bytes!(TabId);
impl_id_wire_bytes!(PaneId);
impl_id_wire_bytes!(WindowId);

fn unique_id(domain: u64) -> u128 {
    let sequence = NEXT_ID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .expect("Seyal process-local identifier sequence exhausted");
    compose_unique_id(process_id_prefix(), domain, sequence)
}

fn process_id_prefix() -> u64 {
    *PROCESS_ID_PREFIX.get_or_init(|| {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let low = nanos as u64;
        let high = (nanos >> 64) as u64;
        let pid = std::process::id() as u64;
        let address = (&NEXT_ID as *const AtomicU64 as usize) as u64;
        mix64(low ^ high.rotate_left(17) ^ pid.rotate_left(31) ^ address)
    })
}

fn compose_unique_id(process_prefix: u64, domain: u64, sequence: u64) -> u128 {
    let namespace = mix64(process_prefix ^ domain);
    ((namespace as u128) << 64) | sequence as u128
}

fn mix64(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

macro_rules! impl_id_display {
    ($type:ty) => {
        impl fmt::Display for $type {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{:032x}", self.0)
            }
        }
    };
}

impl_id_display!(RuntimeId);
impl_id_display!(WorkspaceId);
impl_id_display!(ExecutionId);
impl_id_display!(AttachmentId);
impl_id_display!(ProjectionId);
impl_id_display!(BlockId);
impl_id_display!(TabId);
impl_id_display!(PaneId);
impl_id_display!(WindowId);

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn runtime_and_execution_ids_do_not_reuse_process_local_counter_values() {
        let first_runtime = RuntimeId::new();
        let second_runtime = RuntimeId::new();
        let first_execution = ExecutionId::new();
        let second_execution = ExecutionId::new();
        assert_ne!(first_runtime, second_runtime);
        assert_ne!(first_execution, second_execution);
        assert_ne!(first_runtime.to_string(), first_execution.to_string());
    }

    #[test]
    fn composition_keeps_sequence_changes_disjoint_from_prefix_changes() {
        let prefix = 0x0123_4567_89ab_cdef;
        let domain = 0x4154_5441_4348_0001;
        let first = compose_unique_id(prefix, domain, 1);
        let second = compose_unique_id(prefix, domain, 2);
        assert_ne!(first, second);
        assert_eq!(first as u64, 1);
        assert_eq!(second as u64, 2);
    }

    #[test]
    fn window_ids_are_unique_across_a_batch() {
        let mut seen = HashSet::with_capacity(64);
        for _ in 0..64 {
            assert!(seen.insert(WindowId::new()));
        }
    }

    #[test]
    fn rapid_attachment_ids_are_unique() {
        let mut seen = HashSet::with_capacity(100_000);
        for _ in 0..100_000 {
            assert!(seen.insert(AttachmentId::new()));
        }
    }

    #[test]
    fn block_ids_are_unique_and_disjoint_from_runtime_identity_domain() {
        let mut seen = HashSet::with_capacity(100_000);
        for _ in 0..100_000 {
            assert!(seen.insert(BlockId::new()));
        }
        let block = BlockId::new();
        let runtime = RuntimeId::new();
        assert_ne!(block.to_string(), runtime.to_string());
    }

    #[test]
    fn block_id_generation_namespace_changes_across_runtime_like_prefixes() {
        let domain = 0x424c_4f43_4b00_0001;
        let prior_runtime_like_prefix = 0x1111_2222_3333_4444;
        let later_runtime_like_prefix = 0x5555_6666_7777_8888;
        let prior = compose_unique_id(prior_runtime_like_prefix, domain, 1);
        let later = compose_unique_id(later_runtime_like_prefix, domain, 1);
        assert_ne!(prior, later);
    }

    #[test]
    fn default_workspace_identity_is_runtime_independent() {
        let before = WorkspaceId::m001_default();
        let _runtime_a = RuntimeId::new();
        let _runtime_b = RuntimeId::new();
        assert_eq!(before, WorkspaceId::m001_default());
    }

    #[test]
    fn wire_ids_round_trip_through_raw_little_endian_bytes() {
        let runtime = RuntimeId::new();
        let workspace = WorkspaceId::m001_default();
        let execution = ExecutionId::new();
        let attachment = AttachmentId::new();
        let projection = ProjectionId::from_bytes([0x5a; 16]);
        let block = BlockId::new();
        assert_eq!(RuntimeId::from_bytes(runtime.to_bytes()), runtime);
        assert_eq!(WorkspaceId::from_bytes(workspace.to_bytes()), workspace);
        assert_eq!(ExecutionId::from_bytes(execution.to_bytes()), execution);
        assert_eq!(AttachmentId::from_bytes(attachment.to_bytes()), attachment);
        assert_eq!(ProjectionId::from_bytes(projection.to_bytes()), projection);
        assert_eq!(BlockId::from_bytes(block.to_bytes()), block);
        let tab = TabId::new();
        let pane = PaneId::new();
        let window = WindowId::new();
        assert_eq!(TabId::from_bytes(tab.to_bytes()), tab);
        assert_eq!(PaneId::from_bytes(pane.to_bytes()), pane);
        assert_eq!(WindowId::from_bytes(window.to_bytes()), window);
        assert_ne!(window.to_string(), tab.to_string());
        assert_ne!(tab.to_string(), pane.to_string());
        assert_ne!(tab.to_string(), block.to_string());
    }
}
