//! Block quick-action row FFI (#1010 / #1474).
//!
//! Rust owns the action set, order, labels, shortcut hints and availability
//! (`composer::block_actions`); rows are encoded with the Block rows once per
//! root generation. Copy resolves history spans here and hands final UTF-8 to
//! the host through `seyal_bridge_take_block_copy`.

use crate::ffi::error_code;

use super::encode::encode_block_rows;
use super::{SeyalAppRow, APPS};

/// Quick-action rows for every Block, flat, with each Block's `(first, count)`
/// span. `generation` is the root generation `block_rows` and these rows were
/// encoded for, so hosts reading every row per rebuild never re-encode.
#[derive(Default)]
pub(super) struct BlockActionRows {
    pub(super) rows: Vec<SeyalAppRow>,
    pub(super) text: Vec<u8>,
    pub(super) spans: Vec<(u32, u32)>,
    pub(super) generation: u64,
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_block_row(handle: u64, index: u32) -> SeyalAppRow {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return SeyalAppRow::empty();
        };
        encode_block_rows(state);
        state
            .block_rows
            .get(index as usize)
            .copied()
            .unwrap_or_else(SeyalAppRow::empty)
    })
}

/// Number of quick actions for the Block at `block_index` (#1010 / #1474).
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_block_action_count(handle: u64, block_index: u32) -> u32 {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return 0;
        };
        encode_block_rows(state);
        state
            .block_actions
            .spans
            .get(block_index as usize)
            .map_or(0, |span| span.1)
    })
}

/// One Rust-projected quick action (#1010 / #1474): `kind` = action, `flags` =
/// enabled bit plus placement, `title` = label, `detail` = shortcut hint.
/// Pointers stay valid until the root generation changes.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_block_action_row(
    handle: u64,
    block_index: u32,
    action_index: u32,
) -> SeyalAppRow {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return SeyalAppRow::empty();
        };
        encode_block_rows(state);
        let Some(&(first, count)) = state.block_actions.spans.get(block_index as usize) else {
            return SeyalAppRow::empty();
        };
        if action_index >= count {
            return SeyalAppRow::empty();
        }
        state
            .block_actions
            .rows
            .get((first + action_index) as usize)
            .copied()
            .unwrap_or_else(SeyalAppRow::empty)
    })
}

/// Start a Rust-owned Block pasteboard copy (#1010 / #1474). `kind` is a
/// `SEYAL_APP_BLOCK_ACTION_COPY_*` value. Final text arrives via
/// `seyal_bridge_take_block_copy` / `_for`. Returns 0 on accept, negative on refuse.
///
/// History is pulled from the focused Pane's Controller (`pane_client_raw`),
/// not a stale global display selection, so multipane leaves stay exact-pane.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_request_block_copy(handle: u64, block_index: u32, kind: u16) -> i32 {
    use crate::ffi::with_client_mut;
    use crate::history_text::BlockCopyKind;
    let Some(copy_kind) = BlockCopyKind::from_u16(kind) else {
        return -6;
    };
    let prepared = APPS.with(|apps| {
        let apps = apps.borrow();
        let state = apps.get(&handle)?;
        let pane = state.root.focused_pane_id();
        let display = state.root.pane_client_raw(pane)?;
        let snap = state.root.snapshot();
        let composer = snap.composer?;
        let block = composer.blocks.get(block_index as usize)?;
        let output_range = if copy_kind == BlockCopyKind::Command {
            None
        } else if block.start_line == 0 {
            return None;
        } else {
            match block.end_line {
                None => Some((block.start_line, u64::MAX)),
                Some(end) if end >= block.start_line => Some((block.start_line, end)),
                Some(_) => None,
            }
        };
        let bytes = block.id.to_bytes();
        let block_id = u64::from_le_bytes(bytes[..8].try_into().ok()?);
        if block_id == 0 {
            return None;
        }
        Some((display, block_id, block.command.clone(), output_range))
    });
    let Some((display, block_id, command, output_range)) = prepared else {
        return -4;
    };
    with_client_mut(display, |client| {
        client.begin_block_copy(block_id, copy_kind, command, output_range)
    })
    .map_or(-1, |result| result.map_or_else(error_code, |_| 0))
}

#[cfg(test)]
mod tests {
    use std::{ptr, slice};

    use seyal_core::BlockId;
    use seyal_protocol::framing::BlockScope;

    use super::*;
    use crate::app::{AppAction, APP_ABI_VERSION};
    use crate::composer::{RuntimeBlockRecord, RuntimeComposerEligibility};
    use crate::ffi::app::encode::{
        BLOCK_ACTION_ENABLED, BLOCK_ACTION_PLACEMENT_SHIFT, SNAP_CONTROLLER, SNAP_HAS_ATTACHMENT,
        SNAP_HAS_EXECUTION,
    };
    use crate::ffi::app::{
        seyal_app_apply, seyal_app_block_row, seyal_app_composer, seyal_app_create,
        seyal_app_destroy, seyal_app_last_error, seyal_app_snapshot, SeyalAppAction,
        SeyalAppSnapshot, FLAG_CONTROLLER, FLAG_HAS_ATTACHMENT, FLAG_HAS_EXECUTION,
        FLAG_TARGET_CONTROLLER,
    };

    fn copy_text(row: SeyalAppRow) -> String {
        if row.title.is_null() || row.title_len == 0 {
            return String::new();
        }
        let bytes = unsafe { slice::from_raw_parts(row.title, row.title_len as usize) };
        String::from_utf8_lossy(bytes).into_owned()
    }

    fn identity_fence(kind: u16, snap: &SeyalAppSnapshot) -> SeyalAppAction {
        let mut flags = 0u16;
        if snap.flags & SNAP_HAS_EXECUTION != 0 {
            flags |= FLAG_HAS_EXECUTION;
        }
        if snap.flags & SNAP_HAS_ATTACHMENT != 0 {
            flags |= FLAG_HAS_ATTACHMENT;
        }
        if snap.flags & SNAP_CONTROLLER != 0 {
            flags |= FLAG_CONTROLLER;
        }
        SeyalAppAction {
            version: APP_ABI_VERSION,
            size: size_of::<SeyalAppAction>() as u16,
            kind,
            flags,
            fence_pane_lo: snap.pane_lo,
            fence_pane_hi: snap.pane_hi,
            fence_execution_lo: snap.execution_lo,
            fence_execution_hi: snap.execution_hi,
            fence_attachment_lo: snap.attachment_lo,
            fence_attachment_hi: snap.attachment_hi,
            fence_epoch: snap.epoch,
            target_execution_lo: 0,
            target_execution_hi: 0,
            target_attachment_lo: 0,
            target_attachment_hi: 0,
            target_generation: 0,
            payload: ptr::null(),
            payload_len: 0,
            reserved: 0,
        }
    }

    #[test]
    fn block_rerun_ffi_projects_availability_and_loads_draft() {
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
        let fence = APPS.with(|apps| apps.borrow().get(&handle).unwrap().root.fence());
        let seed = |available: bool| {
            APPS.with(|apps| {
                let mut apps = apps.borrow_mut();
                let root = &mut apps.get_mut(&handle).unwrap().root;
                root.apply(AppAction::ApplyRuntimeBlocks {
                    fence,
                    records: Some(vec![
                        RuntimeBlockRecord {
                            id: BlockId::from_bytes([0x51; 16]),
                            command: "ls /".into(),
                            start_line: 1,
                            end_line: Some(3),
                            running: false,
                            exit_status: Some(0),
                            canvas_floor: false,
                            scope: BlockScope::Local,
                        },
                        RuntimeBlockRecord {
                            id: BlockId::from_bytes([0x61; 16]),
                            command: "sleep 9".into(),
                            start_line: 4,
                            end_line: None,
                            running: true,
                            exit_status: None,
                            canvas_floor: false,
                            scope: BlockScope::Local,
                        },
                    ]),
                })
                .unwrap();
                if available {
                    root.apply(AppAction::ApplyRuntimeComposerStatus {
                        fence,
                        eligibility: Some(RuntimeComposerEligibility::Available),
                        revision: 1,
                        target: None,
                    })
                    .unwrap();
                }
            });
        };

        let seam_rerun = |block: u32| {
            (0..seyal_app_block_action_count(handle, block))
                .map(|index| seyal_app_block_action_row(handle, block, index))
                .find(|row| row.kind == 5 && row.flags >> BLOCK_ACTION_PLACEMENT_SHIFT == 0)
                .expect("seam Rerun action")
        };

        seed(false);
        assert_eq!(seam_rerun(0).flags & BLOCK_ACTION_ENABLED, 0);

        seed(true);
        let done = seyal_app_block_row(handle, 0);
        let running = seyal_app_block_row(handle, 1);
        assert_eq!(seyal_app_block_action_count(handle, 0), 10);
        let rerun_row = seam_rerun(0);
        assert_eq!(rerun_row.flags & BLOCK_ACTION_ENABLED, BLOCK_ACTION_ENABLED);
        assert_eq!(copy_text(rerun_row), "Rerun", "label is Rust-owned");
        assert_eq!(
            seam_rerun(1).flags & BLOCK_ACTION_ENABLED,
            0,
            "running Block never offers Rerun"
        );
        let copy_command = seyal_app_block_action_row(handle, 0, 3);
        assert_eq!(copy_command.kind, 2);
        assert_eq!(
            copy_command.flags >> BLOCK_ACTION_PLACEMENT_SHIFT,
            1,
            "in the Copy menu"
        );
        assert_eq!(copy_text(copy_command), "Copy command");
        assert_eq!(
            copy_command.detail_len, 0,
            "no Copy shortcut hint: cmd+c is reserved (design §9)"
        );
        assert_eq!(
            seyal_app_block_action_row(handle, 0, 10).kind,
            0,
            "out of range is empty"
        );
        assert_eq!(
            seyal_app_block_action_count(handle, 9),
            0,
            "unknown Block has no actions"
        );
        let before = seyal_app_block_row(handle, 0).title;
        let _ = seyal_app_block_action_row(handle, 0, 0);
        let _ = seyal_app_block_action_count(handle, 1);
        assert_eq!(
            seyal_app_block_row(handle, 0).title,
            before,
            "no re-encode without a state change"
        );

        let bound = seyal_app_snapshot(handle);
        let mut rerun = identity_fence(87, &bound);
        rerun.target_execution_lo = running.id_lo;
        rerun.target_execution_hi = running.id_hi;
        rerun.target_generation = seyal_app_composer(handle).epoch;
        assert_eq!(unsafe { seyal_app_apply(handle, &rerun) }, -4);
        assert_eq!(seyal_app_last_error(handle), 63, "BlockRunning");

        rerun.target_execution_lo = done.id_lo;
        rerun.target_execution_hi = done.id_hi;
        assert_eq!(unsafe { seyal_app_apply(handle, &rerun) }, 0);
        let composer = seyal_app_composer(handle);
        let draft =
            unsafe { slice::from_raw_parts(composer.draft_utf8, composer.draft_utf8_len as usize) };
        assert_eq!(draft, b"ls /");

        let rerun_rows = |block: u32| {
            (0..seyal_app_block_action_count(handle, block))
                .map(|index| seyal_app_block_action_row(handle, block, index))
                .filter(|row| row.kind == 5)
                .map(|row| row.flags & BLOCK_ACTION_ENABLED != 0)
                .collect::<Vec<_>>()
        };
        assert_eq!(rerun_rows(0), [false, false], "draft occupied");
        let mut again = identity_fence(87, &seyal_app_snapshot(handle));
        again.target_execution_lo = done.id_lo;
        again.target_execution_hi = done.id_hi;
        again.target_generation = seyal_app_composer(handle).epoch;
        assert_eq!(unsafe { seyal_app_apply(handle, &again) }, -4);
        assert_eq!(seyal_app_last_error(handle), 65, "ComposerDraftOccupied");
        assert_eq!(
            seyal_app_composer(handle).draft_utf8_len,
            4,
            "refused Rerun leaves the draft"
        );

        let epoch = seyal_app_composer(handle).epoch;
        APPS.with(|apps| {
            let mut apps = apps.borrow_mut();
            let root = &mut apps.get_mut(&handle).unwrap().root;
            let fence = root.fence();
            root.apply(AppAction::SetComposerDraft {
                fence,
                text: String::new(),
                composer_epoch: epoch,
            })
            .unwrap();
        });
        assert_eq!(
            rerun_rows(0),
            [true, true],
            "cleared draft re-enables Rerun"
        );
        assert_eq!(seyal_app_destroy(handle), 0);
    }
}
