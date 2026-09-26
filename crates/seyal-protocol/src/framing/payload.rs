//! SPEC-004 control and host message payloads (hello/attach/input/host UX).

use crate::{AttachmentId, ExecutionId};

use super::envelope::{
    attachment_id_from, exact_len, execution_id_from, read_u128, write_u128, FramingError,
    MAX_EXECUTION_LIST_ENTRIES, MAX_INPUT_BYTES,
};
use super::TerminalKeyV2Modifiers;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Observer = 0,
    Controller = 1,
}
impl Role {
    fn from_u8(value: u8) -> Result<Self, FramingError> {
        match value {
            0 => Ok(Self::Observer),
            1 => Ok(Self::Controller),
            _ => Err(FramingError::MalformedPayload),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifecycle {
    Running = 0,
    Terminating = 1,
    Finalized = 2,
}
impl Lifecycle {
    fn from_u8(value: u8) -> Result<Self, FramingError> {
        match value {
            0 => Ok(Self::Running),
            1 => Ok(Self::Terminating),
            2 => Ok(Self::Finalized),
            _ => Err(FramingError::MalformedPayload),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientHello {
    pub client_capabilities: u32,
}
impl ClientHello {
    pub const WIRE_LEN: usize = 8;
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        out.extend_from_slice(&self.client_capabilities.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        let client_capabilities = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        if u32::from_le_bytes(bytes[4..8].try_into().unwrap()) != 0 {
            return Err(FramingError::MalformedPayload);
        }
        Ok(Self {
            client_capabilities,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServerHello {
    pub runtime_id: u128,
    pub server_capabilities: u32,
    pub max_frame_payload: u32,
    pub max_input_payload: u32,
}
impl ServerHello {
    pub const WIRE_LEN: usize = 32;
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        write_u128(&mut out, self.runtime_id);
        out.extend_from_slice(&self.server_capabilities.to_le_bytes());
        out.extend_from_slice(&self.max_frame_payload.to_le_bytes());
        out.extend_from_slice(&self.max_input_payload.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        if u32::from_le_bytes(bytes[28..32].try_into().unwrap()) != 0 {
            return Err(FramingError::MalformedPayload);
        }
        Ok(Self {
            runtime_id: read_u128(&bytes[0..16]),
            server_capabilities: u32::from_le_bytes(bytes[16..20].try_into().unwrap()),
            max_frame_payload: u32::from_le_bytes(bytes[20..24].try_into().unwrap()),
            max_input_payload: u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecutionListEntry {
    pub execution_id: ExecutionId,
    pub lifecycle: Lifecycle,
    pub has_controller: bool,
    pub attachment_count: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionList {
    pub entries: Vec<ExecutionListEntry>,
}
impl ExecutionList {
    const ENTRY_LEN: usize = 20;
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + self.entries.len() * Self::ENTRY_LEN);
        out.extend_from_slice(&(self.entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        for entry in &self.entries {
            write_u128(&mut out, u128::from_le_bytes(entry.execution_id.to_bytes()));
            out.push(entry.lifecycle as u8);
            out.push(entry.has_controller as u8);
            out.extend_from_slice(&entry.attachment_count.to_le_bytes());
        }
        out
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        if bytes.len() < 4 {
            return Err(FramingError::TruncatedPayload);
        }
        let count = u16::from_le_bytes(bytes[0..2].try_into().unwrap());
        if u16::from_le_bytes(bytes[2..4].try_into().unwrap()) != 0
            || count > MAX_EXECUTION_LIST_ENTRIES
        {
            return Err(FramingError::MalformedPayload);
        }
        let expected = 4usize
            .checked_add(
                (count as usize)
                    .checked_mul(Self::ENTRY_LEN)
                    .ok_or(FramingError::LengthOverflow)?,
            )
            .ok_or(FramingError::LengthOverflow)?;
        exact_len(bytes, expected)?;
        let mut entries = Vec::with_capacity(count as usize);
        let mut offset = 4;
        for _ in 0..count {
            let lifecycle = Lifecycle::from_u8(bytes[offset + 16])?;
            let has_controller = match bytes[offset + 17] {
                0 => false,
                1 => true,
                _ => return Err(FramingError::MalformedPayload),
            };
            entries.push(ExecutionListEntry {
                execution_id: execution_id_from(&bytes[offset..offset + 16]),
                lifecycle,
                has_controller,
                attachment_count: u16::from_le_bytes(
                    bytes[offset + 18..offset + 20].try_into().unwrap(),
                ),
            });
            offset += Self::ENTRY_LEN;
        }
        Ok(Self { entries })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attach {
    pub execution_id: ExecutionId,
    pub requested_role: Role,
}
impl Attach {
    pub const WIRE_LEN: usize = 20;
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        write_u128(&mut out, u128::from_le_bytes(self.execution_id.to_bytes()));
        out.push(self.requested_role as u8);
        out.extend_from_slice(&[0u8; 3]);
        out
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        if bytes[17..20] != [0, 0, 0] {
            return Err(FramingError::MalformedPayload);
        }
        Ok(Self {
            execution_id: execution_id_from(&bytes[..16]),
            requested_role: Role::from_u8(bytes[16])?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attached {
    pub execution_id: ExecutionId,
    pub attachment_id: AttachmentId,
    pub granted_role: Role,
    pub current_generation: u64,
}
impl Attached {
    pub const WIRE_LEN: usize = 48;
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        write_u128(&mut out, u128::from_le_bytes(self.execution_id.to_bytes()));
        write_u128(&mut out, u128::from_le_bytes(self.attachment_id.to_bytes()));
        out.push(self.granted_role as u8);
        out.extend_from_slice(&[0u8; 7]);
        out.extend_from_slice(&self.current_generation.to_le_bytes());
        out
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        if bytes[33..40] != [0u8; 7] {
            return Err(FramingError::MalformedPayload);
        }
        Ok(Self {
            execution_id: execution_id_from(&bytes[0..16]),
            attachment_id: attachment_id_from(&bytes[16..32]),
            granted_role: Role::from_u8(bytes[32])?,
            current_generation: u64::from_le_bytes(bytes[40..48].try_into().unwrap()),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Detach {
    pub attachment_id: AttachmentId,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Detached {
    pub attachment_id: AttachmentId,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resync {
    pub attachment_id: AttachmentId,
}

macro_rules! impl_attachment_payload {
    ($type:ty) => {
        impl $type {
            pub const WIRE_LEN: usize = 16;
            pub fn encode(&self) -> Vec<u8> {
                self.attachment_id.to_bytes().to_vec()
            }
            pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
                exact_len(bytes, Self::WIRE_LEN)?;
                Ok(Self {
                    attachment_id: attachment_id_from(bytes),
                })
            }
        }
    };
}
impl_attachment_payload!(Detach);
impl_attachment_payload!(Detached);
impl_attachment_payload!(Resync);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputRef<'a> {
    pub attachment_id: AttachmentId,
    pub bytes: &'a [u8],
}
impl<'a> InputRef<'a> {
    pub const HEADER_LEN: usize = 20;
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::HEADER_LEN + self.bytes.len());
        out.extend_from_slice(&self.attachment_id.to_bytes());
        out.extend_from_slice(&(self.bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(self.bytes);
        out
    }
    pub fn decode(bytes: &'a [u8]) -> Result<Self, FramingError> {
        if bytes.len() < Self::HEADER_LEN {
            return Err(FramingError::TruncatedPayload);
        }
        let byte_count = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
        if byte_count > MAX_INPUT_BYTES {
            return Err(FramingError::OversizedPayload);
        }
        let expected = Self::HEADER_LEN
            .checked_add(byte_count as usize)
            .ok_or(FramingError::LengthOverflow)?;
        exact_len(bytes, expected)?;
        Ok(Self {
            attachment_id: attachment_id_from(&bytes[..16]),
            bytes: &bytes[20..],
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resize {
    pub attachment_id: AttachmentId,
    pub rows: u16,
    pub columns: u16,
}
impl Resize {
    pub const WIRE_LEN: usize = 20;
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        out.extend_from_slice(&self.attachment_id.to_bytes());
        out.extend_from_slice(&self.rows.to_le_bytes());
        out.extend_from_slice(&self.columns.to_le_bytes());
        out
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        Ok(Self {
            attachment_id: attachment_id_from(&bytes[..16]),
            rows: u16::from_le_bytes(bytes[16..18].try_into().unwrap()),
            columns: u16::from_le_bytes(bytes[18..20].try_into().unwrap()),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum HostSelectionAction {
    EnterCopyMode = 0,
    ExitCopyMode = 1,
    ToggleAnchor = 2,
    ToggleKind = 3,
    Yank = 4,
    SetVisual = 5,
    Clear = 6,
}

impl HostSelectionAction {
    pub fn from_u8(value: u8) -> Result<Self, FramingError> {
        match value {
            0 => Ok(Self::EnterCopyMode),
            1 => Ok(Self::ExitCopyMode),
            2 => Ok(Self::ToggleAnchor),
            3 => Ok(Self::ToggleKind),
            4 => Ok(Self::Yank),
            5 => Ok(Self::SetVisual),
            6 => Ok(Self::Clear),
            _ => Err(FramingError::MalformedPayload),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostSelection {
    pub attachment_id: AttachmentId,
    pub action: HostSelectionAction,
    pub kind: u8,
    pub start_col: u16,
    pub start_row: u16,
    pub end_col: u16,
    pub end_row: u16,
}

impl HostSelection {
    pub const WIRE_LEN: usize = 32;

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        out.extend_from_slice(&self.attachment_id.to_bytes());
        out.push(self.action as u8);
        out.push(self.kind);
        out.extend_from_slice(&self.start_col.to_le_bytes());
        out.extend_from_slice(&self.start_row.to_le_bytes());
        out.extend_from_slice(&self.end_col.to_le_bytes());
        out.extend_from_slice(&self.end_row.to_le_bytes());
        out.extend_from_slice(&[0u8; 6]);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        if bytes[26..32] != [0u8; 6] {
            return Err(FramingError::MalformedPayload);
        }
        Ok(Self {
            attachment_id: attachment_id_from(&bytes[..16]),
            action: HostSelectionAction::from_u8(bytes[16])?,
            kind: bytes[17],
            start_col: u16::from_le_bytes(bytes[18..20].try_into().unwrap()),
            start_row: u16::from_le_bytes(bytes[20..22].try_into().unwrap()),
            end_col: u16::from_le_bytes(bytes[22..24].try_into().unwrap()),
            end_row: u16::from_le_bytes(bytes[24..26].try_into().unwrap()),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TerminalMouseKind {
    Press = 1,
    Release = 2,
    Move = 3,
    Wheel = 4,
}

impl TerminalMouseKind {
    fn from_u8(value: u8) -> Result<Self, FramingError> {
        match value {
            1 => Ok(Self::Press),
            2 => Ok(Self::Release),
            3 => Ok(Self::Move),
            4 => Ok(Self::Wheel),
            _ => Err(FramingError::MalformedPayload),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalMouse {
    pub attachment_id: AttachmentId,
    pub action_id: u32,
    pub kind: TerminalMouseKind,
    pub button: u8,
    pub modifiers: TerminalKeyV2Modifiers,
    pub col: u16,
    pub row: u16,
}

impl TerminalMouse {
    pub const WIRE_LEN: usize = 32;

    pub fn encode(&self) -> Vec<u8> {
        debug_assert!(self.validate().is_ok());
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        out.extend_from_slice(&self.attachment_id.to_bytes());
        out.extend_from_slice(&self.action_id.to_le_bytes());
        out.push(self.kind as u8);
        out.push(self.button);
        out.extend_from_slice(&self.modifiers.bits().to_le_bytes());
        out.extend_from_slice(&self.col.to_le_bytes());
        out.extend_from_slice(&self.row.to_le_bytes());
        out.extend_from_slice(&[0u8; 4]);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        if bytes[28..32] != [0u8; 4] {
            return Err(FramingError::MalformedPayload);
        }
        let value = Self {
            attachment_id: attachment_id_from(&bytes[..16]),
            action_id: u32::from_le_bytes(bytes[16..20].try_into().unwrap()),
            kind: TerminalMouseKind::from_u8(bytes[20])?,
            button: bytes[21],
            modifiers: TerminalKeyV2Modifiers::from_bits_for_ffi(u16::from_le_bytes(
                bytes[22..24].try_into().unwrap(),
            ))
            .ok_or(FramingError::MalformedPayload)?,
            col: u16::from_le_bytes(bytes[24..26].try_into().unwrap()),
            row: u16::from_le_bytes(bytes[26..28].try_into().unwrap()),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), FramingError> {
        if self.action_id == 0 || self.col >= 512 || self.row >= 256 {
            return Err(FramingError::MalformedPayload);
        }
        let button_ok = match self.kind {
            TerminalMouseKind::Press | TerminalMouseKind::Release => self.button <= 2,
            TerminalMouseKind::Move => self.button <= 3,
            TerminalMouseKind::Wheel => (64..=67).contains(&self.button),
        };
        if button_ok {
            Ok(())
        } else {
            Err(FramingError::MalformedPayload)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostSearch<'a> {
    pub attachment_id: AttachmentId,
    pub forward: bool,
    pub needle: &'a str,
}

impl<'a> HostSearch<'a> {
    pub const HEADER_LEN: usize = 24;

    pub fn encode(&self) -> Vec<u8> {
        let bytes = self.needle.as_bytes();
        let mut out = Vec::with_capacity(Self::HEADER_LEN + bytes.len());
        out.extend_from_slice(&self.attachment_id.to_bytes());
        out.push(u8::from(self.forward));
        out.extend_from_slice(&[0u8; 3]);
        out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(bytes);
        out
    }

    pub fn decode(bytes: &'a [u8]) -> Result<Self, FramingError> {
        if bytes.len() < Self::HEADER_LEN {
            return Err(FramingError::TruncatedPayload);
        }
        if bytes[17..20] != [0u8; 3] {
            return Err(FramingError::MalformedPayload);
        }
        let forward = match bytes[16] {
            0 => false,
            1 => true,
            _ => return Err(FramingError::MalformedPayload),
        };
        let byte_count = u32::from_le_bytes(bytes[20..24].try_into().unwrap());
        if byte_count > MAX_INPUT_BYTES {
            return Err(FramingError::OversizedPayload);
        }
        let expected = Self::HEADER_LEN
            .checked_add(byte_count as usize)
            .ok_or(FramingError::LengthOverflow)?;
        exact_len(bytes, expected)?;
        let needle =
            std::str::from_utf8(&bytes[24..]).map_err(|_| FramingError::MalformedPayload)?;
        Ok(Self {
            attachment_id: attachment_id_from(&bytes[..16]),
            forward,
            needle,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LifecycleMessage {
    pub execution_id: ExecutionId,
    pub lifecycle: Lifecycle,
}
impl LifecycleMessage {
    pub const WIRE_LEN: usize = 24;
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        out.extend_from_slice(&self.execution_id.to_bytes());
        out.push(self.lifecycle as u8);
        out.extend_from_slice(&[0u8; 7]);
        out
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        if bytes[17..24] != [0u8; 7] {
            return Err(FramingError::MalformedPayload);
        }
        Ok(Self {
            execution_id: execution_id_from(&bytes[..16]),
            lifecycle: Lifecycle::from_u8(bytes[16])?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ErrorMessage {
    pub error_code: u16,
    pub offending_message_type: u16,
    pub detail_code: u32,
}
impl ErrorMessage {
    pub const WIRE_LEN: usize = 16;
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        out.extend_from_slice(&self.error_code.to_le_bytes());
        out.extend_from_slice(&self.offending_message_type.to_le_bytes());
        out.extend_from_slice(&self.detail_code.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes());
        out
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        if u64::from_le_bytes(bytes[8..16].try_into().unwrap()) != 0 {
            return Err(FramingError::MalformedPayload);
        }
        Ok(Self {
            error_code: u16::from_le_bytes(bytes[0..2].try_into().unwrap()),
            offending_message_type: u16::from_le_bytes(bytes[2..4].try_into().unwrap()),
            detail_code: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
        })
    }
}
