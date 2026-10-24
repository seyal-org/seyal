//! Pass 7 command-block timeline wire payloads.
//!
//! Two record shapes exist, selected only by the negotiated capability, never
//! by payload length (ADR-009 2026-09-19 amendment, SPEC-008 §5.4): the legacy
//! 36-byte fixed header and the duration-capable 52-byte fixed header.

use crate::framing::{FramingError, CAP_COMMAND_BLOCKS, CAP_COMMAND_BLOCK_DURATION};

use super::composer::MAX_COMPOSER_COMMAND_BYTES;

// A replacement timeline must fit in one bounded IPC frame. Per-record text
// stays at 16 KiB; Runtime admits/evicts so the encoded timeline never exceeds
// MAX_FRAME_PAYLOAD. Larger histories require a separately versioned
// continuation protocol.
pub const MAX_COMMAND_BLOCK_RECORDS: usize = 128;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandBlockState {
    Running,
    /// `exit_status` is `None` when completion was observed without a
    /// finishing marker; encoded as state tag 2 with a zero status field.
    Completed {
        exit_status: Option<i32>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandBlock {
    pub id: u64,
    pub command: String,
    pub start_line: u64,
    pub end_line: Option<u64>,
    pub state: CommandBlockState,
    /// Runtime-measured `C`→`D` elapsed time. `Some` only for a Block that
    /// completed through a matching trusted `D` (state tag 1); never a
    /// sentinel for unknown.
    pub duration_ns: Option<u64>,
}

impl CommandBlock {
    /// A duration exists only after a matching trusted `D`, which also
    /// supplies the exit status.
    fn duration_is_consistent(&self) -> bool {
        self.duration_ns.is_none()
            || matches!(
                self.state,
                CommandBlockState::Completed {
                    exit_status: Some(_)
                }
            )
    }
}

/// `BlockTimeline` record shape for one connection: the intersection of the
/// ClientHello request and the ServerHello capabilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlockTimelineSchema {
    /// 36-byte fixed record header; carries no duration.
    Legacy,
    /// 52-byte fixed record header with explicit duration presence.
    Duration,
}

impl BlockTimelineSchema {
    /// `None` without command Blocks (duration alone negotiates nothing).
    pub fn negotiated(capabilities: u32) -> Option<Self> {
        if capabilities & CAP_COMMAND_BLOCKS == 0 {
            None
        } else if capabilities & CAP_COMMAND_BLOCK_DURATION != 0 {
            Some(Self::Duration)
        } else {
            Some(Self::Legacy)
        }
    }

    pub const fn record_header_len(self) -> usize {
        match self {
            Self::Legacy => 36,
            Self::Duration => 52,
        }
    }

    /// Offset of `command_len` within the fixed record header.
    const fn command_len_offset(self) -> usize {
        self.record_header_len() - 4
    }
}

/// A bounded full replacement cache for one attached execution. A client must
/// atomically replace its disposable projection when this arrives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockTimeline {
    pub revision: u64,
    pub records: Vec<CommandBlock>,
}

impl BlockTimeline {
    const HEADER_LEN: usize = 16;

    /// Legacy-schema encode (peers without `CAP_COMMAND_BLOCK_DURATION`).
    pub fn try_encode(&self) -> Result<Vec<u8>, FramingError> {
        self.try_encode_schema(BlockTimelineSchema::Legacy)
    }

    /// Exact encoded length under `schema`, for admission/eviction planning.
    pub fn encoded_len(&self, schema: BlockTimelineSchema) -> usize {
        Self::HEADER_LEN
            + self
                .records
                .iter()
                .map(|record| schema.record_header_len() + record.command.len())
                .sum::<usize>()
    }

    pub fn try_encode_schema(&self, schema: BlockTimelineSchema) -> Result<Vec<u8>, FramingError> {
        if self.records.len() > MAX_COMMAND_BLOCK_RECORDS {
            return Err(FramingError::OversizedPayload);
        }
        let mut out = Vec::with_capacity(Self::HEADER_LEN);
        out.extend_from_slice(&self.revision.to_le_bytes());
        out.extend_from_slice(&(self.records.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        for record in &self.records {
            let command = record.command.as_bytes();
            if command.is_empty()
                || command.len() > MAX_COMPOSER_COMMAND_BYTES
                || !record.duration_is_consistent()
            {
                return Err(FramingError::MalformedPayload);
            }
            let (state, exit_status) = match record.state {
                CommandBlockState::Running => (0u8, 0i32),
                CommandBlockState::Completed {
                    exit_status: Some(exit_status),
                } => (1u8, exit_status),
                CommandBlockState::Completed { exit_status: None } => (2u8, 0i32),
            };
            out.extend_from_slice(&record.id.to_le_bytes());
            out.extend_from_slice(&record.start_line.to_le_bytes());
            out.extend_from_slice(&record.end_line.unwrap_or(0).to_le_bytes());
            out.push(state);
            out.extend_from_slice(&[0; 3]);
            out.extend_from_slice(&exit_status.to_le_bytes());
            if schema == BlockTimelineSchema::Duration {
                out.push(u8::from(record.duration_ns.is_some()));
                out.extend_from_slice(&[0; 7]);
                out.extend_from_slice(&record.duration_ns.unwrap_or(0).to_le_bytes());
            }
            out.extend_from_slice(&(command.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(command);
            if out.len() > crate::framing::MAX_FRAME_PAYLOAD as usize {
                return Err(FramingError::OversizedPayload);
            }
        }
        Ok(out)
    }

    pub fn encode(&self) -> Vec<u8> {
        self.try_encode()
            .expect("BlockTimeline must satisfy bounded wire limits")
    }

    /// Legacy-schema decode (peers without `CAP_COMMAND_BLOCK_DURATION`).
    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        Self::decode_schema(bytes, BlockTimelineSchema::Legacy)
    }

    /// Decode with the connection's negotiated schema. A shape that disagrees
    /// with `schema` fails closed before any record is applied.
    pub fn decode_schema(bytes: &[u8], schema: BlockTimelineSchema) -> Result<Self, FramingError> {
        if bytes.len() > crate::framing::MAX_FRAME_PAYLOAD as usize {
            return Err(FramingError::OversizedPayload);
        }
        if bytes.len() < Self::HEADER_LEN {
            return Err(FramingError::TruncatedPayload);
        }
        let revision = u64::from_le_bytes(bytes[..8].try_into().unwrap());
        let count = u16::from_le_bytes(bytes[8..10].try_into().unwrap()) as usize;
        if u16::from_le_bytes(bytes[10..12].try_into().unwrap()) != 0
            || u32::from_le_bytes(bytes[12..16].try_into().unwrap()) != 0
            || count > MAX_COMMAND_BLOCK_RECORDS
        {
            return Err(FramingError::MalformedPayload);
        }
        let header_len = schema.record_header_len();
        let mut offset = Self::HEADER_LEN;
        let mut records = Vec::with_capacity(count);
        for _ in 0..count {
            let header_end = offset
                .checked_add(header_len)
                .ok_or(FramingError::MalformedPayload)?;
            if header_end > bytes.len() {
                return Err(FramingError::TruncatedPayload);
            }
            let record = &bytes[offset..header_end];
            let id = u64::from_le_bytes(record[0..8].try_into().unwrap());
            let start_line = u64::from_le_bytes(record[8..16].try_into().unwrap());
            let end_raw = u64::from_le_bytes(record[16..24].try_into().unwrap());
            let state_tag = record[24];
            // Preserved from the legacy decoder: bytes 30..32 (the high half of
            // `exit_status`) must be zero, so only 0..=65535 statuses decode.
            if record[25..28] != [0; 3] || record[30..32] != [0; 2] {
                return Err(FramingError::MalformedPayload);
            }
            let exit_status = i32::from_le_bytes(record[28..32].try_into().unwrap());
            let duration_ns = match schema {
                BlockTimelineSchema::Legacy => None,
                BlockTimelineSchema::Duration => {
                    let value = u64::from_le_bytes(record[40..48].try_into().unwrap());
                    if record[33..40] != [0; 7] {
                        return Err(FramingError::MalformedPayload);
                    }
                    match record[32] {
                        0 if value == 0 => None,
                        1 => Some(value),
                        _ => return Err(FramingError::MalformedPayload),
                    }
                }
            };
            let len_at = schema.command_len_offset();
            let command_len =
                u16::from_le_bytes(record[len_at..len_at + 2].try_into().unwrap()) as usize;
            if command_len == 0
                || command_len > MAX_COMPOSER_COMMAND_BYTES
                || u16::from_le_bytes(record[len_at + 2..len_at + 4].try_into().unwrap()) != 0
            {
                return Err(FramingError::MalformedPayload);
            }
            let command_end = header_end
                .checked_add(command_len)
                .ok_or(FramingError::MalformedPayload)?;
            if command_end > bytes.len() {
                return Err(FramingError::TruncatedPayload);
            }
            let command = std::str::from_utf8(&bytes[header_end..command_end])
                .map_err(|_| FramingError::MalformedPayload)?
                .to_owned();
            let (end_line, state) = match state_tag {
                0 if end_raw == 0 && exit_status == 0 => (None, CommandBlockState::Running),
                1 if end_raw >= start_line => (
                    Some(end_raw),
                    CommandBlockState::Completed {
                        exit_status: Some(exit_status),
                    },
                ),
                2 if end_raw >= start_line && exit_status == 0 => (
                    Some(end_raw),
                    CommandBlockState::Completed { exit_status: None },
                ),
                _ => return Err(FramingError::MalformedPayload),
            };
            if id == 0 || start_line == 0 {
                return Err(FramingError::MalformedPayload);
            }
            let block = CommandBlock {
                id,
                command,
                start_line,
                end_line,
                state,
                duration_ns,
            };
            if !block.duration_is_consistent() {
                return Err(FramingError::MalformedPayload);
            }
            records.push(block);
            offset = command_end;
        }
        if offset != bytes.len() {
            return Err(FramingError::ExactLengthMismatch);
        }
        Ok(Self { revision, records })
    }
}
