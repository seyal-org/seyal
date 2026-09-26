//! SPEC-004 Candidate-D binary wire envelope (header, bounds, framing errors).
//!
//! The 24-byte envelope is shared by control and presentation messages. Rust
//! memory layout is never wire format and all client-controlled lengths are
//! validated before allocation/use.

use crate::{AttachmentId, ExecutionId};

pub const MAGIC: [u8; 8] = *b"SEYALIPC";
pub const HEADER_LEN: usize = 24;
pub const MAJOR: u16 = 1;
pub const MINOR: u16 = 0;
pub const MAX_FRAME_PAYLOAD: u32 = 262_144;
pub const MAX_INPUT_BYTES: u32 = 65_536;
pub const MAX_EXECUTION_LIST_ENTRIES: u16 = 512;
pub const CAP_BINARY_DISPLAY: u32 = 1 << 0;
pub const CAP_OBSERVER: u32 = 1 << 1;
/// The peer can submit trusted composer commands and receive bounded command
/// Block metadata. This capability never changes raw terminal input semantics.
pub const CAP_COMMAND_BLOCKS: u32 = 1 << 4;
/// Peer accepts Candidate-D grapheme display v2 (types 27/28, schema 2).
pub const CAP_GRAPHEME_DISPLAY: u32 = 1 << 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum ErrorCode {
    UnsupportedVersion = 1,
    UnknownMessage = 2,
    InvalidState = 3,
    InvalidExecution = 4,
    InvalidAttachment = 5,
    StaleIdentity = 6,
    PermissionDenied = 7,
    ControllerBusy = 8,
    CapacityExceeded = 9,
    Backpressure = 10,
    InvalidGeometry = 11,
    DisplayUnavailable = 12,
    MalformedPayload = 13,
    InternalFailure = 14,
}

impl ErrorCode {
    pub fn from_u16(value: u16) -> Option<Self> {
        Some(match value {
            1 => Self::UnsupportedVersion,
            2 => Self::UnknownMessage,
            3 => Self::InvalidState,
            4 => Self::InvalidExecution,
            5 => Self::InvalidAttachment,
            6 => Self::StaleIdentity,
            7 => Self::PermissionDenied,
            8 => Self::ControllerBusy,
            9 => Self::CapacityExceeded,
            10 => Self::Backpressure,
            11 => Self::InvalidGeometry,
            12 => Self::DisplayUnavailable,
            13 => Self::MalformedPayload,
            14 => Self::InternalFailure,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FramingError {
    TruncatedHeader,
    InvalidMagic,
    NonzeroReserved,
    OversizedPayload,
    LengthOverflow,
    UnsupportedMajorVersion,
    UnsupportedMinorVersion,
    UnknownMessageType,
    TruncatedPayload,
    ExactLengthMismatch,
    MalformedPayload,
}

impl FramingError {
    pub fn is_fatal(self) -> bool {
        !matches!(self, Self::UnknownMessageType)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameHeader {
    pub message_type: u16,
    pub flags: u16,
    pub payload_len: u32,
}

impl FrameHeader {
    pub fn new(message_type: u16, payload_len: u32) -> Self {
        Self {
            message_type,
            flags: 0,
            payload_len,
        }
    }

    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let mut out = [0u8; HEADER_LEN];
        out[0..8].copy_from_slice(&MAGIC);
        out[8..10].copy_from_slice(&MAJOR.to_le_bytes());
        out[10..12].copy_from_slice(&MINOR.to_le_bytes());
        out[12..14].copy_from_slice(&self.message_type.to_le_bytes());
        out[14..16].copy_from_slice(&self.flags.to_le_bytes());
        out[16..20].copy_from_slice(&self.payload_len.to_le_bytes());
        out[20..24].copy_from_slice(&0u32.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        if bytes.len() < HEADER_LEN {
            return Err(FramingError::TruncatedHeader);
        }
        if bytes[0..8] != MAGIC {
            return Err(FramingError::InvalidMagic);
        }
        let major = u16::from_le_bytes(bytes[8..10].try_into().unwrap());
        let minor = u16::from_le_bytes(bytes[10..12].try_into().unwrap());
        let message_type = u16::from_le_bytes(bytes[12..14].try_into().unwrap());
        let flags = u16::from_le_bytes(bytes[14..16].try_into().unwrap());
        let payload_len = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
        let reserved = u32::from_le_bytes(bytes[20..24].try_into().unwrap());
        if reserved != 0 {
            return Err(FramingError::NonzeroReserved);
        }
        if major != MAJOR {
            return Err(FramingError::UnsupportedMajorVersion);
        }
        if minor != MINOR {
            return Err(FramingError::UnsupportedMinorVersion);
        }
        if flags != 0 {
            return Err(FramingError::MalformedPayload);
        }
        if payload_len > MAX_FRAME_PAYLOAD {
            return Err(FramingError::OversizedPayload);
        }
        HEADER_LEN
            .checked_add(payload_len as usize)
            .ok_or(FramingError::LengthOverflow)?;
        Ok(Self {
            message_type,
            flags,
            payload_len,
        })
    }
}

pub(in crate::framing) fn read_u128(bytes: &[u8]) -> u128 {
    let mut raw = [0u8; 16];
    raw.copy_from_slice(&bytes[..16]);
    u128::from_le_bytes(raw)
}

pub(in crate::framing) fn write_u128(out: &mut Vec<u8>, value: u128) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(in crate::framing) fn execution_id_from(bytes: &[u8]) -> ExecutionId {
    ExecutionId::from_bytes(read_u128(bytes).to_le_bytes())
}

pub(in crate::framing) fn attachment_id_from(bytes: &[u8]) -> AttachmentId {
    AttachmentId::from_bytes(read_u128(bytes).to_le_bytes())
}

pub(in crate::framing) fn exact_len(bytes: &[u8], expected: usize) -> Result<(), FramingError> {
    if bytes.len() != expected {
        return Err(FramingError::ExactLengthMismatch);
    }
    Ok(())
}
