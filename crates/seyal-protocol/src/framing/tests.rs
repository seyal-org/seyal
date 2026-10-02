//! Framing envelope and control-payload encode/decode tests.

use super::*;
use crate::{AttachmentId, ExecutionId};

fn exec_id() -> ExecutionId {
    ExecutionId::from_bytes(1u128.to_le_bytes())
}
fn attach_id() -> AttachmentId {
    AttachmentId::from_bytes(2u128.to_le_bytes())
}

#[test]
fn header_round_trip_and_bounds() {
    let header = FrameHeader::new(MessageType::Attach as u16, Attach::WIRE_LEN as u32);
    assert_eq!(FrameHeader::decode(&header.encode()).unwrap(), header);
    let mut oversized = header.encode();
    oversized[16..20].copy_from_slice(&(MAX_FRAME_PAYLOAD + 1).to_le_bytes());
    assert_eq!(
        FrameHeader::decode(&oversized),
        Err(FramingError::OversizedPayload)
    );
}

#[test]
fn attached_has_no_projection_descriptor_metadata() {
    let attached = Attached {
        execution_id: exec_id(),
        attachment_id: attach_id(),
        granted_role: Role::Observer,
        current_generation: 9,
    };
    let encoded = attached.encode();
    assert_eq!(encoded.len(), 48);
    assert_eq!(Attached::decode(&encoded).unwrap(), attached);
}

#[test]
fn control_payloads_round_trip() {
    let attach = Attach {
        execution_id: exec_id(),
        requested_role: Role::Controller,
    };
    assert_eq!(Attach::decode(&attach.encode()).unwrap(), attach);
    let resize = Resize {
        attachment_id: attach_id(),
        rows: 24,
        columns: 80,
    };
    assert_eq!(Resize::decode(&resize.encode()).unwrap(), resize);
    let resync = Resync {
        attachment_id: attach_id(),
    };
    assert_eq!(Resync::decode(&resync.encode()).unwrap(), resync);
}

#[test]
fn display_message_ids_replace_candidate_b_projection_messages() {
    assert_eq!(
        MessageType::from_u16(12),
        Some(MessageType::DisplaySnapshot)
    );
    assert_eq!(MessageType::from_u16(13), Some(MessageType::DisplayDelta));
    assert_eq!(
        MessageType::from_u16(27),
        Some(MessageType::DisplaySnapshotV2)
    );
    assert_eq!(MessageType::from_u16(28), Some(MessageType::DisplayDeltaV2));
    assert_eq!(MessageType::from_u16(26), None);
    assert_eq!(MessageType::from_u16(30), Some(MessageType::Paste));
    assert_eq!(MessageType::from_u16(31), Some(MessageType::HostSelection));
    assert_eq!(MessageType::from_u16(32), Some(MessageType::CopiedText));
    assert_eq!(MessageType::from_u16(33), Some(MessageType::HostSearch));
    assert_eq!(MessageType::from_u16(34), Some(MessageType::TerminalMouse));
}

#[test]
fn terminal_mouse_round_trips() {
    let event = TerminalMouse {
        attachment_id: attach_id(),
        action_id: 1,
        kind: TerminalMouseKind::Press,
        button: 0,
        modifiers: TerminalKeyV2Modifiers::SHIFT,
        col: 3,
        row: 4,
    };
    assert_eq!(TerminalMouse::decode(&event.encode()).unwrap(), event);
    let mut reserved = event.encode();
    reserved[31] = 1;
    assert_eq!(
        TerminalMouse::decode(&reserved),
        Err(FramingError::MalformedPayload)
    );
}

#[test]
fn paste_reuses_input_layout() {
    let payload = InputRef {
        attachment_id: attach_id(),
        bytes: b"paste",
    }
    .encode();
    let header = FrameHeader::new(MessageType::Paste as u16, payload.len() as u32);
    match decode_message(&header, &payload).unwrap() {
        Message::Paste(paste) => assert_eq!(paste.bytes, b"paste"),
        other => panic!("expected Paste, got {other:?}"),
    }
}

#[test]
fn host_selection_round_trip() {
    let command = HostSelection {
        attachment_id: attach_id(),
        action: HostSelectionAction::EnterCopyMode,
        kind: 0,
        start_col: 0,
        start_row: 0,
        end_col: 0,
        end_row: 0,
    };
    assert_eq!(HostSelection::decode(&command.encode()).unwrap(), command);
    assert_eq!(MessageType::from_u16(31), Some(MessageType::HostSelection));
    let mut reserved = command.encode();
    reserved[26] = 1;
    assert_eq!(
        HostSelection::decode(&reserved),
        Err(FramingError::MalformedPayload)
    );
    assert_eq!(
        HostSelectionAction::from_u8(7),
        Err(FramingError::MalformedPayload)
    );
    let search = HostSearch {
        attachment_id: attach_id(),
        forward: true,
        needle: "one",
    };
    assert_eq!(HostSearch::decode(&search.encode()).unwrap(), search);
}

#[test]
fn input_borrows_payload_and_enforces_bound() {
    let payload = InputRef {
        attachment_id: attach_id(),
        bytes: b"hello",
    }
    .encode();
    let decoded = InputRef::decode(&payload).unwrap();
    assert_eq!(decoded.bytes, b"hello");
    let mut too_large = attach_id().to_bytes().to_vec();
    too_large.extend_from_slice(&(MAX_INPUT_BYTES + 1).to_le_bytes());
    assert_eq!(
        InputRef::decode(&too_large),
        Err(FramingError::OversizedPayload)
    );
}
