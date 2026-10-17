use seyal_render::{
    CellSource, CommittedDisplay, CursorState, PreparationResult, PreparedSurface,
    RenderAttributes, RenderCell, RenderCellRole, RenderColor, RowDamage,
};
use seyal_runtime::{
    display::{
        DecodedDisplayChunk, DisplayAttributes, DisplayCache, DisplayCell, DisplayCellRole,
        DisplayColor, DisplayError, DisplayKind, DISPLAY_CELL_LEN, DISPLAY_CHUNK_HEADER_LEN,
        DISPLAY_CHUNK_HEADER_V2_LEN, DISPLAY_SCHEMA_V2, MAX_DISPLAY_CELLS,
        MAX_LOGICAL_DISPLAY_BYTES,
    },
    local_ipc::framing::HEADER_LEN,
};

use super::{ClientError, LocalDisplayClient};

#[derive(Debug, Default)]
pub(crate) struct PendingDisplayBatch {
    chunks: Vec<DecodedDisplayChunk>,
    cells: usize,
    wire_bytes: usize,
}

impl PendingDisplayBatch {
    pub(crate) fn push(&mut self, chunk: DecodedDisplayChunk) -> Result<bool, ClientError> {
        // A newer logical update can arrive after an earlier update has
        // started crossing transport-batch boundaries. Drop the incomplete
        // disposable assembly before accepting its chunk zero; otherwise the
        // old generation would poison the new snapshot/delta transaction.
        let superseded = self.chunks.first().is_some_and(|first| {
            chunk.chunk_index == 0
                && (chunk.kind != first.kind
                    || chunk.schema != first.schema
                    || chunk.generation != first.generation
                    || chunk.base_generation != first.base_generation
                    || chunk.rows != first.rows
                    || chunk.columns != first.columns)
        });
        if superseded {
            self.clear();
        }

        let expected_count = usize::from(chunk.chunk_count);
        if expected_count == 0 {
            return Err(ClientError::Capacity);
        }

        if self.chunks.is_empty() {
            if chunk.chunk_index != 0 {
                return Err(ClientError::Protocol);
            }
            if chunk.kind == DisplayKind::Snapshot
                && chunk.schema != DISPLAY_SCHEMA_V2
                && chunk.first_row != 0
            {
                return Err(ClientError::Protocol);
            }
            if chunk.kind == DisplayKind::Snapshot
                && chunk.schema == DISPLAY_SCHEMA_V2
                && (chunk.first_row != 0 || chunk.first_col != 0)
            {
                return Err(ClientError::Protocol);
            }
        } else {
            let first = self.chunks.first().ok_or(ClientError::Protocol)?;
            let previous = self.chunks.last().ok_or(ClientError::Protocol)?;
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
                || usize::from(chunk.chunk_index) != self.chunks.len()
            {
                return Err(ClientError::Protocol);
            }
            if chunk.schema == DISPLAY_SCHEMA_V2 {
                let expected_row = if previous.row_count == 1
                    && !(previous.first_col == 0
                        && previous.cells.len() == usize::from(previous.columns))
                {
                    let next_col = previous
                        .first_col
                        .checked_add(previous.cells.len() as u16)
                        .ok_or(ClientError::Capacity)?;
                    if next_col == previous.columns {
                        (
                            previous
                                .first_row
                                .checked_add(1)
                                .ok_or(ClientError::Capacity)?,
                            0u16,
                        )
                    } else {
                        (previous.first_row, next_col)
                    }
                } else {
                    (
                        previous
                            .first_row
                            .checked_add(previous.row_count)
                            .ok_or(ClientError::Capacity)?,
                        0u16,
                    )
                };
                if chunk.first_row != expected_row.0 || chunk.first_col != expected_row.1 {
                    return Err(ClientError::Protocol);
                }
            } else {
                let expected_first_row = previous
                    .first_row
                    .checked_add(previous.row_count)
                    .ok_or(ClientError::Capacity)?;
                if chunk.first_row != expected_first_row || chunk.first_col != 0 {
                    return Err(ClientError::Protocol);
                }
            }
        }

        if self.chunks.len() >= expected_count {
            return Err(ClientError::Protocol);
        }

        let geometry_cells = usize::from(chunk.rows)
            .checked_mul(usize::from(chunk.columns))
            .ok_or(ClientError::Capacity)?;
        let next_cells = self
            .cells
            .checked_add(chunk.cells.len())
            .ok_or(ClientError::Capacity)?;
        if next_cells > geometry_cells || next_cells > MAX_DISPLAY_CELLS {
            return Err(ClientError::Capacity);
        }

        let header_len = if chunk.schema == DISPLAY_SCHEMA_V2 {
            DISPLAY_CHUNK_HEADER_V2_LEN
        } else {
            DISPLAY_CHUNK_HEADER_LEN
        };
        let sidecar_bytes = if chunk.schema == DISPLAY_SCHEMA_V2 {
            chunk
                .cells
                .iter()
                .filter(|cell| cell.sidecar)
                .try_fold(0usize, |total, cell| {
                    total
                        .checked_add(cell.text.len())
                        .ok_or(ClientError::Capacity)
                })?
        } else {
            0
        };
        let chunk_wire_bytes = HEADER_LEN
            .checked_add(header_len)
            .and_then(|value| {
                chunk
                    .cells
                    .len()
                    .checked_mul(DISPLAY_CELL_LEN)
                    .and_then(|cell_bytes| value.checked_add(cell_bytes))
                    .and_then(|value| value.checked_add(sidecar_bytes))
            })
            .ok_or(ClientError::Capacity)?;
        let next_wire_bytes = self
            .wire_bytes
            .checked_add(chunk_wire_bytes)
            .ok_or(ClientError::Capacity)?;
        if next_wire_bytes > MAX_LOGICAL_DISPLAY_BYTES {
            return Err(ClientError::Capacity);
        }

        if self.chunks.is_empty() {
            self.chunks.reserve(expected_count);
        }
        self.cells = next_cells;
        self.wire_bytes = next_wire_bytes;
        self.chunks.push(chunk);
        Ok(self.chunks.len() == expected_count)
    }

    pub(crate) fn chunks(&self) -> &[DecodedDisplayChunk] {
        &self.chunks
    }

    pub(crate) fn clear(&mut self) {
        self.chunks.clear();
        self.cells = 0;
        self.wire_bytes = 0;
    }
}

impl LocalDisplayClient {
    pub(crate) fn clear_presentation_state(&mut self) {
        self.pending_batch.clear();
        self.cache = seyal_runtime::display::empty_cache();
        self.prepared = PreparedSurface::default();
        // An empty fenced cache cannot be prepared. The matching resumed
        // snapshot is prepared in the ordinary committed-display path.
        self.needs_initial_prepare = false;
        self.viewport_line_ids.clear();
        self.viewport_line_ids_generation = 0;
        self.resume_fenced = true;
    }

    pub fn ensure_prepared_surface(&mut self) -> Result<PreparationResult, ClientError> {
        if !self.needs_initial_prepare {
            return Ok(self.last_preparation);
        }
        if self.cache.rows == 0 || self.cache.columns == 0 {
            return Err(ClientError::Protocol);
        }
        let result = prepare_cache(
            &mut self.prepared,
            &self.cache,
            RowDamage::full(self.cache.rows),
            true,
        )?;
        self.last_preparation = result;
        self.needs_initial_prepare = false;
        Ok(result)
    }

    pub(crate) fn complete_frame_end(&self) -> Result<Option<usize>, ClientError> {
        let available = self.buffered.len().saturating_sub(self.read_offset);
        if available < HEADER_LEN {
            return Ok(None);
        }
        let header_end = self
            .read_offset
            .checked_add(HEADER_LEN)
            .ok_or(ClientError::Capacity)?;
        let header = seyal_runtime::local_ipc::framing::FrameHeader::decode(
            &self.buffered[self.read_offset..header_end],
        )
        .map_err(|_| ClientError::Protocol)?;
        let total = HEADER_LEN
            .checked_add(header.payload_len as usize)
            .ok_or(ClientError::Capacity)?;
        if available < total {
            return Ok(None);
        }
        self.read_offset
            .checked_add(total)
            .map(Some)
            .ok_or(ClientError::Capacity)
    }

    pub(crate) fn accept_display_chunk(
        &mut self,
        chunk: DecodedDisplayChunk,
        damage: &mut RowDamage,
        full_invalidation: &mut bool,
    ) -> Result<bool, ClientError> {
        match self.pending_batch.push(chunk) {
            Ok(false) => return Ok(false),
            Ok(true) => {}
            Err(ClientError::Protocol | ClientError::Capacity) => {
                // A decodable frame can still violate the logical update
                // sequence (gap, overlap, stale identity, or capacity bound).
                // The projection is disposable: discard the complete pending
                // transaction and ask for one authoritative snapshot while
                // leaving the committed cache untouched.
                self.pending_batch.clear();
                self.request_resync()?;
                return Ok(false);
            }
            Err(error) => return Err(error),
        }

        let first_kind = self
            .pending_batch
            .chunks()
            .first()
            .map(|first| first.kind)
            .ok_or(ClientError::Protocol)?;
        match self.cache.apply_chunks(self.pending_batch.chunks()) {
            Ok(()) => {}
            Err(
                DisplayError::GenerationMismatch
                | DisplayError::DimensionMismatch
                | DisplayError::InvalidSidecar
                | DisplayError::InvalidRole
                | DisplayError::InvalidWidth
                | DisplayError::InvalidChunk
                | DisplayError::InvalidUnicode
                | DisplayError::InvalidCell,
            ) => {
                self.pending_batch.clear();
                self.request_resync()?;
                return Ok(false);
            }
            Err(_) => return Err(ClientError::Display),
        }

        match first_kind {
            DisplayKind::Snapshot => {
                self.resume_fenced = false;
                *damage = RowDamage::full(self.cache.rows);
                *full_invalidation = true;
            }
            DisplayKind::Delta => {
                for committed in self.pending_batch.chunks() {
                    damage.union(
                        RowDamage::from_range(committed.first_row, committed.row_count)
                            .map_err(|_| ClientError::Prepare)?,
                    );
                }
            }
        }
        self.pending_batch.clear();
        self.observe_projection()?;
        Ok(true)
    }

    pub(crate) fn compact_buffer(&mut self) {
        if self.read_offset == 0 {
            return;
        }
        if self.read_offset >= self.buffered.len() {
            self.buffered.clear();
        } else {
            self.buffered.drain(..self.read_offset);
        }
        self.read_offset = 0;
    }
}

pub(crate) struct RuntimeCells<'a>(pub(crate) &'a [DisplayCell]);

impl CellSource for RuntimeCells<'_> {
    fn len(&self) -> usize {
        self.0.len()
    }

    fn cell(&self, index: usize) -> Option<RenderCell> {
        self.0.get(index).cloned().map(runtime_cell_to_render)
    }
}

pub(crate) fn prepare_cache(
    prepared: &mut PreparedSurface,
    cache: &DisplayCache,
    damage: RowDamage,
    full_invalidation: bool,
) -> Result<PreparationResult, ClientError> {
    let source = RuntimeCells(&cache.cells);
    prepared
        .prepare(
            CommittedDisplay {
                generation: cache.generation,
                rows: cache.rows,
                columns: cache.columns,
                cursor: CursorState::new(cache.cursor_row, cache.cursor_col, cache.cursor_visible),
                alternate_screen: cache.alternate_screen,
                cells: &source,
            },
            damage,
            full_invalidation,
        )
        .map_err(|_| ClientError::Prepare)
}

fn runtime_cell_to_render(cell: DisplayCell) -> RenderCell {
    RenderCell {
        scalar: cell.scalar,
        role: match cell.role {
            DisplayCellRole::Empty => RenderCellRole::Empty,
            DisplayCellRole::Lead => RenderCellRole::Lead,
            DisplayCellRole::Continuation => RenderCellRole::Continuation,
        },
        width: cell.width,
        text: cell.text,
        foreground: runtime_color_to_render(cell.foreground),
        background: runtime_color_to_render(cell.background),
        attributes: runtime_attributes_to_render(cell.attributes),
    }
}

fn runtime_color_to_render(color: DisplayColor) -> RenderColor {
    match color {
        DisplayColor::Default => RenderColor::Default,
        DisplayColor::Indexed(index) => RenderColor::Indexed(index),
        DisplayColor::Rgb { r, g, b } => RenderColor::Rgb { r, g, b },
    }
}

fn runtime_attributes_to_render(attributes: DisplayAttributes) -> RenderAttributes {
    RenderAttributes {
        bold: attributes.bold,
        underline: attributes.underline,
        inverse: attributes.inverse,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn display_cell() -> DisplayCell {
        DisplayCell::lead_scalar(
            'x',
            1,
            DisplayColor::Default,
            DisplayColor::Default,
            DisplayAttributes::default(),
        )
    }

    fn decoded_chunk(
        chunk_index: u16,
        chunk_count: u16,
        first_row: u16,
        row_count: u16,
    ) -> DecodedDisplayChunk {
        let columns = 1;
        DecodedDisplayChunk {
            kind: DisplayKind::Delta,
            schema: 1,
            generation: 2,
            base_generation: 1,
            rows: 4,
            columns,
            cursor_row: 0,
            cursor_col: 0,
            cursor_visible: true,
            alternate_screen: false,
            first_row,
            row_count,
            first_col: 0,
            chunk_index,
            chunk_count,
            cells: vec![display_cell(); usize::from(row_count) * usize::from(columns)],
        }
    }

    #[test]
    fn runtime_cell_adapter_preserves_scalar_style_and_color_without_copying_cache() {
        let cells = [DisplayCell::lead_scalar(
            'Q',
            1,
            DisplayColor::Indexed(5),
            DisplayColor::Rgb { r: 1, g: 2, b: 3 },
            DisplayAttributes {
                bold: true,
                underline: true,
                inverse: false,
            },
        )];
        let source = RuntimeCells(&cells);
        let converted = source.cell(0).unwrap();
        assert_eq!(converted.scalar, 'Q');
        assert_eq!(converted.foreground, RenderColor::Indexed(5));
        assert_eq!(converted.background, RenderColor::Rgb { r: 1, g: 2, b: 3 });
        assert!(converted.attributes.bold);
        assert!(converted.attributes.underline);
    }

    #[test]
    fn pending_display_batch_accepts_valid_first_chunk() {
        let mut batch = PendingDisplayBatch::default();
        let chunk = decoded_chunk(0, 2, 0, 1);
        assert!(!batch.push(chunk).unwrap());
        assert_eq!(batch.chunks().len(), 1);
    }

    #[test]
    fn pending_display_batch_rejects_replayed_or_noncontiguous_rows() {
        let mut batch = PendingDisplayBatch::default();
        assert!(!batch.push(decoded_chunk(0, 2, 0, 1)).unwrap());

        let replayed = decoded_chunk(1, 2, 0, 1);
        assert_eq!(batch.push(replayed), Err(ClientError::Protocol));
        assert_eq!(batch.chunks().len(), 1);
    }

    #[test]
    fn pending_display_batch_discards_incomplete_logical_update_on_supersession() {
        let mut batch = PendingDisplayBatch::default();
        assert!(!batch.push(decoded_chunk(0, 2, 0, 1)).unwrap());

        let mut replacement = decoded_chunk(0, 2, 0, 1);
        replacement.generation = 3;
        replacement.base_generation = 0;
        replacement.kind = DisplayKind::Snapshot;
        assert!(!batch.push(replacement).unwrap());
        assert_eq!(batch.chunks().len(), 1);
        assert_eq!(batch.chunks()[0].generation, 3);
    }

    #[test]
    fn pending_display_batch_counts_v2_sidecar_bytes_against_logical_cap() {
        let sidecar = Arc::<[u8]>::from(vec![b'x'; 8_192]);
        let rows = 20u16;
        let columns = 512u16;
        let chunks_per_row = usize::from(columns) / 8;
        let chunk_count = rows as usize * chunks_per_row;
        let mut pending = PendingDisplayBatch::default();

        for index in 0..chunk_count {
            let row = index / chunks_per_row;
            let col = (index % chunks_per_row) * 8;
            let cells = (0..8)
                .map(|_| DisplayCell {
                    scalar: 'x',
                    role: DisplayCellRole::Lead,
                    width: 1,
                    text: sidecar.clone(),
                    sidecar: true,
                    foreground: DisplayColor::Default,
                    background: DisplayColor::Default,
                    attributes: DisplayAttributes::default(),
                })
                .collect();
            let chunk = DecodedDisplayChunk {
                kind: DisplayKind::Snapshot,
                schema: DISPLAY_SCHEMA_V2,
                generation: 4,
                base_generation: 0,
                rows,
                columns,
                cursor_row: 0,
                cursor_col: 0,
                cursor_visible: true,
                alternate_screen: false,
                first_row: row as u16,
                row_count: 1,
                first_col: col as u16,
                chunk_index: index as u16,
                chunk_count: chunk_count as u16,
                cells,
            };
            if let Err(error) = pending.push(chunk) {
                assert_eq!(error, ClientError::Capacity);
                assert!(index > 100, "sidecar bytes were not included in the cap");
                return;
            }
        }
        panic!("sidecar-heavy logical update exceeded the cap without rejection");
    }
}
