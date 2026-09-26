use super::*;
use std::{
    mem::{align_of, offset_of, size_of},
    slice,
};

use seyal_core::BlockId;
use seyal_protocol::framing::{CommandBlock, CommandBlockState};

use crate::app::AppAction;
use crate::composer::RuntimeBlockRecord;

use super::decode::runtime_block_from_command;
use super::visual::SeyalAppTheme;

fn fence_action(kind: u16, root: &ApplicationRoot) -> SeyalAppAction {
    let fence = root.fence();
    let pane = fence.pane.to_bytes();
    SeyalAppAction {
        version: APP_ABI_VERSION,
        size: size_of::<SeyalAppAction>() as u16,
        kind,
        flags: 0,
        fence_pane_lo: u64::from_le_bytes(pane[..8].try_into().unwrap()),
        fence_pane_hi: u64::from_le_bytes(pane[8..].try_into().unwrap()),
        fence_execution_lo: 0,
        fence_execution_hi: 0,
        fence_attachment_lo: 0,
        fence_attachment_hi: 0,
        fence_epoch: fence.presentation_epoch,
        target_execution_lo: 0,
        target_execution_hi: 0,
        target_attachment_lo: 0,
        target_attachment_hi: 0,
        target_pty_generation: 0,
        payload: ptr::null(),
        payload_len: 0,
        reserved: 0,
    }
}

#[test]
fn action_and_snapshot_match_published_sizes() {
    assert_eq!(size_of::<SeyalAppAction>(), 120);
    assert_eq!(align_of::<SeyalAppAction>(), 8);
    assert_eq!(offset_of!(SeyalAppAction, version), 0);
    assert_eq!(offset_of!(SeyalAppAction, payload), 104);
    assert_eq!(size_of::<SeyalAppSnapshot>(), 112);
    assert_eq!(offset_of!(SeyalAppSnapshot, output_utf8), 80);
    assert_eq!(offset_of!(SeyalAppSnapshot, recovery_generation), 104);
    assert_eq!(size_of::<SeyalAppAxNode>(), 72);
    assert_eq!(size_of::<SeyalAppAccessibility>(), 24);
    assert_eq!(size_of::<SeyalAppShell>(), 64);
    assert_eq!(size_of::<SeyalAppRow>(), 56);
    assert_eq!(size_of::<SeyalAppBlockSpan>(), 16);
    assert_eq!(size_of::<SeyalAppTheme>(), 16);
    assert_eq!(size_of::<SeyalAppComposerHistory>(), 32);
    assert_eq!(offset_of!(SeyalAppComposerHistory, query_utf8), 16);
}

fn bound_handle() -> (u64, SeyalAppSnapshot) {
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
    relay_composer_status(handle, 1, 1);
    let bound = seyal_app_snapshot(handle);
    (handle, bound)
}

/// Relay a Runtime composer status exactly as the thin host does:
/// `reserved` = eligibility code, `target_execution_lo` = revision.
fn relay_composer_status(handle: u64, eligibility: u32, revision: u64) -> i32 {
    let snap = seyal_app_snapshot(handle);
    let mut status = identity_fence(52, &snap);
    status.reserved = eligibility;
    status.target_execution_lo = revision;
    unsafe { seyal_app_apply(handle, &status) }
}

#[test]
fn composer_status_relay_gates_availability_and_rejects_bad_codes() {
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
    // Bound but Runtime has published nothing: busy, not submittable,
    // and the placeholder says so.
    let composer = seyal_app_composer(handle);
    assert_eq!(composer.mode, 2);
    assert_eq!(composer.flags & 1, 0);
    assert_eq!(
        copy_text(seyal_app_copy(handle, 0)),
        "Waiting for prompt..."
    );
    // An unknown eligibility code is a malformed action, not a guess.
    assert_eq!(relay_composer_status(handle, 9, 1), -6);
    assert_eq!(seyal_app_composer(handle).mode, 2);
    assert_eq!(relay_composer_status(handle, 1, 1), 0);
    assert_eq!(seyal_app_composer(handle).mode, 1);
    assert_eq!(copy_text(seyal_app_copy(handle, 0)), "Type a command...");
    // Busy at a newer revision disables; a stale Available cannot undo it.
    assert_eq!(relay_composer_status(handle, 2, 3), 0);
    assert_eq!(seyal_app_composer(handle).mode, 2);
    assert_eq!(relay_composer_status(handle, 1, 2), 0);
    assert_eq!(seyal_app_composer(handle).mode, 2);
    // Transport lost: cleared, still busy; the next attachment restarts.
    assert_eq!(relay_composer_status(handle, 0, 0), 0);
    assert_eq!(seyal_app_composer(handle).mode, 2);
    assert_eq!(relay_composer_status(handle, 3, 1), 0);
    assert_eq!(seyal_app_composer(handle).mode, 1);
    assert_eq!(seyal_app_destroy(handle), 0);
}

fn accepted_submit(handle: u64, command: &str) {
    let snap = seyal_app_snapshot(handle);
    let composer = seyal_app_composer(handle);
    let mut draft = identity_fence(10, &snap);
    draft.target_pty_generation = composer.epoch;
    draft.payload = command.as_ptr();
    draft.payload_len = command.len() as u32;
    assert_eq!(unsafe { seyal_app_apply(handle, &draft) }, 0);
    let mut submit = identity_fence(11, &snap);
    submit.target_pty_generation = composer.epoch;
    assert_eq!(unsafe { seyal_app_apply(handle, &submit) }, 0);
    let pending = seyal_app_composer(handle);
    let mut result = identity_fence(12, &snap);
    result.target_execution_lo = pending.request_id;
    result.reserved = 1;
    assert_eq!(unsafe { seyal_app_apply(handle, &result) }, 0);
}

fn utf8(pointer: *const u8, len: u32) -> String {
    if pointer.is_null() || len == 0 {
        return String::new();
    }
    let bytes = unsafe { slice::from_raw_parts(pointer, len as usize) };
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[test]
fn composer_history_ffi_round_trips_open_filter_select() {
    let (handle, bound) = bound_handle();
    let closed = seyal_app_composer_history(handle);
    assert_eq!(closed.size as usize, size_of::<SeyalAppComposerHistory>());
    assert_eq!(closed.flags, 0);
    assert_eq!(closed.entry_count, 0);

    accepted_submit(handle, "cargo build");
    accepted_submit(handle, "git status");
    let recorded = seyal_app_composer_history(handle);
    assert_eq!(recorded.flags, HISTORY_HAS_ENTRIES);
    assert_eq!(recorded.entry_count, 2);
    assert_eq!(recorded.row_count, 0);
    assert!(recorded.query_utf8.is_null());
    assert_eq!(seyal_app_history_row(handle, 0).title_len, 0);

    let open = identity_fence(40, &bound);
    assert_eq!(unsafe { seyal_app_apply(handle, &open) }, 0);
    let opened = seyal_app_composer_history(handle);
    assert_eq!(opened.flags, HISTORY_OPEN | HISTORY_HAS_ENTRIES);
    assert_eq!(opened.row_count, 2);
    assert_eq!(opened.selected, 0);
    let first = seyal_app_history_row(handle, 0);
    assert_eq!(utf8(first.title, first.title_len), "git status");
    assert_eq!(first.flags & ROW_SELECTED, ROW_SELECTED);
    let second = seyal_app_history_row(handle, 1);
    assert_eq!(utf8(second.title, second.title_len), "cargo build");
    assert_eq!(second.flags & ROW_SELECTED, 0);

    let mut filter = identity_fence(41, &bound);
    let query = b"car";
    filter.payload = query.as_ptr();
    filter.payload_len = query.len() as u32;
    assert_eq!(unsafe { seyal_app_apply(handle, &filter) }, 0);
    let filtered = seyal_app_composer_history(handle);
    assert_eq!(filtered.row_count, 1);
    assert_eq!(utf8(filtered.query_utf8, filtered.query_utf8_len), "car");

    let mut down = identity_fence(42, &bound);
    down.reserved = 1u32;
    assert_eq!(unsafe { seyal_app_apply(handle, &down) }, 0);
    assert_eq!(seyal_app_composer_history(handle).selected, 0, "clamped");
    let mut up = identity_fence(42, &bound);
    up.reserved = (-1i32) as u32;
    assert_eq!(unsafe { seyal_app_apply(handle, &up) }, 0);

    let mut stale = identity_fence(43, &bound);
    stale.target_pty_generation = seyal_app_composer(handle).epoch + 7;
    assert_eq!(unsafe { seyal_app_apply(handle, &stale) }, -4);
    assert_eq!(seyal_app_last_error(handle), 18);

    let mut select = identity_fence(43, &bound);
    select.target_pty_generation = seyal_app_composer(handle).epoch;
    assert_eq!(unsafe { seyal_app_apply(handle, &select) }, 0);
    let composer = seyal_app_composer(handle);
    assert_eq!(
        utf8(composer.draft_utf8, composer.draft_utf8_len),
        "cargo build"
    );
    assert_eq!(composer.request_id, 0, "select never submits");
    assert_eq!(
        seyal_app_composer_history(handle).flags,
        HISTORY_HAS_ENTRIES
    );

    let open_again = identity_fence(40, &bound);
    assert_eq!(unsafe { seyal_app_apply(handle, &open_again) }, 0);
    let close = identity_fence(44, &bound);
    assert_eq!(unsafe { seyal_app_apply(handle, &close) }, 0);
    assert_eq!(seyal_app_composer_history(handle).flags & HISTORY_OPEN, 0);

    let mut unopened_filter = identity_fence(41, &bound);
    unopened_filter.payload = query.as_ptr();
    unopened_filter.payload_len = query.len() as u32;
    assert_eq!(unsafe { seyal_app_apply(handle, &unopened_filter) }, -4);
    assert_eq!(seyal_app_last_error(handle), 24);
    assert_eq!(seyal_app_destroy(handle), 0);
}

#[test]
fn composer_history_copy_and_unbound_open_fail_closed() {
    let handle = seyal_app_create();
    let label = seyal_app_copy(handle, 3);
    assert_eq!(utf8(label.title, label.title_len), COMPOSER_HISTORY_LABEL);
    let placeholder = seyal_app_copy(handle, 4);
    assert_eq!(
        utf8(placeholder.title, placeholder.title_len),
        COMPOSER_HISTORY_PLACEHOLDER
    );
    // Unbound Pane: fence is valid but the composer is not Available.
    let open = identity_fence(40, &seyal_app_snapshot(handle));
    assert_eq!(unsafe { seyal_app_apply(handle, &open) }, -4);
    assert_eq!(seyal_app_last_error(handle), 23);
    assert_eq!(seyal_app_composer_history(handle).flags, 0);
    assert_eq!(seyal_app_destroy(handle), 0);
}

#[test]
fn core_terminal_chrome_ffi_is_visible_by_default_and_can_be_hidden() {
    let handle = seyal_app_create();
    let chrome = seyal_app_chrome(handle);
    assert_eq!(chrome.reserved, 1 | 2 | 4);
    let mut hide = SeyalAppAction {
        version: APP_ABI_VERSION,
        size: size_of::<SeyalAppAction>() as u16,
        kind: 22,
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
        target_pty_generation: 0,
        payload: ptr::null(),
        payload_len: 0,
        reserved: 0,
    };
    assert_eq!(unsafe { seyal_app_apply(handle, &hide) }, 0);
    assert_eq!(seyal_app_chrome(handle).reserved, 0);
    hide.reserved = 1 | 2 | 4;
    assert_eq!(unsafe { seyal_app_apply(handle, &hide) }, 0);
    let shown = seyal_app_chrome(handle);
    assert_eq!(shown.reserved & 1, 1);
    assert_eq!(shown.reserved & 2, 2);
    assert_eq!(shown.reserved & 4, 4);
    assert_eq!(seyal_app_destroy(handle), 0);
}

#[test]
fn shell_composition_actions_decode_and_reach_shell_state_and_fail_closed() {
    // CreateTab/CloseTab/SplitFocused/ClosePane (#922) are new at the FFI
    // boundary; this proves each code decodes into the right AppAction
    // and actually reaches ShellState rather than being silently
    // unwired or misdecoded into a different action.
    let handle = seyal_app_create();
    let snap = seyal_app_snapshot(handle);
    let tab_row = seyal_app_shell_row(handle, 1, 0);
    let pane_row = seyal_app_shell_row(handle, 2, 0);

    // CreateTab (23) and SplitFocused (25) reach the M001 default policy
    // that disallows composition growth until a distinct execution
    // route exists; they fail closed rather than no-op silently.
    assert_eq!(
        unsafe { seyal_app_apply(handle, &identity_fence(23, &snap)) },
        -4
    );
    assert_eq!(seyal_app_last_error(handle), 28, "TabCreationUnavailable");
    let mut split = identity_fence(25, &snap);
    split.reserved = 1; // SplitAxis::Down
    assert_eq!(unsafe { seyal_app_apply(handle, &split) }, -4);
    assert_eq!(seyal_app_last_error(handle), 29, "PaneSplitUnavailable");

    // CloseTab (24) / ClosePane (26) on the sole Tab/Pane reach
    // ShellState's last-of-one guard, whether the id is real or not.
    let mut close_tab = identity_fence(24, &snap);
    close_tab.target_execution_lo = tab_row.id_lo;
    close_tab.target_execution_hi = tab_row.id_hi;
    assert_eq!(unsafe { seyal_app_apply(handle, &close_tab) }, -4);
    assert_eq!(seyal_app_last_error(handle), 31, "CannotCloseLastTab");

    let mut close_pane = identity_fence(26, &snap);
    close_pane.target_execution_lo = pane_row.id_lo;
    close_pane.target_execution_hi = pane_row.id_hi;
    assert_eq!(unsafe { seyal_app_apply(handle, &close_pane) }, -4);
    assert_eq!(seyal_app_last_error(handle), 32, "CannotCloseLastPane");

    // Shell composition is unchanged by every rejected action above.
    let shell = seyal_app_shell(handle);
    assert_eq!(shell.tab_count, 1);
    assert_eq!(shell.pane_count, 1);
    assert_eq!(seyal_app_destroy(handle), 0);
}

#[test]
fn shell_projection_is_one_local_workspace() {
    let handle = seyal_app_create();
    let shell = seyal_app_shell(handle);
    assert_eq!(shell.workspace_count, 1);
    assert_eq!(shell.tab_count, 1);
    assert_eq!(shell.pane_count, 1);
    assert_eq!(
        shell.flags, 0,
        "M001 default shell policy disallows tab creation/pane splitting, \
         and the sole Tab/Pane cannot be closed"
    );
    let workspace = seyal_app_shell_row(handle, 0, 0);
    assert_eq!(workspace.flags & 1, 1);
    let title = unsafe {
        std::str::from_utf8(std::slice::from_raw_parts(
            workspace.title,
            workspace.title_len as usize,
        ))
        .unwrap()
    };
    assert_eq!(title, "Local");
    let inspector = seyal_app_chrome_row(handle, 0, 0);
    assert!(inspector.title_len > 0);
    assert_eq!(seyal_app_destroy(handle), 0);
}

#[test]
fn explicit_handle_round_trip_and_unknown_handle_fail_closed() {
    let handle = seyal_app_create();
    assert_ne!(handle, 0);
    let snap = seyal_app_snapshot(handle);
    assert_eq!(snap.version, APP_ABI_VERSION);
    assert_eq!(snap.eligibility, 0);
    assert_eq!(unsafe { seyal_app_apply(u64::MAX, ptr::null()) }, -5);
    assert_eq!(seyal_app_destroy(handle), 0);
    assert_eq!(seyal_app_destroy(handle), -1);
    let missing = seyal_app_snapshot(handle);
    assert_eq!(missing.generation, 0);
    assert_eq!(seyal_app_option_as_alt(u64::MAX), 0);
}

#[test]
fn version_and_size_mismatch_fail_closed() {
    let handle = seyal_app_create();
    let mut action = fence_action(0, &ApplicationRoot::new());
    action.version = 99;
    assert_eq!(unsafe { seyal_app_apply(handle, &action) }, -2);
    action.version = APP_ABI_VERSION;
    action.size = 4;
    assert_eq!(unsafe { seyal_app_apply(handle, &action) }, -3);
    assert_eq!(seyal_app_destroy(handle), 0);
}

#[test]
fn bind_through_explicit_handle_does_not_use_implicit_select() {
    let handle = seyal_app_create();
    let snap = seyal_app_snapshot(handle);
    let mut action = SeyalAppAction {
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
    assert_eq!(unsafe { seyal_app_apply(handle, &action) }, 0);
    let bound = seyal_app_snapshot(handle);
    assert_eq!(bound.eligibility, 1);
    assert_eq!(bound.flags & SNAP_COMPOSER, SNAP_COMPOSER);
    assert_eq!(bound.flags & SNAP_HAS_EXECUTION, SNAP_HAS_EXECUTION);
    action.kind = 0;
    assert_eq!(unsafe { seyal_app_apply(handle, &action) }, -4);
    assert_eq!(seyal_app_last_error(handle), 3);

    let composer = seyal_app_composer(handle);
    let mut draft = identity_fence(10, &bound);
    draft.target_pty_generation = composer.epoch;
    let text = b"echo hi";
    draft.payload = text.as_ptr();
    draft.payload_len = text.len() as u32;
    let mut unfenced = draft;
    unfenced.flags = 0;
    unfenced.fence_execution_lo = 0;
    unfenced.fence_execution_hi = 0;
    unfenced.fence_attachment_lo = 0;
    unfenced.fence_attachment_hi = 0;
    assert_eq!(unsafe { seyal_app_apply(handle, &unfenced) }, -4);
    assert_eq!(seyal_app_last_error(handle), 3);
    assert_eq!(unsafe { seyal_app_apply(handle, &draft) }, 0);

    let blocks = identity_fence(13, &seyal_app_snapshot(handle));
    assert_eq!(unsafe { seyal_app_apply(handle, &blocks) }, 0);
    assert_eq!(seyal_app_composer(handle).block_count, 0);
    let span = seyal_app_block_span(handle, 0);
    assert_eq!(span.start_line, 0);
    assert_eq!(span.end_line, 0);
    assert_eq!(seyal_app_destroy(handle), 0);
}

#[test]
fn block_selection_ffi_marks_row_and_switches_inspector() {
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
    let bound = seyal_app_snapshot(handle);

    // No Blocks yet: select fails closed with the published error code.
    let mut select = identity_fence(45, &bound);
    select.target_execution_lo = 0x5151_5151_5151_5151;
    select.target_execution_hi = 0x5151_5151_5151_5151;
    assert_eq!(unsafe { seyal_app_apply(handle, &select) }, -4);
    assert_eq!(seyal_app_last_error(handle), 30);
    assert_eq!(seyal_app_chrome(handle).inspector_mode, 0);

    // Runtime-projected Blocks are the only row source; seed them through
    // the application root (the C entry point reads the live client).
    let fence = APPS.with(|apps| apps.borrow().get(&handle).unwrap().root.fence());
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let state = apps.get_mut(&handle).unwrap();
        state
            .root
            .apply(AppAction::ApplyRuntimeBlocks {
                fence,
                records: vec![
                    RuntimeBlockRecord {
                        id: BlockId::from_bytes([0x51; 16]),
                        command: "git status".into(),
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
    });
    let first = seyal_app_block_row(handle, 0);
    assert_eq!(first.flags, 2, "completed, unselected");
    select.target_execution_lo = first.id_lo;
    select.target_execution_hi = first.id_hi;
    assert_eq!(unsafe { seyal_app_apply(handle, &select) }, 0);
    let chrome = seyal_app_chrome(handle);
    assert_eq!(chrome.inspector_mode, 4);
    assert_ne!(chrome.reserved & 2, 0, "inspector revealed");
    assert_eq!(chrome.inspector_row_count, 6);
    let command = seyal_app_chrome_row(handle, 0, 0);
    assert_eq!(utf8(command.title, command.title_len), "Block · Command");
    assert_eq!(utf8(command.detail, command.detail_len), "git status");
    let exit = seyal_app_chrome_row(handle, 0, 2);
    assert_eq!(utf8(exit.title, exit.title_len), "Block · Exit code");
    assert_eq!(utf8(exit.detail, exit.detail_len), "0");
    let selected = seyal_app_block_row(handle, 0);
    assert_eq!(selected.flags & BLOCK_STATE_MASK, 2);
    assert_eq!(selected.flags & BLOCK_SELECTED, BLOCK_SELECTED);
    assert_eq!(
        seyal_app_block_row(handle, 1).flags,
        1,
        "running, unselected"
    );

    let clear = identity_fence(46, &seyal_app_snapshot(handle));
    assert_eq!(unsafe { seyal_app_apply(handle, &clear) }, 0);
    assert_eq!(seyal_app_chrome(handle).inspector_mode, 0);
    assert_eq!(seyal_app_block_row(handle, 0).flags, 2);
    assert_eq!(seyal_app_destroy(handle), 0);
}

#[test]
fn palette_ffi_round_trips_open_filter_move_run_and_fails_closed() {
    let handle = seyal_app_create();
    let snap = seyal_app_snapshot(handle);

    let closed = seyal_app_palette(handle);
    assert_eq!(closed.size as usize, size_of::<SeyalAppPalette>());
    assert_eq!(closed.flags, 0);
    assert_eq!(closed.row_count, 0);
    assert!(closed.query_utf8.is_null());

    let open = identity_fence(47, &snap);
    assert_eq!(unsafe { seyal_app_apply(handle, &open) }, 0);
    let opened = seyal_app_palette(handle);
    assert_eq!(opened.flags & PALETTE_OPEN, PALETTE_OPEN);
    assert!(opened.row_count > 0);

    // Core Terminal chrome is visible by default, so the available
    // toggle command is "Hide Inspector", not "Show Inspector".
    let mut filter = identity_fence(48, &snap);
    let query = b"Hide Inspector";
    filter.payload = query.as_ptr();
    filter.payload_len = query.len() as u32;
    assert_eq!(unsafe { seyal_app_apply(handle, &filter) }, 0);
    let filtered = seyal_app_palette(handle);
    assert_eq!(filtered.row_count, 1);
    assert_eq!(
        utf8(filtered.query_utf8, filtered.query_utf8_len),
        "Hide Inspector"
    );
    let row = seyal_app_palette_row(handle, 0);
    assert_eq!(utf8(row.title, row.title_len), "Hide Inspector");
    assert_eq!(utf8(row.detail, row.detail_len), "View");
    assert_eq!(
        seyal_app_palette_row(handle, 1).title_len,
        0,
        "out of range is empty"
    );

    // No match: Run fails closed with the published error code and the
    // palette stays open.
    let mut none_filter = identity_fence(48, &snap);
    let no_match = b"zzz-no-such-command";
    none_filter.payload = no_match.as_ptr();
    none_filter.payload_len = no_match.len() as u32;
    assert_eq!(unsafe { seyal_app_apply(handle, &none_filter) }, 0);
    let run_empty = identity_fence(50, &snap);
    assert_eq!(unsafe { seyal_app_apply(handle, &run_empty) }, -4);
    assert_eq!(seyal_app_last_error(handle), 27);
    assert_eq!(seyal_app_palette(handle).flags & PALETTE_OPEN, PALETTE_OPEN);

    // Re-filter to the known single row and run it.
    assert_eq!(unsafe { seyal_app_apply(handle, &filter) }, 0);
    let run = identity_fence(50, &snap);
    assert_eq!(unsafe { seyal_app_apply(handle, &run) }, 0);
    assert_eq!(
        seyal_app_palette(handle).flags & PALETTE_OPEN,
        0,
        "Run closes the palette"
    );
    let chrome = seyal_app_chrome(handle);
    assert_eq!(
        chrome.reserved & 2,
        0,
        "SEYAL_APP_CHROME_INSPECTOR_VISIBLE bit cleared by Hide Inspector"
    );

    assert_eq!(seyal_app_destroy(handle), 0);
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
        target_pty_generation: 0,
        payload: ptr::null(),
        payload_len: 0,
        reserved: 0,
    }
}

#[test]
fn runtime_block_from_command_preserves_protocol_lifecycle() {
    let running = CommandBlock {
        id: 7,
        command: "echo hi".into(),
        start_line: 3,
        end_line: None,
        state: CommandBlockState::Running,
    };
    let projected = runtime_block_from_command(&running);
    assert_eq!(projected.command, "echo hi");
    assert!(projected.running);
    assert_eq!(projected.exit_status, None);
    assert_eq!(projected.start_line, 3);
    let mut expected = [0u8; 16];
    expected[..8].copy_from_slice(&7u64.to_le_bytes());
    assert_eq!(projected.id, BlockId::from_bytes(expected));

    let failed = CommandBlock {
        id: 8,
        command: "false".into(),
        start_line: 4,
        end_line: Some(4),
        state: CommandBlockState::Completed {
            exit_status: Some(1),
        },
    };
    let projected = runtime_block_from_command(&failed);
    assert!(!projected.running);
    assert_eq!(projected.exit_status, Some(1));
    assert_eq!(projected.end_line, Some(4));
}

#[test]
fn adaptive_depth_copy_is_rust_owned() {
    let handle = seyal_app_create();
    let placeholder = seyal_app_copy(handle, 0);
    let execute = seyal_app_copy(handle, 1);
    let prompt = seyal_app_copy(handle, 2);
    assert_eq!(copy_text(placeholder), "Type a command...");
    assert_eq!(copy_text(execute), "⏎");
    assert_eq!(copy_text(prompt), "$");
    assert_eq!(seyal_app_destroy(handle), 0);
}

#[test]
fn refresh_alternate_screen_after_bind_derives_tui() {
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
    let bound = seyal_app_snapshot(handle);
    assert_eq!(bound.eligibility, 1);
    let mut refresh = identity_fence(2, &bound);
    refresh.flags |= FLAG_ALTERNATE_SCREEN;
    assert_eq!(unsafe { seyal_app_apply(handle, &refresh) }, 0);
    let tui = seyal_app_snapshot(handle);
    assert_eq!(tui.eligibility, 3);
    assert_eq!(tui.flags & SNAP_COMPOSER, 0);
    assert_eq!(seyal_app_composer(handle).mode, 0);
    refresh.flags &= !FLAG_ALTERNATE_SCREEN;
    refresh.fence_epoch = tui.epoch;
    assert_eq!(unsafe { seyal_app_apply(handle, &refresh) }, 0);
    assert_eq!(seyal_app_snapshot(handle).eligibility, 1);
    assert_eq!(seyal_app_destroy(handle), 0);
}

fn copy_text(row: SeyalAppRow) -> String {
    if row.title.is_null() || row.title_len == 0 {
        return String::new();
    }
    let bytes = unsafe { slice::from_raw_parts(row.title, row.title_len as usize) };
    String::from_utf8_lossy(bytes).into_owned()
}
