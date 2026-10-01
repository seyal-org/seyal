//! SPEC-004 §18 execution provisioning/disposition wire payloads (types 36–39).

use crate::{AttachmentId, ExecutionId};

use super::envelope::{
    attachment_id_from, exact_len, execution_id_from, read_u128, write_u128, ErrorCode,
    FramingError,
};

/// Capability bit for SPEC-004 §18 create/terminate messages (types 36–39).
pub const CAP_EXECUTION_PROVISIONING: u32 = 1 << 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreateExecutionRequest {
    pub workspace_id: u128,
    pub request_id: u64,
    pub launch_profile: u16,
    pub rows: u16,
    pub columns: u16,
}

impl CreateExecutionRequest {
    pub const WIRE_LEN: usize = 32;

    pub fn encode(&self) -> Vec<u8> {
        debug_assert!(self.validate().is_ok());
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        write_u128(&mut out, self.workspace_id);
        out.extend_from_slice(&self.request_id.to_le_bytes());
        out.extend_from_slice(&self.launch_profile.to_le_bytes());
        out.extend_from_slice(&self.rows.to_le_bytes());
        out.extend_from_slice(&self.columns.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        if u16::from_le_bytes(bytes[30..32].try_into().unwrap()) != 0 {
            return Err(FramingError::MalformedPayload);
        }
        let value = Self {
            workspace_id: read_u128(&bytes[0..16]),
            request_id: u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
            launch_profile: u16::from_le_bytes(bytes[24..26].try_into().unwrap()),
            rows: u16::from_le_bytes(bytes[26..28].try_into().unwrap()),
            columns: u16::from_le_bytes(bytes[28..30].try_into().unwrap()),
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), FramingError> {
        if self.request_id == 0 {
            return Err(FramingError::MalformedPayload);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreateExecutionResultCode {
    Created,
    Error(ErrorCode),
}

impl CreateExecutionResultCode {
    pub const fn wire_value(self) -> u16 {
        match self {
            Self::Created => 0,
            Self::Error(error) => error as u16,
        }
    }

    fn from_u16(value: u16) -> Result<Self, FramingError> {
        if value == 0 {
            return Ok(Self::Created);
        }
        ErrorCode::from_u16(value)
            .map(Self::Error)
            .ok_or(FramingError::MalformedPayload)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreateExecutionResult {
    pub execution_id: ExecutionId,
    pub request_id: u64,
    pub result_code: CreateExecutionResultCode,
    pub detail_code: u32,
}

impl CreateExecutionResult {
    pub const WIRE_LEN: usize = 32;

    pub fn encode(&self) -> Vec<u8> {
        debug_assert!(self.validate().is_ok());
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        out.extend_from_slice(&self.execution_id.to_bytes());
        out.extend_from_slice(&self.request_id.to_le_bytes());
        out.extend_from_slice(&self.result_code.wire_value().to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&self.detail_code.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        if u16::from_le_bytes(bytes[26..28].try_into().unwrap()) != 0 {
            return Err(FramingError::MalformedPayload);
        }
        let value = Self {
            execution_id: execution_id_from(&bytes[..16]),
            request_id: u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
            result_code: CreateExecutionResultCode::from_u16(u16::from_le_bytes(
                bytes[24..26].try_into().unwrap(),
            ))?,
            detail_code: u32::from_le_bytes(bytes[28..32].try_into().unwrap()),
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), FramingError> {
        if self.request_id == 0 || self.detail_code != 0 {
            return Err(FramingError::MalformedPayload);
        }
        let zero = self.execution_id.to_bytes() == [0; 16];
        match self.result_code {
            CreateExecutionResultCode::Created if !zero => Ok(()),
            CreateExecutionResultCode::Error(_) if zero => Ok(()),
            _ => Err(FramingError::MalformedPayload),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminateExecutionRequest {
    pub attachment_id: AttachmentId,
    pub execution_id: ExecutionId,
    pub request_id: u64,
}

impl TerminateExecutionRequest {
    pub const WIRE_LEN: usize = 40;

    pub fn encode(&self) -> Vec<u8> {
        debug_assert!(self.validate().is_ok());
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        out.extend_from_slice(&self.attachment_id.to_bytes());
        out.extend_from_slice(&self.execution_id.to_bytes());
        out.extend_from_slice(&self.request_id.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        let value = Self {
            attachment_id: attachment_id_from(&bytes[..16]),
            execution_id: execution_id_from(&bytes[16..32]),
            request_id: u64::from_le_bytes(bytes[32..40].try_into().unwrap()),
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), FramingError> {
        if self.request_id == 0 {
            return Err(FramingError::MalformedPayload);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminateExecutionResultCode {
    TerminationRequested,
    Error(ErrorCode),
}

impl TerminateExecutionResultCode {
    pub const fn wire_value(self) -> u16 {
        match self {
            Self::TerminationRequested => 0,
            Self::Error(error) => error as u16,
        }
    }

    fn from_u16(value: u16) -> Result<Self, FramingError> {
        if value == 0 {
            return Ok(Self::TerminationRequested);
        }
        ErrorCode::from_u16(value)
            .map(Self::Error)
            .ok_or(FramingError::MalformedPayload)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminateExecutionResult {
    pub attachment_id: AttachmentId,
    pub request_id: u64,
    pub result_code: TerminateExecutionResultCode,
    pub detail_code: u32,
}

impl TerminateExecutionResult {
    pub const WIRE_LEN: usize = 32;

    pub fn encode(&self) -> Vec<u8> {
        debug_assert!(self.validate().is_ok());
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        out.extend_from_slice(&self.attachment_id.to_bytes());
        out.extend_from_slice(&self.request_id.to_le_bytes());
        out.extend_from_slice(&self.result_code.wire_value().to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&self.detail_code.to_le_bytes());
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
            result_code: TerminateExecutionResultCode::from_u16(u16::from_le_bytes(
                bytes[24..26].try_into().unwrap(),
            ))?,
            detail_code: u32::from_le_bytes(bytes[28..32].try_into().unwrap()),
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), FramingError> {
        if self.request_id == 0 || self.detail_code != 0 {
            return Err(FramingError::MalformedPayload);
        }
        Ok(())
    }
}
