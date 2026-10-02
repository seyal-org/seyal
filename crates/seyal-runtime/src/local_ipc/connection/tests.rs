//! Unit coverage for Candidate-D outbound queue and flush ordering.

use super::*;
use crate::display::MAX_DISPLAY_BATCH_BYTES;
use crate::display::{encode_delta, encode_snapshot, DisplayKind, EncodedDisplayBatch};
use crate::local_ipc::framing::{encode_frame, FrameHeader, MessageType, ViewportLineIds};
use seyal_exec::{
    ProjectionAttributes, ProjectionCell, ProjectionDamage, TerminalProjectionSnapshot,
    TerminalProjectionUpdate,
};
use seyal_protocol::pass8::{
    encode_block_state_frame, BlockKind, BlockLifecycle, BlockState, BLOCK_STATE_MESSAGE_TYPE,
};
use seyal_protocol::{BlockId, ExecutionId};
use std::{collections::VecDeque, io::Read, os::unix::net::UnixStream, sync::Arc};

fn sample_cells(count: usize) -> Vec<ProjectionCell> {
    vec![ProjectionCell::lead_scalar('x', ProjectionAttributes::default()); count]
}

fn snapshot(generation: u64) -> TerminalProjectionSnapshot {
    TerminalProjectionSnapshot {
        rows: 2,
        columns: 2,
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: true,
        alternate_screen: false,
        source_damage_generation: generation,
        damage: ProjectionDamage::full(2),
        cells: sample_cells(4),
    }
}

fn update(generation: u64) -> TerminalProjectionUpdate {
    TerminalProjectionUpdate {
        rows: 2,
        columns: 2,
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: true,
        alternate_screen: false,
        source_damage_generation: generation,
        damage: ProjectionDamage {
            full: false,
            first_row: 0,
            last_row: 0,
        },
        cells: sample_cells(2),
    }
}

fn connection() -> Connection {
    Connection {
        stream: UnixStream::pair().unwrap().0,
        state: ConnectionState::Attached,
        read_buf: Vec::new(),
        mandatory: VecDeque::new(),
        after_display: VecDeque::new(),
        queued_control_bytes: 0,
        display_inflight: None,
        pending_display: None,
        display_generation: 1,
        attach_snapshot_pin: false,
        deferred_after_attach: None,
    }
}

#[test]
fn pending_slot_prevents_unbounded_display_history() {
    let mut connection = connection();
    assert_eq!(
        connection.try_queue_delta(encode_delta(&update(2), 1).unwrap()),
        DeltaEnqueueResult::Queued
    );
    assert_eq!(
        connection.try_queue_delta(encode_delta(&update(3), 2).unwrap()),
        DeltaEnqueueResult::NeedSnapshot
    );
    assert_eq!(
        connection
            .pending_display
            .as_ref()
            .and_then(|batches| batches.front())
            .unwrap()
            .generation,
        2
    );
}

#[test]
fn snapshot_supersedes_not_started_pending_state() {
    let mut connection = connection();
    connection.queue_snapshot(encode_snapshot(&snapshot(7)).unwrap());
    connection.queue_snapshot(encode_snapshot(&snapshot(9)).unwrap());
    assert_eq!(connection.display_generation, 9);
    assert!(connection.has_snapshot_delivery());
}

#[test]
fn attach_pin_defers_fanout_supersession_until_attach_snapshot_flushes() {
    let mut connection = connection();
    connection.queue_snapshot(encode_snapshot(&snapshot(136)).unwrap());
    connection.attach_snapshot_pin = true;
    connection.queue_snapshot(encode_snapshot(&snapshot(201)).unwrap());
    assert_eq!(
        connection
            .pending_display
            .as_ref()
            .and_then(|batches| batches.front())
            .unwrap()
            .generation,
        136,
        "fanout must not replace the attach snapshot while Attached gen is pinned"
    );
    assert_eq!(connection.display_generation, 136);
    assert_eq!(
        connection
            .deferred_after_attach
            .as_ref()
            .map(|batch| batch.generation),
        Some(201)
    );

    // Simulate attach snapshot moving to inflight (delivery started).
    let batches = connection.pending_display.take().unwrap();
    connection.display_inflight = Some(crate::local_ipc::connection::DisplayItem::new(batches));
    connection.clear_attach_pin_starting_inflight();
    assert!(!connection.attach_snapshot_pin);
    assert_eq!(
        connection
            .pending_display
            .as_ref()
            .and_then(|batches| batches.front())
            .unwrap()
            .generation,
        201,
        "deferred fanout applies once attach snapshot is inflight"
    );
    assert!(connection.deferred_after_attach.is_none());
}

#[test]
fn oversized_logical_snapshot_is_queued_as_bounded_transport_fragments() {
    let first = Arc::<[u8]>::from(vec![0u8; MAX_DISPLAY_BATCH_BYTES - 64]);
    let second = Arc::<[u8]>::from(vec![1u8; 128]);
    let third = Arc::<[u8]>::from(vec![2u8; MAX_DISPLAY_BATCH_BYTES - 128]);
    let batch = EncodedDisplayBatch {
        kind: DisplayKind::Snapshot,
        schema: 2,
        generation: 9,
        base_generation: 0,
        rows: 1,
        columns: 1,
        frames: vec![first, second, third],
        total_bytes: MAX_DISPLAY_BATCH_BYTES * 2,
    };

    let mut connection = connection();
    connection.queue_snapshot(batch);
    let fragments = connection.pending_display.as_ref().unwrap();
    assert_eq!(fragments.len(), 2);
    assert!(fragments
        .iter()
        .all(|fragment| fragment.total_bytes <= MAX_DISPLAY_BATCH_BYTES));
}

#[test]
fn mandatory_frame_waits_for_a_partially_written_display_frame() {
    let (stream, mut peer) = UnixStream::pair().unwrap();
    let display_frame = Arc::<[u8]>::from(vec![1u8, 2, 3, 4]);
    let mut connection = Connection {
        stream,
        state: ConnectionState::Attached,
        read_buf: Vec::new(),
        mandatory: VecDeque::from([OutboundItem::new(vec![9u8, 10])]),
        after_display: VecDeque::from([OutboundItem::new(vec![11u8, 12])]),
        queued_control_bytes: 4,
        display_inflight: Some(DisplayItem {
            #[cfg(feature = "benchmark-instrumentation")]
            kind: DisplayKind::Snapshot,
            batches: VecDeque::from([EncodedDisplayBatch {
                kind: DisplayKind::Snapshot,
                schema: 2,
                generation: 1,
                base_generation: 0,
                rows: 1,
                columns: 1,
                frames: vec![display_frame],
                total_bytes: 4,
            }]),
            frame_index: 0,
            sent: 1,
        }),
        pending_display: None,
        display_generation: 1,
        attach_snapshot_pin: false,
        deferred_after_attach: None,
    };

    outbound::flush_outbound(&mut connection).unwrap();
    let mut first = [0u8; 3];
    peer.read_exact(&mut first).unwrap();
    assert_eq!(first, [2, 3, 4]);

    outbound::flush_outbound(&mut connection).unwrap();
    let mut second = [0u8; 2];
    peer.read_exact(&mut second).unwrap();
    assert_eq!(second, [9, 10]);

    outbound::flush_outbound(&mut connection).unwrap();
    let mut third = [0u8; 2];
    peer.read_exact(&mut third).unwrap();
    assert_eq!(third, [11, 12]);
}

#[test]
fn partial_after_display_frame_finishes_before_mandatory_and_display_work() {
    let (stream, mut peer) = UnixStream::pair().unwrap();
    let mut connection = Connection {
        stream,
        state: ConnectionState::Attached,
        read_buf: Vec::new(),
        mandatory: VecDeque::from([OutboundItem::new(vec![9u8, 10])]),
        after_display: VecDeque::from([OutboundItem {
            bytes: vec![1u8, 2, 3, 4],
            sent: 1,
        }]),
        queued_control_bytes: 5,
        display_inflight: None,
        pending_display: Some(VecDeque::from([EncodedDisplayBatch {
            kind: DisplayKind::Snapshot,
            schema: 2,
            generation: 1,
            base_generation: 0,
            rows: 1,
            columns: 1,
            frames: vec![Arc::<[u8]>::from(vec![5u8, 6])],
            total_bytes: 2,
        }])),
        display_generation: 1,
        attach_snapshot_pin: false,
        deferred_after_attach: None,
    };

    outbound::flush_outbound(&mut connection).unwrap();
    let mut first = [0u8; 3];
    peer.read_exact(&mut first).unwrap();
    assert_eq!(first, [2, 3, 4]);
    let mut second = [0u8; 2];
    peer.read_exact(&mut second).unwrap();
    assert_eq!(second, [9, 10]);
    let mut third = [0u8; 2];
    peer.read_exact(&mut third).unwrap();
    assert_eq!(third, [5, 6]);

    outbound::flush_outbound(&mut connection).unwrap();
    assert_eq!(connection.queued_control_bytes, 0);
}

fn viewport_line_ids_frame(generation: u64, line_id: u64) -> Vec<u8> {
    encode_frame(
        MessageType::ViewportLineIds,
        &ViewportLineIds {
            generation,
            line_ids: vec![line_id],
        }
        .encode(),
    )
}

fn composer_status_frame(marker: u8) -> Vec<u8> {
    // Minimal well-framed ComposerStatus-typed bytes; coalesce only peeks the
    // message type, so the payload need not decode as ComposerStatus.
    let mut frame = FrameHeader::new(MessageType::ComposerStatus as u16, 1)
        .encode()
        .to_vec();
    frame.push(marker);
    frame
}

#[test]
fn viewport_line_ids_coalesce_preserves_block_state_and_composer_status() {
    let path = std::env::temp_dir().join(format!(
        "seyal-after-display-coalesce-{}.sock",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let mut server = LocalIpcServer::bind(&path, 4).expect("bind");
    let _ = std::fs::remove_file(&path);

    let token = 7u64;
    let (stream, _peer) = UnixStream::pair().unwrap();
    server.connections.insert(
        token,
        Connection {
            stream,
            state: ConnectionState::Attached,
            read_buf: Vec::new(),
            mandatory: VecDeque::new(),
            after_display: VecDeque::new(),
            queued_control_bytes: 0,
            display_inflight: None,
            pending_display: None,
            display_generation: 1,
            attach_snapshot_pin: false,
            deferred_after_attach: None,
        },
    );

    let block = encode_block_state_frame(&BlockState {
        execution_id: ExecutionId::from_bytes([1; 16]),
        block_id: BlockId::from_bytes([2; 16]),
        revision: 1,
        start_line_id: 3,
        kind: BlockKind::TerminalActivity,
        state: BlockLifecycle::Current,
    })
    .expect("BlockState frame");
    let stale_line_ids = viewport_line_ids_frame(1, 10);
    let status = composer_status_frame(0xAB);
    let fresh_line_ids = viewport_line_ids_frame(2, 20);

    server
        .enqueue_after_display(token, block.clone())
        .expect("BlockState");
    server
        .enqueue_after_display(token, stale_line_ids)
        .expect("stale ViewportLineIds");
    server
        .enqueue_after_display(token, status.clone())
        .expect("ComposerStatus");
    server
        .enqueue_after_display(token, fresh_line_ids.clone())
        .expect("fresh ViewportLineIds");

    let queued = &server.connections.get(&token).unwrap().after_display;
    assert_eq!(queued.len(), 3, "stale type 35 must drop; others stay");
    assert_eq!(
        FrameHeader::decode(&queued[0].bytes).unwrap().message_type,
        BLOCK_STATE_MESSAGE_TYPE
    );
    assert_eq!(queued[0].bytes, block);
    assert_eq!(
        FrameHeader::decode(&queued[1].bytes).unwrap().message_type,
        MessageType::ComposerStatus as u16
    );
    assert_eq!(queued[1].bytes, status);
    assert_eq!(queued[2].bytes, fresh_line_ids);
}
