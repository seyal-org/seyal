//! Block quick-action rows and Rust-owned Block copy FFI exports (#1010).
//!
//! Rust owns the action set, order, labels, shortcut hints and availability
//! (`composer::block_actions`); rows are encoded with the Block rows once per
//! root generation. Copy resolves the Block's span and command here and hands
//! the final text to the host through `seyal_bridge_take_block_copy`.

use crate::ffi::{error_code, with_active_client_mut};

use super::encode::encode_block_rows;
use super::{SeyalAppRow, APPS};

/// Number of quick actions for the Block at `block_index` (#1010).
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_block_action_count(handle: u64, block_index: u32) -> u32 {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return 0;
        };
        encode_block_rows(state);
        state
            .block_action_spans
            .get(block_index as usize)
            .map_or(0, |span| span.1)
    })
}

/// One Rust-projected quick action (#1010): `kind` = action, `flags` =
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
        let Some(&(first, count)) = state.block_action_spans.get(block_index as usize) else {
            return SeyalAppRow::empty();
        };
        if action_index >= count {
            return SeyalAppRow::empty();
        }
        state
            .block_action_rows
            .get((first + action_index) as usize)
            .copied()
            .unwrap_or_else(SeyalAppRow::empty)
    })
}

/// Start a Rust-owned Block pasteboard copy (#1010). Resolves the Block's
/// history span and command from the application root, then asks the active
/// display client to fetch and compose one final UTF-8 string. The host only
/// writes that string to the pasteboard when `seyal_bridge_take_block_copy`
/// returns it. Returns 0 on accept, negative on refuse.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_request_block_copy(handle: u64, block_index: u32, kind: u16) -> i32 {
    use crate::history_text::BlockCopyKind;
    let Some(copy_kind) = BlockCopyKind::from_u16(kind) else {
        return -6;
    };
    let prepared = APPS.with(|apps| {
        let apps = apps.borrow();
        let state = apps.get(&handle)?;
        let snap = state.root.snapshot();
        let composer = snap.composer?;
        let block = composer.blocks.get(block_index as usize)?;
        if block.start_line == 0 {
            return None;
        }
        // Running: through the current tail, no line cap (design §6).
        // Finished before its start: no output rows.
        let output_range = match block.end_line {
            None => Some((block.start_line, u64::MAX)),
            Some(end) if end >= block.start_line => Some((block.start_line, end)),
            Some(_) => None,
        };
        let bytes = block.id.to_bytes();
        let block_id = u64::from_le_bytes(bytes[..8].try_into().ok()?);
        if block_id == 0 {
            return None;
        }
        Some((block_id, block.command.clone(), output_range))
    });
    let Some((block_id, command, output_range)) = prepared else {
        return -4;
    };
    with_active_client_mut(|client| {
        client.begin_block_copy(block_id, copy_kind, command, output_range)
    })
    .map_or(-1, |result| result.map_or_else(error_code, |_| 0))
}

#[cfg(test)]
mod tests {
    use std::{ptr, slice};

    use seyal_core::BlockId;

    use super::*;
    use crate::app::{AppAction, APP_ABI_VERSION};
    use crate::composer::{RuntimeBlockRecord, RuntimeComposerEligibility};
    use crate::ffi::app::encode::{BLOCK_ACTION_ENABLED, BLOCK_ACTION_PLACEMENT_SHIFT};
    use crate::ffi::app::tests::{copy_text, identity_fence};
    use crate::ffi::app::{
        seyal_app_apply, seyal_app_block_row, seyal_app_composer, seyal_app_create,
        seyal_app_destroy, seyal_app_last_error, seyal_app_snapshot, SeyalAppAction,
        FLAG_TARGET_CONTROLLER,
    };

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
            target_pty_generation: 1,
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
                    records: vec![
                        RuntimeBlockRecord {
                            id: BlockId::from_bytes([0x51; 16]),
                            command: "ls /".into(),
                            start_line: 1,
                            end_line: Some(3),
                            running: false,
                            exit_status: Some(0),
                        },
                        RuntimeBlockRecord {
                            id: BlockId::from_bytes([0x61; 16]),
                            command: "sleep 9".into(),
                            start_line: 4,
                            end_line: None,
                            running: true,
                            exit_status: None,
                        },
                    ],
                })
                .unwrap();
                if available {
                    root.apply(AppAction::ApplyRuntimeComposerStatus {
                        fence,
                        eligibility: Some(RuntimeComposerEligibility::Available),
                        revision: 1,
                    })
                    .unwrap();
                }
            });
        };

        // Seam Rerun action (kind 5, placement 0) for one Block row.
        let seam_rerun = |block: u32| {
            (0..seyal_app_block_action_count(handle, block))
                .map(|index| seyal_app_block_action_row(handle, block, index))
                .find(|row| row.kind == 5 && row.flags >> BLOCK_ACTION_PLACEMENT_SHIFT == 0)
                .expect("seam Rerun action")
        };

        // Composer busy: Rerun is not offered for any Block.
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
        // Rows are encoded once per root generation, not once per FFI call.
        let before = seyal_app_block_row(handle, 0).title;
        let _ = seyal_app_block_action_row(handle, 0, 0);
        let _ = seyal_app_block_action_count(handle, 1);
        assert_eq!(
            seyal_app_block_row(handle, 0).title,
            before,
            "no re-encode without a state change"
        );

        let bound = seyal_app_snapshot(handle);
        let mut rerun = identity_fence(59, &bound);
        rerun.target_execution_lo = running.id_lo;
        rerun.target_execution_hi = running.id_hi;
        rerun.target_pty_generation = seyal_app_composer(handle).epoch;
        assert_eq!(unsafe { seyal_app_apply(handle, &rerun) }, -4);
        assert_eq!(seyal_app_last_error(handle), 35, "BlockRunning");

        rerun.target_execution_lo = done.id_lo;
        rerun.target_execution_hi = done.id_hi;
        assert_eq!(unsafe { seyal_app_apply(handle, &rerun) }, 0);
        let composer = seyal_app_composer(handle);
        let draft =
            unsafe { slice::from_raw_parts(composer.draft_utf8, composer.draft_utf8_len as usize) };
        assert_eq!(draft, b"ls /");
        assert_eq!(seyal_app_destroy(handle), 0);
    }
}
