use super::*;
use seyal_runtime::{
    local_ipc::framing::{
        encode_frame, Attached, ErrorCode, Lifecycle, Resync, TerminateExecutionRequest,
        TerminateExecutionResult, TerminateExecutionResultCode,
    },
    AttachmentId, ExecutionId,
};
use std::io::Write;

#[test]
fn implicit_resolution_fails_closed_when_multiple_executions_are_running() {
    let first = ExecutionId::from_bytes([1; 16]);
    let second = ExecutionId::from_bytes([2; 16]);
    let list = ExecutionList {
        entries: vec![
            seyal_runtime::local_ipc::framing::ExecutionListEntry {
                execution_id: first,
                lifecycle: Lifecycle::Running,
                has_controller: false,
                attachment_count: 0,
            },
            seyal_runtime::local_ipc::framing::ExecutionListEntry {
                execution_id: second,
                lifecycle: Lifecycle::Running,
                has_controller: false,
                attachment_count: 0,
            },
        ],
    };
    assert_eq!(
        resolve_single_running_execution(&list),
        Err(ClientError::AmbiguousExecutions)
    );
}

#[test]
fn attach_error_wire_codes_preserve_controller_busy_and_capacity_semantics() {
    for (code, expected) in [
        (
            ErrorCode::ControllerBusy,
            ClientError::Server(ErrorCode::ControllerBusy),
        ),
        (
            ErrorCode::CapacityExceeded,
            ClientError::Server(ErrorCode::CapacityExceeded),
        ),
    ] {
        let (client, mut server) = UnixStream::pair().expect("unix stream pair");
        let execution_id = ExecutionId::from_bytes([3; 16]);
        let server_thread = std::thread::spawn(move || {
            let (kind, _) = read_blocking_frame(&mut server).expect("attach request");
            assert_eq!(kind, MessageType::Attach);
            let error = ErrorMessage {
                error_code: code as u16,
                offending_message_type: MessageType::Attach as u16,
                detail_code: 0,
            };
            server
                .write_all(&encode_frame(MessageType::Error, &error.encode()))
                .expect("attach error response");
        });

        let result = LocalDisplayClient::finish_attach(
            client,
            execution_id,
            Role::Controller,
            false,
            false,
            9,
            false,
        );
        assert_eq!(result.err(), Some(expected));
        server_thread.join().expect("server thread");
    }
}

#[test]
fn read_only_attach_requests_observer_authority() {
    let (client, mut server) = UnixStream::pair().expect("unix stream pair");
    let execution_id = ExecutionId::from_bytes([7; 16]);
    let server_thread = std::thread::spawn(move || {
        let (kind, payload) = read_blocking_frame(&mut server).expect("attach request");
        assert_eq!(kind, MessageType::Attach);
        let attach = Attach::decode(&payload).expect("decode attach request");
        assert_eq!(attach.execution_id, execution_id);
        assert_eq!(attach.requested_role, Role::Observer);
        let error = ErrorMessage {
            error_code: ErrorCode::InvalidExecution as u16,
            offending_message_type: MessageType::Attach as u16,
            detail_code: 0,
        };
        server
            .write_all(&encode_frame(MessageType::Error, &error.encode()))
            .expect("attach error response");
    });

    let result = LocalDisplayClient::finish_attach(
        client,
        execution_id,
        Role::Observer,
        false,
        false,
        9,
        false,
    );
    assert_eq!(
        result.err(),
        Some(ClientError::Server(ErrorCode::InvalidExecution))
    );
    server_thread.join().expect("server thread");
}

#[test]
fn startup_deadline_bounds_a_stalled_attach_read() {
    let (client, mut server) = UnixStream::pair().expect("unix stream pair");
    let execution_id = ExecutionId::from_bytes([4; 16]);
    let server_thread = std::thread::spawn(move || {
        let (kind, _) = read_blocking_frame(&mut server).expect("attach request");
        assert_eq!(kind, MessageType::Attach);
        std::thread::sleep(Duration::from_millis(120));
    });

    let started = std::time::Instant::now();
    let result = LocalDisplayClient::finish_attach_with_deadline(
        client,
        execution_id,
        Role::Controller,
        false,
        false,
        9,
        false,
        false,
        false,
        1,
        std::time::Instant::now() + Duration::from_millis(25),
    );
    assert!(matches!(
        result.as_ref().map_err(|e| &e.error),
        Err(&ClientError::StartupDeadlineExceeded)
    ));
    assert!(
        started.elapsed() < Duration::from_millis(90),
        "stalled attach exceeded the supplied startup deadline"
    );
    server_thread.join().expect("server thread");
}

fn attached(execution_id: ExecutionId, attachment_id: AttachmentId, generation: u64) -> Vec<u8> {
    encode_frame(
        MessageType::Attached,
        &Attached {
            execution_id,
            attachment_id,
            granted_role: Role::Controller,
            current_generation: generation,
        }
        .encode(),
    )
}

fn snapshot_chunk(
    generation: u64,
    columns: u16,
    first_col: u16,
    chunk_index: u16,
    chunk_count: u16,
    scalar: char,
) -> Vec<u8> {
    let mut payload = Vec::with_capacity(64);
    payload.extend_from_slice(&generation.to_le_bytes());
    payload.extend_from_slice(&0u64.to_le_bytes());
    payload.extend_from_slice(&1u16.to_le_bytes());
    payload.extend_from_slice(&columns.to_le_bytes());
    payload.extend_from_slice(&0u16.to_le_bytes());
    payload.extend_from_slice(&0u16.to_le_bytes());
    payload.extend_from_slice(&[1, 0, 0, 0]);
    payload.extend_from_slice(&0u16.to_le_bytes());
    payload.extend_from_slice(&1u16.to_le_bytes());
    payload.extend_from_slice(&chunk_index.to_le_bytes());
    payload.extend_from_slice(&chunk_count.to_le_bytes());
    payload.extend_from_slice(&1u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.extend_from_slice(&2u16.to_le_bytes());
    payload.extend_from_slice(&first_col.to_le_bytes());
    payload.extend_from_slice(&(scalar as u32).to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.extend_from_slice(&(40u32).to_le_bytes());
    encode_frame(MessageType::DisplaySnapshotV2, &payload)
}

fn snapshot(generation: u64, scalar: char) -> Vec<u8> {
    snapshot_chunk(generation, 1, 0, 0, 1, scalar)
}

fn malformed_snapshot(generation: u64) -> Vec<u8> {
    let mut frame = snapshot(generation, 'X');
    let meta_offset = HEADER_LEN + 48 + 12;
    frame[meta_offset..meta_offset + 4].copy_from_slice(&(104u32).to_le_bytes());
    frame
}

fn block_timeline(revision: u64) -> Vec<u8> {
    encode_frame(
        MessageType::BlockTimeline,
        &BlockTimeline {
            revision,
            records: Vec::new(),
        }
        .encode(),
    )
}

#[test]
fn malformed_initial_snapshot_requests_resync_then_converges_transactionally() {
    let (client, mut server) = UnixStream::pair().expect("unix stream pair");
    let execution_id = ExecutionId::from_bytes([5; 16]);
    let attachment_id = AttachmentId::from_bytes([6; 16]);
    let (release_server, hold_server) = std::sync::mpsc::sync_channel(0);
    let server_thread = std::thread::spawn(move || {
        let (kind, _) = read_blocking_frame(&mut server).expect("attach request");
        assert_eq!(kind, MessageType::Attach);
        server
            .write_all(&attached(execution_id, attachment_id, 2))
            .expect("attached response");
        server
            .write_all(&malformed_snapshot(2))
            .expect("malformed snapshot");

        let (kind, payload) = read_blocking_frame(&mut server).expect("resync request");
        assert_eq!(kind, MessageType::Resync);
        assert_eq!(
            Resync::decode(&payload).unwrap().attachment_id,
            attachment_id
        );
        server.write_all(&snapshot(2, 'V')).expect("valid snapshot");
        hold_server.recv().expect("client release");
    });

    let attached = LocalDisplayClient::finish_attach_with_deadline(
        client,
        execution_id,
        Role::Controller,
        false,
        false,
        9,
        false,
        false,
        false,
        1,
        std::time::Instant::now() + Duration::from_millis(250),
    )
    .expect("initial resync should converge");
    assert_eq!(attached.cache().generation, 2);
    assert_eq!(attached.cache().cells[0].scalar, 'V');
    release_server.send(()).expect("release server");
    server_thread.join().expect("server thread");
}

#[test]
fn repeated_malformed_initial_snapshots_terminate_within_existing_deadline() {
    let (client, mut server) = UnixStream::pair().expect("unix stream pair");
    let execution_id = ExecutionId::from_bytes([7; 16]);
    let attachment_id = AttachmentId::from_bytes([8; 16]);
    let server_thread = std::thread::spawn(move || {
        let (kind, _) = read_blocking_frame(&mut server).expect("attach request");
        assert_eq!(kind, MessageType::Attach);
        server
            .write_all(&attached(execution_id, attachment_id, 3))
            .expect("attached response");
        server
            .write_all(&malformed_snapshot(3))
            .expect("first malformed snapshot");
        let (kind, _) = read_blocking_frame(&mut server).expect("bounded resync request");
        assert_eq!(kind, MessageType::Resync);
        server
            .write_all(&malformed_snapshot(3))
            .expect("second malformed snapshot");
    });

    let started = std::time::Instant::now();
    let result = LocalDisplayClient::finish_attach_with_deadline(
        client,
        execution_id,
        Role::Controller,
        false,
        false,
        9,
        false,
        false,
        false,
        1,
        std::time::Instant::now() + Duration::from_millis(250),
    );
    assert_eq!(
        result.as_ref().err().map(|e| &e.error),
        Some(&ClientError::Display)
    );
    assert!(started.elapsed() < Duration::from_millis(250));
    server_thread.join().expect("server thread");
}

#[test]
fn malformed_multichunk_attach_quarantines_stale_remainder_before_resync_snapshot() {
    let (client, mut server) = UnixStream::pair().expect("unix stream pair");
    let execution_id = ExecutionId::from_bytes([9; 16]);
    let attachment_id = AttachmentId::from_bytes([10; 16]);
    let (release_server, hold_server) = std::sync::mpsc::sync_channel(0);
    let server_thread = std::thread::spawn(move || {
        let (kind, _) = read_blocking_frame(&mut server).expect("attach request");
        assert_eq!(kind, MessageType::Attach);
        server
            .write_all(&attached(execution_id, attachment_id, 4))
            .expect("attached response");

        let mut malformed_first = snapshot_chunk(4, 2, 0, 0, 2, 'X');
        let meta_offset = HEADER_LEN + 48 + 12;
        malformed_first[meta_offset..meta_offset + 4].copy_from_slice(&(104u32).to_le_bytes());
        assert_eq!(display_chunk_remainder_hint(&malformed_first), Some(1));
        server
            .write_all(&malformed_first)
            .expect("malformed first chunk");
        server
            .write_all(&snapshot_chunk(4, 2, 1, 1, 2, 'Y'))
            .expect("stale remainder");

        let (kind, payload) = read_blocking_frame(&mut server).expect("resync request");
        assert_eq!(kind, MessageType::Resync);
        assert_eq!(
            Resync::decode(&payload).unwrap().attachment_id,
            attachment_id
        );
        let valid = snapshot(4, 'V');
        assert_eq!(display_chunk_remainder_hint(&valid), Some(0));
        assert!(matches!(
            classify_resync_scan_frame(&valid).unwrap(),
            ResyncScanFrame::SnapshotStart
        ));
        assert!(decode_chunk(&valid).is_ok());
        server.write_all(&valid).expect("valid resync snapshot");
        hold_server.recv().expect("client release");
    });

    let attached = LocalDisplayClient::finish_attach_with_deadline(
        client,
        execution_id,
        Role::Controller,
        false,
        false,
        9,
        false,
        false,
        false,
        1,
        std::time::Instant::now() + Duration::from_millis(250),
    )
    .expect("stale remainder should not poison bounded resync");
    assert_eq!(attached.cache().generation, 4);
    assert_eq!(attached.cache().cells[0].scalar, 'V');
    release_server.send(()).expect("release server");
    server_thread.join().expect("server thread");
}

fn assert_attach_resync_preserves_block_timeline(timeline_before_snapshot: bool) {
    let (client, mut server) = UnixStream::pair().expect("unix stream pair");
    let execution_id = ExecutionId::from_bytes([11; 16]);
    let attachment_id = AttachmentId::from_bytes([12; 16]);
    let (release_server, hold_server) = std::sync::mpsc::sync_channel(0);
    let server_thread = std::thread::spawn(move || {
        let (kind, _) = read_blocking_frame(&mut server).expect("attach request");
        assert_eq!(kind, MessageType::Attach);
        server
            .write_all(&attached(execution_id, attachment_id, 5))
            .expect("attached response");

        let mut malformed_first = snapshot_chunk(5, 2, 0, 0, 2, 'X');
        let meta_offset = HEADER_LEN + 48 + 12;
        malformed_first[meta_offset..meta_offset + 4].copy_from_slice(&(104u32).to_le_bytes());
        server
            .write_all(&malformed_first)
            .expect("malformed first chunk");
        server
            .write_all(&snapshot_chunk(5, 2, 1, 1, 2, 'Y'))
            .expect("stale remainder");

        let (kind, _) = read_blocking_frame(&mut server).expect("resync request");
        assert_eq!(kind, MessageType::Resync);
        if timeline_before_snapshot {
            server
                .write_all(&block_timeline(7))
                .expect("queued block timeline");
        }
        server
            .write_all(&snapshot(5, 'V'))
            .expect("valid resync snapshot");
        if !timeline_before_snapshot {
            server
                .write_all(&block_timeline(7))
                .expect("queued block timeline");
        }
        hold_server.recv().expect("client release");
    });

    let mut attached = LocalDisplayClient::finish_attach_with_deadline(
        client,
        execution_id,
        Role::Controller,
        true,
        false,
        9,
        false,
        false,
        false,
        1,
        std::time::Instant::now() + Duration::from_millis(250),
    )
    .expect("valid control frame must not poison bounded resync");
    assert_eq!(attached.cache().generation, 5);
    assert_eq!(attached.cache().cells[0].scalar, 'V');
    attached
        .poll_prepare()
        .expect("retained timeline should reach normal consumer");
    assert_eq!(attached.block_timeline().revision, 7);
    release_server.send(()).expect("release server");
    server_thread.join().expect("server thread");
}

#[test]
fn attach_resync_retains_block_timeline_before_replacement_snapshot() {
    assert_attach_resync_preserves_block_timeline(true);
}

#[test]
fn attach_resync_processes_block_timeline_after_replacement_snapshot() {
    assert_attach_resync_preserves_block_timeline(false);
}

fn terminate_result_frame(attachment_id: AttachmentId, request_id: u64) -> Vec<u8> {
    encode_frame(
        MessageType::TerminateExecutionResult,
        &TerminateExecutionResult {
            attachment_id,
            request_id,
            result_code: TerminateExecutionResultCode::TerminationRequested,
            detail_code: 0,
        }
        .encode(),
    )
}

/// ADR-017 §6.3 row 2: Created never-bound orphan Attached then snapshot fails
/// → exactly one TerminateExecutionRequest on that attachment.
#[test]
fn orphan_attached_snapshot_failure_terminates_once_on_existing_attachment() {
    let (client, mut server) = UnixStream::pair().expect("unix stream pair");
    let execution_id = ExecutionId::from_bytes([0xA1; 16]);
    let attachment_id = AttachmentId::from_bytes([0xA2; 16]);
    let server_thread = std::thread::spawn(move || {
        let (kind, _) = read_blocking_frame(&mut server).expect("attach request");
        assert_eq!(kind, MessageType::Attach);
        server
            .write_all(&attached(execution_id, attachment_id, 1))
            .expect("attached");
        server
            .write_all(&malformed_snapshot(1))
            .expect("first malformed");
        let (kind, _) = read_blocking_frame(&mut server).expect("resync");
        assert_eq!(kind, MessageType::Resync);
        server
            .write_all(&malformed_snapshot(1))
            .expect("second malformed");
        let (kind, payload) = read_blocking_frame(&mut server).expect("terminate");
        assert_eq!(kind, MessageType::TerminateExecutionRequest);
        let req = TerminateExecutionRequest::decode(&payload).expect("decode terminate");
        assert_eq!(req.attachment_id, attachment_id);
        assert_eq!(req.execution_id, execution_id);
        assert_eq!(req.request_id, 2);
        server
            .write_all(&terminate_result_frame(attachment_id, 2))
            .expect("terminate result");
        // No further control frames: row 2 forbids a second attach on this path.
        server
            .set_read_timeout(Some(Duration::from_millis(50)))
            .ok();
        assert!(
            read_blocking_frame(&mut server).is_err(),
            "must not open a second attach or repeat terminate"
        );
    });

    let result = LocalDisplayClient::finish_attach_with_deadline(
        client,
        execution_id,
        Role::Controller,
        false,
        false,
        9,
        false,
        true, // provisioning negotiated
        true, // Created never-bound orphan
        2,
        std::time::Instant::now() + Duration::from_millis(500),
    );
    let failure = match result {
        Err(failure) => failure,
        Ok(_) => panic!("orphan snapshot failure must err"),
    };
    assert_eq!(failure.error, ClientError::Display);
    assert!(
        failure.terminated_on_attachment,
        "row 2 must mark terminate-on-attachment so callers skip dispose attach"
    );
    server_thread.join().expect("server thread");
}

/// Adopt/reconnect must not kill a live survivor on transient snapshot failure.
#[test]
fn adopt_attached_snapshot_failure_does_not_terminate_execution() {
    let (client, mut server) = UnixStream::pair().expect("unix stream pair");
    let execution_id = ExecutionId::from_bytes([0xB1; 16]);
    let attachment_id = AttachmentId::from_bytes([0xB2; 16]);
    let server_thread = std::thread::spawn(move || {
        let (kind, _) = read_blocking_frame(&mut server).expect("attach request");
        assert_eq!(kind, MessageType::Attach);
        server
            .write_all(&attached(execution_id, attachment_id, 1))
            .expect("attached");
        server
            .write_all(&malformed_snapshot(1))
            .expect("first malformed");
        let (kind, _) = read_blocking_frame(&mut server).expect("resync");
        assert_eq!(kind, MessageType::Resync);
        server
            .write_all(&malformed_snapshot(1))
            .expect("second malformed");
        server
            .set_read_timeout(Some(Duration::from_millis(50)))
            .ok();
        assert!(
            read_blocking_frame(&mut server).is_err(),
            "adopt/reconnect must not send TerminateExecutionRequest"
        );
    });

    let result = LocalDisplayClient::finish_attach_with_deadline(
        client,
        execution_id,
        Role::Controller,
        false,
        false,
        9,
        false,
        true,  // provisioning negotiated — still must not dispose
        false, // adopt/reconnect survivor, not Created orphan
        1,
        std::time::Instant::now() + Duration::from_millis(500),
    );
    let failure = match result {
        Err(failure) => failure,
        Ok(_) => panic!("snapshot failure must still surface"),
    };
    assert_eq!(failure.error, ClientError::Display);
    assert!(
        !failure.terminated_on_attachment,
        "live survivor must not be terminated on attach snapshot failure"
    );
    server_thread.join().expect("server thread");
}
