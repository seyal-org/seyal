//! Candidate-D display producer adapter owned by Runtime.
//!
//! Terminal authority remains in `TerminalExecution`. Wire schema, decoder, and the
//! sole `DisplayCache` type live in `seyal-protocol`; instance/lifetime belong to
//! `seyal-client`. This module re-exports that type and owns encode/publish only —
//! do not define a second cache here.

use std::sync::Arc;

#[cfg(feature = "benchmark-instrumentation")]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(test)]
use seyal_exec::ProjectionAttributes;
use seyal_exec::{
    CellRole, ProjectionCell, ProjectionColor, TerminalProjectionSnapshot, TerminalProjectionUpdate,
};
#[cfg(test)]
use seyal_protocol::framing::HEADER_LEN;
use seyal_protocol::framing::{self, MAX_FRAME_PAYLOAD};

pub use seyal_protocol::display::{
    decode_chunk, empty_cache, encode_color, encode_v2_cell_meta, DecodedDisplayChunk,
    DisplayAttributes, DisplayCache, DisplayCell, DisplayCellRole, DisplayColor, DisplayError,
    DisplayKind, EncodedDisplayBatch, DISPLAY_CELL_LEN, DISPLAY_CHUNK_HEADER_LEN,
    DISPLAY_CHUNK_HEADER_V2_LEN, DISPLAY_SCHEMA_V2, MAX_DISPLAY_BATCH_BYTES, MAX_DISPLAY_CELLS,
    MAX_DISPLAY_COLUMNS, MAX_DISPLAY_ROWS, MAX_GRAPHEME_SIDECAR_BYTES, MAX_GRAPHEME_UTF8_BYTES,
    MAX_LOGICAL_DISPLAY_BYTES,
};

#[cfg(feature = "benchmark-instrumentation")]
static BENCH_SNAPSHOT_ENCODES: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "benchmark-instrumentation")]
static BENCH_DELTA_ENCODES: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "benchmark-instrumentation")]
static BENCH_SNAPSHOT_BYTES: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "benchmark-instrumentation")]
static BENCH_DELTA_BYTES: AtomicU64 = AtomicU64::new(0);

#[cfg(feature = "benchmark-instrumentation")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BenchmarkDisplayCounters {
    pub snapshot_encodes: u64,
    pub delta_encodes: u64,
    pub snapshot_bytes: u64,
    pub delta_bytes: u64,
}

#[cfg(feature = "benchmark-instrumentation")]
pub fn reset_benchmark_display_counters() {
    BENCH_SNAPSHOT_ENCODES.store(0, Ordering::Relaxed);
    BENCH_DELTA_ENCODES.store(0, Ordering::Relaxed);
    BENCH_SNAPSHOT_BYTES.store(0, Ordering::Relaxed);
    BENCH_DELTA_BYTES.store(0, Ordering::Relaxed);
}

#[cfg(feature = "benchmark-instrumentation")]
pub fn benchmark_display_counters() -> BenchmarkDisplayCounters {
    BenchmarkDisplayCounters {
        snapshot_encodes: BENCH_SNAPSHOT_ENCODES.load(Ordering::Relaxed),
        delta_encodes: BENCH_DELTA_ENCODES.load(Ordering::Relaxed),
        snapshot_bytes: BENCH_SNAPSHOT_BYTES.load(Ordering::Relaxed),
        delta_bytes: BENCH_DELTA_BYTES.load(Ordering::Relaxed),
    }
}

#[derive(Clone, Copy)]
struct DisplayMeta {
    generation: u64,
    rows: u16,
    columns: u16,
    cursor_row: u16,
    cursor_col: u16,
    cursor_visible: bool,
    alternate_screen: bool,
}

pub fn encode_snapshot(
    snapshot: &TerminalProjectionSnapshot,
) -> Result<EncodedDisplayBatch, DisplayError> {
    validate_snapshot(snapshot)?;
    if !snapshot.is_scalar_lossless() {
        return Err(DisplayError::InvalidCell);
    }
    let batch = encode_rows(
        DisplayMeta {
            generation: snapshot.source_damage_generation,
            rows: snapshot.rows,
            columns: snapshot.columns,
            cursor_row: snapshot.cursor_row,
            cursor_col: snapshot.cursor_col,
            cursor_visible: snapshot.cursor_visible,
            alternate_screen: snapshot.alternate_screen,
        },
        DisplayKind::Snapshot,
        0,
        0,
        snapshot.rows,
        &snapshot.cells,
    )?;
    #[cfg(feature = "benchmark-instrumentation")]
    {
        BENCH_SNAPSHOT_ENCODES.fetch_add(1, Ordering::Relaxed);
        BENCH_SNAPSHOT_BYTES.fetch_add(batch.total_bytes as u64, Ordering::Relaxed);
    }
    Ok(batch)
}

pub fn encode_delta(
    update: &TerminalProjectionUpdate,
    base_generation: u64,
) -> Result<EncodedDisplayBatch, DisplayError> {
    validate_update(update)?;
    if !update.is_scalar_lossless() {
        return Err(DisplayError::InvalidCell);
    }
    let first_row = update.damage.first_row;
    let row_count = update.damage.row_count();
    let batch = encode_rows(
        DisplayMeta {
            generation: update.source_damage_generation,
            rows: update.rows,
            columns: update.columns,
            cursor_row: update.cursor_row,
            cursor_col: update.cursor_col,
            cursor_visible: update.cursor_visible,
            alternate_screen: update.alternate_screen,
        },
        DisplayKind::Delta,
        base_generation,
        first_row,
        row_count,
        &update.cells,
    )?;
    #[cfg(feature = "benchmark-instrumentation")]
    {
        BENCH_DELTA_ENCODES.fetch_add(1, Ordering::Relaxed);
        BENCH_DELTA_BYTES.fetch_add(batch.total_bytes as u64, Ordering::Relaxed);
    }
    Ok(batch)
}

pub fn encode_snapshot_v2(
    snapshot: &TerminalProjectionSnapshot,
) -> Result<EncodedDisplayBatch, DisplayError> {
    validate_snapshot(snapshot)?;
    let batch = encode_cells_v2(
        DisplayMeta {
            generation: snapshot.source_damage_generation,
            rows: snapshot.rows,
            columns: snapshot.columns,
            cursor_row: snapshot.cursor_row,
            cursor_col: snapshot.cursor_col,
            cursor_visible: snapshot.cursor_visible,
            alternate_screen: snapshot.alternate_screen,
        },
        DisplayKind::Snapshot,
        0,
        0,
        0,
        snapshot.rows,
        &snapshot.cells,
    )?;
    #[cfg(feature = "benchmark-instrumentation")]
    {
        BENCH_SNAPSHOT_ENCODES.fetch_add(1, Ordering::Relaxed);
        BENCH_SNAPSHOT_BYTES.fetch_add(batch.total_bytes as u64, Ordering::Relaxed);
    }
    Ok(batch)
}

pub fn encode_delta_v2(
    update: &TerminalProjectionUpdate,
    base_generation: u64,
) -> Result<EncodedDisplayBatch, DisplayError> {
    validate_update(update)?;
    let first_row = update.damage.first_row;
    let row_count = update.damage.row_count();
    let batch = encode_cells_v2(
        DisplayMeta {
            generation: update.source_damage_generation,
            rows: update.rows,
            columns: update.columns,
            cursor_row: update.cursor_row,
            cursor_col: update.cursor_col,
            cursor_visible: update.cursor_visible,
            alternate_screen: update.alternate_screen,
        },
        DisplayKind::Delta,
        base_generation,
        first_row,
        0,
        row_count,
        &update.cells,
    )?;
    #[cfg(feature = "benchmark-instrumentation")]
    {
        BENCH_DELTA_ENCODES.fetch_add(1, Ordering::Relaxed);
        BENCH_DELTA_BYTES.fetch_add(batch.total_bytes as u64, Ordering::Relaxed);
    }
    Ok(batch)
}

fn encode_rows(
    meta: DisplayMeta,
    kind: DisplayKind,
    base_generation: u64,
    first_row: u16,
    row_count: u16,
    cells: &[ProjectionCell],
) -> Result<EncodedDisplayBatch, DisplayError> {
    validate_geometry(meta.rows, meta.columns, meta.cursor_row, meta.cursor_col)?;
    if row_count == 0
        || first_row >= meta.rows
        || first_row as u32 + row_count as u32 > meta.rows as u32
    {
        return Err(DisplayError::InvalidDamage);
    }
    let expected_cells = (row_count as usize)
        .checked_mul(meta.columns as usize)
        .ok_or(DisplayError::Overflow)?;
    if cells.len() != expected_cells {
        return Err(DisplayError::InvalidDamage);
    }

    let bytes_per_row = (meta.columns as usize)
        .checked_mul(DISPLAY_CELL_LEN)
        .ok_or(DisplayError::Overflow)?;
    let available = (MAX_FRAME_PAYLOAD as usize)
        .checked_sub(DISPLAY_CHUNK_HEADER_LEN)
        .ok_or(DisplayError::Overflow)?;
    let rows_per_chunk = available / bytes_per_row;
    if rows_per_chunk == 0 {
        return Err(DisplayError::InvalidGeometry);
    }
    let chunk_count = (row_count as usize).div_ceil(rows_per_chunk);
    let chunk_count_u16 = u16::try_from(chunk_count).map_err(|_| DisplayError::Overflow)?;
    let mut frames = Vec::with_capacity(chunk_count);
    let mut total_bytes = 0usize;
    let mut emitted_rows = 0usize;

    for chunk_index in 0..chunk_count {
        let chunk_rows = rows_per_chunk.min(row_count as usize - emitted_rows);
        let chunk_first_row = first_row as usize + emitted_rows;
        let cell_count = chunk_rows
            .checked_mul(meta.columns as usize)
            .ok_or(DisplayError::Overflow)?;
        let payload_len = DISPLAY_CHUNK_HEADER_LEN
            .checked_add(
                cell_count
                    .checked_mul(DISPLAY_CELL_LEN)
                    .ok_or(DisplayError::Overflow)?,
            )
            .ok_or(DisplayError::Overflow)?;
        if payload_len > MAX_FRAME_PAYLOAD as usize {
            return Err(DisplayError::InvalidLength);
        }

        let mut payload = Vec::with_capacity(payload_len);
        payload.extend_from_slice(&meta.generation.to_le_bytes());
        payload.extend_from_slice(&base_generation.to_le_bytes());
        payload.extend_from_slice(&meta.rows.to_le_bytes());
        payload.extend_from_slice(&meta.columns.to_le_bytes());
        payload.extend_from_slice(&meta.cursor_row.to_le_bytes());
        payload.extend_from_slice(&meta.cursor_col.to_le_bytes());
        payload.push(meta.cursor_visible as u8);
        payload.push(meta.alternate_screen as u8);
        payload.push(0);
        payload.push(0);
        payload.extend_from_slice(&(chunk_first_row as u16).to_le_bytes());
        payload.extend_from_slice(&(chunk_rows as u16).to_le_bytes());
        payload.extend_from_slice(&(chunk_index as u16).to_le_bytes());
        payload.extend_from_slice(&chunk_count_u16.to_le_bytes());
        payload.extend_from_slice(&(cell_count as u32).to_le_bytes());

        let source_first = emitted_rows
            .checked_mul(meta.columns as usize)
            .ok_or(DisplayError::Overflow)?;
        let source_last = source_first
            .checked_add(cell_count)
            .ok_or(DisplayError::Overflow)?;
        for cell in &cells[source_first..source_last] {
            encode_projection_cell_v1(cell, &mut payload)?;
        }

        let frame = framing::encode_frame(kind.message_type_v1(), &payload);
        total_bytes = total_bytes
            .checked_add(frame.len())
            .ok_or(DisplayError::Overflow)?;
        if total_bytes > MAX_DISPLAY_BATCH_BYTES {
            return Err(DisplayError::BatchTooLarge);
        }
        frames.push(Arc::<[u8]>::from(frame));
        emitted_rows += chunk_rows;
    }

    Ok(EncodedDisplayBatch {
        kind,
        schema: 1,
        generation: meta.generation,
        base_generation,
        rows: meta.rows,
        columns: meta.columns,
        frames,
        total_bytes,
    })
}

fn encode_cells_v2(
    meta: DisplayMeta,
    kind: DisplayKind,
    base_generation: u64,
    first_row: u16,
    first_col: u16,
    row_count: u16,
    cells: &[ProjectionCell],
) -> Result<EncodedDisplayBatch, DisplayError> {
    validate_geometry(meta.rows, meta.columns, meta.cursor_row, meta.cursor_col)?;
    if row_count == 0
        || first_row >= meta.rows
        || first_row as u32 + row_count as u32 > meta.rows as u32
        || first_col >= meta.columns
    {
        return Err(DisplayError::InvalidDamage);
    }
    let expected_cells = (row_count as usize)
        .checked_mul(meta.columns as usize)
        .ok_or(DisplayError::Overflow)?;
    if cells.len() != expected_cells || first_col != 0 {
        // Encoder currently packs full damaged rows starting at first_col=0.
        if first_col != 0 {
            return Err(DisplayError::InvalidDamage);
        }
        if cells.len() != expected_cells {
            return Err(DisplayError::InvalidDamage);
        }
    }

    // Plan spans greedily: whole rows when sidecar fits, else partial-row cuts
    // that never split a lead from its continuation.
    let spans = plan_v2_spans(meta.columns, cells)?;
    let chunk_count = spans.len();
    let chunk_count_u16 = u16::try_from(chunk_count).map_err(|_| DisplayError::Overflow)?;
    let mut frames = Vec::with_capacity(chunk_count);
    let mut total_bytes = 0usize;

    for (chunk_index, span) in spans.iter().enumerate() {
        let absolute_row = first_row
            .checked_add(span.row_offset)
            .ok_or(DisplayError::Overflow)?;
        let cell_slice = &cells[span.cell_start..span.cell_start + span.cell_count];
        let (payload, _) = encode_v2_chunk_payload(
            meta,
            base_generation,
            V2ChunkSpan {
                first_row: absolute_row,
                row_count: span.row_count,
                first_col: span.first_col,
                chunk_index: chunk_index as u16,
                chunk_count: chunk_count_u16,
            },
            cell_slice,
        )?;
        let frame = framing::encode_frame(kind.message_type_v2(), &payload);
        total_bytes = total_bytes
            .checked_add(frame.len())
            .ok_or(DisplayError::Overflow)?;
        frames.push(Arc::<[u8]>::from(frame));
    }

    Ok(EncodedDisplayBatch {
        kind,
        schema: DISPLAY_SCHEMA_V2,
        generation: meta.generation,
        base_generation,
        rows: meta.rows,
        columns: meta.columns,
        frames,
        total_bytes,
    })
}

#[derive(Clone, Copy)]
struct SpanPlan {
    row_offset: u16,
    first_col: u16,
    row_count: u16,
    cell_start: usize,
    cell_count: usize,
}

fn plan_v2_spans(columns: u16, cells: &[ProjectionCell]) -> Result<Vec<SpanPlan>, DisplayError> {
    let columns_usize = columns as usize;
    if !cells.len().is_multiple_of(columns_usize) {
        return Err(DisplayError::InvalidDamage);
    }
    let row_count = cells.len() / columns_usize;
    let mut spans = Vec::new();
    let mut row = 0usize;
    while row < row_count {
        // Try to pack as many whole rows as fit in one frame with sidecar budget.
        let mut rows_taken = 0usize;
        let mut sidecar_bytes = 0usize;
        while row + rows_taken < row_count {
            let row_cells =
                &cells[(row + rows_taken) * columns_usize..(row + rows_taken + 1) * columns_usize];
            let row_sidecar = sidecar_bytes_for_cells(row_cells)?;
            let next_sidecar = sidecar_bytes
                .checked_add(row_sidecar)
                .ok_or(DisplayError::Overflow)?;
            let next_cells = (rows_taken + 1)
                .checked_mul(columns_usize)
                .ok_or(DisplayError::Overflow)?;
            let payload = DISPLAY_CHUNK_HEADER_V2_LEN
                .checked_add(
                    next_cells
                        .checked_mul(DISPLAY_CELL_LEN)
                        .ok_or(DisplayError::Overflow)?,
                )
                .and_then(|v| v.checked_add(next_sidecar))
                .ok_or(DisplayError::Overflow)?;
            if next_sidecar > MAX_GRAPHEME_SIDECAR_BYTES || payload > MAX_FRAME_PAYLOAD as usize {
                break;
            }
            sidecar_bytes = next_sidecar;
            rows_taken += 1;
        }

        if rows_taken > 0 {
            spans.push(SpanPlan {
                row_offset: row as u16,
                first_col: 0,
                row_count: rows_taken as u16,
                cell_start: row * columns_usize,
                cell_count: rows_taken * columns_usize,
            });
            row += rows_taken;
            continue;
        }

        // Single row does not fit whole — emit partial-row spans.
        let row_cells = &cells[row * columns_usize..(row + 1) * columns_usize];
        let mut col = 0usize;
        while col < columns_usize {
            let mut end = col;
            let mut sidecar_bytes = 0usize;
            while end < columns_usize {
                // Never end between lead and continuation.
                if end > col && row_cells[end].role == CellRole::Continuation {
                    break;
                }
                let unit_end = if row_cells[end].role == CellRole::Lead && row_cells[end].width == 2
                {
                    if end + 1 >= columns_usize || row_cells[end + 1].role != CellRole::Continuation
                    {
                        return Err(DisplayError::InvalidCell);
                    }
                    end + 2
                } else {
                    end + 1
                };
                let unit = &row_cells[end..unit_end];
                let unit_sidecar = sidecar_bytes_for_cells(unit)?;
                let next_sidecar = sidecar_bytes
                    .checked_add(unit_sidecar)
                    .ok_or(DisplayError::Overflow)?;
                let next_cells = unit_end - col;
                let payload = DISPLAY_CHUNK_HEADER_V2_LEN
                    .checked_add(
                        next_cells
                            .checked_mul(DISPLAY_CELL_LEN)
                            .ok_or(DisplayError::Overflow)?,
                    )
                    .and_then(|v| v.checked_add(next_sidecar))
                    .ok_or(DisplayError::Overflow)?;
                if next_sidecar > MAX_GRAPHEME_SIDECAR_BYTES || payload > MAX_FRAME_PAYLOAD as usize
                {
                    break;
                }
                sidecar_bytes = next_sidecar;
                end = unit_end;
            }
            if end == col {
                return Err(DisplayError::InvalidLength);
            }
            spans.push(SpanPlan {
                row_offset: row as u16,
                first_col: col as u16,
                row_count: 1,
                cell_start: row * columns_usize + col,
                cell_count: end - col,
            });
            col = end;
        }
        row += 1;
    }
    if spans.is_empty() {
        return Err(DisplayError::InvalidDamage);
    }
    Ok(spans)
}

fn sidecar_bytes_for_cells(cells: &[ProjectionCell]) -> Result<usize, DisplayError> {
    let mut total = 0usize;
    for cell in cells {
        if cell.role == CellRole::Lead && needs_sidecar(cell)? {
            total = total
                .checked_add(cell.text.len())
                .ok_or(DisplayError::Overflow)?;
        }
    }
    Ok(total)
}

fn needs_sidecar(cell: &ProjectionCell) -> Result<bool, DisplayError> {
    if cell.role != CellRole::Lead {
        return Ok(false);
    }
    if cell.text.is_empty() || cell.text.len() > MAX_GRAPHEME_UTF8_BYTES {
        return Err(DisplayError::InvalidSidecar);
    }
    let chars = std::str::from_utf8(&cell.text)
        .map_err(|_| DisplayError::InvalidUnicode)?
        .chars()
        .count();
    Ok(chars != 1)
}

#[derive(Clone, Copy)]
struct V2ChunkSpan {
    first_row: u16,
    row_count: u16,
    first_col: u16,
    chunk_index: u16,
    chunk_count: u16,
}

fn encode_v2_chunk_payload(
    meta: DisplayMeta,
    base_generation: u64,
    span: V2ChunkSpan,
    cells: &[ProjectionCell],
) -> Result<(Vec<u8>, usize), DisplayError> {
    let mut sidecar = Vec::new();
    let mut cell_bytes = Vec::with_capacity(cells.len() * DISPLAY_CELL_LEN);
    for cell in cells {
        encode_projection_cell_v2(cell, &mut cell_bytes, &mut sidecar)?;
    }
    if sidecar.len() > MAX_GRAPHEME_SIDECAR_BYTES {
        return Err(DisplayError::InvalidSidecar);
    }
    let payload_len = DISPLAY_CHUNK_HEADER_V2_LEN
        .checked_add(cell_bytes.len())
        .and_then(|v| v.checked_add(sidecar.len()))
        .ok_or(DisplayError::Overflow)?;
    if payload_len > MAX_FRAME_PAYLOAD as usize {
        return Err(DisplayError::InvalidLength);
    }

    let mut payload = Vec::with_capacity(payload_len);
    payload.extend_from_slice(&meta.generation.to_le_bytes());
    payload.extend_from_slice(&base_generation.to_le_bytes());
    payload.extend_from_slice(&meta.rows.to_le_bytes());
    payload.extend_from_slice(&meta.columns.to_le_bytes());
    payload.extend_from_slice(&meta.cursor_row.to_le_bytes());
    payload.extend_from_slice(&meta.cursor_col.to_le_bytes());
    payload.push(meta.cursor_visible as u8);
    payload.push(meta.alternate_screen as u8);
    payload.push(0);
    payload.push(0);
    payload.extend_from_slice(&span.first_row.to_le_bytes());
    payload.extend_from_slice(&span.row_count.to_le_bytes());
    payload.extend_from_slice(&span.chunk_index.to_le_bytes());
    payload.extend_from_slice(&span.chunk_count.to_le_bytes());
    payload.extend_from_slice(&(cells.len() as u32).to_le_bytes());
    payload.extend_from_slice(&(sidecar.len() as u32).to_le_bytes());
    payload.extend_from_slice(&DISPLAY_SCHEMA_V2.to_le_bytes());
    payload.extend_from_slice(&span.first_col.to_le_bytes());
    payload.extend_from_slice(&cell_bytes);
    payload.extend_from_slice(&sidecar);
    Ok((payload, sidecar.len()))
}

fn encode_projection_cell_v1(cell: &ProjectionCell, out: &mut Vec<u8>) -> Result<(), DisplayError> {
    if !cell.is_scalar_lossless() {
        return Err(DisplayError::InvalidCell);
    }
    out.extend_from_slice(&(cell.scalar as u32).to_le_bytes());
    out.extend_from_slice(&encode_color(proj_color(cell.foreground)).to_le_bytes());
    out.extend_from_slice(&encode_color(proj_color(cell.background)).to_le_bytes());
    let attributes = (cell.attributes.bold as u16)
        | ((cell.attributes.underline as u16) << 1)
        | ((cell.attributes.inverse as u16) << 2);
    out.extend_from_slice(&attributes.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    Ok(())
}

fn encode_projection_cell_v2(
    cell: &ProjectionCell,
    out: &mut Vec<u8>,
    sidecar: &mut Vec<u8>,
) -> Result<(), DisplayError> {
    let role = match cell.role {
        CellRole::Empty => DisplayCellRole::Empty,
        CellRole::Lead => DisplayCellRole::Lead,
        CellRole::Continuation => DisplayCellRole::Continuation,
    };
    let attrs = DisplayAttributes {
        bold: cell.attributes.bold,
        underline: cell.attributes.underline,
        inverse: cell.attributes.inverse,
    };
    let (text_ref, meta) = match role {
        DisplayCellRole::Empty => {
            if cell.width != 0 || !cell.text.is_empty() {
                return Err(DisplayError::InvalidCell);
            }
            (0u32, encode_v2_cell_meta(role, 0, false, 0, attrs))
        }
        DisplayCellRole::Continuation => {
            if cell.width != 0 || !cell.text.is_empty() {
                return Err(DisplayError::InvalidCell);
            }
            (0u32, encode_v2_cell_meta(role, 0, false, 0, attrs))
        }
        DisplayCellRole::Lead => {
            if cell.width != 1 && cell.width != 2 {
                return Err(DisplayError::InvalidWidth);
            }
            if needs_sidecar(cell)? {
                let offset = sidecar.len();
                if cell.text.len() > MAX_GRAPHEME_UTF8_BYTES {
                    return Err(DisplayError::InvalidSidecar);
                }
                sidecar.extend_from_slice(&cell.text);
                (
                    offset as u32,
                    encode_v2_cell_meta(role, cell.width, true, cell.text.len() as u16, attrs),
                )
            } else {
                (
                    cell.scalar as u32,
                    encode_v2_cell_meta(role, cell.width, false, 0, attrs),
                )
            }
        }
    };
    out.extend_from_slice(&text_ref.to_le_bytes());
    out.extend_from_slice(&encode_color(proj_color(cell.foreground)).to_le_bytes());
    out.extend_from_slice(&encode_color(proj_color(cell.background)).to_le_bytes());
    out.extend_from_slice(&meta.to_le_bytes());
    Ok(())
}

fn proj_color(color: ProjectionColor) -> DisplayColor {
    match color {
        ProjectionColor::Default => DisplayColor::Default,
        ProjectionColor::Indexed(index) => DisplayColor::Indexed(index),
        ProjectionColor::Rgb { r, g, b } => DisplayColor::Rgb { r, g, b },
    }
}

fn validate_snapshot(snapshot: &TerminalProjectionSnapshot) -> Result<(), DisplayError> {
    validate_geometry(
        snapshot.rows,
        snapshot.columns,
        snapshot.cursor_row,
        snapshot.cursor_col,
    )?;
    let expected = (snapshot.rows as usize)
        .checked_mul(snapshot.columns as usize)
        .ok_or(DisplayError::Overflow)?;
    if expected > MAX_DISPLAY_CELLS || snapshot.cells.len() != expected {
        return Err(DisplayError::InvalidGeometry);
    }
    Ok(())
}

fn validate_update(update: &TerminalProjectionUpdate) -> Result<(), DisplayError> {
    validate_geometry(
        update.rows,
        update.columns,
        update.cursor_row,
        update.cursor_col,
    )?;
    if update.damage.first_row > update.damage.last_row || update.damage.last_row >= update.rows {
        return Err(DisplayError::InvalidDamage);
    }
    if update.damage.full
        && (update.damage.first_row != 0 || update.damage.last_row != update.rows.saturating_sub(1))
    {
        return Err(DisplayError::InvalidDamage);
    }
    let expected = (update.damage.row_count() as usize)
        .checked_mul(update.columns as usize)
        .ok_or(DisplayError::Overflow)?;
    if expected > MAX_DISPLAY_CELLS || update.cells.len() != expected {
        return Err(DisplayError::InvalidDamage);
    }
    Ok(())
}

fn validate_geometry(
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

#[cfg(test)]
mod tests {
    use super::*;
    use seyal_exec::{ProjectionDamage, TerminalProjectionUpdate};
    use seyal_protocol::display::DisplayCellRole;

    fn cell(index: usize) -> ProjectionCell {
        let scalar = char::from_u32(b'a' as u32 + (index % 26) as u32).unwrap();
        let mut buf = [0u8; 4];
        let encoded = scalar.encode_utf8(&mut buf);
        ProjectionCell {
            role: CellRole::Lead,
            width: 1,
            text: Arc::from(encoded.as_bytes().to_vec()),
            scalar,
            foreground: ProjectionColor::Default,
            background: ProjectionColor::Default,
            attributes: ProjectionAttributes::default(),
        }
    }

    fn sample_snapshot(rows: u16, columns: u16, generation: u64) -> TerminalProjectionSnapshot {
        TerminalProjectionSnapshot {
            rows,
            columns,
            cursor_row: 0,
            cursor_col: 0,
            cursor_visible: true,
            alternate_screen: false,
            source_damage_generation: generation,
            damage: ProjectionDamage::full(rows),
            cells: (0..rows as usize * columns as usize).map(cell).collect(),
        }
    }

    fn sample_update(
        rows: u16,
        columns: u16,
        generation: u64,
        damage: ProjectionDamage,
    ) -> TerminalProjectionUpdate {
        TerminalProjectionUpdate {
            rows,
            columns,
            cursor_row: 0,
            cursor_col: 0,
            cursor_visible: true,
            alternate_screen: false,
            source_damage_generation: generation,
            damage,
            cells: (0..damage.row_count() as usize * columns as usize)
                .map(cell)
                .collect(),
        }
    }

    #[test]
    fn snapshot_round_trip_rebuilds_disposable_cache() {
        let snapshot = sample_snapshot(24, 80, 7);
        let batch = encode_snapshot(&snapshot).unwrap();
        assert_eq!(batch.schema, 1);
        let mut cache = empty_cache();
        cache.apply_batch(&batch).unwrap();
        assert_eq!(cache.generation, 7);
        assert_eq!((cache.rows, cache.columns), (24, 80));
        assert_eq!(cache.cells.len(), 24 * 80);
    }

    #[test]
    fn large_snapshot_is_chunked_under_frame_limit() {
        let snapshot = sample_snapshot(256, 512, 8);
        let batch = encode_snapshot(&snapshot).unwrap();
        assert!(batch.frames.len() > 1);
        assert!(batch
            .frames
            .iter()
            .all(|frame| frame.len() <= HEADER_LEN + MAX_FRAME_PAYLOAD as usize));
        let mut cache = empty_cache();
        cache.apply_batch(&batch).unwrap();
        assert_eq!(cache.cells.len(), 131_072);
    }

    #[test]
    fn delta_carries_only_projection_update_cells() {
        let initial = sample_snapshot(24, 80, 10);
        let mut cache = empty_cache();
        cache
            .apply_batch(&encode_snapshot(&initial).unwrap())
            .unwrap();

        let damage = ProjectionDamage {
            full: false,
            first_row: 10,
            last_row: 11,
        };
        let mut update = sample_update(24, 80, 11, damage);
        let scalar = 'Z';
        let mut buf = [0u8; 4];
        let encoded = scalar.encode_utf8(&mut buf);
        update.cells[0] = ProjectionCell {
            role: CellRole::Lead,
            width: 1,
            text: Arc::from(encoded.as_bytes().to_vec()),
            scalar,
            foreground: ProjectionColor::Default,
            background: ProjectionColor::Default,
            attributes: ProjectionAttributes::default(),
        };
        let delta = encode_delta(&update, 10).unwrap();
        let decoded = decode_chunk(&delta.frames[0]).unwrap();
        assert_eq!((decoded.first_row, decoded.row_count), (10, 2));
        cache.apply_batch(&delta).unwrap();
        assert_eq!(cache.generation, 11);
        assert_eq!(cache.cells[10 * 80].scalar, 'Z');
    }

    #[test]
    fn delta_generation_gap_is_rejected_without_partial_commit() {
        let snapshot = sample_snapshot(24, 80, 3);
        let mut cache = empty_cache();
        cache
            .apply_batch(&encode_snapshot(&snapshot).unwrap())
            .unwrap();
        let update = sample_update(
            24,
            80,
            5,
            ProjectionDamage {
                full: false,
                first_row: 0,
                last_row: 0,
            },
        );
        let delta = encode_delta(&update, 4).unwrap();
        let before = cache.clone();
        assert_eq!(
            cache.apply_batch(&delta),
            Err(DisplayError::GenerationMismatch)
        );
        assert_eq!(cache, before);
    }

    #[test]
    fn incomplete_chunked_snapshot_does_not_mutate_cache() {
        let snapshot = sample_snapshot(256, 512, 9);
        let batch = encode_snapshot(&snapshot).unwrap();
        let chunks = batch
            .frames
            .iter()
            .map(|frame| decode_chunk(frame))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let mut cache = empty_cache();
        let before = cache.clone();
        assert_eq!(
            cache.apply_chunks(&chunks[..chunks.len() - 1]),
            Err(DisplayError::IncompleteBatch)
        );
        assert_eq!(cache, before);
    }

    #[test]
    fn incomplete_multi_chunk_delta_does_not_partially_mutate_cache() {
        let snapshot = sample_snapshot(256, 512, 20);
        let mut cache = empty_cache();
        cache
            .apply_batch(&encode_snapshot(&snapshot).unwrap())
            .unwrap();
        let mut update = sample_update(256, 512, 21, ProjectionDamage::full(256));
        let scalar = 'Z';
        let mut buf = [0u8; 4];
        let encoded = scalar.encode_utf8(&mut buf);
        update.cells[0] = ProjectionCell {
            role: CellRole::Lead,
            width: 1,
            text: Arc::from(encoded.as_bytes().to_vec()),
            scalar,
            foreground: ProjectionColor::Default,
            background: ProjectionColor::Default,
            attributes: ProjectionAttributes::default(),
        };
        let batch = encode_delta(&update, 20).unwrap();
        assert!(batch.frames.len() > 1);
        let chunks = batch
            .frames
            .iter()
            .map(|frame| decode_chunk(frame))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let before = cache.clone();
        assert_eq!(
            cache.apply_chunks(&chunks[..chunks.len() - 1]),
            Err(DisplayError::IncompleteBatch)
        );
        assert_eq!(cache, before);
        cache.apply_chunks(&chunks).unwrap();
        assert_eq!(cache.cells[0].scalar, 'Z');
    }

    #[test]
    fn v2_multi_scalar_round_trip() {
        let heart = "❤️".as_bytes();
        let mut cells = vec![cell(0); 4];
        cells[0] = ProjectionCell {
            role: CellRole::Lead,
            width: 2,
            text: Arc::from(heart.to_vec()),
            scalar: '❤',
            foreground: ProjectionColor::Default,
            background: ProjectionColor::Default,
            attributes: ProjectionAttributes::default(),
        };
        cells[1] = ProjectionCell {
            role: CellRole::Continuation,
            width: 0,
            text: Arc::from([]),
            scalar: ' ',
            foreground: ProjectionColor::Default,
            background: ProjectionColor::Default,
            attributes: ProjectionAttributes::default(),
        };
        let snapshot = TerminalProjectionSnapshot {
            rows: 1,
            columns: 4,
            cursor_row: 0,
            cursor_col: 0,
            cursor_visible: true,
            alternate_screen: false,
            source_damage_generation: 3,
            damage: ProjectionDamage::full(1),
            cells,
        };
        let batch = encode_snapshot_v2(&snapshot).unwrap();
        assert_eq!(batch.schema, DISPLAY_SCHEMA_V2);
        let mut cache = empty_cache();
        cache.apply_batch(&batch).unwrap();
        assert_eq!(cache.cells[0].role, DisplayCellRole::Lead);
        assert_eq!(cache.cells[0].text.as_ref(), heart);
        assert_eq!(cache.cells[1].role, DisplayCellRole::Continuation);
        assert!(cache.cells[1].text.is_empty());
    }

    #[test]
    fn v2_reconnect_snapshot_accepts_default_styled_wide_continuation() {
        // Old/reconnect producers can emit a width-2 lead plus a continuation
        // that still carries the terminal's default style. The client must
        // inherit the lead's presentation instead of rejecting InvalidCell.
        let cjk = "中".as_bytes();
        let mut cells = vec![cell(0); 4];
        cells[0] = ProjectionCell {
            role: CellRole::Lead,
            width: 2,
            text: Arc::from(cjk.to_vec()),
            scalar: '中',
            foreground: ProjectionColor::Indexed(208),
            background: ProjectionColor::Default,
            attributes: ProjectionAttributes {
                bold: true,
                underline: false,
                inverse: false,
            },
        };
        cells[1] = ProjectionCell {
            role: CellRole::Continuation,
            width: 0,
            text: Arc::from([]),
            scalar: ' ',
            foreground: ProjectionColor::Default,
            background: ProjectionColor::Default,
            attributes: ProjectionAttributes::default(),
        };
        let snapshot = TerminalProjectionSnapshot {
            rows: 1,
            columns: 4,
            cursor_row: 0,
            cursor_col: 2,
            cursor_visible: true,
            alternate_screen: false,
            source_damage_generation: 9,
            damage: ProjectionDamage::full(1),
            cells,
        };
        let batch = encode_snapshot_v2(&snapshot).unwrap();
        let mut cache = empty_cache();
        cache.apply_batch(&batch).unwrap();
        assert_eq!(cache.cells[0].role, DisplayCellRole::Lead);
        assert_eq!(cache.cells[0].text.as_ref(), cjk);
        assert_eq!(cache.cells[1].role, DisplayCellRole::Continuation);
        assert_eq!(cache.cells[1].foreground, DisplayColor::Indexed(208));
        assert_eq!(cache.cells[1].background, cache.cells[0].background);
        assert!(cache.cells[1].attributes.bold);
        assert!(cache.cells[1].text.is_empty());
    }

    #[test]
    fn v2_partial_row_packing_for_dense_sidecar() {
        // Nine 8192-byte width-1 leads require partial-row spans.
        let payload = vec![b'x'; MAX_GRAPHEME_UTF8_BYTES];
        let columns = 9u16;
        let mut cells = Vec::new();
        for _ in 0..columns {
            cells.push(ProjectionCell {
                role: CellRole::Lead,
                width: 1,
                text: Arc::from(payload.clone()),
                scalar: 'x',
                foreground: ProjectionColor::Default,
                background: ProjectionColor::Default,
                attributes: ProjectionAttributes::default(),
            });
        }
        let snapshot = TerminalProjectionSnapshot {
            rows: 1,
            columns,
            cursor_row: 0,
            cursor_col: 0,
            cursor_visible: true,
            alternate_screen: false,
            source_damage_generation: 1,
            damage: ProjectionDamage::full(1),
            cells,
        };
        let batch = encode_snapshot_v2(&snapshot).unwrap();
        assert!(batch.frames.len() >= 2);
        let mut cache = empty_cache();
        cache.apply_batch(&batch).unwrap();
        assert_eq!(cache.cells.len(), 9);
        assert_eq!(cache.cells[0].text.len(), MAX_GRAPHEME_UTF8_BYTES);
    }

    #[test]
    fn v2_large_logical_snapshot_fragments_transport_and_commits_atomically() {
        let payload = Arc::<[u8]>::from(vec![b'x'; MAX_GRAPHEME_UTF8_BYTES]);
        let columns = 512u16;
        let snapshot = TerminalProjectionSnapshot {
            rows: 1,
            columns,
            cursor_row: 0,
            cursor_col: 0,
            cursor_visible: true,
            alternate_screen: false,
            source_damage_generation: 12,
            damage: ProjectionDamage::full(1),
            cells: (0..usize::from(columns))
                .map(|_| ProjectionCell {
                    role: CellRole::Lead,
                    width: 1,
                    text: payload.clone(),
                    scalar: 'x',
                    foreground: ProjectionColor::Default,
                    background: ProjectionColor::Default,
                    attributes: ProjectionAttributes::default(),
                })
                .collect(),
        };

        let logical = encode_snapshot_v2(&snapshot).unwrap();
        assert!(logical.total_bytes > MAX_DISPLAY_BATCH_BYTES);
        let fragments = logical.clone().into_transport_batches();
        assert!(fragments.len() > 1);
        assert!(fragments
            .iter()
            .all(|fragment| fragment.total_bytes <= MAX_DISPLAY_BATCH_BYTES));

        let chunks = fragments
            .iter()
            .flat_map(|fragment| fragment.frames.iter())
            .map(|frame| decode_chunk(frame))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let mut cache = empty_cache();
        cache.apply_chunks(&chunks).unwrap();
        assert_eq!(cache.generation, snapshot.source_damage_generation);
        assert_eq!(cache.cells[0].text.len(), MAX_GRAPHEME_UTF8_BYTES);
    }
}
