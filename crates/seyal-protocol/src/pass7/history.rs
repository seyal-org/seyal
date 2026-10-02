//! Pass 7 history-range request/snapshot wire payloads.

use crate::{framing::FramingError, AttachmentId};

use super::{attachment_id_from, exact_len};

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
