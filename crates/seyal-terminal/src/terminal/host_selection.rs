//! Host selection, search, and copy-mode orchestration on TerminalState.

use super::state::TerminalState;
use crate::{
    damage::Mutation,
    selection::{
        encode_paste, order_visual, skip_continuation, CopyMode, CopyModeMotion, PasteError,
        SearchSession, SelectionKind, SelectionSession, VisualPos, MAX_SEARCH_MATCHES,
    },
    CellRole, HistoryAnchor, HistoryBreakAfter, HistoryMatch, HistoryRangeError,
};

impl TerminalState {
    pub fn encode_host_paste(&self, bytes: &[u8]) -> Result<Vec<u8>, PasteError> {
        encode_paste(bytes, self.core.modes.bracketed_paste)
    }

    pub fn selection_session(&self) -> SelectionSession {
        self.core.selection
    }

    pub fn search_session(&self) -> &SearchSession {
        &self.core.search
    }

    pub fn copy_mode(&self) -> CopyMode {
        self.core.copy_mode
    }

    pub fn clear_selection(&mut self) {
        self.core.selection.clear();
        self.core.copy_buffer = None;
        self.bump_selection_damage();
    }

    pub fn set_linear_selection(&mut self, start: VisualPos, end: VisualPos) {
        if let Some((start, end)) = Self::clamp_visual_pair(self.cols(), self.rows(), start, end) {
            let start_anchor = self.anchor_at(start.col, start.row);
            let end_anchor = self.anchor_at(end.col, end.row);
            self.core
                .selection
                .set_linear(start, end, start_anchor, end_anchor);
            self.bump_selection_damage();
        }
    }

    pub fn set_rectangular_selection(&mut self, start: VisualPos, end: VisualPos) {
        if let Some((start, end)) = Self::clamp_visual_pair(self.cols(), self.rows(), start, end) {
            let start_anchor = self.anchor_at(start.col, start.row);
            let end_anchor = self.anchor_at(end.col, end.row);
            self.core
                .selection
                .set_rectangular(start, end, start_anchor, end_anchor);
            self.bump_selection_damage();
        }
    }

    pub fn copy_selection_text(&self) -> Result<String, HistoryRangeError> {
        // Linear selections with captured source anchors copy from canonical
        // history so scroll/reflow cannot silently retarget the endpoints.
        if self.core.selection.kind == SelectionKind::Linear
            && !self.core.modes.alternate_screen
            && let Some((start, end)) = self.core.selection.ordered_anchors()
        {
            return self.copy_history_range(start, end);
        }
        let Some((start, end)) = self.core.selection.ordered_corners() else {
            return Err(HistoryRangeError::Stale);
        };
        match self.core.selection.kind {
            SelectionKind::Linear => self.copy_visual_linear(start, end),
            SelectionKind::Rectangular => self.copy_visual_rectangular(start, end),
        }
    }

    /// True when the cell is inside the current host selection.
    ///
    /// Linear selections with source anchors use those anchors (SPEC-010 §11)
    /// so scrolled-off content does not keep highlighting new occupants.
    pub fn selection_covers_cell(&self, col: u16, row: u16) -> bool {
        let selection = self.core.selection;
        if selection.kind == SelectionKind::Linear
            && !self.core.modes.alternate_screen
            && let Some((lo, hi)) = selection.ordered_anchors()
        {
            return self
                .anchor_at(col, row)
                .is_some_and(|anchor| anchor >= lo && anchor <= hi);
        }
        selection.contains_cell(col, row, self.cols())
    }

    pub fn search_and_select(&mut self, needle: &str, forward: bool) -> Option<HistoryMatch> {
        if needle.is_empty() {
            self.core.search.clear();
            return None;
        }
        if self.core.search.needle != needle {
            self.core.search.needle = needle.to_owned();
            self.core.search.index = None;
        }
        let matches = self.primary_history_search(needle, MAX_SEARCH_MATCHES);
        let found = self.core.search.step(&matches, forward).copied()?;
        let start_visual = self.visual_pos_for_anchor(found.start);
        let end_visual = self.visual_pos_for_anchor(found.end);
        // Prefer resolved visual corners when both are on-screen; otherwise
        // still retain the canonical search anchors for copy.
        match (start_visual, end_visual) {
            (Some(start), Some(end)) => {
                self.core
                    .selection
                    .set_linear(start, end, Some(found.start), Some(found.end));
            }
            _ => {
                self.core.selection.kind = SelectionKind::Linear;
                self.core.selection.start = start_visual;
                self.core.selection.end = end_visual;
                self.core.selection.start_anchor = Some(found.start);
                self.core.selection.end_anchor = Some(found.end);
            }
        }
        self.bump_selection_damage();
        Some(found)
    }

    pub fn enter_copy_mode(&mut self) {
        let cursor = self.cursor();
        self.core.copy_mode.enter(VisualPos {
            col: cursor.col,
            row: cursor.row,
        });
        self.bump_selection_damage();
    }

    pub fn exit_copy_mode(&mut self) {
        self.core.copy_mode.exit();
        self.bump_selection_damage();
    }

    pub fn copy_mode_motion(&mut self, motion: CopyModeMotion) {
        if !self.core.copy_mode.active {
            return;
        }
        let moving_right = matches!(motion, CopyModeMotion::Right | CopyModeMotion::LineEnd);
        self.core
            .copy_mode
            .apply_motion(motion, self.cols(), self.rows());
        if let Some(cell) = self.cell(
            self.core.copy_mode.cursor.col,
            self.core.copy_mode.cursor.row,
        ) {
            self.core.copy_mode.cursor.col = skip_continuation(
                cell.role,
                moving_right,
                self.core.copy_mode.cursor.col,
                self.cols(),
            );
        }
        if let Some((start, end, kind)) = self.core.copy_mode.selection() {
            self.apply_copy_mode_selection(start, end, kind);
        }
        self.bump_selection_damage();
    }

    pub fn copy_mode_toggle_anchor(&mut self) {
        self.core.copy_mode.toggle_anchor();
        if let Some((start, end, kind)) = self.core.copy_mode.selection() {
            self.apply_copy_mode_selection(start, end, kind);
        }
        self.bump_selection_damage();
    }

    pub fn copy_mode_toggle_kind(&mut self) {
        self.core.copy_mode.toggle_kind();
        if let Some((start, end, kind)) = self.core.copy_mode.selection() {
            self.apply_copy_mode_selection(start, end, kind);
        }
        self.bump_selection_damage();
    }

    fn apply_copy_mode_selection(&mut self, start: VisualPos, end: VisualPos, kind: SelectionKind) {
        let start_anchor = self.anchor_at(start.col, start.row);
        let end_anchor = self.anchor_at(end.col, end.row);
        match kind {
            SelectionKind::Linear => {
                self.core
                    .selection
                    .set_linear(start, end, start_anchor, end_anchor);
            }
            SelectionKind::Rectangular => {
                self.core
                    .selection
                    .set_rectangular(start, end, start_anchor, end_anchor);
            }
        }
    }

    pub fn yank_selection(&mut self) -> Result<String, HistoryRangeError> {
        let text = self.copy_selection_text()?;
        self.core.copy_buffer = Some(text.clone());
        self.core.copy_mode.exit();
        self.bump_selection_damage();
        Ok(text)
    }

    fn bump_selection_damage(&mut self) {
        let rows = self.rows();
        self.core.damage.mark(Mutation::full(rows));
        self.core.damage.commit();
    }

    pub fn take_copy_buffer(&mut self) -> Option<String> {
        self.core.copy_buffer.take()
    }

    fn clamp_visual_pair(
        cols: u16,
        rows: u16,
        start: VisualPos,
        end: VisualPos,
    ) -> Option<(VisualPos, VisualPos)> {
        if cols == 0 || rows == 0 {
            return None;
        }
        let clamp = |pos: VisualPos| VisualPos {
            col: pos.col.min(cols.saturating_sub(1)),
            row: pos.row.min(rows.saturating_sub(1)),
        };
        Some((clamp(start), clamp(end)))
    }

    fn cell_copy_text(&self, col: u16, row: u16) -> Option<String> {
        let cell = self.cell(col, row)?;
        match cell.role {
            CellRole::Empty => Some(" ".to_owned()),
            CellRole::Continuation => Some(String::new()),
            CellRole::Lead => {
                let utf8 = self.lead_utf8(col, row)?;
                Some(String::from_utf8_lossy(&utf8).into_owned())
            }
        }
    }

    fn copy_visual_linear(
        &self,
        start: VisualPos,
        end: VisualPos,
    ) -> Result<String, HistoryRangeError> {
        let (start, end) = order_visual(start, end);
        if let (Some(start_anchor), Some(end_anchor)) = (
            self.anchor_at(start.col, start.row),
            self.anchor_at(end.col, end.row),
        ) && !self.core.modes.alternate_screen
        {
            let (lo, hi) = if start_anchor <= end_anchor {
                (start_anchor, end_anchor)
            } else {
                (end_anchor, start_anchor)
            };
            if let Ok(text) = self.copy_history_range(lo, hi) {
                return Ok(text);
            }
        }
        self.copy_visual_cells(start, end, false)
    }

    fn copy_visual_rectangular(
        &self,
        start: VisualPos,
        end: VisualPos,
    ) -> Result<String, HistoryRangeError> {
        self.copy_visual_cells(start, end, true)
    }

    fn copy_visual_cells(
        &self,
        start: VisualPos,
        end: VisualPos,
        rectangular: bool,
    ) -> Result<String, HistoryRangeError> {
        let min_row = start.row.min(end.row);
        let max_row = start.row.max(end.row);
        let min_col = start.col.min(end.col);
        let max_col = start.col.max(end.col);
        let mut out = String::new();
        for row in min_row..=max_row {
            let (row_start, row_end) = if rectangular {
                (min_col, max_col)
            } else if row == min_row && row == max_row {
                (start.col.min(end.col), start.col.max(end.col))
            } else if row == min_row {
                if (start.row, start.col) <= (end.row, end.col) {
                    (start.col, self.cols().saturating_sub(1))
                } else {
                    (end.col, self.cols().saturating_sub(1))
                }
            } else if row == max_row {
                if (start.row, start.col) <= (end.row, end.col) {
                    (0, end.col)
                } else {
                    (0, start.col)
                }
            } else {
                (0, self.cols().saturating_sub(1))
            };
            for col in row_start..=row_end {
                if let Some(text) = self.cell_copy_text(col, row) {
                    if out.len().saturating_add(text.len()) > crate::MAX_COPY_BYTES {
                        return Err(HistoryRangeError::Unrepresentable);
                    }
                    out.push_str(&text);
                }
            }
            let emit_break = if rectangular {
                row < max_row
            } else {
                row < max_row
                    && self.core.current().row_break_after(row)
                        == Some(HistoryBreakAfter::HardBreak)
            };
            if emit_break {
                if out.len().saturating_add(1) > crate::MAX_COPY_BYTES {
                    return Err(HistoryRangeError::Unrepresentable);
                }
                out.push('\n');
            }
        }
        Ok(out)
    }

    fn anchor_at(&self, col: u16, row: u16) -> Option<HistoryAnchor> {
        self.core.current().cell_anchor(col, row)
    }

    fn visual_pos_for_anchor(&self, anchor: HistoryAnchor) -> Option<VisualPos> {
        for row in 0..self.rows() {
            for col in 0..self.cols() {
                if self.anchor_at(col, row) == Some(anchor) {
                    return Some(VisualPos { col, row });
                }
            }
        }
        None
    }
}
