//! SPEC-004 Candidate-D binary wire framing.
//!
//! The 24-byte envelope is shared by control and presentation messages. Rust
//! memory layout is never wire format and all client-controlled lengths are
//! validated before allocation/use.

use crate::{AttachmentId, ExecutionId};

pub use crate::pass7::{
    BlockTimeline, CommandBlock, CommandBlockState, ComposerCommandRef, ComposerEligibility,
    ComposerResult, ComposerResultCode, ComposerStatus, HistoryCell, HistoryRangeRequest,
    HistoryRangeSnapshot, HistoryRangeStatus, HistoryRow, HistorySourceCell, ResizeRequest,
    ResizeResult, ResizeResultCode, TerminalKey, TerminalKeyKind, TerminalKeyModifiers,
    TerminalKeyV2, TerminalKeyV2Event, TerminalKeyV2Kind, TerminalKeyV2Modifiers, ViewportLineIds,
    CAP_CORRELATED_RESIZE, CAP_EXTENDED_TERMINAL_KEY, CAP_SEMANTIC_TERMINAL_KEY,
    HISTORY_CELL_CONTINUATION_FLAG, HISTORY_CELL_SIDECAR_FLAG, HISTORY_CELL_WIDTH_MASK,
    HISTORY_CELL_WIDTH_SHIFT, MAX_HISTORY_GRAPHEME_BYTES, MAX_HISTORY_RANGE_BYTES,
    MAX_HISTORY_RANGE_CELLS, MAX_HISTORY_RANGE_LINES, MAX_HISTORY_SIDECAR_BYTES,
};

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
/// Peer accepts Runtime→client primary viewport LineId vectors (type 35) so
/// Flow live-tail can map Block `start_line` onto prepared-frame rows.
/// Bit 8 is reserved by accepted ADR-009 for `CAP_COMMAND_BLOCK_DURATION`.
pub const CAP_VIEWPORT_LINE_IDS: u32 = 1 << 9;

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

fn read_u128(bytes: &[u8]) -> u128 {
    let mut raw = [0u8; 16];
    raw.copy_from_slice(&bytes[..16]);
    u128::from_le_bytes(raw)
}

fn write_u128(out: &mut Vec<u8>, value: u128) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn execution_id_from(bytes: &[u8]) -> ExecutionId {
    ExecutionId::from_bytes(read_u128(bytes).to_le_bytes())
}

fn attachment_id_from(bytes: &[u8]) -> AttachmentId {
    AttachmentId::from_bytes(read_u128(bytes).to_le_bytes())
}

fn exact_len(bytes: &[u8], expected: usize) -> Result<(), FramingError> {
    if bytes.len() != expected {
        return Err(FramingError::ExactLengthMismatch);
    }
    Ok(())
}

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum MessageType {
    ClientHello = 1,
    ServerHello = 2,
    ListExecutions = 3,
    ExecutionList = 4,
    Attach = 5,
    Attached = 6,
    Detach = 7,
    Detached = 8,
    Input = 9,
    Resize = 10,
    Resync = 11,
    DisplaySnapshot = 12,
    DisplayDelta = 13,
    Lifecycle = 14,
    Error = 15,
    Goodbye = 16,
    TerminalKey = 17,
    ResizeRequest = 18,
    ResizeResult = 19,
    ComposerCommand = 20,
    BlockTimeline = 21,
    ComposerResult = 22,
    ComposerStatus = 23,
    HistoryRangeRequest = 24,
    HistoryRangeSnapshot = 25,
    DisplaySnapshotV2 = 27,
    DisplayDeltaV2 = 28,
    TerminalKeyV2 = 29,
    /// Host clipboard paste. Same payload layout as `Input`; Runtime wraps
    /// the bytes using canonical bracketed-paste mode before PTY admission.
    /// Type 26 is Pass 8 block-state metadata (not a control MessageType).
    Paste = 30,
    /// Host selection/copy-mode commands. Never written to the PTY.
    HostSelection = 31,
    /// Runtime→client yanked/copied UTF-8. Same payload layout as `Input`.
    CopiedText = 32,
    /// Host history search. Never written to the PTY.
    HostSearch = 33,
    /// Native mouse event. Runtime encodes SGR/X10 from canonical modes.
    TerminalMouse = 34,
    /// Runtime→client primary viewport LineIds for one display generation.
    /// Gated on `CAP_VIEWPORT_LINE_IDS`.
    ViewportLineIds = 35,
}
impl MessageType {
    pub fn from_u16(value: u16) -> Option<Self> {
        Some(match value {
            1 => Self::ClientHello,
            2 => Self::ServerHello,
            3 => Self::ListExecutions,
            4 => Self::ExecutionList,
            5 => Self::Attach,
            6 => Self::Attached,
            7 => Self::Detach,
            8 => Self::Detached,
            9 => Self::Input,
            10 => Self::Resize,
            11 => Self::Resync,
            12 => Self::DisplaySnapshot,
            13 => Self::DisplayDelta,
            14 => Self::Lifecycle,
            15 => Self::Error,
            16 => Self::Goodbye,
            17 => Self::TerminalKey,
            18 => Self::ResizeRequest,
            19 => Self::ResizeResult,
            20 => Self::ComposerCommand,
            21 => Self::BlockTimeline,
            22 => Self::ComposerResult,
            23 => Self::ComposerStatus,
            24 => Self::HistoryRangeRequest,
            25 => Self::HistoryRangeSnapshot,
            27 => Self::DisplaySnapshotV2,
            28 => Self::DisplayDeltaV2,
            29 => Self::TerminalKeyV2,
            30 => Self::Paste,
            31 => Self::HostSelection,
            32 => Self::CopiedText,
            33 => Self::HostSearch,
            34 => Self::TerminalMouse,
            35 => Self::ViewportLineIds,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message<'a> {
    ClientHello(ClientHello),
    ServerHello(ServerHello),
    ListExecutions,
    ExecutionList(ExecutionList),
    Attach(Attach),
    Attached(Attached),
    Detach(Detach),
    Detached(Detached),
    Input(InputRef<'a>),
    Resize(Resize),
    Resync(Resync),
    DisplaySnapshot(&'a [u8]),
    DisplayDelta(&'a [u8]),
    DisplaySnapshotV2(&'a [u8]),
    DisplayDeltaV2(&'a [u8]),
    Lifecycle(LifecycleMessage),
    Error(ErrorMessage),
    Goodbye,
    TerminalKey(TerminalKey),
    ResizeRequest(ResizeRequest),
    ResizeResult(ResizeResult),
    ComposerCommand(ComposerCommandRef<'a>),
    BlockTimeline(BlockTimeline),
    ComposerResult(ComposerResult),
    ComposerStatus(ComposerStatus),
    HistoryRangeRequest(HistoryRangeRequest),
    HistoryRangeSnapshot(HistoryRangeSnapshot),
    TerminalKeyV2(TerminalKeyV2),
    Paste(InputRef<'a>),
    HostSelection(HostSelection),
    CopiedText(InputRef<'a>),
    HostSearch(HostSearch<'a>),
    TerminalMouse(TerminalMouse),
    ViewportLineIds(ViewportLineIds),
}

pub fn decode_message<'a>(
    header: &FrameHeader,
    payload: &'a [u8],
) -> Result<Message<'a>, FramingError> {
    if payload.len() != header.payload_len as usize {
        return Err(FramingError::ExactLengthMismatch);
    }
    let kind =
        MessageType::from_u16(header.message_type).ok_or(FramingError::UnknownMessageType)?;
    Ok(match kind {
        MessageType::ClientHello => Message::ClientHello(ClientHello::decode(payload)?),
        MessageType::ServerHello => Message::ServerHello(ServerHello::decode(payload)?),
        MessageType::ListExecutions => {
            if !payload.is_empty() {
                return Err(FramingError::ExactLengthMismatch);
            }
            Message::ListExecutions
        }
        MessageType::ExecutionList => Message::ExecutionList(ExecutionList::decode(payload)?),
        MessageType::Attach => Message::Attach(Attach::decode(payload)?),
        MessageType::Attached => Message::Attached(Attached::decode(payload)?),
        MessageType::Detach => Message::Detach(Detach::decode(payload)?),
        MessageType::Detached => Message::Detached(Detached::decode(payload)?),
        MessageType::Input => Message::Input(InputRef::decode(payload)?),
        MessageType::Resize => Message::Resize(Resize::decode(payload)?),
        MessageType::Resync => Message::Resync(Resync::decode(payload)?),
        MessageType::DisplaySnapshot => Message::DisplaySnapshot(payload),
        MessageType::DisplayDelta => Message::DisplayDelta(payload),
        MessageType::DisplaySnapshotV2 => Message::DisplaySnapshotV2(payload),
        MessageType::DisplayDeltaV2 => Message::DisplayDeltaV2(payload),
        MessageType::Lifecycle => Message::Lifecycle(LifecycleMessage::decode(payload)?),
        MessageType::Error => Message::Error(ErrorMessage::decode(payload)?),
        MessageType::Goodbye => {
            if !payload.is_empty() {
                return Err(FramingError::ExactLengthMismatch);
            }
            Message::Goodbye
        }
        MessageType::TerminalKey => Message::TerminalKey(TerminalKey::decode(payload)?),
        MessageType::ResizeRequest => Message::ResizeRequest(ResizeRequest::decode(payload)?),
        MessageType::ResizeResult => Message::ResizeResult(ResizeResult::decode(payload)?),
        MessageType::ComposerCommand => {
            Message::ComposerCommand(ComposerCommandRef::decode(payload)?)
        }
        MessageType::BlockTimeline => Message::BlockTimeline(BlockTimeline::decode(payload)?),
        MessageType::ComposerResult => Message::ComposerResult(ComposerResult::decode(payload)?),
        MessageType::ComposerStatus => Message::ComposerStatus(ComposerStatus::decode(payload)?),
        MessageType::HistoryRangeRequest => {
            Message::HistoryRangeRequest(HistoryRangeRequest::decode(payload)?)
        }
        MessageType::HistoryRangeSnapshot => {
            Message::HistoryRangeSnapshot(HistoryRangeSnapshot::decode(payload)?)
        }
        MessageType::TerminalKeyV2 => Message::TerminalKeyV2(TerminalKeyV2::decode(payload)?),
        MessageType::Paste => Message::Paste(InputRef::decode(payload)?),
        MessageType::HostSelection => Message::HostSelection(HostSelection::decode(payload)?),
        MessageType::CopiedText => Message::CopiedText(InputRef::decode(payload)?),
        MessageType::HostSearch => Message::HostSearch(HostSearch::decode(payload)?),
        MessageType::TerminalMouse => Message::TerminalMouse(TerminalMouse::decode(payload)?),
        MessageType::ViewportLineIds => Message::ViewportLineIds(ViewportLineIds::decode(payload)?),
    })
}

pub fn encode_frame(message_type: MessageType, payload: &[u8]) -> Vec<u8> {
    debug_assert!(payload.len() <= MAX_FRAME_PAYLOAD as usize);
    let header = FrameHeader::new(message_type as u16, payload.len() as u32);
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(&header.encode());
    out.extend_from_slice(payload);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(
            MessageType::from_u16(35),
            Some(MessageType::ViewportLineIds)
        );
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
}
