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
use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    os::unix::net::UnixStream,
    sync::Arc,
};

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
        queued_viewport_line_ids_bytes: 0,
        display_inflight: None,
        pending_display: None,
        display_generation: 1,
        attach_snapshot_pin: false,
        deferred_after_attach: None,
        presentation_slots: HashMap::new(),
        presentation_order: VecDeque::new(),
        partial_display_attachment: None,
        display_round_robin_after: None,
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
        queued_viewport_line_ids_bytes: 0,
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
            started: true,
        }),
        pending_display: None,
        display_generation: 1,
        attach_snapshot_pin: false,
        deferred_after_attach: None,
        presentation_slots: HashMap::new(),
        presentation_order: VecDeque::new(),
        partial_display_attachment: None,
        display_round_robin_after: None,
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
        queued_viewport_line_ids_bytes: 0,
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
        presentation_slots: HashMap::new(),
        presentation_order: VecDeque::new(),
        partial_display_attachment: None,
        display_round_robin_after: None,
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

#[test]
fn partial_attachment_line_ids_frame_finishes_before_display_bytes() {
    let (stream, mut peer) = UnixStream::pair().unwrap();
    stream.set_nonblocking(true).unwrap();
    peer.set_nonblocking(true).unwrap();
    let mut connection = connection();
    connection.stream = stream;

    let attachment_id = crate::AttachmentId::from_bytes([7; 16]);
    let line_ids = viewport_line_ids_frame(9, 42);
    let already_written = 11;
    connection
        .stream
        .write_all(&line_ids[..already_written])
        .unwrap();
    let mut line_ids_item = OutboundItem::new(line_ids.clone());
    line_ids_item.sent = already_written;
    let slot = AttachmentPresentationSlot::new();
    let mut slot = slot;
    slot.viewport_line_ids = Some(line_ids_item);
    slot.pending_display = Some(
        encode_snapshot(&snapshot(9))
            .unwrap()
            .into_transport_batches()
            .into(),
    );
    connection.presentation_slots.insert(attachment_id, slot);
    connection.presentation_order.push_back(attachment_id);
    connection.partial_display_attachment = Some(attachment_id);

    super::attachment_outbound::flush_attachment_outbound(&mut connection).unwrap();
    let mut first = vec![0; line_ids.len()];
    let mut first_len = 0;
    while first_len < first.len() {
        match peer.read(&mut first[first_len..]) {
            Ok(0) => break,
            Ok(count) => first_len += count,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(error) => panic!("read type-35 frame: {error}"),
        }
    }
    assert_eq!(&first[..first_len], line_ids.as_slice());

    super::attachment_outbound::flush_attachment_outbound(&mut connection).unwrap();
    let expected_display = encode_snapshot(&snapshot(9)).unwrap().frames[0].clone();
    let mut display = vec![0; expected_display.len()];
    let mut display_len = 0;
    while display_len < display.len() {
        match peer.read(&mut display[display_len..]) {
            Ok(0) => break,
            Ok(count) => display_len += count,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(error) => panic!("read display frame: {error}"),
        }
    }
    assert_eq!(&display[..display_len], expected_display.as_ref());
}

#[test]
fn retired_attachment_final_snapshot_drains_before_lifecycle_metadata() {
    let (stream, mut peer) = UnixStream::pair().unwrap();
    stream.set_nonblocking(true).unwrap();
    peer.set_nonblocking(true).unwrap();
    let mut connection = connection();
    connection.stream = stream;
    let attachment_id = crate::AttachmentId::from_bytes([8; 16]);
    let snapshot = encode_snapshot(&snapshot(10)).unwrap();
    let expected_snapshot = snapshot.frames[0].clone();
    let mut slot = AttachmentPresentationSlot::new();
    slot.display_generation = 10;
    slot.pending_display = Some(snapshot.into_transport_batches().into());
    slot.retire_after_drain = true;
    connection.presentation_slots.insert(attachment_id, slot);
    connection.presentation_order.push_back(attachment_id);
    let lifecycle = encode_frame(MessageType::Lifecycle, &[3, 4]);
    connection.queued_control_bytes = lifecycle.len();
    connection
        .after_display
        .push_back(OutboundItem::new(lifecycle.clone()));

    super::attachment_outbound::flush_attachment_outbound(&mut connection).unwrap();

    let mut output = vec![0; expected_snapshot.len() + lifecycle.len()];
    let mut read = 0;
    while read < output.len() {
        match peer.read(&mut output[read..]) {
            Ok(0) => break,
            Ok(count) => read += count,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(error) => panic!("read final drain: {error}"),
        }
    }
    assert_eq!(
        &output[..expected_snapshot.len()],
        expected_snapshot.as_ref()
    );
    assert_eq!(&output[expected_snapshot.len()..read], lifecycle.as_slice());
    assert!(!connection.presentation_slots.contains_key(&attachment_id));
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
            queued_viewport_line_ids_bytes: 0,
            display_inflight: None,
            pending_display: None,
            display_generation: 1,
            attach_snapshot_pin: false,
            deferred_after_attach: None,
            presentation_slots: HashMap::new(),
            presentation_order: VecDeque::new(),
            partial_display_attachment: None,
            display_round_robin_after: None,
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

#[test]
fn type_35_queue_does_not_consume_mandatory_control_capacity() {
    let path =
        std::env::temp_dir().join(format!("seyal-type35-budget-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let mut server = LocalIpcServer::bind(&path, 4).expect("bind");
    let _ = std::fs::remove_file(&path);
    let (stream, _peer) = UnixStream::pair().unwrap();
    let token = 8u64;
    let mut connection = connection();
    connection.stream = stream;
    server.connections.insert(token, connection);

    for index in 0..100u8 {
        server
            .enqueue_attachment_viewport_line_ids(
                token,
                crate::AttachmentId::from_bytes([index.wrapping_add(1); 16]),
                vec![0; 2_060],
            )
            .expect("optional type-35 vector is admitted within §19.5 budget");
    }
    assert_eq!(
        server
            .connections
            .get(&token)
            .unwrap()
            .queued_viewport_line_ids_bytes,
        206_000
    );
    assert_eq!(
        server.connections.get(&token).unwrap().queued_control_bytes,
        0
    );

    server
        .enqueue_mandatory(token, vec![0; MAX_OUTBOUND_QUEUE_BYTES])
        .expect("optional type-35 state must not reject mandatory output");
    assert_eq!(
        server.connections.get(&token).unwrap().queued_control_bytes,
        MAX_OUTBOUND_QUEUE_BYTES
    );
}

#[test]
fn suspend_drops_zero_byte_inflight_display() {
    let mut connection = connection();
    let batch = encode_snapshot(&snapshot(1)).unwrap();
    connection.display_inflight = Some(DisplayItem::new(batch.into_transport_batches().into()));

    connection.drop_not_yet_started_presentation();

    assert!(connection.display_inflight.is_none());
}

#[test]
fn suspend_keeps_partially_written_display_frame_for_framing() {
    let mut connection = connection();
    let batch = encode_snapshot(&snapshot(1)).unwrap();
    let mut inflight = DisplayItem::new(batch.into_transport_batches().into());
    inflight.sent = 1;
    inflight.started = true;
    connection.display_inflight = Some(inflight);

    connection.drop_not_yet_started_presentation();

    assert_eq!(
        connection.display_inflight.as_ref().map(|item| item.sent),
        Some(1),
        "a partial frame must finish before suspension takes effect"
    );
}

#[test]
fn suspend_keeps_started_batch_between_frames() {
    let mut connection = connection();
    let batch = EncodedDisplayBatch {
        kind: DisplayKind::Snapshot,
        schema: 2,
        generation: 1,
        base_generation: 0,
        rows: 1,
        columns: 1,
        frames: vec![Arc::<[u8]>::from([1u8]), Arc::<[u8]>::from([2u8])],
        total_bytes: 2,
    };
    let mut inflight = DisplayItem::new(batch.into_transport_batches().into());
    inflight.frame_index = 1;
    inflight.started = true;
    connection.display_inflight = Some(inflight);

    connection.drop_not_yet_started_presentation();

    assert_eq!(
        connection.display_inflight.as_ref().map(|item| (
            item.frame_index,
            item.sent,
            item.started
        )),
        Some((1, 0, true)),
        "once a logical batch starts, its remaining frames must finish"
    );
}

#[test]
fn suspend_clears_attach_snapshot_pin_for_later_recovery() {
    let mut connection = connection();
    let attach_snapshot = encode_snapshot(&snapshot(1)).unwrap();
    let deferred = encode_snapshot(&snapshot(2)).unwrap();
    connection.pending_display = Some(attach_snapshot.into_transport_batches().into());
    connection.attach_snapshot_pin = true;
    connection.deferred_after_attach = Some(deferred);

    connection.drop_not_yet_started_presentation();
    connection.queue_snapshot(encode_snapshot(&snapshot(3)).unwrap());

    assert!(!connection.attach_snapshot_pin);
    assert!(connection.deferred_after_attach.is_none());
    assert_eq!(
        connection
            .pending_display
            .as_ref()
            .and_then(|batches| batches.front())
            .map(|batch| batch.generation),
        Some(3),
        "resume snapshot must be made writable after suspend drops attach work"
    );
}
