//! Decode display chunk payloads after the outer frame has been validated.

use crate::framing::MessageType;

use super::{
    decode_payload_v1, decode_payload_v2, DecodedDisplayChunk, DisplayError, DisplayKind,
    DISPLAY_SCHEMA_V2,
};

/// Decode the inner display payload when a negotiated presentation envelope
/// has already been validated and stripped by the attachment boundary.
pub fn decode_chunk_payload(
    message_type: u16,
    payload: &[u8],
) -> Result<DecodedDisplayChunk, DisplayError> {
    let message_type = MessageType::from_u16(message_type).ok_or(DisplayError::WrongMessageType)?;
    let (kind, schema) = DisplayKind::from_message_type(message_type)?;
    if schema == DISPLAY_SCHEMA_V2 {
        decode_payload_v2(kind, payload)
    } else {
        decode_payload_v1(kind, payload)
    }
}
