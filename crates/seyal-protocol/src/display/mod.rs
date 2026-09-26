//! Candidate-D presentation-neutral display wire types, decoder and disposable client cache.
//!
//! Owns protocol/value validation only (no PTY, VT, Runtime, renderer, or UI state).
//! `DisplayCache` type and apply/decode live here; `seyal-client` owns the instance
//! and lifetime. Runtime may re-export the type for encode/publish — no second cache.

mod v1;
mod v2;

use std::sync::Arc;

use crate::framing::{FrameHeader, MessageType, HEADER_LEN};

pub use v1::decode_payload_v1;
pub use v2::{
    decode_payload_v2, encode_color, encode_v2_cell_meta, validate_v2_span, DisplayCellRole,
    DISPLAY_CHUNK_HEADER_V2_LEN, DISPLAY_SCHEMA_V2, MAX_GRAPHEME_SIDECAR_BYTES,
    MAX_GRAPHEME_UTF8_BYTES,
};

pub const DISPLAY_CHUNK_HEADER_LEN: usize = 40;
pub const DISPLAY_CELL_LEN: usize = 16;
pub const MAX_DISPLAY_ROWS: u16 = 256;
pub const MAX_DISPLAY_COLUMNS: u16 = 512;
pub const MAX_DISPLAY_CELLS: usize = 131_072;
pub const MAX_DISPLAY_BATCH_BYTES: usize = 4 * 1024 * 1024;
/// Upper bound for assembling one logical v2 update across transport batches.
pub const MAX_LOGICAL_DISPLAY_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayKind {
    Snapshot,
    Delta,
}

impl DisplayKind {
    pub fn message_type_v1(self) -> MessageType {
        match self {
            Self::Snapshot => MessageType::DisplaySnapshot,
            Self::Delta => MessageType::DisplayDelta,
        }
    }

    pub fn message_type_v2(self) -> MessageType {
        match self {
            Self::Snapshot => MessageType::DisplaySnapshotV2,
            Self::Delta => MessageType::DisplayDeltaV2,
        }
    }

    /// Backward-compatible alias used by M001 encode paths.
    pub fn message_type(self) -> MessageType {
        self.message_type_v1()
    }

    fn from_message_type(message_type: MessageType) -> Result<(Self, u16), DisplayError> {
        match message_type {
            MessageType::DisplaySnapshot => Ok((Self::Snapshot, 1)),
            MessageType::DisplayDelta => Ok((Self::Delta, 1)),
            MessageType::DisplaySnapshotV2 => Ok((Self::Snapshot, DISPLAY_SCHEMA_V2)),
            MessageType::DisplayDeltaV2 => Ok((Self::Delta, DISPLAY_SCHEMA_V2)),
            _ => Err(DisplayError::WrongMessageType),
        }
    }
}

#[derive(Clone, Debug)]
pub struct EncodedDisplayBatch {
    pub kind: DisplayKind,
    pub schema: u16,
    pub generation: u64,
    pub base_generation: u64,
    pub rows: u16,
    pub columns: u16,
    pub frames: Vec<Arc<[u8]>>,
    pub total_bytes: usize,
}

impl EncodedDisplayBatch {
    /// Split one logical update into contiguous replaceable transport batches.
    ///
    /// The wire chunk headers retain the logical `chunk_count` and indices, so
    /// a client can keep assembling the update across these transport units.
    /// Every encoded frame is already bounded by `MAX_FRAME_PAYLOAD`; the
    /// additional batch boundary is the SPEC-011 4 MiB queue/backpressure
    /// boundary.
    pub fn into_transport_batches(self) -> Vec<Self> {
        if self.total_bytes <= MAX_DISPLAY_BATCH_BYTES {
            return vec![self];
        }

        let EncodedDisplayBatch {
            kind,
            schema,
            generation,
            base_generation,
            rows,
            columns,
            frames,
            total_bytes: _,
        } = self;
        let mut fragments = Vec::new();
        let mut current_frames = Vec::new();
        let mut current_bytes = 0usize;

        for frame in frames {
            let frame_len = frame.len();
            debug_assert!(frame_len <= MAX_DISPLAY_BATCH_BYTES);
            if !current_frames.is_empty()
                && current_bytes.saturating_add(frame_len) > MAX_DISPLAY_BATCH_BYTES
            {
                fragments.push(Self {
                    kind,
                    schema,
                    generation,
                    base_generation,
                    rows,
                    columns,
                    frames: std::mem::take(&mut current_frames),
                    total_bytes: current_bytes,
                });
                current_bytes = 0;
            }
            current_bytes = current_bytes.saturating_add(frame_len);
            current_frames.push(frame);
        }
        if !current_frames.is_empty() {
            fragments.push(Self {
                kind,
                schema,
                generation,
                base_generation,
                rows,
                columns,
                frames: current_frames,
                total_bytes: current_bytes,
            });
        }
        fragments
    }
}

#[cfg(test)]
mod transport_tests {
    use super::*;

    #[test]
    fn logical_batch_fragments_into_bounded_contiguous_transport_batches() {
        let first = Arc::<[u8]>::from(vec![0u8; MAX_DISPLAY_BATCH_BYTES - 64]);
        let second = Arc::<[u8]>::from(vec![1u8; 128]);
        let third = Arc::<[u8]>::from(vec![2u8; MAX_DISPLAY_BATCH_BYTES - 128]);
        let batch = EncodedDisplayBatch {
            kind: DisplayKind::Snapshot,
            schema: DISPLAY_SCHEMA_V2,
            generation: 7,
            base_generation: 0,
            rows: 1,
            columns: 1,
            frames: vec![first.clone(), second.clone(), third.clone()],
            total_bytes: first.len() + second.len() + third.len(),
        };

        let fragments = batch.into_transport_batches();
        assert_eq!(fragments.len(), 2);
        assert!(fragments
            .iter()
            .all(|fragment| fragment.total_bytes <= MAX_DISPLAY_BATCH_BYTES));
        assert_eq!(fragments[0].frames, vec![first]);
        assert_eq!(fragments[1].frames, vec![second, third]);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayError {
    InvalidGeometry,
    InvalidCursor,
    InvalidDamage,
    InvalidChunk,
    InvalidLength,
    InvalidCell,
    InvalidColor,
    InvalidAttributes,
    InvalidUnicode,
    InvalidRole,
    InvalidWidth,
    InvalidSidecar,
    WrongMessageType,
    GenerationMismatch,
    DimensionMismatch,
    IncompleteBatch,
    BatchTooLarge,
    Overflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayColor {
    Default,
    Indexed(u8),
    Rgb { r: u8, g: u8, b: u8 },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DisplayAttributes {
    pub bold: bool,
    pub underline: bool,
    pub inverse: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayCell {
    /// First Unicode scalar for prepared/legacy paths; space for Empty/Continuation.
    pub scalar: char,
    pub role: DisplayCellRole,
    /// Terminal occupation: Lead uses 1 or 2; Empty/Continuation use 0.
    pub width: u8,
    /// Full grapheme UTF-8 for Lead; empty for Empty/Continuation.
    pub text: Arc<[u8]>,
    /// Whether `text` came from the v2 variable-length sidecar.
    pub sidecar: bool,
    pub foreground: DisplayColor,
    pub background: DisplayColor,
    pub attributes: DisplayAttributes,
}

impl DisplayCell {
    pub fn blank() -> Self {
        Self {
            scalar: ' ',
            role: DisplayCellRole::Empty,
            width: 0,
            text: Arc::from([]),
            sidecar: false,
            foreground: DisplayColor::Default,
            background: DisplayColor::Default,
            attributes: DisplayAttributes::default(),
        }
    }

    pub fn lead_scalar(
        scalar: char,
        width: u8,
        foreground: DisplayColor,
        background: DisplayColor,
        attributes: DisplayAttributes,
    ) -> Self {
        let mut buf = [0u8; 4];
        let encoded = scalar.encode_utf8(&mut buf);
        Self {
            scalar,
            role: DisplayCellRole::Lead,
            width,
            text: Arc::from(encoded.as_bytes().to_vec()),
            sidecar: false,
            foreground,
            background,
            attributes,
        }
    }
}

/// Sole disposable DisplayCache type; instance/lifetime owned by `seyal-client`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayCache {
    pub generation: u64,
    pub rows: u16,
    pub columns: u16,
    pub cursor_row: u16,
    pub cursor_col: u16,
    pub cursor_visible: bool,
    pub alternate_screen: bool,
    pub cells: Vec<DisplayCell>,
}

#[derive(Clone, Debug)]
pub struct DecodedDisplayChunk {
    pub kind: DisplayKind,
    pub schema: u16,
    pub generation: u64,
    pub base_generation: u64,
    pub rows: u16,
    pub columns: u16,
    pub cursor_row: u16,
    pub cursor_col: u16,
    pub cursor_visible: bool,
    pub alternate_screen: bool,
    pub first_row: u16,
    pub row_count: u16,
    pub first_col: u16,
    pub chunk_index: u16,
    pub chunk_count: u16,
    pub cells: Vec<DisplayCell>,
}

pub fn decode_chunk(frame: &[u8]) -> Result<DecodedDisplayChunk, DisplayError> {
    if frame.len() < HEADER_LEN {
        return Err(DisplayError::InvalidLength);
    }
    let header =
        FrameHeader::decode(&frame[..HEADER_LEN]).map_err(|_| DisplayError::InvalidLength)?;
    let expected = HEADER_LEN
        .checked_add(header.payload_len as usize)
        .ok_or(DisplayError::Overflow)?;
    if frame.len() != expected {
        return Err(DisplayError::InvalidLength);
    }
    let message_type =
        MessageType::from_u16(header.message_type).ok_or(DisplayError::WrongMessageType)?;
    let (kind, schema) = DisplayKind::from_message_type(message_type)?;
    if schema == DISPLAY_SCHEMA_V2 {
        decode_payload_v2(kind, &frame[HEADER_LEN..])
    } else {
        decode_payload_v1(kind, &frame[HEADER_LEN..])
    }
}

impl DisplayCache {
    pub fn apply_batch(&mut self, batch: &EncodedDisplayBatch) -> Result<(), DisplayError> {
        let chunks = batch
            .frames
            .iter()
            .map(|frame| decode_chunk(frame))
            .collect::<Result<Vec<_>, _>>()?;
        self.apply_chunks(&chunks)
    }

    pub fn apply_chunks(&mut self, chunks: &[DecodedDisplayChunk]) -> Result<(), DisplayError> {
        let first = chunks.first().ok_or(DisplayError::IncompleteBatch)?;
        if chunks.len() != first.chunk_count as usize {
            return Err(DisplayError::IncompleteBatch);
        }
        for (index, chunk) in chunks.iter().enumerate() {
            if chunk.kind != first.kind
                || chunk.schema != first.schema
                || chunk.generation != first.generation
                || chunk.base_generation != first.base_generation
                || chunk.rows != first.rows
                || chunk.columns != first.columns
                || chunk.cursor_row != first.cursor_row
                || chunk.cursor_col != first.cursor_col
                || chunk.cursor_visible != first.cursor_visible
                || chunk.alternate_screen != first.alternate_screen
                || chunk.chunk_count != first.chunk_count
                || chunk.chunk_index as usize != index
            {
                return Err(DisplayError::InvalidChunk);
            }
        }

        if first.schema == DISPLAY_SCHEMA_V2 {
            apply_chunks_v2(self, chunks, first)?;
        } else {
            apply_chunks_v1(self, chunks, first)?;
        }

        self.generation = first.generation;
        self.cursor_row = first.cursor_row;
        self.cursor_col = first.cursor_col;
        self.cursor_visible = first.cursor_visible;
        self.alternate_screen = first.alternate_screen;
        Ok(())
    }
}

fn apply_chunks_v1(
    cache: &mut DisplayCache,
    chunks: &[DecodedDisplayChunk],
    first: &DecodedDisplayChunk,
) -> Result<(), DisplayError> {
    let mut expected_row = if first.kind == DisplayKind::Snapshot {
        0
    } else {
        first.first_row
    };
    for chunk in chunks {
        if chunk.first_col != 0 || chunk.first_row != expected_row {
            return Err(DisplayError::InvalidChunk);
        }
        expected_row = expected_row
            .checked_add(chunk.row_count)
            .ok_or(DisplayError::Overflow)?;
    }
    if first.kind == DisplayKind::Snapshot && expected_row != first.rows {
        return Err(DisplayError::IncompleteBatch);
    }

    match first.kind {
        DisplayKind::Snapshot => {
            let expected_cells = (first.rows as usize)
                .checked_mul(first.columns as usize)
                .ok_or(DisplayError::Overflow)?;
            let mut cells = Vec::with_capacity(expected_cells);
            for chunk in chunks {
                cells.extend_from_slice(&chunk.cells);
            }
            if cells.len() != expected_cells {
                return Err(DisplayError::IncompleteBatch);
            }
            cache.cells = cells;
            cache.rows = first.rows;
            cache.columns = first.columns;
        }
        DisplayKind::Delta => {
            if cache.generation != first.base_generation {
                return Err(DisplayError::GenerationMismatch);
            }
            if cache.rows != first.rows || cache.columns != first.columns {
                return Err(DisplayError::DimensionMismatch);
            }
            for chunk in chunks {
                let first_cell = (chunk.first_row as usize)
                    .checked_mul(cache.columns as usize)
                    .ok_or(DisplayError::Overflow)?;
                let last_cell = first_cell
                    .checked_add(chunk.cells.len())
                    .ok_or(DisplayError::Overflow)?;
                if last_cell > cache.cells.len() {
                    return Err(DisplayError::InvalidChunk);
                }
                cache.cells[first_cell..last_cell].clone_from_slice(&chunk.cells);
            }
        }
    }
    Ok(())
}

fn apply_chunks_v2(
    cache: &mut DisplayCache,
    chunks: &[DecodedDisplayChunk],
    first: &DecodedDisplayChunk,
) -> Result<(), DisplayError> {
    // Chunks must cover required snapshot rows or delta damage spans without
    // gaps/overlap in row-major physical-cell order (§11.7).
    let mut cursor_row = 0u16;
    let mut cursor_col = 0u16;
    let mut started = false;

    match first.kind {
        DisplayKind::Snapshot => {
            let expected_cells = (first.rows as usize)
                .checked_mul(first.columns as usize)
                .ok_or(DisplayError::Overflow)?;
            let mut cells = vec![DisplayCell::blank(); expected_cells];
            for chunk in chunks {
                apply_span_to_grid(
                    &mut cells,
                    first.columns,
                    chunk,
                    &mut cursor_row,
                    &mut cursor_col,
                    &mut started,
                    true,
                )?;
            }
            if cursor_row != first.rows || cursor_col != 0 {
                return Err(DisplayError::IncompleteBatch);
            }
            cache.cells = cells;
            cache.rows = first.rows;
            cache.columns = first.columns;
        }
        DisplayKind::Delta => {
            if cache.generation != first.base_generation {
                return Err(DisplayError::GenerationMismatch);
            }
            if cache.rows != first.rows || cache.columns != first.columns {
                return Err(DisplayError::DimensionMismatch);
            }
            // Working copy so failure never partially mutates the cache.
            let mut cells = cache.cells.clone();
            for chunk in chunks {
                if !started {
                    cursor_row = chunk.first_row;
                    cursor_col = chunk.first_col;
                    started = true;
                }
                apply_span_to_grid(
                    &mut cells,
                    first.columns,
                    chunk,
                    &mut cursor_row,
                    &mut cursor_col,
                    &mut started,
                    false,
                )?;
            }
            cache.cells = cells;
        }
    }
    Ok(())
}

fn apply_span_to_grid(
    cells: &mut [DisplayCell],
    columns: u16,
    chunk: &DecodedDisplayChunk,
    cursor_row: &mut u16,
    cursor_col: &mut u16,
    started: &mut bool,
    require_contiguous_from_origin: bool,
) -> Result<(), DisplayError> {
    validate_v2_span(chunk.first_col, chunk.row_count, chunk.cells.len(), columns)?;

    if require_contiguous_from_origin {
        if !*started {
            if chunk.first_row != 0 || chunk.first_col != 0 {
                return Err(DisplayError::InvalidChunk);
            }
            *started = true;
        } else if chunk.first_row != *cursor_row || chunk.first_col != *cursor_col {
            return Err(DisplayError::InvalidChunk);
        }
    } else if *started && (chunk.first_row != *cursor_row || chunk.first_col != *cursor_col) {
        // Delta chunks continue in physical order after the previous span.
        return Err(DisplayError::InvalidChunk);
    } else if !*started {
        *cursor_row = chunk.first_row;
        *cursor_col = chunk.first_col;
        *started = true;
    }

    let columns_usize = columns as usize;
    let mut offset = 0usize;
    if chunk.row_count == 1 && !(chunk.first_col == 0 && chunk.cells.len() == columns_usize) {
        // Partial-row span.
        let row = chunk.first_row as usize;
        let start = row
            .checked_mul(columns_usize)
            .and_then(|base| base.checked_add(chunk.first_col as usize))
            .ok_or(DisplayError::Overflow)?;
        let end = start
            .checked_add(chunk.cells.len())
            .ok_or(DisplayError::Overflow)?;
        if end > cells.len() {
            return Err(DisplayError::InvalidChunk);
        }
        cells[start..end].clone_from_slice(&chunk.cells);
        *cursor_row = chunk.first_row;
        *cursor_col = chunk
            .first_col
            .checked_add(chunk.cells.len() as u16)
            .ok_or(DisplayError::Overflow)?;
        if *cursor_col == columns {
            *cursor_row = cursor_row.checked_add(1).ok_or(DisplayError::Overflow)?;
            *cursor_col = 0;
        }
    } else {
        // Whole-row span.
        for row_offset in 0..chunk.row_count {
            let row = chunk
                .first_row
                .checked_add(row_offset)
                .ok_or(DisplayError::Overflow)? as usize;
            let start = row
                .checked_mul(columns_usize)
                .ok_or(DisplayError::Overflow)?;
            let end = start
                .checked_add(columns_usize)
                .ok_or(DisplayError::Overflow)?;
            if end > cells.len() || offset + columns_usize > chunk.cells.len() {
                return Err(DisplayError::InvalidChunk);
            }
            cells[start..end].clone_from_slice(&chunk.cells[offset..offset + columns_usize]);
            offset += columns_usize;
        }
        *cursor_row = chunk
            .first_row
            .checked_add(chunk.row_count)
            .ok_or(DisplayError::Overflow)?;
        *cursor_col = 0;
    }
    Ok(())
}

pub fn empty_cache() -> DisplayCache {
    DisplayCache {
        generation: 0,
        rows: 0,
        columns: 0,
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: false,
        alternate_screen: false,
        cells: Vec::new(),
    }
}

pub(crate) fn validate_geometry(
    rows: u16,
    columns: u16,
    cursor_row: u16,
    cursor_col: u16,
) -> Result<(), DisplayError> {
    if rows == 0
        || columns == 0
        || rows > MAX_DISPLAY_ROWS
        || columns > MAX_DISPLAY_COLUMNS
        || cursor_row >= rows
        || cursor_col >= columns
    {
        return Err(DisplayError::InvalidGeometry);
    }
    let cells = (rows as usize)
        .checked_mul(columns as usize)
        .ok_or(DisplayError::Overflow)?;
    if cells > MAX_DISPLAY_CELLS {
        return Err(DisplayError::InvalidGeometry);
    }
    Ok(())
}

pub(crate) fn decode_color(value: u32) -> Result<DisplayColor, DisplayError> {
    let kind = value >> 30;
    let payload = value & 0x3fff_ffff;
    match kind {
        0b00 if payload == 0 => Ok(DisplayColor::Default),
        0b01 if payload <= 0xff => Ok(DisplayColor::Indexed(payload as u8)),
        0b10 if payload <= 0x00ff_ffff => Ok(DisplayColor::Rgb {
            r: ((payload >> 16) & 0xff) as u8,
            g: ((payload >> 8) & 0xff) as u8,
            b: (payload & 0xff) as u8,
        }),
        _ => Err(DisplayError::InvalidColor),
    }
}

pub(crate) fn decode_bool(value: u8) -> Result<bool, DisplayError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(DisplayError::InvalidChunk),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framing;

    fn encoded_cell_v1(scalar: char) -> [u8; DISPLAY_CELL_LEN] {
        let mut out = [0u8; DISPLAY_CELL_LEN];
        out[0..4].copy_from_slice(&(scalar as u32).to_le_bytes());
        out
    }

    #[test]
    fn one_row_snapshot_decodes_and_commits_atomically() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&7u64.to_le_bytes());
        payload.extend_from_slice(&0u64.to_le_bytes());
        payload.extend_from_slice(&1u16.to_le_bytes());
        payload.extend_from_slice(&2u16.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.extend_from_slice(&[1, 0, 0, 0]);
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.extend_from_slice(&1u16.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.extend_from_slice(&1u16.to_le_bytes());
        payload.extend_from_slice(&2u32.to_le_bytes());
        payload.extend_from_slice(&encoded_cell_v1('A'));
        payload.extend_from_slice(&encoded_cell_v1('B'));
        let frame = framing::encode_frame(MessageType::DisplaySnapshot, &payload);
        let chunk = decode_chunk(&frame).unwrap();
        let mut cache = empty_cache();
        cache.apply_chunks(&[chunk]).unwrap();
        assert_eq!(cache.generation, 7);
        assert_eq!((cache.rows, cache.columns), (1, 2));
        assert_eq!(cache.cells[0].scalar, 'A');
        assert_eq!(cache.cells[1].scalar, 'B');
    }

    #[test]
    fn v2_sidecar_round_trip_and_malformed_rejects() {
        let heart = "❤️".as_bytes();
        let mut sidecar = Vec::new();
        sidecar.extend_from_slice(heart);

        let mut cell = [0u8; DISPLAY_CELL_LEN];
        cell[0..4].copy_from_slice(&(0u32).to_le_bytes()); // text_ref offset 0
        let meta = encode_v2_cell_meta(
            DisplayCellRole::Lead,
            2,
            true,
            heart.len() as u16,
            DisplayAttributes::default(),
        );
        cell[12..16].copy_from_slice(&meta.to_le_bytes());

        let cont = {
            let mut c = [0u8; DISPLAY_CELL_LEN];
            let meta = encode_v2_cell_meta(
                DisplayCellRole::Continuation,
                0,
                false,
                0,
                DisplayAttributes::default(),
            );
            c[12..16].copy_from_slice(&meta.to_le_bytes());
            c
        };

        let mut payload = Vec::new();
        payload.extend_from_slice(&1u64.to_le_bytes());
        payload.extend_from_slice(&0u64.to_le_bytes());
        payload.extend_from_slice(&1u16.to_le_bytes());
        payload.extend_from_slice(&2u16.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.extend_from_slice(&[1, 0, 0, 0]);
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.extend_from_slice(&1u16.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.extend_from_slice(&1u16.to_le_bytes());
        payload.extend_from_slice(&2u32.to_le_bytes());
        payload.extend_from_slice(&(sidecar.len() as u32).to_le_bytes());
        payload.extend_from_slice(&DISPLAY_SCHEMA_V2.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.extend_from_slice(&cell);
        payload.extend_from_slice(&cont);
        payload.extend_from_slice(&sidecar);

        let frame = framing::encode_frame(MessageType::DisplaySnapshotV2, &payload);
        let chunk = decode_chunk(&frame).unwrap();
        assert_eq!(chunk.cells[0].role, DisplayCellRole::Lead);
        assert_eq!(chunk.cells[0].text.as_ref(), heart);
        assert!(chunk.cells[0].sidecar);
        assert_eq!(chunk.cells[1].role, DisplayCellRole::Continuation);
        assert!(chunk.cells[1].text.is_empty());

        let mut bad = payload.clone();
        // Corrupt sidecar length in header to exceed payload.
        bad[40..44].copy_from_slice(&65_537u32.to_le_bytes());
        let bad_frame = framing::encode_frame(MessageType::DisplaySnapshotV2, &bad);
        assert!(decode_chunk(&bad_frame).is_err());
    }

    #[test]
    fn v2_wide_lead_cannot_continue_across_row_boundary() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&1u64.to_le_bytes());
        payload.extend_from_slice(&0u64.to_le_bytes());
        payload.extend_from_slice(&2u16.to_le_bytes()); // rows
        payload.extend_from_slice(&1u16.to_le_bytes()); // columns
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.extend_from_slice(&[1, 0, 0, 0]);
        payload.extend_from_slice(&0u16.to_le_bytes()); // first row
        payload.extend_from_slice(&2u16.to_le_bytes()); // row count
        payload.extend_from_slice(&0u16.to_le_bytes()); // chunk index
        payload.extend_from_slice(&1u16.to_le_bytes()); // chunk count
        payload.extend_from_slice(&2u32.to_le_bytes()); // cell count
        payload.extend_from_slice(&0u32.to_le_bytes()); // sidecar len
        payload.extend_from_slice(&DISPLAY_SCHEMA_V2.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes()); // first col

        let mut lead = [0u8; DISPLAY_CELL_LEN];
        lead[0..4].copy_from_slice(&('A' as u32).to_le_bytes());
        let lead_meta = encode_v2_cell_meta(
            DisplayCellRole::Lead,
            2,
            false,
            0,
            DisplayAttributes::default(),
        );
        lead[12..16].copy_from_slice(&lead_meta.to_le_bytes());
        payload.extend_from_slice(&lead);

        let mut continuation = [0u8; DISPLAY_CELL_LEN];
        let continuation_meta = encode_v2_cell_meta(
            DisplayCellRole::Continuation,
            0,
            false,
            0,
            DisplayAttributes::default(),
        );
        continuation[12..16].copy_from_slice(&continuation_meta.to_le_bytes());
        payload.extend_from_slice(&continuation);

        let frame = framing::encode_frame(MessageType::DisplaySnapshotV2, &payload);
        assert!(matches!(
            decode_chunk(&frame),
            Err(DisplayError::InvalidCell)
        ));
    }

    #[test]
    fn v2_wide_continuation_inherits_lead_presentation() {
        let heart = "❤️".as_bytes();
        let mut sidecar = Vec::new();
        sidecar.extend_from_slice(heart);

        let mut payload = Vec::new();
        payload.extend_from_slice(&1u64.to_le_bytes());
        payload.extend_from_slice(&0u64.to_le_bytes());
        payload.extend_from_slice(&1u16.to_le_bytes());
        payload.extend_from_slice(&2u16.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.extend_from_slice(&[1, 0, 0, 0]);
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.extend_from_slice(&1u16.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.extend_from_slice(&1u16.to_le_bytes());
        payload.extend_from_slice(&2u32.to_le_bytes());
        payload.extend_from_slice(&(sidecar.len() as u32).to_le_bytes());
        payload.extend_from_slice(&DISPLAY_SCHEMA_V2.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes());

        let lead_attributes = DisplayAttributes {
            bold: true,
            underline: false,
            inverse: false,
        };
        let mut lead = [0u8; DISPLAY_CELL_LEN];
        lead[0..4].copy_from_slice(&0u32.to_le_bytes());
        lead[4..8].copy_from_slice(&encode_color(DisplayColor::Indexed(7)).to_le_bytes());
        lead[12..16].copy_from_slice(
            &encode_v2_cell_meta(
                DisplayCellRole::Lead,
                2,
                true,
                heart.len() as u16,
                lead_attributes,
            )
            .to_le_bytes(),
        );
        payload.extend_from_slice(&lead);

        let mut continuation = [0u8; DISPLAY_CELL_LEN];
        continuation[4..8].copy_from_slice(&encode_color(DisplayColor::Indexed(7)).to_le_bytes());
        continuation[12..16].copy_from_slice(
            &encode_v2_cell_meta(DisplayCellRole::Continuation, 0, false, 0, lead_attributes)
                .to_le_bytes(),
        );
        payload.extend_from_slice(&continuation);
        payload.extend_from_slice(&sidecar);

        let continuation_offset = DISPLAY_CHUNK_HEADER_V2_LEN + DISPLAY_CELL_LEN;
        let mut mismatched_color = payload.clone();
        mismatched_color[continuation_offset + 4..continuation_offset + 8]
            .copy_from_slice(&encode_color(DisplayColor::Indexed(8)).to_le_bytes());
        let mismatched_color_frame =
            framing::encode_frame(MessageType::DisplaySnapshotV2, &mismatched_color);
        let color_chunk = decode_chunk(&mismatched_color_frame).unwrap();
        assert_eq!(color_chunk.cells[1].role, DisplayCellRole::Continuation);
        assert_eq!(color_chunk.cells[1].foreground, DisplayColor::Indexed(7));
        assert_eq!(
            color_chunk.cells[1].background,
            color_chunk.cells[0].background
        );
        assert_eq!(color_chunk.cells[1].attributes, lead_attributes);

        let mut mismatched_attributes = payload;
        let continuation_meta = u32::from_le_bytes(
            mismatched_attributes[continuation_offset + 12..continuation_offset + 16]
                .try_into()
                .unwrap(),
        );
        mismatched_attributes[continuation_offset + 12..continuation_offset + 16]
            .copy_from_slice(&(continuation_meta & !1).to_le_bytes());
        let mismatched_attributes_frame =
            framing::encode_frame(MessageType::DisplaySnapshotV2, &mismatched_attributes);
        let attr_chunk = decode_chunk(&mismatched_attributes_frame).unwrap();
        assert_eq!(attr_chunk.cells[1].role, DisplayCellRole::Continuation);
        assert_eq!(attr_chunk.cells[1].attributes, lead_attributes);
        assert_eq!(attr_chunk.cells[1].foreground, DisplayColor::Indexed(7));
    }
}
