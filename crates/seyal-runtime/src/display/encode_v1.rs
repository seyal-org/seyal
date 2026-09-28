//! Schema-v1 display batch encoding (scalar-lossless rows).

use std::sync::Arc;

use seyal_exec::{ProjectionCell, TerminalProjectionSnapshot, TerminalProjectionUpdate};
use seyal_protocol::framing::{self, MAX_FRAME_PAYLOAD};

use super::{
    encode_color, proj_color, validate_geometry, validate_snapshot, validate_update, DisplayError,
    DisplayKind, DisplayMeta, EncodedDisplayBatch, DISPLAY_CELL_LEN, DISPLAY_CHUNK_HEADER_LEN,
    MAX_DISPLAY_BATCH_BYTES,
};

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
        super::record_snapshot_encode(batch.total_bytes);
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
        super::record_delta_encode(batch.total_bytes);
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
