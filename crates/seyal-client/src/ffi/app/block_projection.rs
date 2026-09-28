//! Flow Block output projection FFI export (#865).
//!
//! Rust owns the clip mapping from Runtime `ViewportLineIds` onto prepared
//! rows; the host only draws the row range it receives and never pairs,
//! stores, or infers LineIds.

use seyal_core::{AttachmentId, ExecutionId};

use crate::app::PresentationEligibility;
use crate::composer::BlockPresentationState;
use crate::live_tail::{project_block_output, LiveTailProjection};
use crate::presentation::PresentationMode;
use crate::LocalDisplayClient;

use super::APPS;

/// Fail closed: host must not invent a history range or draw terminal pixels.
pub const SEYAL_APP_BLOCK_PROJECTION_FAIL_CLOSED: u16 = 0;
/// Completed Block: request the inclusive trusted history span.
pub const SEYAL_APP_BLOCK_PROJECTION_HISTORY: u16 = 1;
/// Running Block: clip the damage-driven prepared primary frame into the
/// Block output region. Not a Pane-wide live grid.
pub const SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP: u16 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppBlockProjection {
    pub kind: u16,
    /// For [`SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP`]: first prepared-frame row.
    pub reserved0: u16,
    /// For [`SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP`]: prepared-frame row count.
    pub reserved1: u32,
    pub start_line: u64,
    /// Inclusive end for [`SEYAL_APP_BLOCK_PROJECTION_HISTORY`]; zero otherwise.
    pub end_line: u64,
}

impl SeyalAppBlockProjection {
    const fn fail_closed() -> Self {
        Self {
            kind: SEYAL_APP_BLOCK_PROJECTION_FAIL_CLOSED,
            reserved0: 0,
            reserved1: 0,
            start_line: 0,
            end_line: 0,
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_block_projection(handle: u64, index: u32) -> SeyalAppBlockProjection {
    APPS.with(|apps| {
        let apps = apps.borrow();
        let Some(state) = apps.get(&handle) else {
            return SeyalAppBlockProjection::fail_closed();
        };
        let fence = state.root.fence();
        let snapshot = state.root.snapshot();
        let Some(block) = snapshot
            .composer
            .as_ref()
            .and_then(|composer| composer.blocks.get(index as usize))
        else {
            return SeyalAppBlockProjection::fail_closed();
        };
        let mode = match snapshot.eligibility {
            PresentationEligibility::Flow => PresentationMode::Flow,
            PresentationEligibility::Raw => PresentationMode::Raw,
            PresentationEligibility::Tui => PresentationMode::Tui,
            PresentationEligibility::Unbound => {
                return SeyalAppBlockProjection::fail_closed();
            }
        };
        let running = block.state == BlockPresentationState::Running;
        // Running PRIMARY_CLIP needs ViewportLineIds from the Pane's owning
        // client (matched by fence attachment/execution), never another Pane's
        // ambient ACTIVE_HANDLE. History/fail-closed paths need no client.
        let projection = if running {
            with_fence_matched_client(fence.execution, fence.attachment, |client| {
                project_block_output(
                    mode,
                    block.start_line,
                    block.end_line,
                    true,
                    paired_viewport_line_ids(client),
                )
            })
            .unwrap_or(LiveTailProjection::FailClosed)
        } else {
            project_block_output(mode, block.start_line, block.end_line, false, &[])
        };
        match projection {
            LiveTailProjection::PrimaryFrame(clip) => SeyalAppBlockProjection {
                kind: SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP,
                reserved0: clip.first_row,
                reserved1: u32::from(clip.row_count),
                start_line: clip.start_line,
                end_line: 0,
            },
            LiveTailProjection::History(span) => SeyalAppBlockProjection {
                kind: SEYAL_APP_BLOCK_PROJECTION_HISTORY,
                reserved0: 0,
                reserved1: 0,
                start_line: span.start_line,
                end_line: span.end_line,
            },
            LiveTailProjection::FailClosed => SeyalAppBlockProjection::fail_closed(),
        }
    })
}

/// Resolve the display client that owns this AppRoot fence. Prefers the active
/// handle when it matches; otherwise scans registered clients.
fn with_fence_matched_client<R>(
    execution: Option<ExecutionId>,
    attachment: Option<AttachmentId>,
    operation: impl FnOnce(&LocalDisplayClient) -> R,
) -> Option<R> {
    let (Some(execution), Some(attachment)) = (execution, attachment) else {
        return None;
    };
    crate::ffi::CLIENTS.with(|clients| {
        let clients = clients.borrow();
        let active = crate::ffi::active_handle();
        let owns =
            |client: &LocalDisplayClient| client.execution_id() == execution && client.attachment_id() == attachment;
        if let Some(client) = clients.get(&active).filter(|client| owns(client)) {
            return Some(operation(client));
        }
        clients
            .iter()
            .find(|(handle, client)| **handle != active && owns(client))
            .map(|(_, client)| operation(client))
    })
}

/// Borrow the client's LineIds only when they pair with the committed display
/// by generation and row count; otherwise the running clip fails closed.
fn paired_viewport_line_ids(client: &LocalDisplayClient) -> &[u64] {
    let ids = client.viewport_line_ids();
    let cache = client.cache();
    let generation = client.viewport_line_ids_generation();
    if generation == 0
        || cache.rows == 0
        || generation != cache.generation
        || ids.len() != usize::from(cache.rows)
    {
        return &[];
    }
    ids
}

#[cfg(test)]
mod tests {
    use std::{
        mem::{offset_of, size_of},
        ptr,
    };

    use seyal_core::BlockId;
    use seyal_protocol::framing::Role;

    use super::super::{
        allocate_handle, seyal_app_apply, seyal_app_create, seyal_app_destroy, seyal_app_snapshot,
        SeyalAppAction, FLAG_TARGET_CONTROLLER,
    };
    use super::*;
    use crate::app::{AppAction, APP_ABI_VERSION};
    use crate::composer::RuntimeBlockRecord;

    fn owned_client(execution_lo: u8, attachment_lo: u8, generation: u64, ids: Vec<u64>) -> u64 {
        let mut execution = [0u8; 16];
        execution[0] = execution_lo;
        let mut attachment = [0u8; 16];
        attachment[0] = attachment_lo;
        let mut client = crate::local::reconstruction_probe_client(
            Role::Controller,
            ids.len() as u16,
            1,
            1,
            ExecutionId::from_bytes(execution),
            AttachmentId::from_bytes(attachment),
        );
        client.set_paired_viewport_line_ids_for_test(generation, ids);
        let handle = allocate_handle();
        crate::ffi::CLIENTS.with(|clients| {
            clients.borrow_mut().insert(handle, Box::new(client));
        });
        handle
    }

    #[test]
    fn block_projection_abi_layout_is_stable() {
        assert_eq!(size_of::<SeyalAppBlockProjection>(), 24);
        assert_eq!(offset_of!(SeyalAppBlockProjection, kind), 0);
        assert_eq!(offset_of!(SeyalAppBlockProjection, reserved0), 2);
        assert_eq!(offset_of!(SeyalAppBlockProjection, reserved1), 4);
        assert_eq!(offset_of!(SeyalAppBlockProjection, start_line), 8);
        assert_eq!(offset_of!(SeyalAppBlockProjection, end_line), 16);
    }

    #[test]
    fn block_projection_ffi_uses_primary_clip_for_running_and_history_for_completed() {
        let handle = seyal_app_create();
        let snap = seyal_app_snapshot(handle);
        let bind = SeyalAppAction {
            version: APP_ABI_VERSION,
            size: size_of::<SeyalAppAction>() as u16,
            kind: 1,
            flags: FLAG_TARGET_CONTROLLER,
            fence_pane_lo: snap.pane_lo,
            fence_pane_hi: snap.pane_hi,
            fence_execution_lo: 0,
            fence_execution_hi: 0,
            fence_attachment_lo: 0,
            fence_attachment_hi: 0,
            fence_epoch: snap.epoch,
            target_execution_lo: 1,
            target_execution_hi: 0,
            target_attachment_lo: 2,
            target_attachment_hi: 0,
            target_pty_generation: 1,
            payload: ptr::null(),
            payload_len: 0,
            reserved: 0,
        };
        assert_eq!(unsafe { seyal_app_apply(handle, &bind) }, 0);
        assert_eq!(seyal_app_snapshot(handle).eligibility, 1, "Flow");

        APPS.with(|apps| {
            let mut apps = apps.borrow_mut();
            let state = apps.get_mut(&handle).unwrap();
            let fence = state.root.fence();
            state
                .root
                .apply(AppAction::ApplyRuntimeBlocks {
                    fence,
                    records: vec![
                        RuntimeBlockRecord {
                            id: BlockId::from_bytes([0x21; 16]),
                            command: "printf hello".into(),
                            start_line: 10,
                            end_line: Some(12),
                            running: false,
                            exit_status: Some(0),
                        },
                        RuntimeBlockRecord {
                            id: BlockId::from_bytes([0x22; 16]),
                            command: "seq 1 1000".into(),
                            start_line: 20,
                            end_line: None,
                            running: true,
                            exit_status: None,
                        },
                    ],
                })
                .unwrap();
        });

        let completed = seyal_app_block_projection(handle, 0);
        assert_eq!(completed.kind, SEYAL_APP_BLOCK_PROJECTION_HISTORY);
        assert_eq!(completed.start_line, 10);
        assert_eq!(completed.end_line, 12);

        let running = seyal_app_block_projection(handle, 1);
        assert_eq!(running.kind, SEYAL_APP_BLOCK_PROJECTION_FAIL_CLOSED);
        assert_eq!(running.end_line, 0, "must not invent a history end");

        let owner = owned_client(1, 2, 7, vec![10, 20, 21, 22]);
        crate::ffi::ACTIVE_HANDLE.with(|active| active.set(0));
        let clipped = seyal_app_block_projection(handle, 1);
        assert_eq!(clipped.kind, SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP);
        assert_eq!(clipped.start_line, 20);
        assert_eq!(clipped.reserved0, 1, "skip preceding line 10");
        assert_eq!(clipped.reserved1, 3);
        assert_eq!(clipped.end_line, 0);

        let other = owned_client(0x99, 0xaa, 7, vec![20, 21, 22, 23]);
        crate::ffi::ACTIVE_HANDLE.with(|active| active.set(other));
        let still_clipped = seyal_app_block_projection(handle, 1);
        assert_eq!(still_clipped.kind, SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP);
        assert_eq!(
            still_clipped.reserved0, 1,
            "must keep owning Pane's mapping, not ambient peer's"
        );
        assert_eq!(still_clipped.reserved1, 3);

        crate::ffi::CLIENTS.with(|clients| {
            let mut clients = clients.borrow_mut();
            clients.remove(&owner);
            clients.remove(&other);
        });
        crate::ffi::ACTIVE_HANDLE.with(|active| active.set(0));
        assert_eq!(seyal_app_destroy(handle), 0);
    }
}
