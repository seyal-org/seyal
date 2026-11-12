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
        // Seal LineId must match HistoryStore fragment anchors (source units),
        // not only the Screen row LineId spine — otherwise RegionId visitors
        // skip every sealed row and AX/retain collapse to the live window.
        let start = row * usize::from(self.cols);
        let end = start + usize::from(self.cols);
        let seal_line_id = self.engine.store.grid_anchors[start..end]
            .iter()
            .zip(self.engine.store.grid_cells[start..end].iter())
            .find(|(_, cell)| cell.role == CellRole::Lead)
            .and_then(|(anchor, _)| anchor.map(|anchor| anchor.line_id))
            .unwrap_or_else(|| self.current_source_line_id(row as u16));
        let _ = self.engine.seal_window_row_indexed(row as u16, seal_line_id);
        let mut fragments: Vec<HistoryLine> = Vec::new();
        for (col, cell) in self.engine.store.grid_cells[start..end].iter().enumerate() {
            if cell.role != CellRole::Lead {
                continue;
            }
            let anchor =
                self.engine.store.grid_anchors[start + col].unwrap_or(crate::HistoryAnchor {
                    line_id: seal_line_id,
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
            // Same LineId as the sealed ownership index above — mismatch blanks
            // RegionId-filtered HistoryStore paint (#1484).
            self.history.append_row(
                seal_line_id,
                fallback_break,
                &self.engine.store.grid_cells[start..end],
                store,
            );
            self.source_breaks.remove(&seal_line_id);
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
