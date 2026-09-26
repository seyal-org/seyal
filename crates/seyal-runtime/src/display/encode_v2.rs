//! Schema-v2 display batch encoding (grapheme sidecar + partial-row spans).

use std::sync::Arc;

use seyal_exec::{CellRole, ProjectionCell, TerminalProjectionSnapshot, TerminalProjectionUpdate};
use seyal_protocol::framing::{self, MAX_FRAME_PAYLOAD};

use super::plan::{needs_sidecar, plan_v2_spans};
use super::{
    encode_color, encode_v2_cell_meta, proj_color, validate_geometry, validate_snapshot,
    validate_update, DisplayAttributes, DisplayCellRole, DisplayError, DisplayKind, DisplayMeta,
    EncodedDisplayBatch, DISPLAY_CELL_LEN, DISPLAY_CHUNK_HEADER_V2_LEN, DISPLAY_SCHEMA_V2,
    MAX_GRAPHEME_SIDECAR_BYTES, MAX_GRAPHEME_UTF8_BYTES,
};

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
        super::record_snapshot_encode(batch.total_bytes);
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
        super::record_delta_encode(batch.total_bytes);
    }
    Ok(batch)
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
