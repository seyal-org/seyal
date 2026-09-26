//! Active screen grid, cursor, scroll region, and history-seal handoff.
//!
//! `Screen` is the sole authority for the live cell grid. Sibling modules add
//! `impl Screen` blocks by responsibility; there is still exactly one `Screen`
//! type and one live buffer.

mod ops;
mod resize;

pub(crate) use resize::PreparedScreen;

use crate::{
    cursor::Cursor,
    damage::Mutation,
    grapheme_store::GraphemeStore,
    history::{HistoryBreakAfter, HistoryLineRef, HistoryStore},
    line::LineIdAllocator,
    Cell, CellRole, CursorState, LineId, Style, TerminalError,
};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug)]
pub(in crate::screen) struct SavedCursor {
    pub(in crate::screen) cursor: Cursor,
    pub(in crate::screen) style: Style,
}

pub(crate) struct Screen {
    pub(in crate::screen) cols: u16,
    pub(in crate::screen) rows: u16,
    pub(in crate::screen) cells: Vec<Cell>,
    pub(in crate::screen) cell_anchors: Vec<Option<crate::HistoryAnchor>>,
    pub(in crate::screen) line_ids: Vec<LineId>,
    pub(in crate::screen) row_breaks: Vec<Option<HistoryBreakAfter>>,
    pub(in crate::screen) source_breaks: HashMap<LineId, HistoryBreakAfter>,
    pub(in crate::screen) cursor: Cursor,
    pub(in crate::screen) pen: Style,
    pub(in crate::screen) saved_cursor: Option<SavedCursor>,
    pub(in crate::screen) history: HistoryStore,
    pub(in crate::screen) retain_history: bool,
    /// Inclusive 0-based DECSTBM top margin.
    pub(in crate::screen) scroll_top: u16,
    /// Inclusive 0-based DECSTBM bottom margin.
    pub(in crate::screen) scroll_bottom: u16,
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

    pub(in crate::screen) fn region_is_full_screen(&self) -> bool {
        self.scroll_top == 0 && self.scroll_bottom + 1 == self.rows
    }

    pub(in crate::screen) fn clamp_scroll_region_to_geometry(&mut self) {
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

    pub(in crate::screen) fn release_cell(cell: Cell, store: &mut GraphemeStore) {
        if cell.role == CellRole::Lead {
            store.release(cell.store_id);
        }
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

    pub(in crate::screen) fn index(&self, col: u16, row: u16) -> usize {
        usize::from(row) * usize::from(self.cols) + usize::from(col)
    }
}
