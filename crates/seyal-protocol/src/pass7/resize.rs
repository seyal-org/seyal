//! Pass 7 correlated resize request/result wire payloads.

use crate::{
    display::{MAX_DISPLAY_COLUMNS, MAX_DISPLAY_ROWS},
    framing::{ErrorCode, FramingError},
    AttachmentId,
};

use super::{attachment_id_from, exact_len};

pub const CAP_CORRELATED_RESIZE: u32 = 1 << 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResizeRequest {
    pub attachment_id: AttachmentId,
    pub request_id: u64,
    pub rows: u16,
    pub columns: u16,
}

impl ResizeRequest {
    pub const WIRE_LEN: usize = 32;

    pub fn encode(&self) -> Vec<u8> {
        debug_assert!(self.validate().is_ok());
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        out.extend_from_slice(&self.attachment_id.to_bytes());
        out.extend_from_slice(&self.request_id.to_le_bytes());
        out.extend_from_slice(&self.rows.to_le_bytes());
        out.extend_from_slice(&self.columns.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        if u32::from_le_bytes(bytes[28..32].try_into().unwrap()) != 0 {
            return Err(FramingError::MalformedPayload);
        }
        let value = Self {
            attachment_id: attachment_id_from(&bytes[..16]),
            request_id: u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
            rows: u16::from_le_bytes(bytes[24..26].try_into().unwrap()),
            columns: u16::from_le_bytes(bytes[26..28].try_into().unwrap()),
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), FramingError> {
        if self.request_id == 0
            || self.rows == 0
            || self.columns == 0
            || self.rows > MAX_DISPLAY_ROWS
            || self.columns > MAX_DISPLAY_COLUMNS
        {
            return Err(FramingError::MalformedPayload);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeResultCode {
    Applied,
    Error(ErrorCode),
}

impl ResizeResultCode {
    pub const fn wire_value(self) -> u16 {
        match self {
            Self::Applied => 0,
            Self::Error(error) => error as u16,
        }
    }

    fn from_u16(value: u16) -> Result<Self, FramingError> {
        if value == 0 {
            return Ok(Self::Applied);
        }
        ErrorCode::from_u16(value)
            .map(Self::Error)
            .ok_or(FramingError::MalformedPayload)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResizeResult {
    pub attachment_id: AttachmentId,
    pub request_id: u64,
    pub result_code: ResizeResultCode,
    pub detail_code: u32,
    pub applied_generation: u64,
}

impl ResizeResult {
    pub const WIRE_LEN: usize = 40;

    pub fn encode(&self) -> Vec<u8> {
        debug_assert!(self.validate().is_ok());
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        out.extend_from_slice(&self.attachment_id.to_bytes());
        out.extend_from_slice(&self.request_id.to_le_bytes());
        out.extend_from_slice(&self.result_code.wire_value().to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&self.detail_code.to_le_bytes());
        out.extend_from_slice(&self.applied_generation.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        if u16::from_le_bytes(bytes[26..28].try_into().unwrap()) != 0 {
            return Err(FramingError::MalformedPayload);
        }
        let value = Self {
            attachment_id: attachment_id_from(&bytes[..16]),
            request_id: u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
            result_code: ResizeResultCode::from_u16(u16::from_le_bytes(
                bytes[24..26].try_into().unwrap(),
            ))?,
            detail_code: u32::from_le_bytes(bytes[28..32].try_into().unwrap()),
            applied_generation: u64::from_le_bytes(bytes[32..40].try_into().unwrap()),
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), FramingError> {
        if self.request_id == 0 || self.detail_code != 0 {
            return Err(FramingError::MalformedPayload);
        }
        match self.result_code {
            ResizeResultCode::Applied if self.applied_generation != 0 => Ok(()),
            ResizeResultCode::Error(_) if self.applied_generation == 0 => Ok(()),
            _ => Err(FramingError::MalformedPayload),
        }
    }
}
