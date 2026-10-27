//! FFI coverage for C2 tab creation enablement and shell composition actions.

use super::*;
use std::{mem::size_of, ptr, slice, str};

use super::encode::{SNAP_CONTROLLER, SNAP_HAS_ATTACHMENT, SNAP_HAS_EXECUTION};

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
fn shell_composition_actions_decode_and_hierarchical_close_reaches_zero_windows() {
    let handle = seyal_app_create();
    let snap = seyal_app_snapshot(handle);

    assert_eq!(
        unsafe { seyal_app_apply(handle, &identity_fence(23, &snap)) },
        0
    );
    let after_create = seyal_app_shell(handle);
    assert_eq!(after_create.tab_count, 2);
    assert_ne!(
        after_create.flags & SHELL_FLAG_ALLOWS_TAB_CREATION,
        0,
        "production composition advertises tab creation after C2b"
    );

    let stale_tab = seyal_app_shell_row(handle, 1, 0);
    let observed_generation = after_create.containment_generation;
    let before_create = seyal_app_snapshot(handle);
    assert_eq!(
        unsafe { seyal_app_apply(handle, &identity_fence(23, &before_create)) },
        0
    );
    let mut stale_close = identity_fence(24, &seyal_app_snapshot(handle));
    stale_close.target_execution_lo = stale_tab.id_lo;
    stale_close.target_execution_hi = stale_tab.id_hi;
    stale_close.target_generation = observed_generation;
    assert_eq!(unsafe { seyal_app_apply(handle, &stale_close) }, -4);
    assert_eq!(seyal_app_last_error(handle), 2, "StalePane");
    assert_eq!(seyal_app_shell(handle).tab_count, 3);

    let mut split = identity_fence(25, &seyal_app_snapshot(handle));
    split.reserved = 1;
    assert_eq!(unsafe { seyal_app_apply(handle, &split) }, 0);
    let after_split = seyal_app_shell(handle);
    assert_eq!(after_split.pane_count, 2);
    assert_ne!(
        after_split.flags & SHELL_FLAG_ALLOWS_PANE_SPLITTING,
        0,
        "production composition advertises pane splitting after C3"
    );

    let created_pane = seyal_app_shell_row(handle, 2, 1);
    let mut close_pane = identity_fence(26, &seyal_app_snapshot(handle));
    close_pane.target_execution_lo = created_pane.id_lo;
    close_pane.target_execution_hi = created_pane.id_hi;
    close_pane.target_generation = seyal_app_shell(handle).containment_generation;
    assert_eq!(unsafe { seyal_app_apply(handle, &close_pane) }, 0);
    assert_eq!(seyal_app_shell(handle).pane_count, 1);

    let sole_pane = seyal_app_shell_row(handle, 2, 0);
    let mut close_last_pane = identity_fence(26, &seyal_app_snapshot(handle));
    close_last_pane.target_execution_lo = sole_pane.id_lo;
    close_last_pane.target_execution_hi = sole_pane.id_hi;
    close_last_pane.target_generation = seyal_app_shell(handle).containment_generation;
    assert_eq!(unsafe { seyal_app_apply(handle, &close_last_pane) }, 0);
    assert_eq!(seyal_app_shell(handle).tab_count, 2);

    // Close the other pre-existing Tab, then the last Tab; the final close
    // removes the Window (hierarchical close).
    for expected_tabs in [1, 0] {
        let remaining = seyal_app_shell_row(handle, 1, 0);
        let mut close_tab = identity_fence(24, &seyal_app_snapshot(handle));
        close_tab.target_execution_lo = remaining.id_lo;
        close_tab.target_execution_hi = remaining.id_hi;
        close_tab.target_generation = seyal_app_shell(handle).containment_generation;
        assert_eq!(unsafe { seyal_app_apply(handle, &close_tab) }, 0);
        assert_eq!(seyal_app_shell(handle).tab_count, expected_tabs);
    }

    let shell = seyal_app_shell(handle);
    assert_eq!(shell.window_count, 0);
    assert_eq!(shell.active_window_lo, 0);
    assert_eq!(shell.active_window_hi, 0);
    assert_eq!(shell.tab_count, 0);
    assert_eq!(shell.pane_count, 0);

    assert_eq!(seyal_app_destroy(handle), 0);
}

#[test]
fn ffi_close_rejects_stale_containment_generation_without_mutation() {
    let handle = seyal_app_create();
    let initial = seyal_app_shell(handle);
    let tab = seyal_app_shell_row(handle, 1, 0);
    let mut close = identity_fence(24, &seyal_app_snapshot(handle));
    close.target_execution_lo = tab.id_lo;
    close.target_execution_hi = tab.id_hi;
    close.target_generation = initial.containment_generation.saturating_add(1);

    assert_eq!(unsafe { seyal_app_apply(handle, &close) }, -4);
    let after = seyal_app_shell(handle);
    assert_eq!(after.containment_generation, initial.containment_generation);
    assert_eq!(after.window_count, initial.window_count);
    assert_eq!(after.tab_count, initial.tab_count);
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
        shell.flags,
        SHELL_FLAG_ALLOWS_TAB_CREATION
            | SHELL_FLAG_ALLOWS_PANE_SPLITTING
            | SHELL_FLAG_ALLOWS_TAB_CLOSE
            | SHELL_FLAG_ALLOWS_PANE_CLOSE,
        "production advertises tab creation and pane splitting after C2b/C3"
    );
    assert_ne!(
        shell.flags & 12,
        0,
        "hierarchical close is admitted while a Window exists"
    );

    let workspace = seyal_app_shell_row(handle, 0, 0);
    assert_eq!(workspace.flags & 1, 1);
    let title = unsafe {
        str::from_utf8(slice::from_raw_parts(
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
fn terminate_execution_action_decodes_as_kind_59() {
    let handle = seyal_app_create();
    let snap = seyal_app_snapshot(handle);
    // Unbound: TerminateExecution fails closed (not a silent no-op).
    assert_eq!(
        unsafe { seyal_app_apply(handle, &identity_fence(59, &snap)) },
        -4
    );
    assert_eq!(seyal_app_last_error(handle), 7, "UnboundUnauthorized");
    assert_eq!(seyal_app_destroy(handle), 0);
}

#[test]
fn close_window_action_uses_observed_containment_generation() {
    let handle = seyal_app_create();
    let mut ack = identity_fence(5, &seyal_app_snapshot(handle));
    while seyal_app_shell(handle).effect_count > 0 {
        assert_eq!(unsafe { seyal_app_apply(handle, &ack) }, 0);
        ack = identity_fence(5, &seyal_app_snapshot(handle));
    }
    let shell = seyal_app_shell(handle);
    let snap = seyal_app_snapshot(handle);
    let mut close = identity_fence(76, &snap);
    close.target_execution_lo = shell.active_window_lo;
    close.target_execution_hi = shell.active_window_hi;
    close.target_generation = shell.containment_generation;

    assert_eq!(unsafe { seyal_app_apply(handle, &close) }, 0);
    let after = seyal_app_shell(handle);
    assert_eq!(after.window_count, 0);
    assert_eq!(after.tab_count, 0);
    assert_eq!(
        after.effect_count, 1,
        "close must expose DestroyWindowRealization"
    );
    let effect = seyal_app_native_effect(handle, 0);
    assert_eq!(effect.kind, 3, "DestroyWindowRealization");
    assert_eq!(
        (effect.window_lo, effect.window_hi),
        (shell.active_window_lo, shell.active_window_hi)
    );
    assert_eq!(seyal_app_destroy(handle), 0);
}

#[test]
fn composer_for_pane_isolates_sibling_drafts_across_split() {
    let handle = seyal_app_create();
    let first_pane_lo = seyal_app_snapshot(handle).pane_lo;
    let first_pane_hi = seyal_app_snapshot(handle).pane_hi;

    // Draft on the sole Pane before splitting.
    let snap = seyal_app_snapshot(handle);
    let epoch = seyal_app_composer(handle).epoch;
    let draft = b"pane-a-draft";
    let mut set = identity_fence(10, &snap);
    set.target_generation = epoch;
    set.payload = draft.as_ptr();
    set.payload_len = draft.len() as u32;
    assert_eq!(unsafe { seyal_app_apply(handle, &set) }, 0);

    let mut split = identity_fence(25, &seyal_app_snapshot(handle));
    split.reserved = 0; // SplitAxis::Right
    assert_eq!(unsafe { seyal_app_apply(handle, &split) }, 0);
    assert_eq!(seyal_app_shell(handle).pane_count, 2);

    let focused = seyal_app_snapshot(handle);
    assert!(
        focused.pane_lo != first_pane_lo || focused.pane_hi != first_pane_hi,
        "split focuses the new leaf"
    );

    let first = seyal_app_composer_for_pane(handle, first_pane_lo, first_pane_hi);
    assert_eq!(first.size as usize, size_of::<SeyalAppComposer>());
    assert_eq!(first.draft_utf8_len, draft.len() as u32);
    let bytes = unsafe { slice::from_raw_parts(first.draft_utf8, first.draft_utf8_len as usize) };
    assert_eq!(bytes, draft.as_slice());

    let second = seyal_app_composer(handle);
    assert_eq!(
        second.draft_utf8_len, 0,
        "new Pane starts with an empty draft"
    );

    // Mutating the focused composer must not clobber the sibling draft.
    let epoch2 = second.epoch;
    let other = b"pane-b-draft";
    let mut set2 = identity_fence(10, &focused);
    set2.target_generation = epoch2;
    set2.payload = other.as_ptr();
    set2.payload_len = other.len() as u32;
    assert_eq!(unsafe { seyal_app_apply(handle, &set2) }, 0);

    let first_after = seyal_app_composer_for_pane(handle, first_pane_lo, first_pane_hi);
    let bytes = unsafe {
        slice::from_raw_parts(first_after.draft_utf8, first_after.draft_utf8_len as usize)
    };
    assert_eq!(bytes, draft.as_slice());
    let focused_after = seyal_app_composer(handle);
    let bytes = unsafe {
        slice::from_raw_parts(
            focused_after.draft_utf8,
            focused_after.draft_utf8_len as usize,
        )
    };
    assert_eq!(bytes, other.as_slice());

    let unknown = seyal_app_composer_for_pane(handle, 0, 0);
    assert_eq!(unknown.draft_utf8_len, 0);
    assert_eq!(unknown.mode, 0);

    assert_eq!(seyal_app_destroy(handle), 0);
}
