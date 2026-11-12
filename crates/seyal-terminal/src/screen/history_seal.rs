//! HistoryStore seal handoff + RegionGrid ownership indexing (#1484).

use super::Screen;
use crate::{
    grapheme_store::GraphemeStore,
    history::{HistoryBreakAfter, HistoryLine},
    CellRole, LineId,
};
use std::collections::HashMap;

impl Screen {
    pub(in crate::screen) fn current_source_line_id(&self, row: u16) -> LineId {
        let start = usize::from(row) * usize::from(self.cols);
        let end = start + usize::from(self.cols);
        self.engine.store.grid_anchors[start..end]
            .iter()
            .rev()
            .flatten()
            .next()
            .map_or(self.line_ids[usize::from(row)], |anchor| anchor.line_id)
    }

    pub(in crate::screen) fn append_row_to_history(
        &mut self,
        row: usize,
        fallback_break: HistoryBreakAfter,
        store: &GraphemeStore,
    ) {
        // #1484: HistoryStore is the sole sealed payload owner (ADR-009). Index
        // RegionId column ownership on RegionGrids without a second cell copy.
        let line_id = self.line_ids.get(row).copied().unwrap_or(LineId(1));
        let _ = self.engine.seal_window_row_indexed(row as u16, line_id);
        let start = row * usize::from(self.cols);
        let end = start + usize::from(self.cols);
        let mut fragments: Vec<HistoryLine> = Vec::new();
        for (col, cell) in self.engine.store.grid_cells[start..end].iter().enumerate() {
            if cell.role != CellRole::Lead {
                continue;
            }
            let anchor =
                self.engine.store.grid_anchors[start + col].unwrap_or(crate::HistoryAnchor {
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
            self.history.append_row(
                line_id,
                fallback_break,
                &self.engine.store.grid_cells[start..end],
                store,
            );
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
