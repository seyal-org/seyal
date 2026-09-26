//! V2 chunk span planning for grapheme sidecars and partial-row cuts.

use seyal_exec::{CellRole, ProjectionCell};
use seyal_protocol::framing::MAX_FRAME_PAYLOAD;

use super::{
    DisplayError, DISPLAY_CELL_LEN, DISPLAY_CHUNK_HEADER_V2_LEN, MAX_GRAPHEME_SIDECAR_BYTES,
    MAX_GRAPHEME_UTF8_BYTES,
};

#[derive(Clone, Copy)]
pub(crate) struct SpanPlan {
    pub(crate) row_offset: u16,
    pub(crate) first_col: u16,
    pub(crate) row_count: u16,
    pub(crate) cell_start: usize,
    pub(crate) cell_count: usize,
}

pub(crate) fn plan_v2_spans(
    columns: u16,
    cells: &[ProjectionCell],
) -> Result<Vec<SpanPlan>, DisplayError> {
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

pub(crate) fn sidecar_bytes_for_cells(cells: &[ProjectionCell]) -> Result<usize, DisplayError> {
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

pub(crate) fn needs_sidecar(cell: &ProjectionCell) -> Result<bool, DisplayError> {
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
