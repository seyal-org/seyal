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
        target_pty_generation: 0,
        payload: ptr::null(),
        payload_len: 0,
        reserved: 0,
    }
}

fn enable_tab_creation_for_test(handle: u64) {
    APPS.with(|apps| {
        apps.borrow_mut()
            .get_mut(&handle)
            .expect("handle")
            .root
            .enable_tab_creation_for_test();
    });
}

#[test]
fn shell_composition_actions_decode_and_reach_shell_state_and_fail_closed() {
    let handle = seyal_app_create();
    let snap = seyal_app_snapshot(handle);

    assert_eq!(
        unsafe { seyal_app_apply(handle, &identity_fence(23, &snap)) },
        -4
    );
    assert_eq!(seyal_app_last_error(handle), 28, "TabCreationUnavailable");

    enable_tab_creation_for_test(handle);
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
        "opted-in composition advertises tab creation"
    );
    let mut split = identity_fence(25, &seyal_app_snapshot(handle));
    split.reserved = 1;
    assert_eq!(unsafe { seyal_app_apply(handle, &split) }, -4);
    assert_eq!(seyal_app_last_error(handle), 29, "PaneSplitUnavailable");

    let active_pane = seyal_app_shell_row(handle, 2, 0);
    let mut close_pane = identity_fence(26, &seyal_app_snapshot(handle));
    close_pane.target_execution_lo = active_pane.id_lo;
    close_pane.target_execution_hi = active_pane.id_hi;
    assert_eq!(unsafe { seyal_app_apply(handle, &close_pane) }, -4);
    assert_eq!(seyal_app_last_error(handle), 32, "CannotCloseLastPane");

    let created = seyal_app_shell_row(handle, 1, 1);
    let mut close_tab = identity_fence(24, &seyal_app_snapshot(handle));
    close_tab.target_execution_lo = created.id_lo;
    close_tab.target_execution_hi = created.id_hi;
    assert_eq!(unsafe { seyal_app_apply(handle, &close_tab) }, 0);
    assert_eq!(seyal_app_shell(handle).tab_count, 1);

    let remaining = seyal_app_shell_row(handle, 1, 0);
    let mut close_last = identity_fence(24, &seyal_app_snapshot(handle));
    close_last.target_execution_lo = remaining.id_lo;
    close_last.target_execution_hi = remaining.id_hi;
    assert_eq!(unsafe { seyal_app_apply(handle, &close_last) }, -4);
    assert_eq!(seyal_app_last_error(handle), 31, "CannotCloseLastTab");

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
        "production keeps tab creation gated until a live create→attach→bind driver exists"
    );

    enable_tab_creation_for_test(handle);
    let opted_in = seyal_app_shell(handle);
    assert_eq!(
        opted_in.flags, SHELL_FLAG_ALLOWS_TAB_CREATION,
        "opted-in composition advertises tab creation; splits and sole Tab/Pane close stay off"
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
fn terminate_execution_action_decodes_as_kind_58() {
    let handle = seyal_app_create();
    let snap = seyal_app_snapshot(handle);
    // Unbound: TerminateExecution fails closed (not a silent no-op).
    assert_eq!(
        unsafe { seyal_app_apply(handle, &identity_fence(58, &snap)) },
        -4
    );
    assert_eq!(seyal_app_last_error(handle), 7, "UnboundUnauthorized");
    assert_eq!(seyal_app_destroy(handle), 0);
}
