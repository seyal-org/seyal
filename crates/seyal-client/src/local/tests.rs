use super::*;
use seyal_runtime::local_ipc::framing::{ErrorCode, ErrorMessage, MessageType};
use seyal_runtime::pass8::CAP_BLOCK_METADATA;
use std::io::{Read, Write};

fn test_client(stream: UnixStream) -> LocalDisplayClient {
    LocalDisplayClient {
        stream,
        buffered: Vec::new(),
        read_offset: 0,
        pending_batch: display_apply::PendingDisplayBatch::default(),
        outbound: VecDeque::new(),
        outbound_wire_bytes: 0,
        runtime_id: 1,
        execution_id: ExecutionId::from_bytes([1; 16]),
        attachment_id: AttachmentId::from_bytes([2; 16]),
        role: Role::Controller,
        block_metadata_negotiated: false,
        extended_terminal_key_supported: false,
        block_cache: BlockCache::default(),
        cache: seyal_runtime::display::empty_cache(),
        prepared: PreparedSurface::default(),
        last_preparation: PreparationResult {
            generation: 0,
            rebuilt_rows: RowDamage::none(),
            rebuilt_row_count: 0,
            rebuilt_cell_count: 0,
            full_rebuild: false,
        },
        needs_initial_prepare: false,
        next_resize_request_id: 1,
        desired_geometry: None,
        committed_geometry: GridGeometry {
            rows: 1,
            columns: 1,
        },
        unresolved_resizes: VecDeque::new(),
        applied_awaiting_projection: None,
        retry_suppression: None,
        resync_needed: false,
        input_failure: None,
        resize_failure: None,
        block_timeline: BlockTimeline {
            revision: 0,
            records: Vec::new(),
        },
        command_blocks_supported: false,
        last_composer_result: None,
        composer_status: None,
        pending_composer_requests: std::collections::HashSet::new(),
        next_composer_request_id: 1,
        history_ranges: HashMap::new(),
        history_requests: HashMap::new(),
        next_history_request_id: 1,
        copied_text: Vec::new(),
        last_admitted_v2_action_id: 0,
        last_sent_v2_action_id: 0,
        highest_v2_error_id: 0,
        last_admitted_mouse_action_id: 0,
    }
}

fn v2_snapshot_chunk(
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

fn v2_snapshot(generation: u64, scalar: char) -> Vec<u8> {
    v2_snapshot_chunk(generation, 1, 0, 0, 1, scalar)
}

#[test]
fn malformed_v2_display_requests_resync_before_valid_snapshot_converges() {
    let (client_stream, mut server_stream) = UnixStream::pair().expect("stream pair");
    client_stream
        .set_nonblocking(true)
        .expect("nonblocking client");
    let malformed = {
        let mut frame = v2_snapshot(1, 'A');
        let meta_offset = HEADER_LEN + 48 + 12;
        frame[meta_offset..meta_offset + 4].copy_from_slice(&(104u32).to_le_bytes());
        frame
    };
    server_stream
        .write_all(&malformed)
        .expect("malformed frame");
    server_stream
        .write_all(&v2_snapshot(2, 'B'))
        .expect("valid snapshot");

    let mut client = test_client(client_stream);
    let result = client.poll_prepare().expect("resync should recover");
    assert!(result.is_some());
    assert_eq!(client.cache.generation, 2);
    assert_eq!(client.cache.cells[0].scalar, 'B');

    let mut outbound = [0u8; 128];
    let count = server_stream.read(&mut outbound).expect("resync frame");
    let header = FrameHeader::decode(&outbound[..count]).expect("resync header");
    assert_eq!(header.message_type, MessageType::Resync as u16);
    assert!(!client.resync_needed);
}

#[test]
fn gapped_v2_display_keeps_committed_cache_and_resyncs_before_converging() {
    let (client_stream, mut server_stream) = UnixStream::pair().expect("stream pair");
    client_stream
        .set_nonblocking(true)
        .expect("nonblocking client");
    server_stream
        .write_all(&v2_snapshot(1, 'A'))
        .expect("initial snapshot");

    let mut client = test_client(client_stream);
    client.poll_prepare().expect("initial commit");
    assert_eq!(client.cache.generation, 1);
    assert_eq!(client.cache.cells[0].scalar, 'A');

    server_stream
        .write_all(&v2_snapshot_chunk(2, 2, 1, 1, 2, 'X'))
        .expect("gapped snapshot chunk");
    let result = client
        .poll_prepare()
        .expect("semantic display corruption should request resync");
    assert!(result.is_none());
    assert_eq!(client.cache.generation, 1);
    assert_eq!(client.cache.cells[0].scalar, 'A');

    let mut outbound = [0u8; 128];
    let count = server_stream.read(&mut outbound).expect("resync frame");
    let header = FrameHeader::decode(&outbound[..count]).expect("resync header");
    assert_eq!(header.message_type, MessageType::Resync as u16);
    assert_eq!(count, HEADER_LEN + header.payload_len as usize);

    server_stream
        .write_all(&v2_snapshot(2, 'B'))
        .expect("authoritative snapshot");
    let result = client
        .poll_prepare()
        .expect("resync snapshot should commit");
    assert!(result.is_some());
    assert_eq!(client.cache.generation, 2);
    assert_eq!(client.cache.cells[0].scalar, 'B');
}

#[test]
fn composer_status_is_fenced_to_attachment_and_forward_revision() {
    use seyal_runtime::local_ipc::framing::ComposerEligibility;
    let mine = AttachmentId::from_bytes([2; 16]);
    let status = |attachment_id, revision| ComposerStatus {
        attachment_id,
        eligibility: ComposerEligibility::Available,
        revision,
    };
    // First status for this attachment is accepted; zero revision never is.
    assert!(validate_composer_status(status(mine, 1), mine, None));
    assert!(!validate_composer_status(status(mine, 0), mine, None));
    // Another attachment's eligibility cannot enable this composer.
    let foreign = AttachmentId::from_bytes([3; 16]);
    assert!(!validate_composer_status(status(foreign, 5), mine, None));
    // Revision must move forward: duplicates and stale frames are dropped.
    let current = Some(status(mine, 4));
    assert!(validate_composer_status(status(mine, 5), mine, current));
    assert!(!validate_composer_status(status(mine, 4), mine, current));
    assert!(!validate_composer_status(status(mine, 3), mine, current));
}

#[test]
fn composer_status_frames_update_the_client_in_revision_order() {
    use seyal_runtime::local_ipc::framing::ComposerEligibility;
    let (ours, mut theirs) = UnixStream::pair().expect("socketpair");
    ours.set_nonblocking(true).expect("nonblocking");
    let mut client = test_client(ours);
    let mine = client.attachment_id;
    let frame = |attachment_id, eligibility, revision| {
        encode_frame(
            MessageType::ComposerStatus,
            &ComposerStatus {
                attachment_id,
                eligibility,
                revision,
            }
            .encode(),
        )
    };
    theirs
        .write_all(&frame(mine, ComposerEligibility::Busy, 1))
        .expect("write");
    theirs
        .write_all(&frame(mine, ComposerEligibility::Available, 2))
        .expect("write");
    // Stale replay and a foreign attachment must not regress the fact.
    theirs
        .write_all(&frame(mine, ComposerEligibility::Busy, 1))
        .expect("write");
    theirs
        .write_all(&frame(
            AttachmentId::from_bytes([9; 16]),
            ComposerEligibility::Busy,
            7,
        ))
        .expect("write");
    assert!(client.composer_status().is_none());
    client.poll_prepare().expect("poll");
    let status = client.composer_status().expect("status published");
    assert_eq!(status.eligibility, ComposerEligibility::Available);
    assert_eq!(status.revision, 2);
}

#[test]
fn raw_metadata_fallback_keeps_pass71_but_drops_only_pass8_capability() {
    let full = discovery::requested_capabilities(true, true);
    let fallback = discovery::requested_capabilities(false, true);
    assert_ne!(
        full & seyal_runtime::local_ipc::framing::CAP_EXTENDED_TERMINAL_KEY,
        0
    );
    assert_ne!(
        fallback & seyal_runtime::local_ipc::framing::CAP_EXTENDED_TERMINAL_KEY,
        0
    );
    assert_ne!(full & CAP_BLOCK_METADATA, 0);
    assert_eq!(fallback & CAP_BLOCK_METADATA, 0);
    assert_ne!(
        full & seyal_runtime::local_ipc::framing::CAP_COMMAND_BLOCKS,
        0
    );
    assert_ne!(
        fallback & seyal_runtime::local_ipc::framing::CAP_COMMAND_BLOCKS,
        0
    );
}

#[test]
fn v2_key_is_rejected_when_extended_capability_was_not_negotiated() {
    let (client_stream, _server_stream) = UnixStream::pair().expect("socket pair");
    let mut client = test_client(client_stream);
    let error = client
        .submit_terminal_key_v2(
            TerminalKeyV2Kind::ArrowUp,
            TerminalKeyV2Modifiers::NONE,
            0,
            TerminalKeyV2Event::Press,
            0,
            1,
        )
        .expect_err("old-server clients must not encode TerminalKeyV2");
    assert_eq!(error, ClientError::UnsupportedInteractiveCapability);
    assert!(client.outbound.is_empty());
}

#[test]
fn v2_key_zero_action_id_is_rejected_before_encoding() {
    let (client_stream, _server_stream) = UnixStream::pair().expect("socket pair");
    let mut client = test_client(client_stream);
    client.extended_terminal_key_supported = true;
    let error = client
        .submit_terminal_key_v2(
            TerminalKeyV2Kind::ArrowUp,
            TerminalKeyV2Modifiers::NONE,
            0,
            TerminalKeyV2Event::Press,
            0,
            0,
        )
        .expect_err("action_id 0 is not a correlated V2 action");
    assert_eq!(error, ClientError::Protocol);
    assert!(client.outbound.is_empty());
}

fn type29(error_code: ErrorCode, detail_code: u32) -> ErrorMessage {
    ErrorMessage {
        error_code: error_code as u16,
        offending_message_type: MessageType::TerminalKeyV2 as u16,
        detail_code,
    }
}

fn submit_v2(client: &mut LocalDisplayClient, action_id: u32) -> Result<(), ClientError> {
    client.submit_terminal_key_v2(
        TerminalKeyV2Kind::ArrowUp,
        TerminalKeyV2Modifiers::NONE,
        0,
        TerminalKeyV2Event::Press,
        0,
        action_id,
    )
}

#[test]
fn v2_backpressure_uses_sent_and_highest_error_bounds() {
    let (client_stream, _server_stream) = UnixStream::pair().expect("socket pair");
    let mut client = test_client(client_stream);
    client.extended_terminal_key_supported = true;
    submit_v2(&mut client, 7).expect("in-range V2 send");
    assert_eq!(client.last_admitted_v2_action_id, 7);
    assert_eq!(client.last_sent_v2_action_id, 7);

    assert_eq!(
        client
            .classify_incoming_error(type29(ErrorCode::Backpressure, 7))
            .unwrap(),
        Some(InputAdmissionFailure::ClientBackpressure)
    );
    assert_eq!(client.highest_v2_error_id, 7);

    assert_eq!(
        client.classify_incoming_error(type29(ErrorCode::Backpressure, 7)),
        Err(ClientError::Protocol)
    );
    assert_eq!(
        client.classify_incoming_error(type29(ErrorCode::Backpressure, 3)),
        Err(ClientError::Protocol)
    );
    assert_eq!(
        client.classify_incoming_error(type29(ErrorCode::Backpressure, 9)),
        Err(ClientError::Protocol)
    );
    assert_eq!(
        client.classify_incoming_error(type29(ErrorCode::Backpressure, 0)),
        Err(ClientError::Protocol)
    );
}

#[test]
fn v2_type29_authorization_and_encoding_keep_the_connection() {
    let (client_stream, _server_stream) = UnixStream::pair().expect("socket pair");
    let mut client = test_client(client_stream);
    client.extended_terminal_key_supported = true;
    submit_v2(&mut client, 4).expect("send 4");
    submit_v2(&mut client, 5).expect("send 5");
    submit_v2(&mut client, 6).expect("send 6");
    submit_v2(&mut client, 8).expect("send 8");

    assert_eq!(
        client
            .classify_incoming_error(type29(ErrorCode::PermissionDenied, 4))
            .unwrap(),
        Some(InputAdmissionFailure::LostController)
    );
    assert_eq!(
        client
            .classify_incoming_error(type29(ErrorCode::StaleIdentity, 5))
            .unwrap(),
        Some(InputAdmissionFailure::LostController)
    );
    assert_eq!(
        client
            .classify_incoming_error(type29(ErrorCode::InvalidExecution, 6))
            .unwrap(),
        Some(InputAdmissionFailure::LostController)
    );
    assert_eq!(
        client
            .classify_incoming_error(type29(ErrorCode::MalformedPayload, 8))
            .unwrap(),
        Some(InputAdmissionFailure::ClientBackpressure)
    );
    assert_eq!(client.highest_v2_error_id, 8);

    assert_eq!(
        client.classify_incoming_error(type29(ErrorCode::PermissionDenied, 4)),
        Err(ClientError::Protocol)
    );
    assert_eq!(
        client.classify_incoming_error(type29(ErrorCode::PermissionDenied, 9)),
        Err(ClientError::Protocol)
    );
    assert_eq!(
        client.classify_incoming_error(type29(ErrorCode::PermissionDenied, 0)),
        Err(ClientError::Protocol)
    );
    assert_eq!(
        client.classify_incoming_error(type29(ErrorCode::MalformedPayload, 0)),
        Err(ClientError::Server(ErrorCode::MalformedPayload))
    );
}

#[test]
fn v2_lost_controller_demotes_role_and_rejects_later_keys() {
    let (client_stream, _server_stream) = UnixStream::pair().expect("socket pair");
    let mut client = test_client(client_stream);
    client.extended_terminal_key_supported = true;
    submit_v2(&mut client, 4).expect("send 4");
    assert_eq!(client.role, Role::Controller);

    assert_eq!(
        client
            .classify_incoming_error(type29(ErrorCode::PermissionDenied, 4))
            .unwrap(),
        Some(InputAdmissionFailure::LostController)
    );
    assert_eq!(client.role, Role::Observer);
    assert_eq!(submit_v2(&mut client, 5), Err(ClientError::LostController));
    assert_eq!(
        client.input_failure,
        Some(InputAdmissionFailure::LostController)
    );
}

#[test]
fn v2_sent_bound_advances_only_after_wire_complete() {
    let (client_stream, mut server_stream) = UnixStream::pair().expect("socket pair");
    client_stream
        .set_nonblocking(true)
        .expect("nonblocking client");
    server_stream
        .set_nonblocking(true)
        .expect("nonblocking server");
    let mut client = test_client(client_stream);
    client.extended_terminal_key_supported = true;

    let chunk = "x".repeat(8192);
    for _ in 0..64 {
        match client.submit_committed_text(&chunk) {
            Ok(()) => {
                if !client.outbound.is_empty() {
                    break;
                }
            }
            Err(ClientError::ClientBackpressure) => break,
            Err(error) => panic!("fill: {error:?}"),
        }
    }
    assert!(
        !client.outbound.is_empty(),
        "an unread peer must leave FIFO bytes after a blocked flush"
    );

    submit_v2(&mut client, 7).expect("V2 admits ahead of a blocked flush");
    assert_eq!(client.last_admitted_v2_action_id, 7);
    assert_eq!(
        client.last_sent_v2_action_id, 0,
        "WouldBlock/partial writes must not advance the sent high-water"
    );
    assert_eq!(
        client.classify_incoming_error(type29(ErrorCode::Backpressure, 7)),
        Err(ClientError::Protocol)
    );

    let mut drain = [0u8; 65_536];
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while client.wants_write() {
        assert!(
            std::time::Instant::now() < deadline,
            "flush did not complete after the peer drained"
        );
        let _ = server_stream.read(&mut drain);
        client.flush_control_write().expect("flush after drain");
    }
    assert_eq!(client.last_sent_v2_action_id, 7);
    assert_eq!(
        client
            .classify_incoming_error(type29(ErrorCode::Backpressure, 7))
            .unwrap(),
        Some(InputAdmissionFailure::ClientBackpressure)
    );
}
