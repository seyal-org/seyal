//! Flow Block output projection FFI export (#865).
//!
//! Rust owns the clip mapping from Runtime `ViewportLineIds` onto prepared
//! rows; the host only draws the row range it receives and never pairs,
//! stores, or infers LineIds. `seyal_app_block_span` remains the raw Runtime
//! anchor export; hosts must not invent `start + 511` from a zero end.

use seyal_core::{AttachmentId, ExecutionId};

use crate::app::PresentationEligibility;
use crate::composer::BlockPresentationState;
use crate::live_tail::{project_block_output_until, LiveTailProjection};
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
        let running = block.state == BlockPresentationState::Running;
        let exclusive_end = following_session_start(snapshot.composer.as_ref(), index as usize);
        let mode = match snapshot.eligibility {
            PresentationEligibility::Flow => PresentationMode::Flow,
            PresentationEligibility::Raw => PresentationMode::Raw,
            PresentationEligibility::Tui => PresentationMode::Tui,
            // Completed history does not need a live client. Unbound after an
            // ended execution must still paint retained Block bodies. A running
            // clip has no viewport here and stays fail-closed.
            PresentationEligibility::Unbound => {
                if running {
                    return SeyalAppBlockProjection::fail_closed();
                }
                PresentationMode::Flow
            }
        };
        // Running PRIMARY_CLIP needs ViewportLineIds from the Pane's owning
        // client (matched by fence attachment/execution), never another Pane's
        // ambient ACTIVE_HANDLE. History/fail-closed paths need no client.
        let projection = if running {
            with_fence_matched_client(fence.execution, fence.attachment, |client| {
                project_block_output_until(
                    mode,
                    block.start_line,
                    block.end_line,
                    true,
                    paired_viewport_line_ids(client),
                    exclusive_end,
                )
            })
            .unwrap_or(LiveTailProjection::FailClosed)
        } else {
            project_block_output_until(mode, block.start_line, block.end_line, false, &[], None)
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
        let owns = |client: &LocalDisplayClient| {
            client.execution_id() == execution && client.attachment_id() == attachment
        };
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
/// First session Block after a local Running host. Its start line is the
/// exclusive end of the host live clip so one terminal line is one card.
fn following_session_start(
    composer: Option<&crate::composer::ComposerSnapshot>,
    index: usize,
) -> Option<u64> {
    use crate::composer::BlockPresentationState;
    use seyal_protocol::framing::BlockScope;
    let composer = composer?;
    let host = composer.blocks.get(index)?;
    if host.scope != BlockScope::Local || host.state != BlockPresentationState::Running {
        return None;
    }
    composer.blocks[index + 1..]
        .iter()
        .find(|block| block.scope == BlockScope::Session)
        .map(|block| block.start_line)
}

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
            target_generation: 1,
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
                    records: Some(vec![
                        RuntimeBlockRecord {
                            id: BlockId::from_bytes([0x21; 16]),
                            command: "printf hello".into(),
                            start_line: 10,
                            end_line: Some(12),
                            running: false,
                            exit_status: Some(0),
                            canvas_floor: false,
                            scope: seyal_protocol::framing::BlockScope::Local,
                        },
                        RuntimeBlockRecord {
                            id: BlockId::from_bytes([0x22; 16]),
                            command: "seq 1 1000".into(),
                            start_line: 20,
                            end_line: None,
                            running: true,
                            exit_status: None,
                            canvas_floor: false,
                            scope: seyal_protocol::framing::BlockScope::Local,
                        },
                    ]),
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

    #[test]
    fn ended_and_unbound_completed_blocks_project_history() {
        use std::time::Duration;

        use crate::app::{BindingEvidence, PresentationEligibility};
        use crate::recovery::AttemptOutcome;
        use seyal_core::{AttachmentId, ExecutionId};

        let handle = seyal_app_create();
        APPS.with(|apps| {
            let mut apps = apps.borrow_mut();
            let state = apps.get_mut(&handle).unwrap();
            let fence = state.root.fence();
            state
                .root
                .apply(AppAction::Bind {
                    fence,
                    evidence: BindingEvidence {
                        execution: ExecutionId::from_bytes([4; 16]),
                        attachment: AttachmentId::from_bytes([5; 16]),
                        controller: true,
                        pty_generation: 1,
                        alternate_screen: false,
                    },
                })
                .unwrap();
            let fence = state.root.fence();
            state
                .root
                .apply(AppAction::ApplyRuntimeBlocks {
                    fence,
                    records: Some(vec![
                        RuntimeBlockRecord {
                            id: BlockId::from_bytes([0x31; 16]),
                            command: "printf exiting".into(),
                            start_line: 10,
                            end_line: Some(12),
                            running: false,
                            exit_status: Some(0),
                            canvas_floor: false,
                            scope: seyal_protocol::framing::BlockScope::Local,
                        },
                        RuntimeBlockRecord {
                            id: BlockId::from_bytes([0x32; 16]),
                            command: "still running".into(),
                            start_line: 20,
                            end_line: None,
                            running: true,
                            exit_status: None,
                            canvas_floor: false,
                            scope: seyal_protocol::framing::BlockScope::Local,
                        },
                    ]),
                })
                .unwrap();
            state
                .root
                .apply(AppAction::BeginRecovery {
                    now: Duration::ZERO,
                })
                .unwrap();
            let generation = state.root.snapshot().recovery_generation;
            state
                .root
                .apply(AppAction::CompleteRecovery {
                    generation,
                    outcome: AttemptOutcome::ExecutionEnded,
                    now: Duration::from_millis(1),
                    launch: None,
                })
                .unwrap();
            assert_eq!(
                state.root.snapshot().eligibility,
                PresentationEligibility::Flow
            );
        });

        let completed = seyal_app_block_projection(handle, 0);
        assert_eq!(completed.kind, SEYAL_APP_BLOCK_PROJECTION_HISTORY);
        assert_eq!(completed.start_line, 10);
        assert_eq!(completed.end_line, 12);
        // ExecutionEnded settles the Running Block with end=start so History
        // projects instead of FailClosed (clear→exit must not collapse bodies).
        let settled = seyal_app_block_projection(handle, 1);
        assert_eq!(settled.kind, SEYAL_APP_BLOCK_PROJECTION_HISTORY);
        assert_eq!(settled.start_line, 20);
        assert_eq!(settled.end_line, 20);

        APPS.with(|apps| {
            let mut apps = apps.borrow_mut();
            let state = apps.get_mut(&handle).unwrap();
            state
                .root
                .apply(AppAction::CreateReplacementExecution {
                    execution: None,
                    attachment: None,
                    pty_generation: 0,
                })
                .unwrap();
            assert_eq!(
                state.root.snapshot().eligibility,
                PresentationEligibility::Unbound
            );
        });
        let retained = seyal_app_block_projection(handle, 0);
        assert_eq!(retained.kind, SEYAL_APP_BLOCK_PROJECTION_HISTORY);
        assert_eq!(retained.start_line, 10);
        assert_eq!(retained.end_line, 12);
        let settled_unbound = seyal_app_block_projection(handle, 1);
        assert_eq!(settled_unbound.kind, SEYAL_APP_BLOCK_PROJECTION_HISTORY);
        assert_eq!(settled_unbound.start_line, 20);
        assert_eq!(settled_unbound.end_line, 20);
        assert_eq!(seyal_app_destroy(handle), 0);
    }

    #[test]
    fn session_blocks_under_a_running_host_do_not_share_a_terminal_line() {
        use seyal_protocol::framing::BlockScope;

        use super::super::encode::BLOCK_SESSION;
        use super::super::seyal_app_block_row;

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
            target_execution_lo: 3,
            target_execution_hi: 0,
            target_attachment_lo: 4,
            target_attachment_hi: 0,
            target_generation: 1,
            payload: ptr::null(),
            payload_len: 0,
            reserved: 0,
        };
        assert_eq!(unsafe { seyal_app_apply(handle, &bind) }, 0);
        APPS.with(|apps| {
            let mut apps = apps.borrow_mut();
            let state = apps.get_mut(&handle).unwrap();
            let fence = state.root.fence();
            state
                .root
                .apply(AppAction::ApplyRuntimeBlocks {
                    fence,
                    records: Some(vec![
                        RuntimeBlockRecord {
                            id: BlockId::from_bytes([0x41; 16]),
                            command: "ssh host".into(),
                            start_line: 10,
                            end_line: None,
                            running: true,
                            exit_status: None,
                            canvas_floor: false,
                            scope: BlockScope::Local,
                        },
                        RuntimeBlockRecord {
                            id: BlockId::from_bytes([0x42; 16]),
                            command: "echo session-ok".into(),
                            start_line: 30,
                            end_line: None,
                            running: true,
                            exit_status: None,
                            canvas_floor: false,
                            scope: BlockScope::Session,
                        },
                    ]),
                })
                .unwrap();
        });
        let ids = vec![10_u64, 11, 30, 31, 32];
        let owner = owned_client(3, 4, 7, ids.clone());
        crate::ffi::ACTIVE_HANDLE.with(|active| active.set(0));

        let host = seyal_app_block_projection(handle, 0);
        let session = seyal_app_block_projection(handle, 1);
        assert_eq!(host.kind, SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP);
        assert_eq!(session.kind, SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP);
        assert_eq!((host.reserved0, host.reserved1), (0, 2));
        assert_eq!((session.reserved0, session.reserved1), (2, 3));
        let host_lines = &ids[host.reserved0 as usize..][..host.reserved1 as usize];
        let session_lines = &ids[session.reserved0 as usize..][..session.reserved1 as usize];
        assert!(
            host_lines.iter().all(|line| !session_lines.contains(line)),
            "a terminal line must not appear in two cards"
        );
        let session_row = seyal_app_block_row(handle, 1);
        assert_ne!(session_row.flags & BLOCK_SESSION, 0);
        assert_eq!(seyal_app_block_row(handle, 0).flags & BLOCK_SESSION, 0);

        crate::ffi::CLIENTS.with(|clients| {
            clients.borrow_mut().remove(&owner);
        });
        crate::ffi::ACTIVE_HANDLE.with(|active| active.set(0));
        assert_eq!(seyal_app_destroy(handle), 0);
    }
}
