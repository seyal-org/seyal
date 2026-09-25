use crate::{
    cursor::Cursor,
    damage::Mutation,
    grapheme_store::GraphemeStore,
    history::{
        range_entirely_before, HistoryBreakAfter, HistoryLine, HistoryLineRef, HistoryStore,
        HistoryUnit,
    },
    line::LineIdAllocator,
    Cell, CellRole, Color, CursorState, LineId, Style, TerminalError,
};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Clone, Copy, Debug)]
struct SavedCursor {
    cursor: Cursor,
    style: Style,
}

pub(crate) struct Screen {
    cols: u16,
    rows: u16,
    cells: Vec<Cell>,
    cell_anchors: Vec<Option<crate::HistoryAnchor>>,
    line_ids: Vec<LineId>,
    row_breaks: Vec<Option<HistoryBreakAfter>>,
    source_breaks: HashMap<LineId, HistoryBreakAfter>,
    cursor: Cursor,
    pen: Style,
    saved_cursor: Option<SavedCursor>,
    history: HistoryStore,
    retain_history: bool,
    /// Inclusive 0-based DECSTBM top margin.
    scroll_top: u16,
    /// Inclusive 0-based DECSTBM bottom margin.
    scroll_bottom: u16,
}

impl Screen {
    pub(crate) fn new(
        cols: u16,
        rows: u16,
        line_ids: &mut LineIdAllocator,
        retain_history: bool,
    ) -> Result<Self, TerminalError> {
        if cols == 0 || rows == 0 {
            return Err(TerminalError::InvalidSize);
        }
        if cols > crate::terminal::MAX_TERMINAL_COLUMNS || rows > crate::terminal::MAX_TERMINAL_ROWS
        {
            return Err(TerminalError::InvalidSize);
        }
        if !line_ids.can_allocate(usize::from(rows)) {
            return Err(TerminalError::LineIdentityExhausted);
        }

        let mut row_ids = Vec::with_capacity(usize::from(rows));
        for _ in 0..rows {
            row_ids.push(line_ids.allocate()?);
        }

        Ok(Self {
            cols,
            rows,
            cells: vec![Cell::default(); usize::from(cols) * usize::from(rows)],
            cell_anchors: vec![None; usize::from(cols) * usize::from(rows)],
            line_ids: row_ids,
            row_breaks: vec![Some(HistoryBreakAfter::HardBreak); usize::from(rows)],
            source_breaks: HashMap::new(),
            cursor: Cursor::default(),
            pen: Style::default(),
            saved_cursor: None,
            history: HistoryStore::default(),
            retain_history,
            scroll_top: 0,
            scroll_bottom: rows.saturating_sub(1),
        })
    }

    /// DECSTBM. Parameters are 1-based inclusive margins; `0` means default.
    /// Invalid ranges (top >= bottom after defaults) leave the region unchanged.
    /// Successful set moves the cursor to the absolute origin (1,1).
    pub(crate) fn set_scroll_region(&mut self, top: u16, bottom: u16) -> Mutation {
        let top = if top == 0 { 1 } else { top };
        let bottom = if bottom == 0 { self.rows } else { bottom };
        if top < 1 || bottom > self.rows || top >= bottom {
            return Mutation::none();
        }
        self.scroll_top = top - 1;
        self.scroll_bottom = bottom - 1;
        self.set_cursor(0, 0)
    }

    fn region_is_full_screen(&self) -> bool {
        self.scroll_top == 0 && self.scroll_bottom + 1 == self.rows
    }

    fn clamp_scroll_region_to_geometry(&mut self) {
        if self.rows == 0 {
            self.scroll_top = 0;
            self.scroll_bottom = 0;
            return;
        }
        let max_row = self.rows - 1;
        if self.scroll_top > max_row {
            self.scroll_top = 0;
        }
        if self.scroll_bottom > max_row || self.scroll_bottom <= self.scroll_top {
            self.scroll_bottom = max_row;
            if self.scroll_top >= self.scroll_bottom {
                self.scroll_top = 0;
            }
        }
    }

    pub(crate) fn cols(&self) -> u16 {
        self.cols
    }

    pub(crate) fn rows(&self) -> u16 {
        self.rows
    }

    pub(crate) fn pen(&self) -> Style {
        self.pen
    }

    pub(crate) fn inherit_pen_for_clean_buffer(&mut self, pen: Style, store: &mut GraphemeStore) {
        self.pen = pen;
        self.release_all_payloads(store);
        self.cells.fill(Cell::blank(pen.bg));
    }

    pub(crate) fn release_all_payloads(&mut self, store: &mut GraphemeStore) {
        for cell in &self.cells {
            Self::release_cell(*cell, store);
        }
    }

    fn release_cell(cell: Cell, store: &mut GraphemeStore) {
        if cell.role == CellRole::Lead {
            store.release(cell.store_id);
        }
    }

    /// Clears a lead or continuation so no orphan half remains (SPEC-011 §7.3).
    pub(crate) fn clear_unit_at(
        &mut self,
        col: u16,
        row: u16,
        store: &mut GraphemeStore,
    ) -> Mutation {
        if col >= self.cols || row >= self.rows {
            return Mutation::none();
        }
        let index = self.index(col, row);
        let cell = self.cells[index];
        let (lead_col, lead_row) = match cell.role {
            CellRole::Continuation if col > 0 => (col - 1, row),
            CellRole::Lead | CellRole::Empty => (col, row),
            CellRole::Continuation => (col, row),
        };
        let lead_index = self.index(lead_col, lead_row);
        let lead = self.cells[lead_index];
        if lead.role == CellRole::Lead {
            Self::release_cell(lead, store);
            let width = lead.width.max(1);
            self.cells[lead_index] = Cell::blank(self.pen.bg);
            self.cell_anchors[lead_index] = None;
            if width >= 2 && lead_col + 1 < self.cols {
                let cont = self.index(lead_col + 1, lead_row);
                if self.cells[cont].role == CellRole::Continuation {
                    self.cells[cont] = Cell::blank(self.pen.bg);
                    self.cell_anchors[cont] = None;
                }
            }
        } else {
            self.cells[index] = Cell::blank(self.pen.bg);
            self.cell_anchors[index] = None;
        }
        Mutation::row(row)
    }

    pub(crate) fn cursor(&self, visible: bool) -> CursorState {
        CursorState {
            col: self.cursor.col,
            row: self.cursor.row,
            visible,
        }
    }

    pub(crate) fn cell(&self, col: u16, row: u16) -> Option<Cell> {
        if col >= self.cols || row >= self.rows {
            return None;
        }
        Some(self.cells[self.index(col, row)])
    }

    pub(crate) fn line_id(&self, row: u16) -> Option<LineId> {
        self.line_ids.get(usize::from(row)).copied()
    }

    pub(crate) fn row_break_after(&self, row: u16) -> Option<HistoryBreakAfter> {
        self.row_breaks.get(usize::from(row)).copied().flatten()
    }

    /// Oldest-to-newest retained primary history entries (storage order).
    pub(crate) fn history_entries(&self) -> impl Iterator<Item = HistoryLineRef<'_>> {
        self.history.entries()
    }

    pub(crate) fn history(&self) -> &HistoryStore {
        &self.history
    }

    #[cfg(test)]
    pub(crate) fn source_break_len(&self) -> usize {
        self.source_breaks.len()
    }

    pub(crate) fn history_mut(&mut self) -> &mut HistoryStore {
        &mut self.history
    }

    pub(crate) fn cell_row(&self, row: u16) -> Option<&[Cell]> {
        if row >= self.rows {
            return None;
        }
        let start = usize::from(row) * usize::from(self.cols);
        Some(&self.cells[start..start + usize::from(self.cols)])
    }

    pub(crate) fn cell_anchor(&self, col: u16, row: u16) -> Option<crate::HistoryAnchor> {
        (col < self.cols && row < self.rows)
            .then(|| self.cell_anchors[self.index(col, row)])
            .flatten()
    }

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

    /// Places a completed canonical unit at the cursor (after wrap handling).
    /// Returns `(mutation, soft_wrapped, lead_col, lead_row)`.
    pub(crate) fn place_new_unit(
        &mut self,
        lead: Cell,
        wraparound: bool,
        line_ids: &mut LineIdAllocator,
        store: &mut GraphemeStore,
    ) -> Result<(Mutation, bool, u16, u16), TerminalError> {
        let width = lead.width.max(1);
        let mut mutation = Mutation::none();
        let mut soft_wrapped = false;

        if self.cursor.pending_wrap {
            if wraparound {
                let wrap_mutation =
                    self.line_feed(line_ids, HistoryBreakAfter::SoftWrap, Some(&mut *store))?;
                self.cursor.col = 0;
                mutation = mutation.merge(wrap_mutation);
                soft_wrapped = true;
            } else {
                self.cursor.pending_wrap = false;
            }
        }

        if width >= 2
            && self.cursor.col + 1 >= self.cols
            && (self.cursor.col == self.cols - 1 || self.cursor.col + 1 > self.cols - 1)
        {
            if wraparound {
                let wrap_mutation =
                    self.line_feed(line_ids, HistoryBreakAfter::SoftWrap, Some(&mut *store))?;
                self.cursor.col = 0;
                mutation = mutation.merge(wrap_mutation);
                soft_wrapped = true;
            } else {
                return Ok((mutation, soft_wrapped, self.cursor.col, self.cursor.row));
            }
        }

        let col = self.cursor.col;
        let row = self.cursor.row;
        let replaced_anchor = self.cell_anchors[self.index(col, row)];
        mutation = mutation.merge(self.clear_unit_at(col, row, store));
        if width >= 2 && col + 1 < self.cols {
            mutation = mutation.merge(self.clear_unit_at(col + 1, row, store));
        }

        let index = self.index(col, row);
        self.cells[index] = lead;
        self.cell_anchors[index] = Some(replaced_anchor.unwrap_or_else(|| {
            let row_start = usize::from(row) * usize::from(self.cols);
            self.cell_anchors[row_start..index]
                .iter()
                .rev()
                .flatten()
                .next()
                .map_or_else(
                    || crate::HistoryAnchor {
                        line_id: self.line_ids[usize::from(row)],
                        unit_offset: self.cells[row_start..index]
                            .iter()
                            .filter(|cell| cell.role == CellRole::Lead)
                            .count() as u32,
                    },
                    |anchor| crate::HistoryAnchor {
                        line_id: anchor.line_id,
                        unit_offset: anchor.unit_offset.saturating_add(1),
                    },
                )
        }));
        if width >= 2 {
            if col + 1 >= self.cols {
                Self::release_cell(lead, store);
                self.cells[index] = Cell::blank(self.pen.bg);
                return Ok((mutation, soft_wrapped, col, row));
            }
            let cont_index = self.index(col + 1, row);
            self.cells[cont_index] = Cell::continuation();
            self.cell_anchors[cont_index] = None;
        }
        mutation = mutation.merge(Mutation::row(row));

        let advance = u16::from(width);
        let next_col = col.saturating_add(advance);
        if next_col >= self.cols {
            self.cursor.col = self.cols - 1;
            self.cursor.pending_wrap = wraparound;
        } else {
            self.cursor.col = next_col;
            self.cursor.pending_wrap = false;
        }
        Ok((mutation, soft_wrapped, col, row))
    }

    /// Overwrites an existing active lead in place (append / late width change).
    pub(crate) fn replace_active_lead(
        &mut self,
        col: u16,
        row: u16,
        lead: Cell,
        previous_width: u8,
        store: &mut GraphemeStore,
    ) -> Mutation {
        let mut mutation = Mutation::none();
        if previous_width >= 2 && col + 1 < self.cols {
            let cont = self.index(col + 1, row);
            if self.cells[cont].role == CellRole::Continuation {
                self.cells[cont] = Cell::blank(self.pen.bg);
            }
        }
        // Release previous store via clear of current lead only if different id.
        let index = self.index(col, row);
        let old = self.cells[index];
        if old.role == CellRole::Lead && old.store_id != lead.store_id {
            Self::release_cell(old, store);
        }
        self.cells[index] = lead;
        if lead.width >= 2 && col + 1 < self.cols {
            mutation = mutation.merge(self.clear_unit_at(col + 1, row, store));
            let cont_index = self.index(col + 1, row);
            self.cells[cont_index] = Cell::continuation();
            self.cell_anchors[cont_index] = None;
        }
        mutation.merge(Mutation::row(row))
    }

    pub(crate) fn pending_wrap(&self) -> bool {
        self.cursor.pending_wrap
    }

    pub(crate) fn set_pending_wrap(&mut self, pending: bool) {
        self.cursor.pending_wrap = pending;
    }

    pub(crate) fn execute(
        &mut self,
        byte: u8,
        line_ids: &mut LineIdAllocator,
        store: &mut GraphemeStore,
    ) -> Result<Mutation, TerminalError> {
        Ok(match byte {
            0x08 => self.backspace(),
            0x09 => self.tab(),
            0x0a..=0x0c => {
                return self.line_feed(line_ids, HistoryBreakAfter::HardBreak, Some(store));
            }
            0x0d => self.carriage_return(),
            _ => Mutation::none(),
        })
    }

    pub(crate) fn cursor_up(&mut self, count: u16) -> Mutation {
        let old = self.cursor.row;
        self.cursor.row = self.cursor.row.saturating_sub(count);
        self.cursor.pending_wrap = false;
        Mutation::rows(old, self.cursor.row)
    }

    pub(crate) fn cursor_down(&mut self, count: u16) -> Mutation {
        let old = self.cursor.row;
        self.cursor.row = self
            .cursor
            .row
            .saturating_add(count)
            .min(self.rows.saturating_sub(1));
        self.cursor.pending_wrap = false;
        Mutation::rows(old, self.cursor.row)
    }

    pub(crate) fn cursor_forward(&mut self, count: u16) -> Mutation {
        let row = self.cursor.row;
        self.cursor.col = self
            .cursor
            .col
            .saturating_add(count)
            .min(self.cols.saturating_sub(1));
        self.cursor.pending_wrap = false;
        Mutation::row(row)
    }

    pub(crate) fn cursor_back(&mut self, count: u16) -> Mutation {
        let row = self.cursor.row;
        self.cursor.col = self.cursor.col.saturating_sub(count);
        self.cursor.pending_wrap = false;
        Mutation::row(row)
    }

    pub(crate) fn set_cursor(&mut self, row: u16, col: u16) -> Mutation {
        let old = self.cursor.row;
        self.cursor.row = row.min(self.rows.saturating_sub(1));
        self.cursor.col = col.min(self.cols.saturating_sub(1));
        self.cursor.pending_wrap = false;
        Mutation::rows(old, self.cursor.row)
    }

    pub(crate) fn set_col(&mut self, col: u16) -> Mutation {
        let row = self.cursor.row;
        self.cursor.col = col.min(self.cols.saturating_sub(1));
        self.cursor.pending_wrap = false;
        Mutation::row(row)
    }

    pub(crate) fn set_row(&mut self, row: u16) -> Mutation {
        let old = self.cursor.row;
        self.cursor.row = row.min(self.rows.saturating_sub(1));
        self.cursor.pending_wrap = false;
        Mutation::rows(old, self.cursor.row)
    }

    pub(crate) fn erase_display(&mut self, mode: u16, store: &mut GraphemeStore) -> Mutation {
        let blank = Cell::blank(self.pen.bg);
        let cursor_index = self.index(self.cursor.col, self.cursor.row);
        match mode {
            0 => {
                for cell in &self.cells[cursor_index..] {
                    Self::release_cell(*cell, store);
                }
                self.cells[cursor_index..].fill(blank);
                Mutation::rows(self.cursor.row, self.rows - 1)
            }
            1 => {
                for cell in &self.cells[..=cursor_index] {
                    Self::release_cell(*cell, store);
                }
                self.cells[..=cursor_index].fill(blank);
                Mutation::rows(0, self.cursor.row)
            }
            2 => {
                for cell in &self.cells {
                    Self::release_cell(*cell, store);
                }
                self.cells.fill(blank);
                Mutation::full(self.rows)
            }
            _ => Mutation::none(),
        }
    }

    pub(crate) fn erase_line(&mut self, mode: u16, store: &mut GraphemeStore) -> Mutation {
        let blank = Cell::blank(self.pen.bg);
        let row = self.cursor.row;
        let start = usize::from(row) * usize::from(self.cols);
        let end = start + usize::from(self.cols);
        let col = usize::from(self.cursor.col);
        match mode {
            0 => {
                for cell in &self.cells[start + col..end] {
                    Self::release_cell(*cell, store);
                }
                self.cells[start + col..end].fill(blank);
            }
            1 => {
                for cell in &self.cells[start..=start + col] {
                    Self::release_cell(*cell, store);
                }
                self.cells[start..=start + col].fill(blank);
            }
            2 => {
                for cell in &self.cells[start..end] {
                    Self::release_cell(*cell, store);
                }
                self.cells[start..end].fill(blank);
            }
            _ => return Mutation::none(),
        }
        Mutation::row(row)
    }

    pub(crate) fn save_cursor(&mut self) {
        self.saved_cursor = Some(SavedCursor {
            cursor: self.cursor,
            style: self.pen,
        });
    }

    pub(crate) fn restore_cursor(&mut self) -> Mutation {
        let Some(saved) = self.saved_cursor else {
            return Mutation::none();
        };
        let old = self.cursor.row;
        self.cursor = saved.cursor;
        self.cursor.clamp(self.cols, self.rows);
        self.pen = saved.style;
        Mutation::rows(old, self.cursor.row)
    }

    pub(crate) fn apply_sgr(&mut self, params: &[u16]) -> bool {
        if params.is_empty() {
            self.pen = Style::default();
            return false;
        }

        let mut deferred = false;
        let mut index = 0;
        while index < params.len() {
            match params[index] {
                0 => self.pen = Style::default(),
                1 => self.pen.bold = true,
                22 => self.pen.bold = false,
                4 => self.pen.underline = true,
                24 => self.pen.underline = false,
                7 => self.pen.inverse = true,
                27 => self.pen.inverse = false,
                30..=37 => self.pen.fg = Color::Indexed((params[index] - 30) as u8),
                39 => self.pen.fg = Color::Default,
                40..=47 => self.pen.bg = Color::Indexed((params[index] - 40) as u8),
                49 => self.pen.bg = Color::Default,
                90..=97 => self.pen.fg = Color::Indexed((params[index] - 90 + 8) as u8),
                100..=107 => self.pen.bg = Color::Indexed((params[index] - 100 + 8) as u8),
                38 | 48 => {
                    let foreground = params[index] == 38;
                    match params.get(index + 1).copied() {
                        Some(5) if index + 2 < params.len() => {
                            let color = Color::Indexed(params[index + 2].min(255) as u8);
                            if foreground {
                                self.pen.fg = color;
                            } else {
                                self.pen.bg = color;
                            }
                            index += 2;
                        }
                        Some(2) if index + 4 < params.len() => {
                            let color = Color::Rgb {
                                r: params[index + 2].min(255) as u8,
                                g: params[index + 3].min(255) as u8,
                                b: params[index + 4].min(255) as u8,
                            };
                            if foreground {
                                self.pen.fg = color;
                            } else {
                                self.pen.bg = color;
                            }
                            index += 4;
                        }
                        _ => deferred = true,
                    }
                }
                _ => deferred = true,
            }
            index += 1;
        }
        deferred
    }

    fn backspace(&mut self) -> Mutation {
        let row = self.cursor.row;
        self.cursor.col = self.cursor.col.saturating_sub(1);
        self.cursor.pending_wrap = false;
        Mutation::row(row)
    }

    fn tab(&mut self) -> Mutation {
        let row = self.cursor.row;
        let next = (self.cursor.col / 8).saturating_add(1).saturating_mul(8);
        self.cursor.col = next.min(self.cols.saturating_sub(1));
        self.cursor.pending_wrap = false;
        Mutation::row(row)
    }

    fn carriage_return(&mut self) -> Mutation {
        let row = self.cursor.row;
        self.cursor.col = 0;
        self.cursor.pending_wrap = false;
        Mutation::row(row)
    }

    /// IND / LF within the scroll region: scroll at the bottom margin.
    fn line_feed(
        &mut self,
        line_ids: &mut LineIdAllocator,
        break_after: HistoryBreakAfter,
        store: Option<&mut GraphemeStore>,
    ) -> Result<Mutation, TerminalError> {
        self.cursor.pending_wrap = false;
        let old = self.cursor.row;
        self.row_breaks[usize::from(self.cursor.row)] = Some(break_after);
        let source_line_id = self.current_source_line_id(self.cursor.row);
        self.source_breaks.insert(source_line_id, break_after);
        if self.cursor.row == self.scroll_bottom {
            return self.scroll_up(1, line_ids, store, break_after);
        }
        if self.cursor.row < self.rows.saturating_sub(1) {
            self.cursor.row += 1;
            return Ok(Mutation::rows(old, self.cursor.row));
        }
        Ok(Mutation::row(old))
    }

    /// ESC D — Index (same scroll rules as LF, without carriage return).
    pub(crate) fn index_down(
        &mut self,
        line_ids: &mut LineIdAllocator,
        store: &mut GraphemeStore,
    ) -> Result<Mutation, TerminalError> {
        self.line_feed(line_ids, HistoryBreakAfter::HardBreak, Some(store))
    }

    /// ESC M — Reverse Index.
    pub(crate) fn reverse_index(
        &mut self,
        line_ids: &mut LineIdAllocator,
        store: &mut GraphemeStore,
    ) -> Result<Mutation, TerminalError> {
        self.cursor.pending_wrap = false;
        let old = self.cursor.row;
        if self.cursor.row == self.scroll_top {
            return self.scroll_down(1, line_ids, Some(store));
        }
        if self.cursor.row > 0 {
            self.cursor.row -= 1;
            return Ok(Mutation::rows(old, self.cursor.row));
        }
        Ok(Mutation::row(old))
    }

    /// ESC E — Next Line (CR + Index).
    pub(crate) fn next_line(
        &mut self,
        line_ids: &mut LineIdAllocator,
        store: &mut GraphemeStore,
    ) -> Result<Mutation, TerminalError> {
        let cr = self.carriage_return();
        Ok(cr.merge(self.index_down(line_ids, store)?))
    }

    /// CSI S — Scroll Up (SU) inside the current region.
    pub(crate) fn scroll_up(
        &mut self,
        count: u16,
        line_ids: &mut LineIdAllocator,
        store: Option<&mut GraphemeStore>,
        break_after: HistoryBreakAfter,
    ) -> Result<Mutation, TerminalError> {
        let count = count.max(1);
        let region_height = self
            .scroll_bottom
            .saturating_sub(self.scroll_top)
            .saturating_add(1);
        let n = count.min(region_height);
        if n == 0 {
            return Ok(Mutation::none());
        }
        self.shift_region_rows_up(
            self.scroll_top,
            self.scroll_bottom,
            n,
            line_ids,
            store,
            break_after,
        )
    }

    /// CSI T — Scroll Down (SD) inside the current region.
    pub(crate) fn scroll_down(
        &mut self,
        count: u16,
        line_ids: &mut LineIdAllocator,
        store: Option<&mut GraphemeStore>,
    ) -> Result<Mutation, TerminalError> {
        let count = count.max(1);
        let region_height = self
            .scroll_bottom
            .saturating_sub(self.scroll_top)
            .saturating_add(1);
        let n = count.min(region_height);
        if n == 0 {
            return Ok(Mutation::none());
        }
        self.shift_region_rows_down(self.scroll_top, self.scroll_bottom, n, line_ids, store)
    }

    /// CSI L — Insert Lines at the cursor row within the scroll region.
    pub(crate) fn insert_lines(
        &mut self,
        count: u16,
        line_ids: &mut LineIdAllocator,
        store: &mut GraphemeStore,
    ) -> Result<Mutation, TerminalError> {
        if self.cursor.row < self.scroll_top || self.cursor.row > self.scroll_bottom {
            return Ok(Mutation::none());
        }
        let count = count.max(1);
        let available = self
            .scroll_bottom
            .saturating_sub(self.cursor.row)
            .saturating_add(1);
        let n = count.min(available);
        self.cursor.pending_wrap = false;
        self.shift_region_rows_down(
            self.cursor.row,
            self.scroll_bottom,
            n,
            line_ids,
            Some(store),
        )
    }

    /// CSI M — Delete Lines at the cursor row within the scroll region.
    pub(crate) fn delete_lines(
        &mut self,
        count: u16,
        line_ids: &mut LineIdAllocator,
        store: &mut GraphemeStore,
    ) -> Result<Mutation, TerminalError> {
        if self.cursor.row < self.scroll_top || self.cursor.row > self.scroll_bottom {
            return Ok(Mutation::none());
        }
        let count = count.max(1);
        let available = self
            .scroll_bottom
            .saturating_sub(self.cursor.row)
            .saturating_add(1);
        let n = count.min(available);
        self.cursor.pending_wrap = false;
        self.shift_region_rows_up(
            self.cursor.row,
            self.scroll_bottom,
            n,
            line_ids,
            Some(store),
            HistoryBreakAfter::HardBreak,
        )
    }

    /// CSI @ — Insert Characters at the cursor.
    pub(crate) fn insert_characters(&mut self, count: u16, store: &mut GraphemeStore) -> Mutation {
        let count = count.max(1);
        let row = self.cursor.row;
        let col = self.cursor.col;
        if col >= self.cols {
            return Mutation::none();
        }
        self.cursor.pending_wrap = false;
        let cols = usize::from(self.cols);
        let start = usize::from(row) * cols;
        let insert_at = start + usize::from(col);
        let n = usize::from(count).min(cols - usize::from(col));
        if n == 0 {
            return Mutation::none();
        }
        // Release cells that will fall off the right edge.
        for cell in &self.cells[start + cols - n..start + cols] {
            Self::release_cell(*cell, store);
        }
        self.cells
            .copy_within(insert_at..start + cols - n, insert_at + n);
        self.cell_anchors
            .copy_within(insert_at..start + cols - n, insert_at + n);
        let blank = Cell::blank(self.pen.bg);
        self.cells[insert_at..insert_at + n].fill(blank);
        self.cell_anchors[insert_at..insert_at + n].fill(None);
        self.sanitize_row(row, store);
        Mutation::row(row)
    }

    /// CSI P — Delete Characters at the cursor.
    pub(crate) fn delete_characters(&mut self, count: u16, store: &mut GraphemeStore) -> Mutation {
        let count = count.max(1);
        let row = self.cursor.row;
        let col = self.cursor.col;
        if col >= self.cols {
            return Mutation::none();
        }
        self.cursor.pending_wrap = false;
        let cols = usize::from(self.cols);
        let start = usize::from(row) * cols;
        let delete_at = start + usize::from(col);
        let n = usize::from(count).min(cols - usize::from(col));
        if n == 0 {
            return Mutation::none();
        }
        for cell in &self.cells[delete_at..delete_at + n] {
            Self::release_cell(*cell, store);
        }
        self.cells
            .copy_within(delete_at + n..start + cols, delete_at);
        self.cell_anchors
            .copy_within(delete_at + n..start + cols, delete_at);
        let blank = Cell::blank(self.pen.bg);
        self.cells[start + cols - n..start + cols].fill(blank);
        self.cell_anchors[start + cols - n..start + cols].fill(None);
        self.sanitize_row(row, store);
        Mutation::row(row)
    }

    /// CSI X — Erase Characters at the cursor (no shift).
    pub(crate) fn erase_characters(&mut self, count: u16, store: &mut GraphemeStore) -> Mutation {
        let count = count.max(1);
        let row = self.cursor.row;
        let col = self.cursor.col;
        if col >= self.cols {
            return Mutation::none();
        }
        self.cursor.pending_wrap = false;
        let end_col = col.saturating_add(count).min(self.cols);
        let mut mutation = Mutation::none();
        let mut c = col;
        while c < end_col {
            mutation = mutation.merge(self.clear_unit_at(c, row, store));
            c = c.saturating_add(1);
        }
        mutation
    }

    fn shift_region_rows_up(
        &mut self,
        top: u16,
        bottom: u16,
        count: u16,
        line_ids: &mut LineIdAllocator,
        mut store: Option<&mut GraphemeStore>,
        break_after: HistoryBreakAfter,
    ) -> Result<Mutation, TerminalError> {
        let cols = usize::from(self.cols);
        let top_i = usize::from(top);
        let bottom_i = usize::from(bottom);
        let n = usize::from(count);
        let region_rows = bottom_i - top_i + 1;
        if n == 0 || n > region_rows {
            return Ok(Mutation::none());
        }

        // Evict scrolled-away rows: full-screen primary-compatible retention only.
        if self.retain_history && self.region_is_full_screen() && top == 0 {
            for row in 0..n {
                let row_start = (top_i + row) * cols;
                let source_break = self.row_breaks[top_i + row].unwrap_or(break_after);
                if let Some(store) = store.as_deref() {
                    self.append_row_to_history(top_i + row, source_break, store);
                }
                if let Some(store) = store.as_deref_mut() {
                    for cell in &self.cells[row_start..row_start + cols] {
                        Self::release_cell(*cell, store);
                    }
                }
            }
        } else if let Some(store) = store.as_mut() {
            for row in 0..n {
                let row_start = (top_i + row) * cols;
                for cell in &self.cells[row_start..row_start + cols] {
                    Self::release_cell(*cell, store);
                }
            }
        }

        let keep = region_rows - n;
        if keep > 0 {
            let src = (top_i + n) * cols;
            let dst = top_i * cols;
            let len = keep * cols;
            self.cells.copy_within(src..src + len, dst);
            self.cell_anchors.copy_within(src..src + len, dst);
            self.line_ids
                .copy_within(top_i + n..top_i + n + keep, top_i);
            self.row_breaks
                .copy_within(top_i + n..top_i + n + keep, top_i);
        }

        let blank = Cell::blank(self.pen.bg);
        for row in 0..n {
            let row_index = bottom_i + 1 - n + row;
            let row_start = row_index * cols;
            // Bottom rows still hold original content after the upward move.
            if let Some(store) = store.as_mut() {
                for cell in &self.cells[row_start..row_start + cols] {
                    Self::release_cell(*cell, store);
                }
            }
            self.cells[row_start..row_start + cols].fill(blank);
            self.cell_anchors[row_start..row_start + cols].fill(None);
            self.line_ids[row_index] = line_ids.allocate()?;
            self.row_breaks[row_index] = Some(HistoryBreakAfter::HardBreak);
        }
        Ok(Mutation::rows(top, bottom))
    }

    fn shift_region_rows_down(
        &mut self,
        top: u16,
        bottom: u16,
        count: u16,
        line_ids: &mut LineIdAllocator,
        mut store: Option<&mut GraphemeStore>,
    ) -> Result<Mutation, TerminalError> {
        let cols = usize::from(self.cols);
        let top_i = usize::from(top);
        let bottom_i = usize::from(bottom);
        let n = usize::from(count);
        let region_rows = bottom_i - top_i + 1;
        if n == 0 || n > region_rows {
            return Ok(Mutation::none());
        }

        if let Some(store) = store.as_mut() {
            for row in 0..n {
                let row_index = bottom_i + 1 - n + row;
                let row_start = row_index * cols;
                for cell in &self.cells[row_start..row_start + cols] {
                    Self::release_cell(*cell, store);
                }
            }
        }

        let keep = region_rows - n;
        if keep > 0 {
            let src = top_i * cols;
            let len = keep * cols;
            let dst = (top_i + n) * cols;
            self.cells.copy_within(src..src + len, dst);
            self.cell_anchors.copy_within(src..src + len, dst);
            for row in (0..keep).rev() {
                self.line_ids[top_i + row + n] = self.line_ids[top_i + row];
                self.row_breaks[top_i + row + n] = self.row_breaks[top_i + row];
            }
        }

        let blank = Cell::blank(self.pen.bg);
        for row in 0..n {
            let row_index = top_i + row;
            let row_start = row_index * cols;
            // Top rows are leftovers of the memmove-down source; payloads now
            // live in the shifted rows, so blank without releasing.
            self.cells[row_start..row_start + cols].fill(blank);
            self.cell_anchors[row_start..row_start + cols].fill(None);
            self.line_ids[row_index] = line_ids.allocate()?;
            self.row_breaks[row_index] = Some(HistoryBreakAfter::HardBreak);
        }
        Ok(Mutation::rows(top, bottom))
    }

    /// After in-row cell shifts, blank orphan lead/continuation halves.
    fn sanitize_row(&mut self, row: u16, store: &mut GraphemeStore) {
        let cols = usize::from(self.cols);
        let start = usize::from(row) * cols;
        let mut col = 0usize;
        while col < cols {
            let cell = self.cells[start + col];
            match cell.role {
                CellRole::Lead if cell.width >= 2 => {
                    if col + 1 >= cols || self.cells[start + col + 1].role != CellRole::Continuation
                    {
                        Self::release_cell(cell, store);
                        self.cells[start + col] = Cell::blank(self.pen.bg);
                    } else {
                        col += 1;
                    }
                }
                CellRole::Continuation
                    if col == 0 || self.cells[start + col - 1].role != CellRole::Lead =>
                {
                    self.cells[start + col] = Cell::blank(self.pen.bg);
                }
                _ => {}
            }
            col += 1;
        }
    }

    fn index(&self, col: u16, row: u16) -> usize {
        usize::from(row) * usize::from(self.cols) + usize::from(col)
    }

    fn current_source_line_id(&self, row: u16) -> LineId {
        let start = usize::from(row) * usize::from(self.cols);
        let end = start + usize::from(self.cols);
        self.cell_anchors[start..end]
            .iter()
            .rev()
            .flatten()
            .next()
            .map_or(self.line_ids[usize::from(row)], |anchor| anchor.line_id)
    }

    fn append_row_to_history(
        &mut self,
        row: usize,
        fallback_break: HistoryBreakAfter,
        store: &GraphemeStore,
    ) {
        let start = row * usize::from(self.cols);
        let end = start + usize::from(self.cols);
        let mut fragments: Vec<HistoryLine> = Vec::new();
        for (col, cell) in self.cells[start..end].iter().enumerate() {
            if cell.role != CellRole::Lead {
                continue;
            }
            let anchor = self.cell_anchors[start + col].unwrap_or(crate::HistoryAnchor {
                line_id: self.line_ids[row],
                unit_offset: fragments.len() as u32,
            });
            let mut fragment = HistoryLine::from_cells(
                anchor.line_id,
                HistoryBreakAfter::SoftWrap,
                std::slice::from_ref(cell),
                store,
            );
            fragment.start_offset = anchor.unit_offset;
            if let Some(previous) = fragments.last_mut().filter(|previous| {
                previous.line_id == fragment.line_id
                    && previous
                        .start_offset
                        .saturating_add(u32::try_from(previous.units.len()).unwrap_or(u32::MAX))
                        == fragment.start_offset
            }) {
                previous.units.extend(fragment.units);
            } else {
                fragments.push(fragment);
            }
        }
        if fragments.is_empty() {
            let line_id = self.line_ids[row];
            self.history
                .append_row(line_id, fallback_break, &self.cells[start..end], store);
            self.source_breaks.remove(&line_id);
            return;
        }
        let mut last_fragment = HashMap::new();
        for (index, fragment) in fragments.iter().enumerate() {
            last_fragment.insert(fragment.line_id, index);
        }
        for (line_id, index) in last_fragment {
            fragments[index].break_after = self
                .source_breaks
                .get(&line_id)
                .copied()
                .unwrap_or(fallback_break);
        }
        for fragment in fragments {
            self.source_breaks.remove(&fragment.line_id);
            self.history.append_line(fragment);
        }
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
