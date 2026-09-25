use crate::{
    display::{MAX_DISPLAY_COLUMNS, MAX_DISPLAY_ROWS},
    framing::{ErrorCode, FramingError},
    AttachmentId,
};

pub const MAX_COMPOSER_COMMAND_BYTES: usize = 16 * 1024;
// A replacement timeline must fit in one bounded IPC frame. Per-record text
// stays at 16 KiB; Runtime admits/evicts so the encoded timeline never exceeds
// MAX_FRAME_PAYLOAD. Larger histories require a separately versioned
// continuation protocol.
pub const MAX_COMMAND_BLOCK_RECORDS: usize = 128;
/// History range requests are deliberately independent from BlockTimeline.
/// They carry only a bounded primary-screen projection and never change block
/// identity or lifecycle authority.
pub const MAX_HISTORY_RANGE_LINES: usize = 512;
pub const MAX_HISTORY_RANGE_CELLS: usize = 131_072;
pub const MAX_HISTORY_RANGE_BYTES: usize = 196_608;
/// Chunk-local UTF-8 sidecar for multi-scalar history cells. Zero keeps the
/// M001 snapshot layout (`reserved == 0`, header bytes 28..32 zero).
pub const MAX_HISTORY_SIDECAR_BYTES: usize = 65_536;
pub const MAX_HISTORY_GRAPHEME_BYTES: usize = 8_192;
/// `HistoryCell.flags` bit 3: this cell is a width-two continuation placeholder.
pub const HISTORY_CELL_CONTINUATION_FLAG: u16 = 1 << 3;
/// `HistoryCell.flags` bits 4–5: terminal cell width (1 or 2) for a lead.
pub const HISTORY_CELL_WIDTH_SHIFT: u16 = 4;
pub const HISTORY_CELL_WIDTH_MASK: u16 = 0b11 << 4;
/// `HistoryCell.flags` bit 7: `reserved` is a sidecar byte offset, not zero.
pub const HISTORY_CELL_SIDECAR_FLAG: u16 = 1 << 7;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryRangeRequest {
    pub attachment_id: AttachmentId,
    pub request_id: u64,
    pub block_id: u64,
    pub start_line: u64,
    pub end_line: u64,
    pub max_lines: u16,
    pub max_cells: u32,
    /// Lead cells to skip from the start of the line range. Zero is the
    /// M001-compatible beginning of the first in-range line.
    pub start_unit: u32,
}

impl HistoryRangeRequest {
    const WIRE_LEN: usize = 64;

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        out.extend_from_slice(&self.attachment_id.to_bytes());
        out.extend_from_slice(&self.request_id.to_le_bytes());
        out.extend_from_slice(&self.block_id.to_le_bytes());
        out.extend_from_slice(&self.start_line.to_le_bytes());
        out.extend_from_slice(&self.end_line.to_le_bytes());
        out.extend_from_slice(&self.max_lines.to_le_bytes());
        out.extend_from_slice(&[0; 2]);
        out.extend_from_slice(&self.max_cells.to_le_bytes());
        out.extend_from_slice(&self.start_unit.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        let request_id = u64::from_le_bytes(bytes[16..24].try_into().unwrap());
        let block_id = u64::from_le_bytes(bytes[24..32].try_into().unwrap());
        let start_line = u64::from_le_bytes(bytes[32..40].try_into().unwrap());
        let end_line = u64::from_le_bytes(bytes[40..48].try_into().unwrap());
        if bytes[50..52] != [0; 2]
            || bytes[60..64] != [0; 4]
            || request_id == 0
            || block_id == 0
            || start_line == 0
            || end_line < start_line
        {
            return Err(FramingError::MalformedPayload);
        }
        let max_lines = u16::from_le_bytes(bytes[48..50].try_into().unwrap());
        let max_cells = u32::from_le_bytes(bytes[52..56].try_into().unwrap());
        let start_unit = u32::from_le_bytes(bytes[56..60].try_into().unwrap());
        if max_lines == 0
            || usize::from(max_lines) > MAX_HISTORY_RANGE_LINES
            || max_cells == 0
            || usize::try_from(max_cells).unwrap_or(usize::MAX) > MAX_HISTORY_RANGE_CELLS
        {
            return Err(FramingError::MalformedPayload);
        }
        Ok(Self {
            attachment_id: attachment_id_from(&bytes[..16]),
            request_id,
            block_id,
            start_line,
            end_line,
            max_lines,
            max_cells,
            start_unit,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum HistoryRangeStatus {
    Complete = 0,
    Truncated = 1,
    Stale = 2,
    Unsupported = 3,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryRow {
    pub line_id: u64,
    pub cells: Vec<HistoryCell>,
}

impl HistoryRow {
    pub const ENCODED_HEADER_LEN: usize = 16;
    pub const ENCODED_CELL_LEN: usize = 16;

    pub fn encoded_len(&self) -> usize {
        Self::ENCODED_HEADER_LEN
            .saturating_add(self.cells.len().saturating_mul(Self::ENCODED_CELL_LEN))
    }
}

/// Canonical terminal cells retain style as well as scalar. The packed colors
/// use the same tagged representation as `PreparedCell` and are resolved by
/// the native renderer, so the UI never reconstructs style from text.
///
/// Layout matches `SeyalHistoryCell` in `macos/Seyal/Sources/SeyalBridge.h`
/// (`reserved` is an explicit ABI field, not accidental padding).
///
/// Single-scalar leads keep `reserved == 0`. Multi-scalar/combining payloads
/// set [`HISTORY_CELL_SIDECAR_FLAG`] and store a length-prefixed UTF-8 record
/// in the snapshot sidecar; `reserved` is the byte offset of that record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct HistoryCell {
    pub scalar: u32,
    pub foreground: u32,
    pub background: u32,
    pub flags: u16,
    pub reserved: u16,
}

impl HistoryCell {
    /// Pack one presented history glyph. Single-scalar text stays inline so
    /// snapshots without combining/ZWJ units remain byte-identical to M001.
    pub fn from_text(
        text: &str,
        foreground: u32,
        background: u32,
        style_flags: u16,
        sidecar: &mut Vec<u8>,
    ) -> Result<Self, FramingError> {
        let style_flags = style_flags
            & !(HISTORY_CELL_SIDECAR_FLAG
                | HISTORY_CELL_CONTINUATION_FLAG
                | HISTORY_CELL_WIDTH_MASK);
        if text.is_empty() {
            return Ok(Self {
                scalar: 0,
                foreground,
                background,
                flags: style_flags,
                reserved: 0,
            });
        }
        let mut scalars = text.chars();
        let scalar = scalars.next().ok_or(FramingError::MalformedPayload)?;
        let multi = scalars.next().is_some() || text.len() != scalar.len_utf8();
        if !multi {
            return Ok(Self {
                scalar: scalar as u32,
                foreground,
                background,
                flags: style_flags,
                reserved: 0,
            });
        }
        let utf8 = text.as_bytes();
        if utf8.len() > MAX_HISTORY_GRAPHEME_BYTES
            || sidecar.len().saturating_add(2).saturating_add(utf8.len())
                > MAX_HISTORY_SIDECAR_BYTES
            || sidecar.len() > u16::MAX as usize
        {
            return Err(FramingError::OversizedPayload);
        }
        let reserved = sidecar.len() as u16;
        sidecar.extend_from_slice(&(utf8.len() as u16).to_le_bytes());
        sidecar.extend_from_slice(utf8);
        Ok(Self {
            scalar: scalar as u32,
            foreground,
            background,
            flags: style_flags | HISTORY_CELL_SIDECAR_FLAG,
            reserved,
        })
    }

    pub fn sidecar_utf8<'a>(&self, sidecar: &'a [u8]) -> Result<Option<&'a [u8]>, FramingError> {
        if self.flags & HISTORY_CELL_SIDECAR_FLAG == 0 {
            if self.reserved != 0 {
                return Err(FramingError::MalformedPayload);
            }
            return Ok(None);
        }
        let start = usize::from(self.reserved);
        let len_end = start.checked_add(2).ok_or(FramingError::MalformedPayload)?;
        if len_end > sidecar.len() {
            return Err(FramingError::MalformedPayload);
        }
        let len = u16::from_le_bytes(sidecar[start..len_end].try_into().unwrap()) as usize;
        if len == 0 || len > MAX_HISTORY_GRAPHEME_BYTES {
            return Err(FramingError::MalformedPayload);
        }
        let end = len_end
            .checked_add(len)
            .ok_or(FramingError::MalformedPayload)?;
        let bytes = sidecar
            .get(len_end..end)
            .ok_or(FramingError::MalformedPayload)?;
        if std::str::from_utf8(bytes).is_err() {
            return Err(FramingError::MalformedPayload);
        }
        Ok(Some(bytes))
    }

    pub fn with_cell_metrics(mut self, width: u8, continuation: bool) -> Self {
        self.flags &= !(HISTORY_CELL_CONTINUATION_FLAG | HISTORY_CELL_WIDTH_MASK);
        if continuation {
            self.flags |= HISTORY_CELL_CONTINUATION_FLAG;
        } else {
            let stored = width.clamp(1, 3);
            self.flags |= u16::from(stored) << HISTORY_CELL_WIDTH_SHIFT;
        }
        self
    }

    pub fn is_continuation(self) -> bool {
        self.flags & HISTORY_CELL_CONTINUATION_FLAG != 0
    }

    pub fn cell_width(self) -> u8 {
        ((self.flags & HISTORY_CELL_WIDTH_MASK) >> HISTORY_CELL_WIDTH_SHIFT) as u8
    }
}

/// One history glyph before sidecar packing. Continuations carry no text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistorySourceCell {
    pub text: String,
    pub width: u8,
    pub continuation: bool,
    pub foreground: u32,
    pub background: u32,
    pub style_flags: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryRangeSnapshot {
    pub request_id: u64,
    pub block_id: u64,
    pub revision: u64,
    pub status: HistoryRangeStatus,
    pub rows: Vec<HistoryRow>,
    pub sidecar: Vec<u8>,
}

impl HistoryRangeSnapshot {
    pub const ENCODED_HEADER_LEN: usize = 32;

    /// Admit rows under line, cell, and encoded-byte budgets.
    ///
    /// Wire admission is the authoritative stop for `MAX_HISTORY_RANGE_BYTES` /
    /// `MAX_FRAME_PAYLOAD`. Callers must treat a `true` truncated flag as
    /// `HistoryRangeStatus::Truncated` and must not surface `CapacityExceeded`
    /// merely because more retained history remains. `sidecar_len` is reserved
    /// in the byte budget so packed multi-scalar snapshots stay encodable.
    pub fn admit_rows(
        rows: impl IntoIterator<Item = HistoryRow>,
        max_lines: usize,
        max_cells: usize,
        sidecar_len: usize,
    ) -> (Vec<HistoryRow>, bool) {
        let byte_limit = MAX_HISTORY_RANGE_BYTES.min(crate::framing::MAX_FRAME_PAYLOAD as usize);
        let mut truncated = false;
        let mut cell_budget = max_cells;
        let mut used = Self::ENCODED_HEADER_LEN.saturating_add(sidecar_len);
        let mut out = Vec::new();
        for row in rows {
            if max_lines == 0 || out.len() >= max_lines {
                truncated = true;
                break;
            }
            if row.cells.len() > cell_budget {
                truncated = true;
                break;
            }
            let row_len = row.encoded_len();
            if used.saturating_add(row_len) > byte_limit {
                truncated = true;
                break;
            }
            cell_budget -= row.cells.len();
            used = used.saturating_add(row_len);
            out.push(row);
        }
        (out, truncated)
    }

    /// Drop trailing lead/continuation groups from the last row until encode
    /// can succeed, or drop empty trailing rows. Used as a safety net so a
    /// Truncated prefix never collapses to zero leads / CapacityExceeded.
    pub fn shrink_for_encode(&mut self) -> bool {
        if self.try_encode().is_ok() {
            return false;
        }
        let Some(last) = self.rows.last_mut() else {
            return false;
        };
        if last.cells.is_empty() {
            self.rows.pop();
            self.trim_sidecar_to_rows();
            self.status = HistoryRangeStatus::Truncated;
            return true;
        }
        while last.cells.last().is_some_and(|cell| cell.is_continuation()) {
            last.cells.pop();
        }
        if last.cells.pop().is_none() || last.cells.is_empty() {
            self.rows.pop();
        }
        self.trim_sidecar_to_rows();
        self.status = HistoryRangeStatus::Truncated;
        true
    }

    pub fn lead_count(rows: &[HistoryRow]) -> u32 {
        rows.iter()
            .flat_map(|row| row.cells.iter())
            .filter(|cell| !cell.is_continuation())
            .count() as u32
    }

    /// Pack glyphs into wire cells. Sidecar overflow keeps the already-packed
    /// prefix of the current row so a `start_unit` continuation can proceed.
    pub fn pack_source_rows(
        rows: impl IntoIterator<Item = (u64, Vec<HistorySourceCell>)>,
    ) -> (Vec<HistoryRow>, Vec<u8>, bool) {
        let mut sidecar = Vec::new();
        let mut packed_rows = Vec::new();
        let mut pack_truncated = false;
        for (line_id, cells) in rows {
            let mut wire_cells = Vec::with_capacity(cells.len());
            for cell in cells {
                let packed = if cell.continuation {
                    HistoryCell {
                        scalar: 0,
                        foreground: cell.foreground,
                        background: cell.background,
                        flags: cell.style_flags
                            & !(HISTORY_CELL_SIDECAR_FLAG
                                | HISTORY_CELL_CONTINUATION_FLAG
                                | HISTORY_CELL_WIDTH_MASK),
                        reserved: 0,
                    }
                    .with_cell_metrics(cell.width, true)
                } else {
                    match HistoryCell::from_text(
                        &cell.text,
                        cell.foreground,
                        cell.background,
                        cell.style_flags,
                        &mut sidecar,
                    ) {
                        Ok(packed) => packed.with_cell_metrics(cell.width, false),
                        Err(_) => {
                            pack_truncated = true;
                            break;
                        }
                    }
                };
                wire_cells.push(packed);
            }
            if pack_truncated && wire_cells.is_empty() {
                break;
            }
            if !wire_cells.is_empty() {
                packed_rows.push(HistoryRow {
                    line_id,
                    cells: wire_cells,
                });
            }
            if pack_truncated {
                break;
            }
        }
        (packed_rows, sidecar, pack_truncated)
    }

    /// Drop sidecar bytes no longer referenced after a trailing-row pop.
    pub fn trim_sidecar_to_rows(&mut self) {
        let mut end = 0usize;
        for cell in self.rows.iter().flat_map(|row| &row.cells) {
            if cell.flags & HISTORY_CELL_SIDECAR_FLAG == 0 {
                continue;
            }
            let start = usize::from(cell.reserved);
            if start + 2 > self.sidecar.len() {
                continue;
            }
            let len =
                u16::from_le_bytes(self.sidecar[start..start + 2].try_into().unwrap()) as usize;
            end = end.max(start.saturating_add(2).saturating_add(len));
        }
        self.sidecar.truncate(end);
    }

    pub fn try_encode(&self) -> Result<Vec<u8>, FramingError> {
        if self.rows.len() > MAX_HISTORY_RANGE_LINES
            || self.rows.iter().map(|row| row.cells.len()).sum::<usize>() > MAX_HISTORY_RANGE_CELLS
            || self.sidecar.len() > MAX_HISTORY_SIDECAR_BYTES
        {
            return Err(FramingError::OversizedPayload);
        }
        for cell in self.rows.iter().flat_map(|row| &row.cells) {
            cell.sidecar_utf8(&self.sidecar)?;
        }
        let mut out =
            Vec::with_capacity(Self::ENCODED_HEADER_LEN.saturating_add(self.sidecar.len()));
        out.extend_from_slice(&self.request_id.to_le_bytes());
        out.extend_from_slice(&self.block_id.to_le_bytes());
        out.extend_from_slice(&self.revision.to_le_bytes());
        out.push(self.status as u8);
        out.push(0);
        out.extend_from_slice(&(self.rows.len() as u16).to_le_bytes());
        out.extend_from_slice(&(self.sidecar.len() as u32).to_le_bytes());
        for row in &self.rows {
            if row.line_id == 0 || row.cells.len() > u32::MAX as usize {
                return Err(FramingError::MalformedPayload);
            }
            out.extend_from_slice(&row.line_id.to_le_bytes());
            out.extend_from_slice(&(row.cells.len() as u32).to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            for cell in &row.cells {
                out.extend_from_slice(&cell.scalar.to_le_bytes());
                out.extend_from_slice(&cell.foreground.to_le_bytes());
                out.extend_from_slice(&cell.background.to_le_bytes());
                out.extend_from_slice(&cell.flags.to_le_bytes());
                out.extend_from_slice(&cell.reserved.to_le_bytes());
            }
            if out.len().saturating_add(self.sidecar.len())
                > crate::framing::MAX_FRAME_PAYLOAD as usize
                || out.len().saturating_add(self.sidecar.len()) > MAX_HISTORY_RANGE_BYTES
            {
                return Err(FramingError::OversizedPayload);
            }
        }
        out.extend_from_slice(&self.sidecar);
        if out.len() > crate::framing::MAX_FRAME_PAYLOAD as usize
            || out.len() > MAX_HISTORY_RANGE_BYTES
        {
            return Err(FramingError::OversizedPayload);
        }
        Ok(out)
    }

    pub fn encode(&self) -> Vec<u8> {
        self.try_encode().expect("bounded history snapshot")
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        if bytes.len() > crate::framing::MAX_FRAME_PAYLOAD as usize
            || bytes.len() > MAX_HISTORY_RANGE_BYTES
        {
            return Err(FramingError::OversizedPayload);
        }
        if bytes.len() < Self::ENCODED_HEADER_LEN || bytes[25] != 0 {
            return Err(FramingError::MalformedPayload);
        }
        let sidecar_len = u32::from_le_bytes(bytes[28..32].try_into().unwrap()) as usize;
        if sidecar_len > MAX_HISTORY_SIDECAR_BYTES {
            return Err(FramingError::OversizedPayload);
        }
        let row_bytes_end = bytes
            .len()
            .checked_sub(sidecar_len)
            .ok_or(FramingError::MalformedPayload)?;
        if row_bytes_end < Self::ENCODED_HEADER_LEN {
            return Err(FramingError::TruncatedPayload);
        }
        let status = match bytes[24] {
            0 => HistoryRangeStatus::Complete,
            1 => HistoryRangeStatus::Truncated,
            2 => HistoryRangeStatus::Stale,
            3 => HistoryRangeStatus::Unsupported,
            _ => return Err(FramingError::MalformedPayload),
        };
        let count = u16::from_le_bytes(bytes[26..28].try_into().unwrap()) as usize;
        if count > MAX_HISTORY_RANGE_LINES {
            return Err(FramingError::MalformedPayload);
        }
        let mut offset = Self::ENCODED_HEADER_LEN;
        let mut total_cells = 0usize;
        let mut rows = Vec::with_capacity(count);
        for _ in 0..count {
            let end = offset
                .checked_add(16)
                .ok_or(FramingError::MalformedPayload)?;
            if end > row_bytes_end {
                return Err(FramingError::TruncatedPayload);
            }
            let line_id = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
            let cells =
                u32::from_le_bytes(bytes[offset + 8..offset + 12].try_into().unwrap()) as usize;
            if line_id == 0 || bytes[offset + 12..offset + 16] != [0; 4] {
                return Err(FramingError::MalformedPayload);
            }
            total_cells = total_cells
                .checked_add(cells)
                .ok_or(FramingError::OversizedPayload)?;
            if total_cells > MAX_HISTORY_RANGE_CELLS {
                return Err(FramingError::OversizedPayload);
            }
            let bytes_len = cells
                .checked_mul(16)
                .ok_or(FramingError::OversizedPayload)?;
            let cells_end = end
                .checked_add(bytes_len)
                .ok_or(FramingError::OversizedPayload)?;
            if cells_end > row_bytes_end {
                return Err(FramingError::TruncatedPayload);
            }
            let (cell_chunks, remainder) = bytes[end..cells_end].as_chunks::<16>();
            if !remainder.is_empty() {
                return Err(FramingError::MalformedPayload);
            }
            let values = cell_chunks
                .iter()
                .map(|chunk| {
                    Ok(HistoryCell {
                        scalar: u32::from_le_bytes(chunk[..4].try_into().unwrap()),
                        foreground: u32::from_le_bytes(chunk[4..8].try_into().unwrap()),
                        background: u32::from_le_bytes(chunk[8..12].try_into().unwrap()),
                        flags: u16::from_le_bytes(chunk[12..14].try_into().unwrap()),
                        reserved: u16::from_le_bytes(chunk[14..16].try_into().unwrap()),
                    })
                })
                .collect::<Result<Vec<_>, FramingError>>()?;
            rows.push(HistoryRow {
                line_id,
                cells: values,
            });
            offset = cells_end;
        }
        let sidecar_end = offset
            .checked_add(sidecar_len)
            .ok_or(FramingError::MalformedPayload)?;
        if offset != row_bytes_end || sidecar_end != bytes.len() {
            return Err(FramingError::ExactLengthMismatch);
        }
        let sidecar = bytes[offset..sidecar_end].to_vec();
        let snapshot = Self {
            request_id: u64::from_le_bytes(bytes[..8].try_into().unwrap()),
            block_id: u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
            revision: u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
            status,
            rows,
            sidecar,
        };
        for cell in snapshot.rows.iter().flat_map(|row| &row.cells) {
            cell.sidecar_utf8(&snapshot.sidecar)?;
        }
        Ok(snapshot)
    }
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

#[cfg(test)]
mod command_block_tests {
    use super::*;

    fn attachment() -> AttachmentId {
        AttachmentId::from_bytes(7u128.to_le_bytes())
    }

    #[test]
    fn composer_command_requires_exact_bounded_utf8_payload() {
        let request = ComposerCommandRef {
            attachment_id: attachment(),
            request_id: 3,
            command: "printf hello",
        };
        assert_eq!(ComposerCommandRef::decode(&request.encode()), Ok(request));
        let mut malformed = request.encode();
        malformed[24..28].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(
            ComposerCommandRef::decode(&malformed),
            Err(FramingError::MalformedPayload)
        );
    }

    #[test]
    fn timeline_round_trip_preserves_only_metadata_and_anchors() {
        let timeline = BlockTimeline {
            revision: 9,
            records: vec![
                CommandBlock {
                    id: 1,
                    command: "printf one".into(),
                    start_line: 31,
                    end_line: None,
                    state: CommandBlockState::Running,
                },
                CommandBlock {
                    id: 2,
                    command: "false".into(),
                    start_line: 34,
                    end_line: Some(36),
                    state: CommandBlockState::Completed {
                        exit_status: Some(1),
                    },
                },
            ],
        };
        assert_eq!(BlockTimeline::decode(&timeline.encode()), Ok(timeline));
    }

    #[test]
    fn timeline_state_tag_2_round_trips_completed_without_exit_status() {
        let timeline = BlockTimeline {
            revision: 11,
            records: vec![CommandBlock {
                id: 3,
                command: "sleep 1".into(),
                start_line: 40,
                end_line: Some(42),
                state: CommandBlockState::Completed { exit_status: None },
            }],
        };
        let encoded = timeline.encode();
        // Fixed record header: id(8)+start(8)+end(8)+tag(1)+pad(3)+exit(4)+cmd_len(2)+rsv(2).
        let record = &encoded[16..];
        assert_eq!(
            record[24], 2,
            "Completed {{ exit_status: None }} encodes as tag 2"
        );
        assert_eq!(
            i32::from_le_bytes(record[28..32].try_into().unwrap()),
            0,
            "tag-2 wire exit field stays zero"
        );
        assert_eq!(BlockTimeline::decode(&encoded), Ok(timeline));
    }

    #[test]
    fn timeline_state_tag_2_rejects_nonzero_exit_status_and_inverted_end_line() {
        let timeline = BlockTimeline {
            revision: 12,
            records: vec![CommandBlock {
                id: 4,
                command: "echo".into(),
                start_line: 50,
                end_line: Some(52),
                state: CommandBlockState::Completed { exit_status: None },
            }],
        };
        let good = timeline.encode();
        let record_offset = 16;

        let mut nonzero_exit = good.clone();
        nonzero_exit[record_offset + 28..record_offset + 32].copy_from_slice(&1i32.to_le_bytes());
        assert_eq!(
            BlockTimeline::decode(&nonzero_exit),
            Err(FramingError::MalformedPayload),
            "tag-2 with non-zero exit_status is malformed"
        );

        let mut inverted_end = good;
        // end_raw at record+16; start_line is 50, so end 49 is inverted.
        inverted_end[record_offset + 16..record_offset + 24].copy_from_slice(&49u64.to_le_bytes());
        assert_eq!(
            BlockTimeline::decode(&inverted_end),
            Err(FramingError::MalformedPayload),
            "tag-2 with end_raw < start_line is malformed"
        );
    }

    #[test]
    fn timeline_try_encode_rejects_unbounded_record_count() {
        let record = CommandBlock {
            id: 1,
            command: "echo".into(),
            start_line: 1,
            end_line: None,
            state: CommandBlockState::Running,
        };
        let timeline = BlockTimeline {
            revision: 1,
            records: vec![record; MAX_COMMAND_BLOCK_RECORDS + 1],
        };
        assert_eq!(timeline.try_encode(), Err(FramingError::OversizedPayload));
    }

    #[test]
    fn timeline_try_encode_rejects_cumulative_command_bytes_over_frame() {
        let large = "x".repeat(MAX_COMPOSER_COMMAND_BYTES);
        let timeline = BlockTimeline {
            revision: 1,
            records: (1..=16)
                .map(|id| CommandBlock {
                    id,
                    command: large.clone(),
                    start_line: id,
                    end_line: Some(id + 1),
                    state: CommandBlockState::Completed {
                        exit_status: Some(0),
                    },
                })
                .collect(),
        };
        assert!(timeline.records.len() <= MAX_COMMAND_BLOCK_RECORDS);
        assert_eq!(timeline.try_encode(), Err(FramingError::OversizedPayload));
    }

    #[test]
    fn composer_status_and_result_are_correlated_to_attachment() {
        let status = ComposerStatus {
            attachment_id: attachment(),
            eligibility: ComposerEligibility::Busy,
            revision: 12,
        };
        assert_eq!(ComposerStatus::decode(&status.encode()), Ok(status));
        let result = ComposerResult {
            attachment_id: attachment(),
            code: ComposerResultCode::Accepted,
            block_id: 44,
            request_id: 3,
        };
        assert_eq!(ComposerResult::decode(&result.encode()), Ok(result));
    }

    #[test]
    fn history_range_request_rejects_unbounded_or_reversed_ranges() {
        let request = HistoryRangeRequest {
            attachment_id: attachment(),
            request_id: 9,
            block_id: 44,
            start_line: 4,
            end_line: 8,
            max_lines: 32,
            max_cells: 4096,
            start_unit: 12,
        };
        assert_eq!(HistoryRangeRequest::decode(&request.encode()), Ok(request));
        let mut reversed = request.encode();
        reversed[40..48].copy_from_slice(&3u64.to_le_bytes());
        assert_eq!(
            HistoryRangeRequest::decode(&reversed),
            Err(FramingError::MalformedPayload)
        );
        let mut oversized = request.encode();
        oversized[48..50].copy_from_slice(&u16::MAX.to_le_bytes());
        assert_eq!(
            HistoryRangeRequest::decode(&oversized),
            Err(FramingError::MalformedPayload)
        );
    }

    #[test]
    fn history_snapshot_round_trip_is_bounded_and_keeps_rows_distinct() {
        let snapshot = HistoryRangeSnapshot {
            request_id: 9,
            block_id: 44,
            revision: 17,
            status: HistoryRangeStatus::Complete,
            rows: vec![
                HistoryRow {
                    line_id: 4,
                    cells: "café"
                        .chars()
                        .map(|c| HistoryCell {
                            scalar: c as u32,
                            foreground: 0x0200_00ff,
                            background: 0,
                            flags: 1,
                            reserved: 0,
                        })
                        .collect(),
                },
                HistoryRow {
                    line_id: 5,
                    cells: vec![
                        HistoryCell {
                            scalar: b' ' as u32,
                            foreground: 0,
                            background: 0,
                            flags: 0,
                            reserved: 0,
                        };
                        3
                    ],
                },
            ],
            sidecar: Vec::new(),
        };
        assert_eq!(
            HistoryRangeSnapshot::decode(&snapshot.encode()),
            Ok(snapshot)
        );
        let too_many = HistoryRangeSnapshot {
            request_id: 1,
            block_id: 1,
            revision: 1,
            status: HistoryRangeStatus::Complete,
            rows: vec![HistoryRow {
                line_id: 1,
                cells: vec![
                    HistoryCell {
                        scalar: 0,
                        foreground: 0,
                        background: 0,
                        flags: 0,
                        reserved: 0,
                    };
                    MAX_HISTORY_RANGE_CELLS + 1
                ],
            }],
            sidecar: Vec::new(),
        };
        assert_eq!(too_many.try_encode(), Err(FramingError::OversizedPayload));
    }

    #[test]
    fn history_admit_rows_truncates_before_wire_byte_budget() {
        fn full_row(line_id: u64, cols: usize) -> HistoryRow {
            HistoryRow {
                line_id,
                cells: vec![
                    HistoryCell {
                        scalar: b'x' as u32,
                        foreground: 0,
                        background: 0,
                        flags: 0,
                        reserved: 0,
                    };
                    cols
                ],
            }
        }

        // Production native requests max_lines=512 / max_cells=131072. At 80
        // columns, ~152 full-width rows exceed MAX_HISTORY_RANGE_BYTES while
        // still inside the line/cell admit limits — the P1 disconnect path.
        let dense: Vec<_> = (1..=200).map(|id| full_row(id, 80)).collect();
        let without_wire = HistoryRangeSnapshot {
            request_id: 1,
            block_id: 2,
            revision: 3,
            status: HistoryRangeStatus::Complete,
            rows: dense.clone(),
            sidecar: Vec::new(),
        };
        assert_eq!(
            without_wire.try_encode(),
            Err(FramingError::OversizedPayload)
        );

        let (admitted, truncated) = HistoryRangeSnapshot::admit_rows(
            dense,
            MAX_HISTORY_RANGE_LINES,
            MAX_HISTORY_RANGE_CELLS,
            0,
        );
        assert!(truncated);
        assert!(admitted.len() < 200);
        assert!(!admitted.is_empty());
        let snapshot = HistoryRangeSnapshot {
            request_id: 1,
            block_id: 2,
            revision: 3,
            status: HistoryRangeStatus::Truncated,
            rows: admitted,
            sidecar: Vec::new(),
        };
        let encoded = snapshot
            .try_encode()
            .expect("wire-admitted snapshot encodes");
        assert!(encoded.len() <= MAX_HISTORY_RANGE_BYTES);
        assert_eq!(HistoryRangeSnapshot::decode(&encoded), Ok(snapshot));
    }

    #[test]
    fn history_admit_marks_truncated_when_line_window_fills() {
        fn row(line_id: u64) -> HistoryRow {
            HistoryRow {
                line_id,
                cells: vec![HistoryCell {
                    scalar: b'x' as u32,
                    foreground: 0,
                    background: 0,
                    flags: 0,
                    reserved: 0,
                }],
            }
        }
        let rows: Vec<_> = (1..=5).map(row).collect();
        let (admitted, truncated) = HistoryRangeSnapshot::admit_rows(rows, 3, 4096, 0);
        assert!(truncated);
        assert_eq!(admitted.len(), 3);
    }

    #[test]
    fn history_snapshot_rejects_nonzero_cell_reserved() {
        let snapshot = HistoryRangeSnapshot {
            request_id: 1,
            block_id: 2,
            revision: 3,
            status: HistoryRangeStatus::Complete,
            rows: vec![HistoryRow {
                line_id: 1,
                cells: vec![HistoryCell {
                    scalar: b'x' as u32,
                    foreground: 0,
                    background: 0,
                    flags: 0,
                    reserved: 0,
                }],
            }],
            sidecar: Vec::new(),
        };
        let mut encoded = snapshot.encode();
        // scalar(4)+fg(4)+bg(4)+flags(2)+reserved(2) — flip reserved.
        // Sidecar is empty, so reserved sits at the last two payload bytes.
        let reserved_at = encoded.len() - 2;
        encoded[reserved_at] = 1;
        assert_eq!(
            HistoryRangeSnapshot::decode(&encoded),
            Err(FramingError::MalformedPayload)
        );
    }

    #[test]
    fn history_snapshot_round_trips_combining_grapheme_sidecar() {
        let mut sidecar = Vec::new();
        let cell =
            HistoryCell::from_text("e\u{301}", 0, 0, 0, &mut sidecar).expect("combining cell");
        assert_eq!(
            cell.flags & HISTORY_CELL_SIDECAR_FLAG,
            HISTORY_CELL_SIDECAR_FLAG
        );
        assert_eq!(
            cell.sidecar_utf8(&sidecar).expect("sidecar"),
            Some("e\u{301}".as_bytes())
        );
        let snapshot = HistoryRangeSnapshot {
            request_id: 11,
            block_id: 2,
            revision: 4,
            status: HistoryRangeStatus::Complete,
            rows: vec![HistoryRow {
                line_id: 1,
                cells: vec![cell],
            }],
            sidecar,
        };
        assert_eq!(
            HistoryRangeSnapshot::decode(&snapshot.encode()),
            Ok(snapshot)
        );
    }

    #[test]
    fn pack_source_rows_keeps_a_sidecar_prefix_when_the_row_exceeds_the_budget() {
        let grapheme = "\u{10000}".repeat(MAX_HISTORY_GRAPHEME_BYTES / 4);
        assert_eq!(grapheme.len(), MAX_HISTORY_GRAPHEME_BYTES);
        let cell = HistorySourceCell {
            text: grapheme,
            width: 2,
            continuation: false,
            foreground: 0,
            background: 0,
            style_flags: 0,
        };
        let continuation = HistorySourceCell {
            text: String::new(),
            width: 0,
            continuation: true,
            foreground: 0,
            background: 0,
            style_flags: 0,
        };
        let mut row = Vec::new();
        for _ in 0..8 {
            row.push(cell.clone());
            row.push(continuation.clone());
        }
        let (rows, sidecar, truncated) = HistoryRangeSnapshot::pack_source_rows([(1, row)]);
        assert!(truncated);
        assert_eq!(rows.len(), 1);
        let leads: Vec<_> = rows[0]
            .cells
            .iter()
            .filter(|cell| !cell.is_continuation())
            .collect();
        assert_eq!(leads.len(), 7);
        assert!(leads.iter().all(|cell| cell.cell_width() == 2));
        assert!(rows[0].cells.iter().any(|cell| cell.is_continuation()));
        HistoryRangeSnapshot {
            request_id: 1,
            block_id: 2,
            revision: 1,
            status: HistoryRangeStatus::Truncated,
            rows: rows.clone(),
            sidecar: sidecar.clone(),
        }
        .try_encode()
        .expect("packed prefix encodes");
        assert!(sidecar.len() <= MAX_HISTORY_SIDECAR_BYTES);
        assert!(!leads.is_empty());
    }

    #[test]
    fn truncated_sidecar_prefix_continues_with_start_unit_without_zero_progress() {
        let grapheme = "\u{10000}".repeat(MAX_HISTORY_GRAPHEME_BYTES / 4);
        let lead = HistorySourceCell {
            text: grapheme,
            width: 2,
            continuation: false,
            foreground: 0,
            background: 0,
            style_flags: 0,
        };
        let continuation = HistorySourceCell {
            text: String::new(),
            width: 0,
            continuation: true,
            foreground: 0,
            background: 0,
            style_flags: 0,
        };
        let mut source = Vec::new();
        for _ in 0..8 {
            source.push(lead.clone());
            source.push(continuation.clone());
        }

        let (first_rows, first_sidecar, first_truncated) =
            HistoryRangeSnapshot::pack_source_rows([(1, source.clone())]);
        assert!(first_truncated);
        let (first_rows, first_budget) = HistoryRangeSnapshot::admit_rows(
            first_rows,
            MAX_HISTORY_RANGE_LINES,
            MAX_HISTORY_RANGE_CELLS,
            first_sidecar.len(),
        );
        assert!(!first_budget || first_truncated);
        let first_leads = HistoryRangeSnapshot::lead_count(&first_rows);
        assert!(
            first_leads > 0,
            "truncated chunk must expose leads for start_unit"
        );
        let first = HistoryRangeSnapshot {
            request_id: 1,
            block_id: 2,
            revision: 1,
            status: HistoryRangeStatus::Truncated,
            rows: first_rows,
            sidecar: first_sidecar,
        };
        first.try_encode().expect("first truncated chunk encodes");

        let skip = first_leads as usize;
        let mut remaining = Vec::new();
        let mut seen_leads = 0usize;
        for cell in source {
            if cell.continuation {
                if seen_leads > skip {
                    remaining.push(cell);
                }
                continue;
            }
            let include = seen_leads >= skip;
            seen_leads += 1;
            if include {
                remaining.push(cell);
            }
        }
        assert!(!remaining.is_empty(), "unconsumed suffix must remain");

        let (second_rows, second_sidecar, second_truncated) =
            HistoryRangeSnapshot::pack_source_rows([(1, remaining)]);
        let (second_rows, _) = HistoryRangeSnapshot::admit_rows(
            second_rows,
            MAX_HISTORY_RANGE_LINES,
            MAX_HISTORY_RANGE_CELLS,
            second_sidecar.len(),
        );
        let second_leads = HistoryRangeSnapshot::lead_count(&second_rows);
        assert!(
            second_leads > 0,
            "continuation chunk must not be zero-progress"
        );
        let second = HistoryRangeSnapshot {
            request_id: 2,
            block_id: 2,
            revision: 1,
            status: if second_truncated {
                HistoryRangeStatus::Truncated
            } else {
                HistoryRangeStatus::Complete
            },
            rows: second_rows,
            sidecar: second_sidecar,
        };
        second.try_encode().expect("continuation chunk encodes");
        assert_eq!(first_leads + second_leads, 8);
    }

    #[test]
    fn admit_rows_reserves_sidecar_bytes_so_packed_prefix_encodes() {
        let grapheme = "\u{10000}".repeat(MAX_HISTORY_GRAPHEME_BYTES / 4);
        let lead = HistorySourceCell {
            text: grapheme,
            width: 2,
            continuation: false,
            foreground: 0,
            background: 0,
            style_flags: 0,
        };
        let continuation = HistorySourceCell {
            text: String::new(),
            width: 0,
            continuation: true,
            foreground: 0,
            background: 0,
            style_flags: 0,
        };
        let mut row = Vec::new();
        for _ in 0..8 {
            row.push(lead.clone());
            row.push(continuation.clone());
        }
        let (packed, sidecar, truncated) = HistoryRangeSnapshot::pack_source_rows([(1, row)]);
        assert!(truncated);
        assert!(!sidecar.is_empty());
        let (admitted, _) = HistoryRangeSnapshot::admit_rows(
            packed,
            MAX_HISTORY_RANGE_LINES,
            MAX_HISTORY_RANGE_CELLS,
            sidecar.len(),
        );
        assert!(!admitted.is_empty());
        assert!(HistoryRangeSnapshot::lead_count(&admitted) > 0);
        HistoryRangeSnapshot {
            request_id: 1,
            block_id: 2,
            revision: 1,
            status: HistoryRangeStatus::Truncated,
            rows: admitted,
            sidecar,
        }
        .try_encode()
        .expect("sidecar-aware admit must encode");
    }
}

pub const CAP_SEMANTIC_TERMINAL_KEY: u32 = 1 << 2;
pub const CAP_CORRELATED_RESIZE: u32 = 1 << 3;
pub const CAP_EXTENDED_TERMINAL_KEY: u32 = 1 << 7;

fn exact_len(bytes: &[u8], expected: usize) -> Result<(), FramingError> {
    if bytes.len() != expected {
        return Err(FramingError::ExactLengthMismatch);
    }
    Ok(())
}

fn attachment_id_from(bytes: &[u8]) -> AttachmentId {
    let mut raw = [0u8; 16];
    raw.copy_from_slice(&bytes[..16]);
    AttachmentId::from_bytes(raw)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum TerminalKeyKind {
    Enter = 1,
    Tab = 2,
    Backspace = 3,
    Escape = 4,
    ArrowUp = 5,
    ArrowDown = 6,
    ArrowRight = 7,
    ArrowLeft = 8,
    ControlAscii = 9,
}

impl TerminalKeyKind {
    fn from_u16(value: u16) -> Result<Self, FramingError> {
        match value {
            1 => Ok(Self::Enter),
            2 => Ok(Self::Tab),
            3 => Ok(Self::Backspace),
            4 => Ok(Self::Escape),
            5 => Ok(Self::ArrowUp),
            6 => Ok(Self::ArrowDown),
            7 => Ok(Self::ArrowRight),
            8 => Ok(Self::ArrowLeft),
            9 => Ok(Self::ControlAscii),
            _ => Err(FramingError::MalformedPayload),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalKeyModifiers(u16);

impl TerminalKeyModifiers {
    pub const NONE: Self = Self(0);
    pub const CONTROL: Self = Self(1 << 0);

    pub const fn bits(self) -> u16 {
        self.0
    }

    fn from_bits(bits: u16) -> Result<Self, FramingError> {
        match bits {
            0 => Ok(Self::NONE),
            1 => Ok(Self::CONTROL),
            _ => Err(FramingError::MalformedPayload),
        }
    }
}

fn valid_control_ascii_scalar(scalar: u32) -> bool {
    matches!(scalar, 0x20 | 0x3f | 0x40 | 0x41..=0x5f)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalKey {
    pub attachment_id: AttachmentId,
    pub kind: TerminalKeyKind,
    pub modifiers: TerminalKeyModifiers,
    pub scalar: u32,
}

impl TerminalKey {
    pub const WIRE_LEN: usize = 24;

    pub fn encode(&self) -> Vec<u8> {
        debug_assert!(self.validate().is_ok());
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        out.extend_from_slice(&self.attachment_id.to_bytes());
        out.extend_from_slice(&(self.kind as u16).to_le_bytes());
        out.extend_from_slice(&self.modifiers.bits().to_le_bytes());
        out.extend_from_slice(&self.scalar.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        let value = Self {
            attachment_id: attachment_id_from(&bytes[..16]),
            kind: TerminalKeyKind::from_u16(u16::from_le_bytes(bytes[16..18].try_into().unwrap()))?,
            modifiers: TerminalKeyModifiers::from_bits(u16::from_le_bytes(
                bytes[18..20].try_into().unwrap(),
            ))?,
            scalar: u32::from_le_bytes(bytes[20..24].try_into().unwrap()),
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), FramingError> {
        match self.kind {
            TerminalKeyKind::ControlAscii => {
                if self.modifiers != TerminalKeyModifiers::CONTROL
                    || !valid_control_ascii_scalar(self.scalar)
                {
                    return Err(FramingError::MalformedPayload);
                }
            }
            _ => {
                if self.modifiers != TerminalKeyModifiers::NONE || self.scalar != 0 {
                    return Err(FramingError::MalformedPayload);
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum TerminalKeyV2Kind {
    Enter = 1,
    Tab = 2,
    Backspace = 3,
    Escape = 4,
    ArrowUp = 5,
    ArrowDown = 6,
    ArrowRight = 7,
    ArrowLeft = 8,
    Home = 9,
    End = 10,
    Insert = 11,
    Delete = 12,
    PageUp = 13,
    PageDown = 14,
    Function = 15,
    Keypad = 16,
    Ascii = 17,
}

impl TerminalKeyV2Kind {
    fn from_u16(value: u16) -> Result<Self, FramingError> {
        Ok(match value {
            1 => Self::Enter,
            2 => Self::Tab,
            3 => Self::Backspace,
            4 => Self::Escape,
            5 => Self::ArrowUp,
            6 => Self::ArrowDown,
            7 => Self::ArrowRight,
            8 => Self::ArrowLeft,
            9 => Self::Home,
            10 => Self::End,
            11 => Self::Insert,
            12 => Self::Delete,
            13 => Self::PageUp,
            14 => Self::PageDown,
            15 => Self::Function,
            16 => Self::Keypad,
            17 => Self::Ascii,
            _ => return Err(FramingError::MalformedPayload),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalKeyV2Modifiers(u16);

impl TerminalKeyV2Modifiers {
    pub const NONE: Self = Self(0);
    pub const SHIFT: Self = Self(1 << 0);
    pub const ALT: Self = Self(1 << 1);
    pub const CONTROL: Self = Self(1 << 2);
    pub const ALT_SHIFT: Self = Self((1 << 0) | (1 << 1));
    pub const ALT_CONTROL: Self = Self((1 << 1) | (1 << 2));

    pub const fn bits(self) -> u16 {
        self.0
    }

    pub fn from_bits_for_ffi(bits: u16) -> Option<Self> {
        if bits & !0b111 != 0 {
            None
        } else {
            Some(Self(bits))
        }
    }

    fn from_bits(bits: u16) -> Result<Self, FramingError> {
        if bits & !0b111 != 0 {
            return Err(FramingError::MalformedPayload);
        }
        Ok(Self(bits))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TerminalKeyV2Event {
    Press = 1,
    Repeat = 2,
    Release = 3,
}

impl TerminalKeyV2Event {
    fn from_u8(value: u8) -> Result<Self, FramingError> {
        match value {
            1 => Ok(Self::Press),
            2 => Ok(Self::Repeat),
            3 => Ok(Self::Release),
            _ => Err(FramingError::MalformedPayload),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalKeyV2 {
    pub attachment_id: AttachmentId,
    pub kind: TerminalKeyV2Kind,
    pub modifiers: TerminalKeyV2Modifiers,
    pub value: u32,
    pub event: TerminalKeyV2Event,
    pub shifted_ascii: u32,
    pub action_id: u32,
}

impl TerminalKeyV2 {
    pub const WIRE_LEN: usize = 40;
    pub const VERSION: u16 = 2;

    pub fn encode(&self) -> Vec<u8> {
        debug_assert!(self.validate().is_ok());
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        out.extend_from_slice(&self.attachment_id.to_bytes());
        out.extend_from_slice(&(self.kind as u16).to_le_bytes());
        out.extend_from_slice(&self.modifiers.bits().to_le_bytes());
        out.extend_from_slice(&self.value.to_le_bytes());
        out.push(self.event as u8);
        out.push(0);
        out.extend_from_slice(&Self::VERSION.to_le_bytes());
        out.extend_from_slice(&self.shifted_ascii.to_le_bytes());
        out.extend_from_slice(&self.action_id.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        if bytes[25] != 0
            || u16::from_le_bytes(bytes[26..28].try_into().unwrap()) != Self::VERSION
            || u32::from_le_bytes(bytes[36..40].try_into().unwrap()) != 0
        {
            return Err(FramingError::MalformedPayload);
        }
        let value = Self {
            attachment_id: attachment_id_from(&bytes[..16]),
            kind: TerminalKeyV2Kind::from_u16(u16::from_le_bytes(
                bytes[16..18].try_into().unwrap(),
            ))?,
            modifiers: TerminalKeyV2Modifiers::from_bits(u16::from_le_bytes(
                bytes[18..20].try_into().unwrap(),
            ))?,
            value: u32::from_le_bytes(bytes[20..24].try_into().unwrap()),
            event: TerminalKeyV2Event::from_u8(bytes[24])?,
            shifted_ascii: u32::from_le_bytes(bytes[28..32].try_into().unwrap()),
            action_id: u32::from_le_bytes(bytes[32..36].try_into().unwrap()),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), FramingError> {
        if self.action_id == 0 {
            return Err(FramingError::MalformedPayload);
        }
        let modifiers = self.modifiers.bits();
        match self.kind {
            TerminalKeyV2Kind::Function => {
                if !(1..=12).contains(&self.value) || self.shifted_ascii != 0 {
                    return Err(FramingError::MalformedPayload);
                }
            }
            TerminalKeyV2Kind::Keypad => {
                if !(self.value <= 16) || self.shifted_ascii != 0 {
                    return Err(FramingError::MalformedPayload);
                }
            }
            TerminalKeyV2Kind::Ascii => {
                if !(0x20..=0x7e).contains(&self.value)
                    || (b'A' as u32..=b'Z' as u32).contains(&self.value)
                    || modifiers
                        & (TerminalKeyV2Modifiers::ALT.bits()
                            | TerminalKeyV2Modifiers::CONTROL.bits())
                        == 0
                {
                    return Err(FramingError::MalformedPayload);
                }
                if modifiers & TerminalKeyV2Modifiers::SHIFT.bits() != 0 {
                    if !(0x20..=0x7e).contains(&self.shifted_ascii) {
                        return Err(FramingError::MalformedPayload);
                    }
                } else if self.shifted_ascii != 0 {
                    return Err(FramingError::MalformedPayload);
                }
            }
            _ => {
                if self.value != 0 || self.shifted_ascii != 0 {
                    return Err(FramingError::MalformedPayload);
                }
            }
        }
        Ok(())
    }
}

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

/// Runtime→client primary viewport LineIds for one display generation
/// (`MessageType::ViewportLineIds` = 35), gated on `CAP_VIEWPORT_LINE_IDS`.
///
/// Wire: `generation(u64 LE)` + `row_count(u16 LE)` + `reserved(u16=0)` +
/// `row_count` little-endian `u64` LineIds. Length must equal the primary
/// viewport row count and every id must be non-zero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewportLineIds {
    pub generation: u64,
    pub line_ids: Vec<u64>,
}

impl ViewportLineIds {
    pub const HEADER_LEN: usize = 12;

    pub fn encode(&self) -> Vec<u8> {
        debug_assert!(self.validate().is_ok());
        let mut out = Vec::with_capacity(Self::HEADER_LEN + self.line_ids.len() * 8);
        out.extend_from_slice(&self.generation.to_le_bytes());
        out.extend_from_slice(&(self.line_ids.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        for id in &self.line_ids {
            out.extend_from_slice(&id.to_le_bytes());
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        if bytes.len() < Self::HEADER_LEN {
            return Err(FramingError::TruncatedPayload);
        }
        let generation = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
        let row_count = u16::from_le_bytes(bytes[8..10].try_into().unwrap());
        let reserved = u16::from_le_bytes(bytes[10..12].try_into().unwrap());
        if reserved != 0 {
            return Err(FramingError::MalformedPayload);
        }
        if row_count == 0 || row_count > MAX_DISPLAY_ROWS {
            return Err(FramingError::MalformedPayload);
        }
        let expected = Self::HEADER_LEN
            .checked_add(
                usize::from(row_count)
                    .checked_mul(8)
                    .ok_or(FramingError::LengthOverflow)?,
            )
            .ok_or(FramingError::LengthOverflow)?;
        if bytes.len() != expected {
            return Err(FramingError::ExactLengthMismatch);
        }
        let mut line_ids = Vec::with_capacity(usize::from(row_count));
        let mut offset = Self::HEADER_LEN;
        for _ in 0..row_count {
            let id = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
            if id == 0 {
                return Err(FramingError::MalformedPayload);
            }
            line_ids.push(id);
            offset += 8;
        }
        let message = Self {
            generation,
            line_ids,
        };
        message.validate()?;
        Ok(message)
    }

    fn validate(&self) -> Result<(), FramingError> {
        if self.generation == 0 {
            return Err(FramingError::MalformedPayload);
        }
        if self.line_ids.is_empty() || self.line_ids.len() > usize::from(MAX_DISPLAY_ROWS) {
            return Err(FramingError::MalformedPayload);
        }
        if self.line_ids.contains(&0) {
            return Err(FramingError::MalformedPayload);
        }
        // LineIds must be strictly increasing (primary scrollback order).
        if self.line_ids.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(FramingError::MalformedPayload);
        }
        Ok(())
    }
}

#[cfg(test)]
mod viewport_line_ids_tests {
    use super::*;

    #[test]
    fn viewport_line_ids_round_trip() {
        let message = ViewportLineIds {
            generation: 42,
            line_ids: vec![1, 2, 3, 10],
        };
        assert_eq!(ViewportLineIds::decode(&message.encode()).unwrap(), message);
    }

    #[test]
    fn viewport_line_ids_reject_zero_ids() {
        let mut encoded = ViewportLineIds {
            generation: 1,
            line_ids: vec![7, 8, 9],
        }
        .encode();
        // Overwrite the middle id with zero.
        encoded[12 + 8..12 + 16].copy_from_slice(&0u64.to_le_bytes());
        assert_eq!(
            ViewportLineIds::decode(&encoded),
            Err(FramingError::MalformedPayload)
        );
    }

    #[test]
    fn viewport_line_ids_reject_row_count_mismatch() {
        let mut encoded = ViewportLineIds {
            generation: 1,
            line_ids: vec![1, 2],
        }
        .encode();
        encoded[8..10].copy_from_slice(&3u16.to_le_bytes());
        assert_eq!(
            ViewportLineIds::decode(&encoded),
            Err(FramingError::ExactLengthMismatch)
        );
    }

    #[test]
    fn viewport_line_ids_reject_nonzero_reserved() {
        let mut encoded = ViewportLineIds {
            generation: 1,
            line_ids: vec![1],
        }
        .encode();
        encoded[10..12].copy_from_slice(&1u16.to_le_bytes());
        assert_eq!(
            ViewportLineIds::decode(&encoded),
            Err(FramingError::MalformedPayload)
        );
    }

    #[test]
    fn viewport_line_ids_reject_generation_zero() {
        let mut encoded = ViewportLineIds {
            generation: 1,
            line_ids: vec![1, 2],
        }
        .encode();
        encoded[0..8].copy_from_slice(&0u64.to_le_bytes());
        assert_eq!(
            ViewportLineIds::decode(&encoded),
            Err(FramingError::MalformedPayload)
        );
    }

    #[test]
    fn viewport_line_ids_reject_non_increasing() {
        let mut encoded = ViewportLineIds {
            generation: 1,
            line_ids: vec![1, 2, 3],
        }
        .encode();
        encoded[12 + 8..12 + 16].copy_from_slice(&1u64.to_le_bytes());
        assert_eq!(
            ViewportLineIds::decode(&encoded),
            Err(FramingError::MalformedPayload)
        );
    }
}
