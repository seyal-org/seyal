//! Frame-boundary regressions for attachment-scoped metadata and display work.

use super::*;
use crate::local_ipc::framing::{encode_frame, FrameHeader, MessageType};
use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    os::unix::net::UnixStream,
    sync::Arc,
};

fn connection(stream: UnixStream) -> Connection {
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
        partial_after_display_attachment: None,
        display_round_robin_after: None,
    }
}

fn composer_status_frame(marker: u8) -> Vec<u8> {
    let mut frame = FrameHeader::new(MessageType::ComposerStatus as u16, 1)
        .encode()
        .to_vec();
    frame.push(marker);
    frame
}

fn assert_framed_stream(bytes: &[u8], expected_types: &[u16]) {
    let mut offset = 0;
    let mut types = Vec::new();
    while offset < bytes.len() {
        let header = FrameHeader::decode(&bytes[offset..]).expect("complete frame header");
        let frame_len = 24 + header.payload_len as usize;
        assert!(offset + frame_len <= bytes.len(), "truncated frame");
        types.push(header.message_type);
        offset += frame_len;
    }
    assert_eq!(types, expected_types);
}

fn partial_attachment_metadata_connection() -> (
    Connection,
    UnixStream,
    Vec<u8>,
    Vec<u8>,
    crate::AttachmentId,
) {
    let (mut stream, peer) = UnixStream::pair().unwrap();
    let attachment_id = crate::AttachmentId::from_bytes([12; 16]);
    let metadata = composer_status_frame(0x42);
    let prefix_len = 1;
    stream.write_all(&metadata[..prefix_len]).unwrap();
    let display_frame = encode_frame(MessageType::ComposerStatus, &[0x99]);
    let snapshot = EncodedDisplayBatch {
        kind: DisplayKind::Snapshot,
        schema: 2,
        generation: 2,
        base_generation: 0,
        rows: 1,
        columns: 1,
        total_bytes: display_frame.len(),
        frames: vec![Arc::<[u8]>::from(display_frame.clone())],
    };
    let mut slot = AttachmentPresentationSlot::new();
    slot.after_display.push_back(OutboundItem {
        bytes: metadata.clone(),
        sent: prefix_len,
    });
    slot.pending_display = Some(VecDeque::from([snapshot]));
    slot.display_generation = 2;
    let mut connection = connection(stream);
    connection.presentation_slots.insert(attachment_id, slot);
    connection.presentation_order.push_back(attachment_id);
    connection.partial_after_display_attachment = Some(attachment_id);
    connection.queued_control_bytes = metadata.len() - prefix_len;
    (connection, peer, metadata, display_frame, attachment_id)
}

#[test]
fn partial_attachment_metadata_finishes_before_new_snapshot() {
    let (mut connection, mut peer, metadata, display_frame, attachment_id) =
        partial_attachment_metadata_connection();
    let mut prefix = [0];
    peer.read_exact(&mut prefix).unwrap();

    super::attachment_outbound::flush_attachment_outbound(&mut connection).unwrap();

    let mut output = vec![0; metadata.len() - 1 + display_frame.len()];
    peer.read_exact(&mut output).unwrap();
    let mut stream = prefix.to_vec();
    stream.extend(output);
    assert_framed_stream(
        &stream,
        &[
            MessageType::ComposerStatus as u16,
            FrameHeader::decode(&display_frame).unwrap().message_type,
        ],
    );
    assert!(!connection
        .presentation_slots
        .get(&attachment_id)
        .is_some_and(|slot| slot.has_display()));
}

#[test]
fn partial_attachment_metadata_finishes_before_detach_removes_slot() {
    let (mut connection, mut peer, metadata, _display_frame, attachment_id) =
        partial_attachment_metadata_connection();
    let mut prefix = [0];
    peer.read_exact(&mut prefix).unwrap();
    connection.remove_attachment_presentation(attachment_id);

    super::attachment_outbound::flush_attachment_outbound(&mut connection).unwrap();

    let mut output = vec![0; metadata.len() - 1];
    peer.read_exact(&mut output).unwrap();
    let mut stream = prefix.to_vec();
    stream.extend(output);
    assert_framed_stream(&stream, &[MessageType::ComposerStatus as u16]);
    assert!(connection.presentation_slots.is_empty());
}
