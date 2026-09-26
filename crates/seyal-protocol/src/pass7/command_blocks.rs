//! Pass 7 command-block timeline wire payloads.

use crate::framing::FramingError;

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
    const RECORD_HEADER_LEN: usize = 36;

    pub fn try_encode(&self) -> Result<Vec<u8>, FramingError> {
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
            if command.is_empty() || command.len() > MAX_COMPOSER_COMMAND_BYTES {
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

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
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
        let mut offset = Self::HEADER_LEN;
        let mut records = Vec::with_capacity(count);
        for _ in 0..count {
            let header_end = offset
                .checked_add(Self::RECORD_HEADER_LEN)
                .ok_or(FramingError::MalformedPayload)?;
            if header_end > bytes.len() {
                return Err(FramingError::TruncatedPayload);
            }
            let id = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
            let start_line = u64::from_le_bytes(bytes[offset + 8..offset + 16].try_into().unwrap());
            let end_raw = u64::from_le_bytes(bytes[offset + 16..offset + 24].try_into().unwrap());
            let state_tag = bytes[offset + 24];
            if bytes[offset + 25..offset + 28] != [0; 3]
                || u16::from_le_bytes(bytes[offset + 30..offset + 32].try_into().unwrap()) != 0
            {
                return Err(FramingError::MalformedPayload);
            }
            let exit_status =
                i32::from_le_bytes(bytes[offset + 28..offset + 32].try_into().unwrap());
            let command_len =
                u16::from_le_bytes(bytes[offset + 32..offset + 34].try_into().unwrap()) as usize;
            // The record fixed prefix includes the command length/reserved fields.
            let command_start = offset + 36;
            if command_len == 0
                || command_len > MAX_COMPOSER_COMMAND_BYTES
                || u16::from_le_bytes(bytes[offset + 34..offset + 36].try_into().unwrap()) != 0
            {
                return Err(FramingError::MalformedPayload);
            }
            let command_end = command_start
                .checked_add(command_len)
                .ok_or(FramingError::MalformedPayload)?;
            if command_end > bytes.len() {
                return Err(FramingError::TruncatedPayload);
            }
            let command = std::str::from_utf8(&bytes[command_start..command_end])
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
            records.push(CommandBlock {
                id,
                command,
                start_line,
                end_line,
                state,
            });
            offset = command_end;
        }
        if offset != bytes.len() {
            return Err(FramingError::ExactLengthMismatch);
        }
        Ok(Self { revision, records })
    }
}
