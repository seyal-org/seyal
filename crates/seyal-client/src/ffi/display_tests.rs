use std::{cell::Cell, os::unix::net::UnixStream};

use seyal_runtime::local_ipc::framing::{
    HistoryCell, HistoryRangeSnapshot, HistoryRangeStatus, HistoryRow,
};

use super::display::{
    seyal_bridge_history_range_consume_for_handle, seyal_bridge_history_range_peek_for_handle,
    seyal_bridge_history_range_row_for_handle, seyal_bridge_next_history_request_id_for,
    seyal_bridge_request_history_range_for,
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
