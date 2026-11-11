use std::{cell::Cell, os::unix::net::UnixStream};

use seyal_runtime::local_ipc::framing::{
    HistoryCell, HistoryRangeSnapshot, HistoryRangeStatus, HistoryRow, SessionOverlayWire,
    SessionStatus,
};

use super::display::{
    seyal_bridge_history_range_consume_for_handle, seyal_bridge_history_range_peek_for_handle,
    seyal_bridge_history_range_row_for_handle, seyal_bridge_next_history_request_id_for,
    seyal_bridge_request_history_range_for, seyal_bridge_session_request_for_handle,
    seyal_bridge_session_surface_for_handle,
};
use crate::{
    ffi::{CLIENTS, FOCUSED_DISPLAY_HANDLE},
    local::test_client_for_ffi,
};

#[test]
fn unknown_handle_history_apis_fail_closed() {
    assert_eq!(seyal_bridge_next_history_request_id_for(0), 0);
    assert_eq!(
        seyal_bridge_request_history_range_for(0, 1, 1, 2, 512, 131_072, 0),
        -1
    );
    let peek = seyal_bridge_history_range_peek_for_handle(0, 1, 1);
    assert_eq!(peek.block_id, 0);
    assert_eq!(peek.request_id, 0);
    assert_eq!(seyal_bridge_history_range_consume_for_handle(0, 1, 1), 0);
}

#[test]
fn explicit_session_request_is_scoped_and_rejects_stale_block_menu_targets() {
    const HANDLE_A: u64 = u64::MAX - 111;
    const HANDLE_B: u64 = u64::MAX - 112;
    let (stream_a, _peer_a) = UnixStream::pair().expect("controller A stream");
    let (stream_b, _peer_b) = UnixStream::pair().expect("controller B stream");
    let mut client_a = test_client_for_ffi(stream_a);
    let mut client_b = test_client_for_ffi(stream_b);
    client_a.session_bootstrap_negotiated = true;
    client_b.session_bootstrap_negotiated = true;
    client_a.session_status = Some(SessionStatus {
        attachment_id: client_a.attachment_id,
        execution_id: client_a.execution_id,
        host_block_id: 17,
        overlay: SessionOverlayWire::Candidate,
        revision: 3,
    });
    client_b.session_status = Some(SessionStatus {
        attachment_id: client_b.attachment_id,
        execution_id: client_b.execution_id,
        host_block_id: 27,
        overlay: SessionOverlayWire::Candidate,
        revision: 5,
    });
    CLIENTS.with(|clients| {
        let mut clients = clients.borrow_mut();
        assert!(clients.insert(HANDLE_A, Box::new(client_a)).is_none());
        assert!(clients.insert(HANDLE_B, Box::new(client_b)).is_none());
    });
    let previous_focus = FOCUSED_DISPLAY_HANDLE.with(Cell::get);
    FOCUSED_DISPLAY_HANDLE.with(|focused| focused.set(HANDLE_A));

    let surface_b = seyal_bridge_session_surface_for_handle(HANDLE_B);
    assert_eq!(surface_b.kind, 0, "a Candidate must not create a banner");
    assert_eq!(surface_b.host_block_id, 27);
    assert_eq!(surface_b.revision, 5);

    // Runtime publishes a newer Candidate after the menu was opened. Its old
    // Block/revision pair must not be retargeted to the replacement state.
    CLIENTS.with(|clients| {
        let mut clients = clients.borrow_mut();
        let client = clients.get_mut(&HANDLE_A).unwrap();
        client.session_status = Some(SessionStatus {
            attachment_id: client.attachment_id,
            execution_id: client.execution_id,
            host_block_id: 18,
            overlay: SessionOverlayWire::Candidate,
            revision: 4,
        });
    });
    assert_eq!(seyal_bridge_session_request_for_handle(HANDLE_A, 17, 3), -1);
    // Focus still points at A, but the explicit action remains scoped to B.
    assert_eq!(seyal_bridge_session_request_for_handle(HANDLE_B, 27, 5), 0);

    FOCUSED_DISPLAY_HANDLE.with(|focused| focused.set(previous_focus));
    CLIENTS.with(|clients| {
        let mut clients = clients.borrow_mut();
        clients.remove(&HANDLE_A);
        clients.remove(&HANDLE_B);
    });
}

#[test]
fn history_response_lookup_stays_on_requested_controller_when_another_is_focused() {
    const HANDLE_A: u64 = u64::MAX - 101;
    const HANDLE_B: u64 = u64::MAX - 102;
    const BLOCK_ID: u64 = 7;
    const REQUEST_ID: u64 = 1;

    let (stream_a, _peer_a) = UnixStream::pair().expect("controller A stream");
    let (stream_b, _peer_b) = UnixStream::pair().expect("controller B stream");
    let mut client_a = test_client_for_ffi(stream_a);
    let mut client_b = test_client_for_ffi(stream_b);
    let response = |scalar| HistoryRangeSnapshot {
        request_id: REQUEST_ID,
        block_id: BLOCK_ID,
        revision: 1,
        status: HistoryRangeStatus::Complete,
        rows: vec![HistoryRow {
            line_id: 11,
            cells: vec![HistoryCell {
                scalar,
                foreground: 0,
                background: 0,
                flags: 0,
                reserved: 0,
            }],
        }],
        sidecar: Vec::new(),
    };
    client_a
        .history_ranges
        .insert((BLOCK_ID, REQUEST_ID), response('A' as u32));
    client_b
        .history_ranges
        .insert((BLOCK_ID, REQUEST_ID), response('B' as u32));

    CLIENTS.with(|clients| {
        let mut clients = clients.borrow_mut();
        assert!(clients.insert(HANDLE_A, Box::new(client_a)).is_none());
        assert!(clients.insert(HANDLE_B, Box::new(client_b)).is_none());
    });
    let previous_focus = FOCUSED_DISPLAY_HANDLE.with(Cell::get);
    FOCUSED_DISPLAY_HANDLE.with(|focused| focused.set(HANDLE_A));

    let range = seyal_bridge_history_range_peek_for_handle(HANDLE_B, BLOCK_ID, REQUEST_ID);
    assert_eq!(range.block_id, BLOCK_ID);
    assert_eq!(range.request_id, REQUEST_ID);
    let row = seyal_bridge_history_range_row_for_handle(HANDLE_B, BLOCK_ID, REQUEST_ID, 0);
    assert_eq!(row.cell_count, 1);
    // SAFETY: the row borrows Controller B's cache, which remains live until
    // this test removes the clients after copying the scalar.
    let scalar = unsafe { (*row.cells).scalar };
    assert_eq!(scalar, 'B' as u32);

    FOCUSED_DISPLAY_HANDLE.with(|focused| focused.set(previous_focus));
    CLIENTS.with(|clients| {
        let mut clients = clients.borrow_mut();
        clients.remove(&HANDLE_A);
        clients.remove(&HANDLE_B);
    });
}
