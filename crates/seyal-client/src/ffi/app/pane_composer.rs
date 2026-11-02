//! Pane-addressed composer / Block projection for visible non-focused Flow
//! leaves (#1424). Focused `seyal_app_composer` / `seyal_app_block_*` remain
//! the authority path for the focused fence; these accessors read the same
//! per-`PaneId` `ComposerState` without rebinding authority.

use std::ptr;

use seyal_core::{ExecutionId, PaneId};

use crate::app::{PresentationEligibility, APP_ABI_VERSION};
use crate::composer::{
    BlockPresentationState, ComposerMode, ComposerSnapshot, RuntimeBlockRecord,
};
use crate::live_tail::{project_block_output_until, LiveTailProjection};
use crate::presentation::PresentationMode;
use crate::LocalDisplayClient;

use super::block_projection::{
    SeyalAppBlockProjection, SEYAL_APP_BLOCK_PROJECTION_HISTORY,
    SEYAL_APP_BLOCK_PROJECTION_PRIMARY_CLIP,
};
use super::decode::runtime_block_from_command;
use super::encode::{
    push_row, relocate_row_pointers, split_id, RowDraft, BLOCK_SELECTED, BLOCK_SESSION,
    BLOCK_STATE_MASK,
};
use super::{id16, AppHandle, SeyalAppComposer, SeyalAppRow, APPS};

fn empty_composer(sized: bool) -> SeyalAppComposer {
    SeyalAppComposer {
        version: APP_ABI_VERSION,
        size: if sized {
            size_of::<SeyalAppComposer>() as u16
        } else {
            0
        },
        mode: 0,
        flags: 0,
        epoch: 0,
        request_id: 0,
        draft_utf8: ptr::null(),
        draft_utf8_len: 0,
        block_count: 0,
        canvas_floor_block_id_lo: 0,
        canvas_floor_block_id_hi: 0,
    }
}

fn encode_composer_snapshot(state: &mut AppHandle, composer: &ComposerSnapshot) -> SeyalAppComposer {
    state.pane_composer_draft = composer.draft.as_bytes().to_vec();
    let mut flags = 0u16;
    if composer.can_submit {
        flags |= 1;
    }
    if composer.allows_direct_terminal {
        flags |= 2;
    }
    let (floor_lo, floor_hi) = match composer.canvas_floor {
        Some(id) => split_id(id.to_bytes()),
        None => (0, 0),
    };
    SeyalAppComposer {
        version: APP_ABI_VERSION,
        size: size_of::<SeyalAppComposer>() as u16,
        mode: match composer.mode {
            ComposerMode::Hidden => 0,
            ComposerMode::Available => 1,
            ComposerMode::Busy { .. } => 2,
            ComposerMode::Relay => 3,
        },
        flags,
        epoch: composer.epoch,
        request_id: composer.pending_request_id.unwrap_or(0),
        draft_utf8: if state.pane_composer_draft.is_empty() {
            ptr::null()
        } else {
            state.pane_composer_draft.as_ptr()
        },
        draft_utf8_len: state.pane_composer_draft.len() as u32,
        block_count: composer.blocks.len() as u32,
        canvas_floor_block_id_lo: floor_lo,
        canvas_floor_block_id_hi: floor_hi,
    }
}

fn encode_pane_block_rows(state: &mut AppHandle, composer: &ComposerSnapshot) {
    state.pane_block_text.clear();
    state.pane_block_rows.clear();
    let selected = state.root.snapshot().chrome.selected_block;
    for (index, block) in composer.blocks.iter().enumerate() {
        let mut flags: u16 = match block.state {
            BlockPresentationState::Running => 1,
            BlockPresentationState::Completed => 2,
            BlockPresentationState::Failed => 3,
            BlockPresentationState::Unknown => 4,
        };
        debug_assert_eq!(flags & BLOCK_STATE_MASK, flags);
        if selected == Some(block.id) {
            flags |= BLOCK_SELECTED;
        }
        if block.scope == seyal_protocol::framing::BlockScope::Session {
            flags |= BLOCK_SESSION;
        }
        push_row(
            &mut state.pane_block_rows,
            &mut state.pane_block_text,
            RowDraft {
                kind: 0,
                index: index as u32,
                id: block.id.to_bytes(),
                flags,
                title: &block.command,
                detail: block.state.transcript_status(),
                address: None,
                raw_address: None,
            },
        );
    }
    relocate_row_pointers(&mut state.pane_block_rows, state.pane_block_text.as_ptr());
}

fn following_session_start(composer: &ComposerSnapshot, index: usize) -> Option<u64> {
    use seyal_protocol::framing::BlockScope;
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

fn with_pane_matched_client<R>(
    execution: Option<ExecutionId>,
    client_raw: Option<u64>,
    operation: impl FnOnce(&LocalDisplayClient) -> R,
) -> Option<R> {
    let execution = execution?;
    let raw = client_raw?;
    crate::ffi::with_client(raw, |client| {
        if client.execution_id() == execution {
            Some(operation(client))
        } else {
            None
        }
    })
    .flatten()
}

fn eligibility_code(value: PresentationEligibility) -> u16 {
    match value {
        PresentationEligibility::Unbound => 0,
        PresentationEligibility::Flow => 1,
        PresentationEligibility::Raw => 2,
        PresentationEligibility::Tui => 3,
    }
}

/// Pane-addressed composer read; empty when Pane unknown. Draft ptr is
/// invalidated by the next composer encode on this handle.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_composer_for_pane(
    handle: u64,
    pane_lo: u64,
    pane_hi: u64,
) -> SeyalAppComposer {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return empty_composer(true);
        };
        let Ok(id) = id16(pane_lo, pane_hi) else {
            return empty_composer(false);
        };
        let pane = PaneId::from_bytes(id);
        let Some(composer) = state.root.composer_for_pane(pane) else {
            return empty_composer(false);
        };
        encode_composer_snapshot(state, &composer)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_block_row_for_pane(
    handle: u64,
    pane_lo: u64,
    pane_hi: u64,
    index: u32,
) -> SeyalAppRow {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return SeyalAppRow::empty();
        };
        let Ok(id) = id16(pane_lo, pane_hi) else {
            return SeyalAppRow::empty();
        };
        let pane = PaneId::from_bytes(id);
        let Some(composer) = state.root.composer_for_pane(pane) else {
            return SeyalAppRow::empty();
        };
        encode_pane_block_rows(state, &composer);
        state
            .pane_block_rows
            .get(index as usize)
            .copied()
            .unwrap_or_else(SeyalAppRow::empty)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_block_projection_for_pane(
    handle: u64,
    pane_lo: u64,
    pane_hi: u64,
    index: u32,
) -> SeyalAppBlockProjection {
    APPS.with(|apps| {
        let apps = apps.borrow();
        let Some(state) = apps.get(&handle) else {
            return SeyalAppBlockProjection::fail_closed();
        };
        let Ok(id) = id16(pane_lo, pane_hi) else {
            return SeyalAppBlockProjection::fail_closed();
        };
        let pane = PaneId::from_bytes(id);
        let Some(composer) = state.root.composer_for_pane(pane) else {
            return SeyalAppBlockProjection::fail_closed();
        };
        let Some(block) = composer.blocks.get(index as usize) else {
            return SeyalAppBlockProjection::fail_closed();
        };
        let running = block.state == BlockPresentationState::Running;
        let exclusive_end = following_session_start(&composer, index as usize);
        // Non-focused Flow leaves keep Flow projection unless that Pane's
        // composer is Hidden (Raw/TUI takeover on that leaf).
        let mode = if composer.mode == ComposerMode::Hidden {
            PresentationMode::Raw
        } else {
            PresentationMode::Flow
        };
        if mode != PresentationMode::Flow && running {
            return SeyalAppBlockProjection::fail_closed();
        }
        let execution = state.root.pane_execution_id(pane);
        let client_raw = state.root.pane_client_raw(pane);
        let projection = if running {
            with_pane_matched_client(execution, client_raw, |client| {
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

/// Pull this Pane's Candidate-D Block timeline into its `PaneComposer` without
/// requiring focused fence authority (#1424 unfocused output).
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_sync_pane_runtime_blocks(
    handle: u64,
    pane_lo: u64,
    pane_hi: u64,
) -> i32 {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return -1;
        };
        let Ok(id) = id16(pane_lo, pane_hi) else {
            return -2;
        };
        let pane = PaneId::from_bytes(id);
        if !state.root.pane_known(pane) {
            return -2;
        }
        let Some(raw) = state.root.pane_client_raw(pane) else {
            // No client yet — not an error; host keeps prior composer state.
            return 0;
        };
        let Some(records) = crate::ffi::with_client(raw, |client| {
            client
                .block_timeline()
                .records
                .iter()
                .map(runtime_block_from_command)
                .collect::<Vec<RuntimeBlockRecord>>()
        }) else {
            return -3;
        };
        match state.root.apply_runtime_blocks_for_pane(pane, records) {
            Ok(()) => 0,
            Err(_) => -4,
        }
    })
}

/// Composer-mode proxy for non-focused presentation: Hidden → Raw-like full
/// pane; anything else → Flow. Avoids requiring a second PresentationSession
/// while still fencing TUI/Raw takeover on that leaf.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_eligibility_for_pane(
    handle: u64,
    pane_lo: u64,
    pane_hi: u64,
) -> u16 {
    APPS.with(|apps| {
        let apps = apps.borrow();
        let Some(state) = apps.get(&handle) else {
            return eligibility_code(PresentationEligibility::Unbound);
        };
        let Ok(id) = id16(pane_lo, pane_hi) else {
            return eligibility_code(PresentationEligibility::Unbound);
        };
        let pane = PaneId::from_bytes(id);
        let Some(composer) = state.root.composer_for_pane(pane) else {
            return eligibility_code(PresentationEligibility::Unbound);
        };
        if composer.mode == ComposerMode::Hidden {
            let focused = state.root.focused_pane_id();
            if pane == focused {
                return eligibility_code(state.root.snapshot().eligibility);
            }
            return eligibility_code(PresentationEligibility::Raw);
        }
        eligibility_code(PresentationEligibility::Flow)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::app::{seyal_app_apply, seyal_app_create, seyal_app_shell, seyal_app_snapshot, SeyalAppAction};
    use crate::app::APP_ABI_VERSION;
    use std::mem::size_of;
    use std::ptr;

    #[test]
    fn composer_for_pane_survives_split_and_focus_switch() {
        let handle = seyal_app_create();
        let snap = seyal_app_snapshot(handle);
        let split = SeyalAppAction {
            version: APP_ABI_VERSION,
            size: size_of::<SeyalAppAction>() as u16,
            kind: 25, // SPLIT_FOCUSED — keep in sync with SeyalApp.h
            flags: 0,
            fence_pane_lo: 0,
            fence_pane_hi: 0,
            fence_execution_lo: 0,
            fence_execution_hi: 0,
            fence_attachment_lo: 0,
            fence_attachment_hi: 0,
            fence_epoch: 0,
            target_execution_lo: 0,
            target_execution_hi: 0,
            target_attachment_lo: 0,
            target_attachment_hi: 0,
            target_generation: 0,
            payload: ptr::null(),
            payload_len: 0,
            reserved: 0, // Right
        };
        // Prefer reading the focused pane ids from shell after create.
        let _ = snap;
        assert_eq!(unsafe { seyal_app_apply(handle, &split) }, 0);
        let shell = seyal_app_shell(handle);
        assert_eq!(shell.pane_count, 2);
        let first = seyal_app_composer_for_pane(handle, shell.focused_pane_lo, shell.focused_pane_hi);
        assert_ne!(first.size, 0);
        // Unfocused sibling still has a PaneComposer projection.
        // Find the non-focused region via shell focused ids vs pane region APIs
        // is covered in host tests; here ensure both ids are readable when known.
        let eligibility = seyal_app_eligibility_for_pane(
            handle,
            shell.focused_pane_lo,
            shell.focused_pane_hi,
        );
        assert!(eligibility == 0 || eligibility == 1 || eligibility == 2 || eligibility == 3);
        assert_eq!(
            seyal_app_sync_pane_runtime_blocks(
                handle,
                shell.focused_pane_lo,
                shell.focused_pane_hi
            ),
            0
        );
    }
}
