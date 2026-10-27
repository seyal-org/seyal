//! Attachment delivery controls and identity envelopes for negotiated presentation frames.

use crate::AttachmentId;

use super::envelope::{
    attachment_id_from, exact_len, FrameHeader, FramingError, HEADER_LEN, MAX_FRAME_PAYLOAD,
};

/// SPEC-004 §19 types 40/41: 24-byte attachment delivery control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SuspendDelivery {
    pub attachment_id: AttachmentId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResumeDelivery {
    pub attachment_id: AttachmentId,
    pub resume_sequence: u64,
}

impl SuspendDelivery {
    pub const WIRE_LEN: usize = 24;

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        out.extend_from_slice(&self.attachment_id.to_bytes());
        out.extend_from_slice(&[0; 8]);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        if bytes[16..24].iter().any(|byte| *byte != 0) {
            return Err(FramingError::MalformedPayload);
        }
        Ok(Self {
            attachment_id: attachment_id_from(&bytes[..16]),
        })
    }
}

impl ResumeDelivery {
    pub const WIRE_LEN: usize = 24;

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        out.extend_from_slice(&self.attachment_id.to_bytes());
        out.extend_from_slice(&self.resume_sequence.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        let resume_sequence = u64::from_le_bytes(bytes[16..24].try_into().unwrap());
        if resume_sequence == 0 {
            return Err(FramingError::MalformedPayload);
        }
        Ok(Self {
            attachment_id: attachment_id_from(&bytes[..16]),
            resume_sequence,
        })
    }
}

/// Fixed 24-byte identity/epoch prefix for negotiated presentation frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresentationEnvelope<'a> {
    pub attachment_id: AttachmentId,
    pub resume_sequence: u64,
    pub payload: &'a [u8],
}

impl PresentationEnvelope<'_> {
    pub const PREFIX_LEN: usize = 24;
}

/// Prefix one complete Runtime→client presentation frame without changing
/// its message type or inner payload. Legacy connections must not call this.
pub fn envelope_presentation_frame(
    frame: &[u8],
    attachment_id: AttachmentId,
    resume_sequence: u64,
) -> Result<Vec<u8>, FramingError> {
    if frame.len() < HEADER_LEN {
        return Err(FramingError::TruncatedHeader);
    }
    let header = FrameHeader::decode(&frame[..HEADER_LEN])?;
    let payload_len =
        usize::try_from(header.payload_len).map_err(|_| FramingError::LengthOverflow)?;
    if frame.len() != HEADER_LEN + payload_len {
        return Err(FramingError::ExactLengthMismatch);
    }
    if !is_presentation_message(header.message_type) {
        return Err(FramingError::MalformedPayload);
    }
    let new_payload_len = payload_len
        .checked_add(PresentationEnvelope::PREFIX_LEN)
        .ok_or(FramingError::LengthOverflow)?;
    if new_payload_len > MAX_FRAME_PAYLOAD as usize {
        return Err(FramingError::OversizedPayload);
    }
    let mut output = Vec::with_capacity(HEADER_LEN + new_payload_len);
    output
        .extend_from_slice(&FrameHeader::new(header.message_type, new_payload_len as u32).encode());
    output.extend_from_slice(&attachment_id.to_bytes());
    output.extend_from_slice(&resume_sequence.to_le_bytes());
    output.extend_from_slice(&frame[HEADER_LEN..]);
    Ok(output)
}

/// Decode the envelope from a complete negotiated Runtime→client frame.
pub fn decode_presentation_envelope(
    frame: &[u8],
) -> Result<PresentationEnvelope<'_>, FramingError> {
    if frame.len() < HEADER_LEN {
        return Err(FramingError::TruncatedHeader);
    }
    let header = FrameHeader::decode(&frame[..HEADER_LEN])?;
    let payload_len =
        usize::try_from(header.payload_len).map_err(|_| FramingError::LengthOverflow)?;
    if frame.len() != HEADER_LEN + payload_len {
        return Err(FramingError::ExactLengthMismatch);
    }
    if !is_presentation_message(header.message_type) {
        return Err(FramingError::MalformedPayload);
    }
    if payload_len < PresentationEnvelope::PREFIX_LEN {
        return Err(FramingError::TruncatedPayload);
    }
    let payload = &frame[HEADER_LEN..];
    Ok(PresentationEnvelope {
        attachment_id: attachment_id_from(&payload[..16]),
        resume_sequence: u64::from_le_bytes(payload[16..24].try_into().unwrap()),
        payload: &payload[PresentationEnvelope::PREFIX_LEN..],
    })
}

fn is_presentation_message(message_type: u16) -> bool {
    matches!(message_type, 12 | 13 | 21 | 26 | 27 | 28 | 35)
}
