use super::*;
use seyal_runtime::local_ipc::framing::{
    encode_frame, envelope_presentation_frame, BlockViewportContentClip,
    BlockViewportContentRequest, BlockViewportContentSnapshot, BlockViewportRegionContent,
    ErrorCode, ErrorMessage, ExecutionList, ExecutionListEntry, FrameHeader, HistoryRangeSnapshot,
    HistoryRangeStatus, Lifecycle, LifecycleMessage, MessageType, ResumeDelivery,
};
use seyal_runtime::pass8::CAP_BLOCK_METADATA;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

#[path = "tests/delivery.rs"]
mod delivery;
#[path = "tests/session_bootstrap.rs"]
mod session_bootstrap;
pub(super) fn test_client(stream: UnixStream) -> LocalDisplayClient {
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
        last_layout_metrics: None,
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
        block_native_regions: seyal_runtime::local_ipc::framing::BlockNativeRegions {
            revision: 0,
            total_height: 0,
            records: Vec::new(),
        },
        block_viewport_index: crate::viewport::BlockViewportIndex::default(),
        block_viewport_plan: None,
        block_viewport_presentation_epoch: 0,
        last_plan_native_paint_identity: Vec::new(),
        block_viewport_scroll_anchor: crate::viewport::ScrollAnchor::LiveEnd,
        block_viewport_window_rows: None,
        command_blocks_supported: false,
        last_composer_result: None,
        composer_status: None,
        pending_composer_requests: std::collections::HashSet::new(),
        next_composer_request_id: 1,
        history_ranges: HashMap::new(),
        history_requests: HashMap::new(),
        block_viewport_content_requests: HashMap::new(),
        next_history_request_id: 1,
        copied_text: Vec::new(),
        last_admitted_v2_action_id: 0,
        last_sent_v2_action_id: 0,
        highest_v2_error_id: 0,
        last_admitted_mouse_action_id: 0,
        execution_provisioning_negotiated: false,
        delivery_control_negotiated: false,
        delivery_suspended: false,
        resume_sequence: 0,
        resume_fenced: false,
        next_provisioning_request_id: 1,
        harness_probe: true,
        pending_create_requests: std::collections::HashSet::new(),
        pending_terminate_requests: std::collections::HashSet::new(),
        pending_mint_requests: std::collections::HashSet::new(),
        last_create_result: VecDeque::new(),
        last_terminate_result: None,
        last_mint_result: None,
        viewport_line_ids: Vec::new(),
        viewport_line_ids_generation: 0,
        session_bootstrap_negotiated: false,
        composer_target: None,
        target_generation: 0,
        session_offer: None,
        session_status: None,
        pending_relay_requests: std::collections::HashSet::new(),
        projected_eligibility: 0,
        projected_wire_revision: 0,
        composer_projection_revision: 0,
    }
}

pub(super) fn v2_snapshot_chunk(
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
fn finalized_execution_ends_client_after_consuming_lifecycle_marker() {
    let (client_stream, mut server_stream) = UnixStream::pair().expect("stream pair");
    client_stream
        .set_nonblocking(true)
        .expect("nonblocking client");
    let execution_id = ExecutionId::from_bytes([1; 16]);
    let finalized = encode_frame(
        MessageType::Lifecycle,
        &LifecycleMessage {
            execution_id,
            lifecycle: Lifecycle::Finalized,
        }
        .encode(),
    );
    server_stream
        .write_all(&finalized)
        .expect("write finalized lifecycle");

    let mut client = test_client(client_stream);
    assert!(matches!(
        client.poll_prepare(),
        Err(ClientError::NoRunningExecution)
    ));
}

#[test]
fn pinned_execution_must_still_be_running_before_reconnect() {
    let execution_id = ExecutionId::from_bytes([1; 16]);
    let other_id = ExecutionId::from_bytes([2; 16]);
    let running = ExecutionList {
        entries: vec![ExecutionListEntry {
            execution_id,
            lifecycle: Lifecycle::Running,
            has_controller: false,
            attachment_count: 0,
        }],
    };
    let finalized = ExecutionList {
        entries: vec![ExecutionListEntry {
            execution_id,
            lifecycle: Lifecycle::Finalized,
            has_controller: false,
            attachment_count: 0,
        }],
    };
    let different_execution = ExecutionList {
        entries: vec![ExecutionListEntry {
            execution_id: other_id,
            lifecycle: Lifecycle::Running,
            has_controller: false,
            attachment_count: 0,
        }],
    };

    assert!(execution_is_running(&running, execution_id));
    assert!(!execution_is_running(&finalized, execution_id));
    assert!(!execution_is_running(&different_execution, execution_id));
}

#[test]
fn native_regions_commit_before_plan_rebuild_emits_clips() {
    // #1484: first-seen Blocks must clip from the committed map, not the prior
    // empty records that previously drove plan rebuild.
    let (client_stream, _server) = UnixStream::pair().expect("stream pair");
    client_stream
        .set_nonblocking(true)
        .expect("nonblocking client");
    let mut client = test_client(client_stream);
    client.block_viewport_window_rows = Some(24);
    client.block_native_regions = seyal_runtime::local_ipc::framing::BlockNativeRegions {
        revision: 1,
        total_height: 103,
        records: vec![seyal_runtime::local_ipc::framing::NativeBlockRegions {
            block_id: 42,
            header_region: 100,
            output_region: 101,
            first_row: 0,
            row_count: 0,
            sealed_rows: 100,
            header_rows: 3,
        }],
    };
    client.apply_block_native_regions_to_viewport();
    let plan = client
        .block_viewport_plan()
        .expect("plan must rebuild from committed map");
    assert!(
        plan.total_document_extent > 0,
        "committed map must produce non-zero extent"
    );
    assert_eq!(
        client.block_viewport_total_height(),
        104,
        "Rust document extent includes the explicit Block spacing row"
    );
    assert!(
        plan.clip_for(42).is_some(),
        "first-seen Block must emit a sealed clip from committed RegionIds"
    );
}

#[test]
fn viewport_content_request_batches_the_current_rust_clip_plan() {
    let (client_stream, mut server) = UnixStream::pair().expect("stream pair");
    client_stream
        .set_nonblocking(true)
        .expect("nonblocking client");
    let mut client = test_client(client_stream);
    client.block_viewport_window_rows = Some(24);
    client.block_native_regions = seyal_runtime::local_ipc::framing::BlockNativeRegions {
        revision: 7,
        total_height: 64,
        records: vec![seyal_runtime::local_ipc::framing::NativeBlockRegions {
            block_id: 42,
            header_region: 100,
            output_region: 101,
            first_row: 0,
            row_count: 40,
            sealed_rows: 40,
            header_rows: 2,
        }],
    };
    client.apply_block_native_regions_to_viewport();
    let plan = client.block_viewport_plan().expect("Rust viewport plan");
    let expected = plan
        .clipping
        .iter()
        .map(|clip| BlockViewportContentClip {
            region_id: clip.region_id,
            first_row: clip.first_row,
            row_count: clip.row_count,
        })
        .collect::<Vec<_>>();
    let generation = plan.content_generation;

    let request_id = client
        .request_block_viewport_content(generation)
        .expect("bounded batch request");
    let mut bytes = [0u8; 8192];
    let count = server.read(&mut bytes).expect("request frame");
    let header = FrameHeader::decode(&bytes[..count]).expect("frame header");
    assert_eq!(
        header.message_type,
        MessageType::BlockViewportContentRequest as u16
    );
    let request = BlockViewportContentRequest::decode(
        &bytes[seyal_runtime::local_ipc::framing::HEADER_LEN..count],
    )
    .expect("viewport batch payload");
    assert_eq!(request.request_id, request_id);
    assert_eq!(request.plan_generation, generation);
    assert_eq!(request.clips, expected);
    assert_eq!(
        client
            .request_block_viewport_content(generation)
            .expect("same plan reuses in-flight batch"),
        request_id
    );
}

#[test]
fn viewport_content_response_is_cached_only_for_the_current_clip_plan() {
    let (client_stream, mut server) = UnixStream::pair().expect("stream pair");
    client_stream
        .set_nonblocking(true)
        .expect("nonblocking client");
    let mut client = test_client(client_stream);
    client.block_viewport_window_rows = Some(24);
    client.block_native_regions = seyal_runtime::local_ipc::framing::BlockNativeRegions {
        revision: 9,
        total_height: 64,
        records: vec![seyal_runtime::local_ipc::framing::NativeBlockRegions {
            block_id: 42,
            header_region: 100,
            output_region: 101,
            first_row: 0,
            row_count: 40,
            sealed_rows: 40,
            header_rows: 2,
        }],
    };
    client.apply_block_native_regions_to_viewport();
    let plan = client.block_viewport_plan().expect("Rust viewport plan");
    let clips = plan.clipping.clone();
    let generation = plan.content_generation;
    let request_id = client
        .request_block_viewport_content(generation)
        .expect("bounded batch request");
    let mut ignored_request = [0u8; 8192];
    let _ = server.read(&mut ignored_request).expect("request frame");

    let response = BlockViewportContentSnapshot {
        request_id,
        plan_generation: generation,
        revision: 10,
        status: HistoryRangeStatus::Complete,
        regions: clips
            .iter()
            .map(|clip| BlockViewportRegionContent {
                region_id: clip.region_id,
                first_row: clip.first_row,
                snapshot: HistoryRangeSnapshot {
                    request_id,
                    block_id: clip.region_id,
                    revision: 10,
                    status: HistoryRangeStatus::Complete,
                    rows: Vec::new(),
                    sidecar: Vec::new(),
                },
            })
            .collect(),
    };
    client.buffered.extend(encode_frame(
        MessageType::BlockViewportContentSnapshot,
        &response.encode(),
    ));
    client.poll_prepare().expect("poll viewport response");

    for clip in clips {
        assert!(client
            .history_range_for(clip.region_id, request_id)
            .is_some());
    }
}

#[test]
fn live_row_count_growth_does_not_bump_presentation_epoch() {
    // #1484 P0: seq streaming grows live row_count without sealed RegionId change.
    // presentation_epoch must stay stable so Metal retains HISTORY pages.
    let (client_stream, _server) = UnixStream::pair().expect("stream pair");
    client_stream
        .set_nonblocking(true)
        .expect("nonblocking client");
    let mut client = test_client(client_stream);
    client.block_viewport_window_rows = Some(24);
    client.block_native_regions = seyal_runtime::local_ipc::framing::BlockNativeRegions {
        revision: 1,
        total_height: 10,
        records: vec![seyal_runtime::local_ipc::framing::NativeBlockRegions {
            block_id: 7,
            header_region: 1,
            output_region: 2,
            first_row: 0,
            row_count: 5,
            sealed_rows: 0,
            header_rows: 0,
        }],
    };
    client.apply_block_native_regions_to_viewport();
    let epoch_after_first = client.block_viewport_presentation_epoch;
    assert!(epoch_after_first > 0);

    client.block_native_regions.revision = 2;
    client.block_native_regions.records[0].row_count = 20;
    client.block_native_regions.total_height = 20;
    client.apply_block_native_regions_to_viewport();
    assert_eq!(
        client.block_viewport_presentation_epoch, epoch_after_first,
        "live height growth must not bump presentation_epoch"
    );
    assert_eq!(
        client.block_viewport_total_height(),
        21,
        "Fenwick index must still track live height and Rust-owned spacing"
    );

    client.block_native_regions.revision = 3;
    client.block_native_regions.records[0].sealed_rows = 20;
    client.block_native_regions.records[0].row_count = 0;
    client.block_native_regions.records[0].output_region = 9;
    client.apply_block_native_regions_to_viewport();
    assert!(
        client.block_viewport_presentation_epoch > epoch_after_first,
        "sealed RegionId / ownership change must bump presentation_epoch"
    );
}

#[test]
fn sealed_row_growth_with_stable_region_ids_does_not_bump_presentation_epoch() {
    // Sealing moves rows from the live window into the same canonical
    // RegionGrid. It changes content/extent, not Block structure or ownership.
    let (client_stream, _server) = UnixStream::pair().expect("stream pair");
    client_stream
        .set_nonblocking(true)
        .expect("nonblocking client");
    let mut client = test_client(client_stream);
    client.block_viewport_window_rows = Some(24);
    client.block_native_regions = seyal_runtime::local_ipc::framing::BlockNativeRegions {
        revision: 1,
        total_height: 42,
        records: vec![seyal_runtime::local_ipc::framing::NativeBlockRegions {
            block_id: 7,
            header_region: 1,
            output_region: 2,
            first_row: 0,
            row_count: 40,
            sealed_rows: 0,
            header_rows: 1,
        }],
    };
    client.apply_block_native_regions_to_viewport();
    let epoch = client.block_viewport_presentation_epoch;

    client.block_native_regions.revision = 2;
    client.block_native_regions.total_height = 142;
    client.block_native_regions.records[0].sealed_rows = 100;
    client.apply_block_native_regions_to_viewport();

    assert_eq!(
        client.block_viewport_presentation_epoch, epoch,
        "sealing additional rows into the same RegionGrid must not restart the Flow presentation generation"
    );
    assert_eq!(client.block_viewport_total_height(), 142);
}

#[test]
fn scroll_anchor_stays_on_the_same_block_row_when_earlier_output_grows() {
    let (client_stream, _server) = UnixStream::pair().expect("stream pair");
    client_stream
        .set_nonblocking(true)
        .expect("nonblocking client");
    let mut client = test_client(client_stream);
    client.block_viewport_window_rows = Some(3);
    client.block_native_regions = seyal_runtime::local_ipc::framing::BlockNativeRegions {
        revision: 1,
        total_height: 22,
        records: vec![
            seyal_runtime::local_ipc::framing::NativeBlockRegions {
                block_id: 1,
                header_region: 10,
                output_region: 11,
                first_row: 0,
                row_count: 0,
                sealed_rows: 10,
                header_rows: 0,
            },
            seyal_runtime::local_ipc::framing::NativeBlockRegions {
                block_id: 2,
                header_region: 20,
                output_region: 21,
                first_row: 0,
                row_count: 0,
                sealed_rows: 10,
                header_rows: 0,
            },
        ],
    };
    client.apply_block_native_regions_to_viewport();

    // The second Block begins at row 11; anchor its second row.
    client.set_block_viewport_scroll_anchor(false, 12, 3);
    assert_eq!(
        client.block_viewport_plan().expect("initial plan").scroll_y,
        12
    );

    // Four rows are sealed into the earlier Block while the reader is away
    // from LiveEnd. AbsoluteY=12 now points into Block 1 unless the Rust
    // viewport preserves the source-relative Block/row anchor.
    client.block_native_regions = seyal_runtime::local_ipc::framing::BlockNativeRegions {
        revision: 2,
        total_height: 26,
        records: vec![
            seyal_runtime::local_ipc::framing::NativeBlockRegions {
                block_id: 1,
                header_region: 10,
                output_region: 11,
                first_row: 0,
                row_count: 0,
                sealed_rows: 14,
                header_rows: 0,
            },
            seyal_runtime::local_ipc::framing::NativeBlockRegions {
                block_id: 2,
                header_region: 20,
                output_region: 21,
                first_row: 0,
                row_count: 0,
                sealed_rows: 10,
                header_rows: 0,
            },
        ],
    };
    client.apply_block_native_regions_to_viewport();

    let plan = client.block_viewport_plan().expect("updated plan");
    assert_eq!(
        plan.scroll_anchor,
        crate::viewport::ScrollAnchor::BlockRow {
            block_id: 2,
            row: 1,
        }
    );
    assert_eq!(plan.scroll_y, 16);
    assert_eq!(plan.visible_items[0].block_id, 2);
    assert_eq!(plan.visible_items[0].first_visible_row, 1);
}

#[test]
fn resizing_block_viewport_preserves_rust_scroll_anchor() {
    let (client_stream, _server) = UnixStream::pair().expect("stream pair");
    client_stream
        .set_nonblocking(true)
        .expect("nonblocking client");
    let mut client = test_client(client_stream);
    client.block_viewport_window_rows = Some(3);
    client.block_native_regions = seyal_runtime::local_ipc::framing::BlockNativeRegions {
        revision: 1,
        total_height: 30,
        records: vec![seyal_runtime::local_ipc::framing::NativeBlockRegions {
            block_id: 7,
            header_region: 70,
            output_region: 71,
            first_row: 0,
            row_count: 0,
            sealed_rows: 30,
            header_rows: 0,
        }],
    };
    client.apply_block_native_regions_to_viewport();
    client.set_block_viewport_scroll_anchor(false, 12, 3);

    client.set_block_viewport_window_rows(5);

    let plan = client.block_viewport_plan().expect("resized plan");
    assert_eq!(plan.window_rows, 5);
    assert_eq!(plan.scroll_y, 12);
    assert_eq!(
        plan.scroll_anchor,
        crate::viewport::ScrollAnchor::AbsoluteY(12)
    );
}

#[test]
fn native_scroller_position_becomes_a_block_relative_rust_anchor() {
    let (client_stream, _server) = UnixStream::pair().expect("stream pair");
    client_stream
        .set_nonblocking(true)
        .expect("nonblocking client");
    let mut client = test_client(client_stream);
    client.block_viewport_window_rows = Some(3);
    client.block_native_regions = seyal_runtime::local_ipc::framing::BlockNativeRegions {
        revision: 1,
        total_height: 22,
        records: vec![
            seyal_runtime::local_ipc::framing::NativeBlockRegions {
                block_id: 1,
                header_region: 10,
                output_region: 11,
                first_row: 0,
                row_count: 0,
                sealed_rows: 10,
                header_rows: 0,
            },
            seyal_runtime::local_ipc::framing::NativeBlockRegions {
                block_id: 2,
                header_region: 20,
                output_region: 21,
                first_row: 0,
                row_count: 0,
                sealed_rows: 10,
                header_rows: 0,
            },
        ],
    };
    client.apply_block_native_regions_to_viewport();

    assert_eq!(client.scroll_block_viewport_to(12, 3), 12);
    assert_eq!(
        client
            .block_viewport_plan()
            .expect("scrolled plan")
            .scroll_anchor,
        crate::viewport::ScrollAnchor::BlockRow {
            block_id: 2,
            row: 1,
        }
    );

    client.block_native_regions.revision = 2;
    client.block_native_regions.records[0].sealed_rows = 14;
    client.block_native_regions.total_height = 26;
    client.apply_block_native_regions_to_viewport();
    assert_eq!(
        client
            .block_viewport_plan()
            .expect("reflowed plan")
            .scroll_y,
        16,
        "native scroller input must resolve to a stable Block-relative anchor"
    );
}

#[test]
fn rust_block_scroll_deltas_clamp_and_restore_live_end() {
    let (client_stream, _server) = UnixStream::pair().expect("stream pair");
    client_stream
        .set_nonblocking(true)
        .expect("nonblocking client");
    let mut client = test_client(client_stream);
    client.block_viewport_window_rows = Some(10);
    client.block_native_regions = seyal_runtime::local_ipc::framing::BlockNativeRegions {
        revision: 1,
        total_height: 41,
        records: vec![seyal_runtime::local_ipc::framing::NativeBlockRegions {
            block_id: 12,
            header_region: 100,
            output_region: 101,
            first_row: 0,
            row_count: 20,
            sealed_rows: 20,
            header_rows: 1,
        }],
    };
    client.apply_block_native_regions_to_viewport();
    let epoch = client
        .block_viewport_plan()
        .expect("initial plan")
        .presentation_epoch;
    assert_eq!(
        client.block_viewport_plan().expect("initial plan").scroll_y,
        32
    );
    assert_eq!(client.scroll_block_viewport_by(-5, 10), 27);
    assert_eq!(client.scroll_block_viewport_by(5, 10), 32);
    assert_eq!(
        client
            .block_viewport_plan()
            .expect("live-end plan")
            .scroll_anchor,
        crate::viewport::ScrollAnchor::LiveEnd
    );
    assert_eq!(client.scroll_block_viewport_by(-100, 10), 0);
    assert_eq!(client.scroll_block_viewport_by(200, 10), 32);
    assert_eq!(
        client
            .block_viewport_plan()
            .expect("scrolled plan")
            .presentation_epoch,
        epoch,
        "scrolling must not invalidate stable RegionGrid paint content"
    );
}

#[test]
fn clearing_a_scrolled_block_drops_extent_and_returns_to_live_end() {
    let (client_stream, _server) = UnixStream::pair().expect("stream pair");
    client_stream
        .set_nonblocking(true)
        .expect("nonblocking client");
    let mut client = test_client(client_stream);
    client.block_viewport_window_rows = Some(10);
    client.block_native_regions = seyal_runtime::local_ipc::framing::BlockNativeRegions {
        revision: 1,
        total_height: 101,
        records: vec![seyal_runtime::local_ipc::framing::NativeBlockRegions {
            block_id: 7,
            header_region: 70,
            output_region: 71,
            first_row: 0,
            row_count: 0,
            sealed_rows: 100,
            header_rows: 0,
        }],
    };
    client.apply_block_native_regions_to_viewport();
    assert_eq!(client.scroll_block_viewport_to(20, 10), 20);
    assert_eq!(client.block_viewport_plan().expect("scrolled").scroll_y, 20);

    client.block_native_regions = seyal_runtime::local_ipc::framing::BlockNativeRegions {
        revision: 2,
        total_height: 0,
        records: vec![],
    };
    client.apply_block_native_regions_to_viewport();
    let cleared = client.block_viewport_plan().expect("cleared plan");
    assert_eq!(cleared.total_document_extent, 0);
    assert_eq!(cleared.scroll_y, 0);
    assert!(cleared.visible_items.is_empty());
    assert_eq!(client.scroll_block_viewport_to(u64::MAX, 10), 0);
    assert_eq!(
        client
            .block_viewport_plan()
            .expect("live end")
            .scroll_anchor,
        crate::viewport::ScrollAnchor::LiveEnd
    );
}

#[test]
fn shrinking_a_scrolled_block_keeps_its_row_and_rust_supplies_bottom_alignment() {
    let (client_stream, _server) = UnixStream::pair().expect("stream pair");
    client_stream
        .set_nonblocking(true)
        .expect("nonblocking client");
    let mut client = test_client(client_stream);
    client.block_viewport_window_rows = Some(10);
    client.block_native_regions = seyal_runtime::local_ipc::framing::BlockNativeRegions {
        revision: 1,
        total_height: 82,
        records: vec![
            seyal_runtime::local_ipc::framing::NativeBlockRegions {
                block_id: 1,
                header_region: 10,
                output_region: 11,
                first_row: 0,
                row_count: 0,
                sealed_rows: 40,
                header_rows: 0,
            },
            seyal_runtime::local_ipc::framing::NativeBlockRegions {
                block_id: 2,
                header_region: 20,
                output_region: 21,
                first_row: 0,
                row_count: 0,
                sealed_rows: 40,
                header_rows: 0,
            },
        ],
    };
    client.apply_block_native_regions_to_viewport();
    assert_eq!(client.scroll_block_viewport_to(42, 10), 42);
    assert_eq!(
        client
            .block_viewport_plan()
            .expect("scrolled")
            .scroll_anchor,
        crate::viewport::ScrollAnchor::BlockRow {
            block_id: 2,
            row: 1
        }
    );

    client.block_native_regions.records[0].sealed_rows = 2;
    client.block_native_regions.revision = 2;
    client.block_native_regions.total_height = 44;
    client.apply_block_native_regions_to_viewport();
    let shrunken = client.block_viewport_plan().expect("shrunken plan");
    assert_eq!(shrunken.scroll_y, 4);
    assert_eq!(shrunken.visible_items[0].block_id, 2);
    assert_eq!(shrunken.visible_items[0].first_visible_row, 1);

    client.block_native_regions.records[1].sealed_rows = 2;
    client.block_native_regions.revision = 3;
    client.block_native_regions.total_height = 6;
    client.apply_block_native_regions_to_viewport();
    let short = client.block_viewport_plan().expect("short plan");
    assert_eq!(short.total_document_extent, 6);
    assert_eq!(short.alignment_padding_rows, 4);
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
    let full = discovery::requested_capabilities(true, true, true, true, true, false);
    let fallback = discovery::requested_capabilities(false, true, true, true, true, false);
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

#[test]
fn malformed_type_35_discards_and_clears_without_protocol_error() {
    let (stream, _peer) = UnixStream::pair().expect("pair");
    let mut client = test_client(stream);
    client.cache.generation = 3;
    client.cache.rows = 2;
    client.viewport_line_ids = vec![10, 11];
    client.viewport_line_ids_generation = 3;
    let mut payload = seyal_runtime::local_ipc::framing::ViewportLineIds {
        generation: 1,
        line_ids: vec![10, 11],
    }
    .encode();
    payload[0..8].copy_from_slice(&0u64.to_le_bytes());
    assert_eq!(client.apply_viewport_line_ids(&payload), Ok(true));
    assert!(client.viewport_line_ids.is_empty());
    assert_eq!(client.viewport_line_ids_generation, 0);
}

#[test]
fn consecutive_soft_wrap_line_ids_commit() {
    let (stream, _peer) = UnixStream::pair().expect("pair");
    let mut client = test_client(stream);
    client.cache.generation = 5;
    client.cache.rows = 3;
    let payload = seyal_runtime::local_ipc::framing::ViewportLineIds {
        generation: 5,
        line_ids: vec![1, 1, 2],
    }
    .encode();
    assert_eq!(client.apply_viewport_line_ids(&payload), Ok(true));
    assert_eq!(client.viewport_line_ids, vec![1, 1, 2]);
}

#[test]
fn same_generation_different_row_count_clears_without_fatal() {
    let (stream, _peer) = UnixStream::pair().expect("pair");
    let mut client = test_client(stream);
    client.cache.generation = 5;
    client.cache.rows = 3;
    client.viewport_line_ids = vec![1, 2, 3];
    client.viewport_line_ids_generation = 5;
    let payload = seyal_runtime::local_ipc::framing::ViewportLineIds {
        generation: 5,
        line_ids: vec![1, 2],
    }
    .encode();
    assert_eq!(client.apply_viewport_line_ids(&payload), Ok(true));
    assert!(client.viewport_line_ids.is_empty());
}

#[test]
fn same_generation_and_row_count_conflicting_ids_are_fatal() {
    let (stream, _peer) = UnixStream::pair().expect("pair");
    let mut client = test_client(stream);
    client.cache.generation = 5;
    client.cache.rows = 2;
    client.viewport_line_ids = vec![1, 2];
    client.viewport_line_ids_generation = 5;
    let payload = seyal_runtime::local_ipc::framing::ViewportLineIds {
        generation: 5,
        line_ids: vec![3, 4],
    }
    .encode();
    assert_eq!(
        client.apply_viewport_line_ids(&payload),
        Err(ClientError::Protocol)
    );
}

#[test]
fn negotiated_type_45_projects_submit_relay_direct_and_unavailable() {
    use seyal_runtime::local_ipc::framing::{
        BlockScope, ComposerEligibility, ComposerStatus, ComposerTarget, ComposerTargetMode,
        ComposerTargetReason, InputTargetClass,
    };

    let (stream, _peer) = UnixStream::pair().expect("pair");
    let mut client = test_client(stream);
    let attachment = client.attachment_id;
    client.session_bootstrap_negotiated = true;
    client.composer_status = Some(ComposerStatus {
        attachment_id: attachment,
        eligibility: ComposerEligibility::Busy,
        revision: 3,
    });
    client.note_composer_projection();
    let (code, revision, _) = client.projected_composer_status().expect("type 23");
    assert_eq!(code, 2, "no type 45 falls back to type 23 Busy");
    assert_eq!(revision, 3);

    let mut target = ComposerTarget {
        attachment_id: attachment,
        target: InputTargetClass::TrustedShell,
        scope: BlockScope::Session,
        mode: ComposerTargetMode::Submit,
        reason: ComposerTargetReason::None,
        block_id: 0,
        target_generation: 1,
        revision: 3,
    };
    client.composer_target = Some(target);
    client.note_composer_projection();
    let (code, submit_revision, _) = client.projected_composer_status().expect("submit");
    assert_eq!(
        code, 1,
        "Submit stays available while type 23 is still Busy"
    );
    assert!(
        submit_revision > revision,
        "projection revision stays monotonic"
    );

    target.mode = ComposerTargetMode::Unavailable;
    target.reason = ComposerTargetReason::NotAtPrompt;
    target.revision = 4;
    client.composer_target = Some(target);
    client.note_composer_projection();
    let (code, unavailable_revision, _) = client.projected_composer_status().expect("unavailable");
    assert_eq!(code, 2);
    assert!(unavailable_revision > submit_revision);

    target.mode = ComposerTargetMode::Relay;
    target.reason = ComposerTargetReason::None;
    target.block_id = 7;
    target.revision = 5;
    client.composer_target = Some(target);
    client.note_composer_projection();
    let (code, relay_revision, _) = client.projected_composer_status().expect("relay");
    assert_eq!(code, 4);
    assert!(relay_revision > unavailable_revision);

    target.mode = ComposerTargetMode::DirectInput;
    target.reason = ComposerTargetReason::None;
    target.block_id = 9;
    target.revision = 6;
    client.composer_target = Some(target);
    client.note_composer_projection();
    let (code, direct_revision, _) = client.projected_composer_status().expect("direct");
    assert_eq!(code, 5, "DirectInput projects as eligibility byte 5");
    assert!(direct_revision > relay_revision);

    client.session_bootstrap_negotiated = false;
    client.composer_status = Some(ComposerStatus {
        attachment_id: attachment,
        eligibility: ComposerEligibility::Busy,
        revision: 9,
    });
    client.note_composer_projection();
    let (code, legacy_revision, _) = client.projected_composer_status().expect("legacy");
    assert_eq!(
        code, 2,
        "without negotiation, type 45 does not override type 23"
    );
    assert!(legacy_revision > direct_revision);
}

#[test]
fn purge_history_retains_presentation_correlation_block_id() {
    let (stream, _peer) = UnixStream::pair().expect("pair");
    let mut client = test_client(stream);
    let correlation = LocalDisplayClient::PRESENTATION_HISTORY_CORRELATION_BLOCK_ID;
    client.history_requests.insert(42, (correlation, 10, 50));
    client.history_requests.insert(43, (7, 1, 2));
    client.purge_history_for_blocks(&std::collections::HashSet::new());
    assert!(
        client.history_requests.contains_key(&42),
        "empty BlockTimeline must not drop Raw scrollback correlation (#1442)"
    );
    assert!(
        !client.history_requests.contains_key(&43),
        "ordinary Block history must still purge when not retained"
    );
}
