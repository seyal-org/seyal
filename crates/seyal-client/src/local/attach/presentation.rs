//! Initial snapshot envelope validation and bounded resync scanning.

use std::{os::unix::net::UnixStream, time::Instant};

use seyal_runtime::local_ipc::framing::{
    decode_presentation_envelope, FrameHeader, MessageType, PresentationEnvelope, HEADER_LEN,
};
use seyal_runtime::{
    local_ipc::framing::ErrorMessage, pass8::BLOCK_STATE_MESSAGE_TYPE, AttachmentId,
};

use super::{server_error, ClientError, MAX_BUFFERED_BYTES, MAX_FRAMES_PER_POLL};

const DISPLAY_CHUNK_INDEX_OFFSET: usize = 32;
const DISPLAY_CHUNK_COUNT_OFFSET: usize = 34;
const DISPLAY_CHUNK_SEQUENCE_END: usize = 36;

use seyal_runtime::display::{decode_chunk, decode_chunk_payload, DecodedDisplayChunk};

pub(super) fn display_chunk_remainder_hint(
    frame: &[u8],
    delivery_control_negotiated: bool,
) -> Option<usize> {
    let header = FrameHeader::decode(frame).ok()?;
    let message_type = MessageType::from_u16(header.message_type)?;
    if !matches!(
        message_type,
        MessageType::DisplaySnapshot
            | MessageType::DisplayDelta
            | MessageType::DisplaySnapshotV2
            | MessageType::DisplayDeltaV2
    ) {
        return None;
    }
    let envelope_len = if delivery_control_negotiated {
        PresentationEnvelope::PREFIX_LEN
    } else {
        0
    };
    let payload = frame.get(HEADER_LEN + envelope_len..)?;
    let chunk_index = u16::from_le_bytes(
        payload
            .get(DISPLAY_CHUNK_INDEX_OFFSET..DISPLAY_CHUNK_COUNT_OFFSET)?
            .try_into()
            .ok()?,
    );
    let chunk_count = u16::from_le_bytes(
        payload
            .get(DISPLAY_CHUNK_COUNT_OFFSET..DISPLAY_CHUNK_SEQUENCE_END)?
            .try_into()
            .ok()?,
    );
    (chunk_count > chunk_index).then(|| usize::from(chunk_count - chunk_index - 1))
}

pub(super) enum ResyncScanFrame {
    SnapshotStart,
    DisplayRemainder,
    DeferredControl,
}

fn validate_initial_presentation_envelope(
    frame: &[u8],
    attachment_id: AttachmentId,
    delivery_control_negotiated: bool,
) -> Result<usize, ClientError> {
    if !delivery_control_negotiated {
        return Ok(0);
    }
    let envelope = decode_presentation_envelope(frame).map_err(|_| ClientError::Protocol)?;
    if envelope.attachment_id != attachment_id || envelope.resume_sequence != 0 {
        return Err(ClientError::Protocol);
    }
    Ok(PresentationEnvelope::PREFIX_LEN)
}

pub(super) fn decode_initial_display_chunk(
    frame: &[u8],
    attachment_id: AttachmentId,
    delivery_control_negotiated: bool,
) -> Result<DecodedDisplayChunk, ClientError> {
    let header = FrameHeader::decode(frame).map_err(|_| ClientError::Protocol)?;
    let envelope_len =
        validate_initial_presentation_envelope(frame, attachment_id, delivery_control_negotiated)?;
    if envelope_len == 0 {
        decode_chunk(frame).map_err(|_| ClientError::Display)
    } else {
        decode_chunk_payload(header.message_type, &frame[HEADER_LEN + envelope_len..])
            .map_err(|_| ClientError::Display)
    }
}

pub(super) fn classify_resync_scan_frame(
    frame: &[u8],
    attachment_id: AttachmentId,
    delivery_control_negotiated: bool,
) -> Result<ResyncScanFrame, ClientError> {
    let header = FrameHeader::decode(frame).map_err(|_| ClientError::Protocol)?;
    if header.message_type == BLOCK_STATE_MESSAGE_TYPE {
        validate_initial_presentation_envelope(frame, attachment_id, delivery_control_negotiated)?;
        return Ok(ResyncScanFrame::DeferredControl);
    }
    let message_type = MessageType::from_u16(header.message_type).ok_or(ClientError::Protocol)?;
    if message_type == MessageType::Error {
        let error = ErrorMessage::decode(frame.get(HEADER_LEN..).ok_or(ClientError::Protocol)?)
            .map_err(|_| ClientError::Protocol)?;
        return Err(server_error(error.error_code));
    }
    match message_type {
        MessageType::DisplaySnapshot | MessageType::DisplaySnapshotV2 => {
            let envelope_len = validate_initial_presentation_envelope(
                frame,
                attachment_id,
                delivery_control_negotiated,
            )?;
            let payload = frame
                .get(HEADER_LEN + envelope_len..)
                .ok_or(ClientError::Protocol)?;
            let Some(index_bytes) =
                payload.get(DISPLAY_CHUNK_INDEX_OFFSET..DISPLAY_CHUNK_COUNT_OFFSET)
            else {
                return Ok(ResyncScanFrame::DisplayRemainder);
            };
            if u16::from_le_bytes(index_bytes.try_into().map_err(|_| ClientError::Protocol)?) == 0 {
                Ok(ResyncScanFrame::SnapshotStart)
            } else {
                Ok(ResyncScanFrame::DisplayRemainder)
            }
        }
        MessageType::DisplayDelta | MessageType::DisplayDeltaV2 => {
            validate_initial_presentation_envelope(
                frame,
                attachment_id,
                delivery_control_negotiated,
            )?;
            Ok(ResyncScanFrame::DisplayRemainder)
        }
        MessageType::BlockTimeline
        | MessageType::Lifecycle
        | MessageType::CopiedText
        | MessageType::ViewportLineIds => {
            if matches!(
                message_type,
                MessageType::BlockTimeline | MessageType::ViewportLineIds
            ) {
                validate_initial_presentation_envelope(
                    frame,
                    attachment_id,
                    delivery_control_negotiated,
                )?;
            }
            Ok(ResyncScanFrame::DeferredControl)
        }
        _ => Err(ClientError::Protocol),
    }
}

pub(super) fn read_resync_snapshot_start_until(
    stream: &mut UnixStream,
    deadline: Instant,
    stale_remainder_hint: Option<usize>,
    deferred_control: &mut Vec<u8>,
    attachment_id: AttachmentId,
    delivery_control_negotiated: bool,
) -> Result<Vec<u8>, ClientError> {
    // A decoded header bounds the exact old logical remainder plus the next
    // authoritative boundary. If corruption prevents that hint, use the same
    // finite frame-work bound as a normal poll. Every read also shares the
    // caller's original absolute startup deadline.
    let scan_limit = stale_remainder_hint
        .map(|remaining| {
            remaining
                .saturating_add(MAX_FRAMES_PER_POLL)
                .saturating_add(1)
        })
        .unwrap_or(MAX_FRAMES_PER_POLL);
    let mut display_remainder = stale_remainder_hint;
    let mut deferred_frames = 0usize;
    for _ in 0..scan_limit {
        let frame = super::read_blocking_raw_frame_until(stream, deadline)?;
        match classify_resync_scan_frame(&frame, attachment_id, delivery_control_negotiated)? {
            ResyncScanFrame::SnapshotStart => return Ok(frame),
            ResyncScanFrame::DisplayRemainder => {
                if let Some(remaining) = display_remainder.as_mut() {
                    if *remaining == 0 {
                        return Err(ClientError::Protocol);
                    }
                    *remaining -= 1;
                }
            }
            ResyncScanFrame::DeferredControl => {
                deferred_frames = deferred_frames
                    .checked_add(1)
                    .ok_or(ClientError::Capacity)?;
                if deferred_frames > MAX_FRAMES_PER_POLL {
                    return Err(ClientError::Capacity);
                }
                let retained_len = deferred_control
                    .len()
                    .checked_add(frame.len())
                    .ok_or(ClientError::Capacity)?;
                if retained_len > MAX_BUFFERED_BYTES {
                    return Err(ClientError::Capacity);
                }
                deferred_control.extend_from_slice(&frame);
            }
        }
    }
    Err(ClientError::Protocol)
}
