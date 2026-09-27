//! Fallible screen resize preparation and canonical commit.

use super::{SavedCursor, Screen};
use crate::{
    cursor::Cursor,
    damage::Mutation,
    grapheme_store::GraphemeStore,
    history::{range_entirely_before, HistoryBreakAfter, HistoryLine, HistoryStore, HistoryUnit},
    line::LineIdAllocator,
    Cell, CellRole, LineId, TerminalError,
};
use std::collections::{HashMap, HashSet, VecDeque};

impl Screen {
    /// Builds the next screen buffers and allocates any new line identities
    /// without mutating the live screen. Dropping the result burns allocated
    /// identities but leaves observable geometry/damage unchanged.
    pub(crate) fn prepare_resize(
        &self,
        cols: u16,
        rows: u16,
        line_ids: &mut LineIdAllocator,
        store: &GraphemeStore,
    ) -> Result<PreparedScreen, TerminalError> {
        if cols == 0 || rows == 0 {
            return Err(TerminalError::InvalidSize);
        }
        if cols > crate::terminal::MAX_TERMINAL_COLUMNS || rows > crate::terminal::MAX_TERMINAL_ROWS
        {
            return Err(TerminalError::InvalidSize);
        }
        if cols == self.cols && rows == self.rows {
            return Ok(PreparedScreen::noop());
        }

        let old_cols = self.cols;
        let old_rows = self.rows;
        let new_row_count = usize::from(rows.saturating_sub(old_rows));
        if !line_ids.can_allocate(new_row_count) {
            return Err(TerminalError::LineIdentityExhausted);
        }

        let mut next = vec![Cell::default(); usize::from(cols) * usize::from(rows)];
        let mut next_anchors = vec![None; usize::from(cols) * usize::from(rows)];
        let mut next_breaks = vec![Some(HistoryBreakAfter::HardBreak); usize::from(rows)];
        let mut next_line_ids = vec![None; usize::from(rows)];
        let mut history_additions = Vec::new();
        let mut pending_history_cells = Vec::new();
        let mut replace_history = false;
        let mut replace_from = None;
        let mut next_source_breaks = HashMap::new();
        let mut mapped_cursor = None;
        if self.retain_history && (cols != old_cols || rows < old_rows) {
            // Trailing blank active rows are omitted when content exists so
            // wrapped source still fills the new viewport. An entirely empty
            // screen still includes those blanks so column-only resize cannot
            // eat scrollback spacers (unwrap_or(0) pulled history onto row 0).
            let last_source_row = (0..old_rows)
                .rev()
                .find(|row| {
                    let start = usize::from(*row) * usize::from(old_cols);
                    self.cells[start..start + usize::from(old_cols)]
                        .iter()
                        .any(|cell| cell.role != CellRole::Empty)
                })
                .unwrap_or(old_rows.saturating_sub(1));
            let mut source_lines: Vec<HistoryLine> = Vec::new();
            let mut source_cells = HashMap::new();
            let mut source_offsets = HashMap::<LineId, u32>::new();
            let mut source_breaks = HashMap::<LineId, HistoryBreakAfter>::new();
            let mut cursor_source = crate::HistoryAnchor {
                line_id: self.line_ids[usize::from(self.cursor.row)],
                unit_offset: 0,
            };
            for row in 0..=last_source_row {
                let row_start = usize::from(row) * usize::from(old_cols);
                let break_after =
                    self.row_breaks[usize::from(row)].unwrap_or(HistoryBreakAfter::HardBreak);
                let cells = &self.cells[row_start..row_start + usize::from(old_cols)];
                let mut row_has_units = false;
                for (col, cell) in cells.iter().enumerate() {
                    if cell.role != CellRole::Lead {
                        continue;
                    }
                    row_has_units = true;
                    let anchor = self.cell_anchors[row_start + col].unwrap_or_else(|| {
                        let line_id = self.line_ids[usize::from(row)];
                        let offset = source_offsets.entry(line_id).or_default();
                        let anchor = crate::HistoryAnchor {
                            line_id,
                            unit_offset: *offset,
                        };
                        *offset = offset.saturating_add(1);
                        anchor
                    });
                    let source_break = self
                        .source_breaks
                        .get(&anchor.line_id)
                        .copied()
                        .unwrap_or(break_after);
                    source_breaks.insert(anchor.line_id, source_break);
                    source_cells.insert(anchor, *cell);
                    if row == self.cursor.row
                        && (col < usize::from(self.cursor.col)
                            || (self.cursor.pending_wrap && col <= usize::from(self.cursor.col)))
                    {
                        cursor_source = crate::HistoryAnchor {
                            line_id: anchor.line_id,
                            unit_offset: anchor.unit_offset.saturating_add(1),
                        };
                    }
                    let mut fragment = HistoryLine::from_cells(
                        anchor.line_id,
                        HistoryBreakAfter::SoftWrap,
                        std::slice::from_ref(cell),
                        store,
                    );
                    fragment.start_offset = anchor.unit_offset;
                    if let Some(previous) = source_lines.last_mut().filter(|previous| {
                        previous.line_id == fragment.line_id
                            && previous.start_offset.saturating_add(
                                u32::try_from(previous.units.len()).unwrap_or(u32::MAX),
                            ) == fragment.start_offset
                    }) {
                        previous.units.extend(fragment.units);
                    } else {
                        source_lines.push(fragment);
                    }
                }
                if !row_has_units {
                    let line_id = self.line_ids[usize::from(row)];
                    source_breaks.insert(line_id, break_after);
                    source_lines.push(HistoryLine::from_cells(line_id, break_after, cells, store));
                }
            }
            let mut last_fragment = HashMap::new();
            for (index, line) in source_lines.iter().enumerate() {
                last_fragment.insert(line.line_id, index);
            }
            for (line_id, index) in last_fragment {
                source_lines[index].break_after = source_breaks
                    .get(&line_id)
                    .copied()
                    .unwrap_or(HistoryBreakAfter::HardBreak);
            }
            let mut history_units = HashMap::<crate::HistoryAnchor, HistoryUnit>::new();
            let budget = HistoryStore::eager_resize_row_budget(rows);
            let (mut combined, suffix_from, start_col) =
                self.history.eager_resize_suffix(cols, budget);
            replace_from = suffix_from;
            for line in &combined {
                for (index, unit) in line.units.iter().enumerate() {
                    let anchor = crate::HistoryAnchor {
                        line_id: line.line_id,
                        unit_offset: line.start_offset.saturating_add(index as u32),
                    };
                    history_units.entry(anchor).or_insert_with(|| unit.clone());
                }
            }
            combined.extend(source_lines);
            let mut source = HistoryStore::default();
            for line in &combined {
                source.append_line(line.clone());
            }
            let reflowed = source.reflow_from(cols, usize::MAX, start_col);
            let first = reflowed.len().saturating_sub(usize::from(rows));
            let active_first = reflowed
                .iter()
                .enumerate()
                .skip(first)
                .filter_map(|(index, row)| row.unavailable.then_some(index + 1))
                .next_back()
                .unwrap_or(first);
            let retained_until = reflowed.get(active_first).and_then(|row| {
                row.anchors.first().copied().or_else(|| {
                    row.source_line_id.map(|line_id| crate::HistoryAnchor {
                        line_id,
                        unit_offset: 0,
                    })
                })
            });
            history_additions = history_prefix(&combined, retained_until);
            replace_history = true;
            for (row, projection) in reflowed.iter().skip(active_first).enumerate() {
                let start = row * usize::from(cols);
                let mut col = 0usize;
                for anchor in &projection.anchors {
                    let width;
                    if let Some(cell) = source_cells.get(anchor).copied() {
                        width = usize::from(cell.width.max(1));
                        if col + width > usize::from(cols) {
                            break;
                        }
                        next[start + col] = cell;
                        next_anchors[start + col] = Some(*anchor);
                        if width == 2 {
                            next[start + col + 1] = Cell::continuation();
                        }
                    } else if let Some(unit) = history_units.get(anchor) {
                        width = usize::from(unit.width.max(1));
                        if col + width > usize::from(cols) {
                            break;
                        }
                        let first = std::str::from_utf8(&unit.utf8)
                            .ok()
                            .and_then(|text| text.chars().next())
                            .unwrap_or('\u{FFFD}');
                        next[start + col] = Cell::lead_inline(first, unit.width.max(1), unit.style);
                        next_anchors[start + col] = Some(*anchor);
                        pending_history_cells.push((start + col, unit.clone()));
                        if width == 2 {
                            next[start + col + 1] = Cell::continuation();
                        }
                    } else {
                        continue;
                    }
                    col += width;
                }
                next_breaks[row] = projection.break_after;
                next_line_ids[row] = projection.source_line_id;
                for anchor in &projection.anchors {
                    if let Some(source_break) = source_breaks.get(&anchor.line_id) {
                        next_source_breaks.insert(anchor.line_id, *source_break);
                    }
                }
                if projection.anchors.is_empty()
                    && let Some(line_id) = projection.source_line_id
                    && let Some(source_break) = source_breaks.get(&line_id)
                {
                    next_source_breaks.insert(line_id, *source_break);
                }
            }
            let mut prior = None;
            'cursor: for row in 0..usize::from(rows) {
                for col in 0..usize::from(cols) {
                    let Some(anchor) = next_anchors[row * usize::from(cols) + col] else {
                        continue;
                    };
                    if anchor == cursor_source {
                        mapped_cursor = Some(Cursor {
                            row: row as u16,
                            col: col as u16,
                            pending_wrap: false,
                        });
                        break 'cursor;
                    }
                    if anchor.line_id == cursor_source.line_id
                        && anchor.unit_offset < cursor_source.unit_offset
                    {
                        prior = Some((row, col, next[row * usize::from(cols) + col]));
                    }
                }
            }
            if mapped_cursor.is_none()
                && let Some((row, col, cell)) = prior
            {
                let after = col.saturating_add(usize::from(cell.width.max(1)));
                mapped_cursor = Some(Cursor {
                    row: row as u16,
                    col: after.min(usize::from(cols.saturating_sub(1))) as u16,
                    pending_wrap: self.cursor.pending_wrap && after >= usize::from(cols),
                });
            }

            let mut reusable_blank_ids = self
                .line_ids
                .iter()
                .copied()
                .skip(usize::from(last_source_row) + 1)
                .collect::<VecDeque<_>>();
            for line_id in &mut next_line_ids {
                if line_id.is_none() {
                    *line_id = reusable_blank_ids.pop_front();
                }
            }
        } else {
            next_source_breaks = self.source_breaks.clone();
            let copy_cols = old_cols.min(cols);
            let copy_rows = old_rows.min(rows);
            for row in 0..copy_rows {
                let old_start = usize::from(row) * usize::from(old_cols);
                let new_start = usize::from(row) * usize::from(cols);
                let count = usize::from(copy_cols);
                next[new_start..new_start + count]
                    .copy_from_slice(&self.cells[old_start..old_start + count]);
                next_anchors[new_start..new_start + count]
                    .copy_from_slice(&self.cell_anchors[old_start..old_start + count]);
                next_breaks[usize::from(row)] = self.row_breaks[usize::from(row)];
                next_line_ids[usize::from(row)] = Some(self.line_ids[usize::from(row)]);
                if cols < old_cols {
                    let final_col = usize::from(cols - 1);
                    let final_index = new_start + final_col;
                    if next[final_index].role == CellRole::Lead && next[final_index].width >= 2 {
                        next[final_index] = Cell::blank(self.pen.bg);
                        next_anchors[final_index] = None;
                    }
                }
            }
        }

        let missing_ids = next_line_ids
            .iter()
            .filter(|line_id| line_id.is_none())
            .count();
        if !line_ids.can_allocate(missing_ids) {
            return Err(TerminalError::LineIdentityExhausted);
        }
        let mut prepared_line_ids = Vec::with_capacity(usize::from(rows));
        for line_id in next_line_ids {
            if let Some(line_id) = line_id {
                prepared_line_ids.push(line_id);
            } else {
                prepared_line_ids.push(line_ids.allocate()?);
            }
        }

        let mut cursor = self.cursor;
        if let Some(mapped) = mapped_cursor {
            cursor = mapped;
        } else {
            cursor.clamp(cols, rows);
        }
        let mut saved_cursor = self.saved_cursor;
        if let Some(saved) = &mut saved_cursor {
            saved.cursor.clamp(cols, rows);
        }

        Ok(PreparedScreen {
            cols,
            rows,
            cells: next,
            cell_anchors: next_anchors,
            line_ids: prepared_line_ids,
            row_breaks: next_breaks,
            source_breaks: next_source_breaks,
            history_additions,
            pending_history_cells,
            replace_history,
            replace_from,
            cursor,
            saved_cursor,
            unchanged: false,
        })
    }

    /// Infallible swap of a prepared resize into the live screen.
    pub(crate) fn commit_prepared(
        &mut self,
        prepared: PreparedScreen,
        store: &mut GraphemeStore,
    ) -> Mutation {
        if prepared.unchanged {
            return Mutation::none();
        }
        let rows = prepared.rows;
        let mut prepared = prepared;
        for (index, unit) in prepared.pending_history_cells.drain(..) {
            let first = std::str::from_utf8(&unit.utf8)
                .ok()
                .and_then(|text| text.chars().next())
                .unwrap_or('\u{FFFD}');
            let admit = store.insert_replacing(&unit.utf8, None);
            prepared.cells[index] =
                Cell::lead_from_admit(first, unit.width.max(1), unit.style, admit);
        }
        if let Some(from) = prepared.replace_from {
            self.history.truncate_from(from);
            for line in prepared.history_additions {
                self.history.append_line(line);
            }
        } else if prepared.replace_history {
            self.history.replace_payload(prepared.history_additions);
        } else {
            for line in prepared.history_additions {
                self.history.append_line(line);
            }
        }
        let retained_store_ids = prepared
            .cells
            .iter()
            .filter(|cell| cell.role == CellRole::Lead)
            .map(|cell| cell.store_id)
            .collect::<HashSet<_>>();
        for cell in &self.cells {
            if cell.role == CellRole::Lead && !retained_store_ids.contains(&cell.store_id) {
                Self::release_cell(*cell, store);
            }
        }
        // Default full-screen margins must stay full-screen after row growth.
        // `clamp_scroll_region_to_geometry` only expands `scroll_bottom` when it
        // is out of range, so a prior full region (bottom == old_rows-1) would
        // otherwise become a partial DECSTBM and discard scrolled rows instead
        // of sealing them into HistoryStore.
        let was_full_screen = self.region_is_full_screen();
        self.cols = prepared.cols;
        self.rows = prepared.rows;
        self.cells = prepared.cells;
        self.cell_anchors = prepared.cell_anchors;
        self.line_ids = prepared.line_ids;
        self.row_breaks = prepared.row_breaks;
        self.source_breaks = prepared.source_breaks;
        self.cursor = prepared.cursor;
        self.saved_cursor = prepared.saved_cursor;
        if was_full_screen && self.rows > 0 {
            self.scroll_top = 0;
            self.scroll_bottom = self.rows - 1;
        } else {
            self.clamp_scroll_region_to_geometry();
        }
        Mutation::full(rows)
    }
}

/// Fallible resize preparation held until canonical commit.
pub(crate) struct PreparedScreen {
    cols: u16,
    rows: u16,
    cells: Vec<Cell>,
    cell_anchors: Vec<Option<crate::HistoryAnchor>>,
    line_ids: Vec<LineId>,
    row_breaks: Vec<Option<HistoryBreakAfter>>,
    source_breaks: HashMap<LineId, HistoryBreakAfter>,
    history_additions: Vec<HistoryLine>,
    pending_history_cells: Vec<(usize, HistoryUnit)>,
    replace_history: bool,
    replace_from: Option<crate::HistoryAnchor>,
    cursor: Cursor,
    saved_cursor: Option<SavedCursor>,
    unchanged: bool,
}

impl PreparedScreen {
    fn noop() -> Self {
        Self {
            cols: 0,
            rows: 0,
            cells: Vec::new(),
            cell_anchors: Vec::new(),
            line_ids: Vec::new(),
            row_breaks: Vec::new(),
            source_breaks: HashMap::new(),
            history_additions: Vec::new(),
            pending_history_cells: Vec::new(),
            replace_history: false,
            replace_from: None,
            cursor: Cursor::default(),
            saved_cursor: None,
            unchanged: true,
        }
    }
}

fn history_prefix(
    lines: &[HistoryLine],
    retained_until: Option<crate::HistoryAnchor>,
) -> Vec<HistoryLine> {
    let Some(boundary) = retained_until else {
        return lines.to_vec();
    };
    let mut retained = Vec::new();
    for line in lines {
        let unit_len = u32::try_from(line.units.len()).unwrap_or(u32::MAX);
        if range_entirely_before(line.line_id, line.start_offset, unit_len, boundary) {
            retained.push(line.clone());
            continue;
        }
        if line.line_id > boundary.line_id {
            break;
        }
        let count = boundary
            .unit_offset
            .saturating_sub(line.start_offset)
            .try_into()
            .unwrap_or(usize::MAX);
        let count = count.min(line.units.len());
        if count > 0 {
            retained.push(HistoryLine {
                line_id: line.line_id,
                units: line.units[..count].to_vec(),
                break_after: HistoryBreakAfter::SoftWrap,
                start_offset: line.start_offset,
            });
        }
        break;
    }
    retained
}
