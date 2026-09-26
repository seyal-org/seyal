//! Pass 7 composer admission/status wire payloads.

use crate::{framing::FramingError, AttachmentId};

use super::{attachment_id_from, exact_len};

pub const MAX_COMPOSER_COMMAND_BYTES: usize = 16 * 1024;

/// Runtime's answer to "would a composer submission be admitted right now?"
/// (ADR-009 invariant 7, 2026-09-16 amendment mechanism 5). `Available` is
/// exactly the admission-time prompt gate: a trusted prompt was announced and
/// nothing was admitted since, on the primary screen, with I/O active.
/// `Busy` covers everything before the first trusted prompt, a submission in
/// flight, a running command, direct input, a foreground full-screen program,
/// and a terminated execution. `Unsupported` means the shell never proves
/// trusted integration, so submissions take the raw path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ComposerEligibility {
    Available = 0,
    Busy = 1,
    Unsupported = 2,
}

/// Runtime→client message type 23 (`MessageType::ComposerStatus`), gated on
/// `CAP_COMMAND_BLOCKS`. Runtime sends it once at attach and again on every
/// eligibility flip; it is never sent per byte or per marker. `revision` is a
/// per-execution monotonic fence: a client accepts a status only for its own
/// `attachment_id` and only when the revision moves forward, so a delayed or
/// replayed frame can never re-enable a composer against a newer fact. A
/// client that has not received one yet must treat the composer as busy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComposerStatus {
    pub attachment_id: AttachmentId,
    pub eligibility: ComposerEligibility,
    pub revision: u64,
}

impl ComposerStatus {
    const WIRE_LEN: usize = 32;

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        out.extend_from_slice(&self.attachment_id.to_bytes());
        out.push(self.eligibility as u8);
        out.extend_from_slice(&[0; 7]);
        out.extend_from_slice(&self.revision.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        if bytes[17..24] != [0; 7] {
            return Err(FramingError::MalformedPayload);
        }
        let eligibility = match bytes[16] {
            0 => ComposerEligibility::Available,
            1 => ComposerEligibility::Busy,
            2 => ComposerEligibility::Unsupported,
            _ => return Err(FramingError::MalformedPayload),
        };
        Ok(Self {
            attachment_id: attachment_id_from(&bytes[..16]),
            eligibility,
            revision: u64::from_le_bytes(bytes[24..32].try_into().unwrap()),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ComposerResultCode {
    Accepted = 0,
    Busy = 1,
    Unsupported = 2,
    Backpressure = 3,
    Invalid = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComposerResult {
    pub attachment_id: AttachmentId,
    pub code: ComposerResultCode,
    pub block_id: u64,
    pub request_id: u64,
}

impl ComposerResult {
    const WIRE_LEN: usize = 40;

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        out.extend_from_slice(&self.attachment_id.to_bytes());
        out.push(self.code as u8);
        out.extend_from_slice(&[0; 7]);
        out.extend_from_slice(&self.block_id.to_le_bytes());
        out.extend_from_slice(&self.request_id.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        if bytes[17..24] != [0; 7] {
            return Err(FramingError::MalformedPayload);
        }
        let code = match bytes[16] {
            0 => ComposerResultCode::Accepted,
            1 => ComposerResultCode::Busy,
            2 => ComposerResultCode::Unsupported,
            3 => ComposerResultCode::Backpressure,
            4 => ComposerResultCode::Invalid,
            _ => return Err(FramingError::MalformedPayload),
        };
        Ok(Self {
            attachment_id: attachment_id_from(&bytes[..16]),
            code,
            block_id: u64::from_le_bytes(bytes[24..32].try_into().unwrap()),
            request_id: u64::from_le_bytes(bytes[32..40].try_into().unwrap()),
        })
    }
}

/// A complete UTF-8 command committed from the unique Pane composer. The
/// borrowed command is bounded at the framing boundary and is never inferred
/// from terminal cells or prompts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComposerCommandRef<'a> {
    pub attachment_id: AttachmentId,
    pub request_id: u64,
    pub command: &'a str,
}

impl<'a> ComposerCommandRef<'a> {
    const HEADER_LEN: usize = 32;

    pub fn encode(&self) -> Vec<u8> {
        let bytes = self.command.as_bytes();
        debug_assert!(!bytes.is_empty() && bytes.len() <= MAX_COMPOSER_COMMAND_BYTES);
        let mut out = Vec::with_capacity(Self::HEADER_LEN + bytes.len());
        out.extend_from_slice(&self.attachment_id.to_bytes());
        out.extend_from_slice(&self.request_id.to_le_bytes());
        out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(bytes);
        out
    }

    pub fn decode(bytes: &'a [u8]) -> Result<Self, FramingError> {
        if bytes.len() < Self::HEADER_LEN {
            return Err(FramingError::TruncatedPayload);
        }
        let request_id = u64::from_le_bytes(bytes[16..24].try_into().unwrap());
        let declared = u32::from_le_bytes(bytes[24..28].try_into().unwrap()) as usize;
        if request_id == 0
            || u32::from_le_bytes(bytes[28..32].try_into().unwrap()) != 0
            || declared == 0
            || declared > MAX_COMPOSER_COMMAND_BYTES
            || bytes.len() != Self::HEADER_LEN + declared
        {
            return Err(FramingError::MalformedPayload);
        }
        let command = std::str::from_utf8(&bytes[Self::HEADER_LEN..])
            .map_err(|_| FramingError::MalformedPayload)?;
        Ok(Self {
            attachment_id: attachment_id_from(&bytes[..16]),
            request_id,
            command,
        })
    }
}
