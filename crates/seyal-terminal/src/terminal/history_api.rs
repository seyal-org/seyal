//! Primary history projection and wire helpers on TerminalState.

use super::state::TerminalState;
use crate::{
    selection::format_history_copy, Cell, CellRole, HistoryAnchor, HistoryAnchorResolution,
    HistoryBreakAfter, HistoryMatch, HistoryRangeError, HistoryUnitView, HistoryWireCell, LineId,
    ReflowRow,
};

impl TerminalState {
    /// Returns a bounded primary-screen history range. The returned rows are
    /// an explicit read-only projection of **primary** retained history plus
    /// primary visible rows. Alternate-screen cells are never included; the
    /// portable API still returns primary history while alternate screen is
    /// active so embedders (not VT) own Blocks/TUI presentation policy.
    ///
    /// Work is bounded by retained history plus visible rows, never by the
    /// numeric distance between `start` and `end` (LineIds may be sparse).
    pub fn primary_history_range(
        &self,
        start: LineId,
        end: LineId,
        max_lines: usize,
    ) -> Result<Vec<(LineId, Vec<Cell>)>, HistoryRangeError> {
        if max_lines == 0 || end < start {
            return Ok(Vec::new());
        }
        if self
            .core
            .primary
            .history()
            .range_intersects_evicted(start, end)
        {
            return Err(HistoryRangeError::Stale);
        }
        let mut lines: Vec<(LineId, Vec<Cell>)> = Vec::new();
        for entry in self.core.primary.history_entries() {
            let id = entry.line_id();
            if id < start {
                continue;
            }
            if id > end {
                break;
            }
            let cells = entry.presentation_cells();
            if let Some((last_id, last_cells)) = lines.last_mut()
                && *last_id == id
            {
                last_cells.extend(cells);
            } else {
                lines.push((id, cells));
                if lines.len() >= max_lines {
                    return Ok(lines);
                }
            }
        }
        for row in 0..self.core.primary.rows() {
            let Some(id) = self.core.primary.line_id(row) else {
                continue;
            };
            if id < start || id > end {
                continue;
            }
            if lines.iter().any(|(existing, _)| *existing == id) {
                continue;
            }
            let Some(cells) = self.core.primary.cell_row(row) else {
                continue;
            };
            lines.push((id, cells.to_vec()));
            if lines.len() >= max_lines {
                break;
            }
        }
        Ok(lines)
    }

    /// History rows for the Pass-7 snapshot wire, including full grapheme
    /// UTF-8. Continuation placeholders carry empty text.
    pub fn primary_history_wire_range(
        &self,
        start: LineId,
        end: LineId,
        max_lines: usize,
        skip_leads: u32,
    ) -> Result<Vec<(LineId, Vec<HistoryWireCell>)>, HistoryRangeError> {
        if max_lines == 0 || end < start {
            return Ok(Vec::new());
        }
        if self
            .core
            .primary
            .history()
            .range_intersects_evicted(start, end)
        {
            return Err(HistoryRangeError::Stale);
        }
        let mut skip = skip_leads;
        let mut lines: Vec<(LineId, Vec<HistoryWireCell>)> = Vec::new();
        for entry in self.core.primary.history_entries() {
            let id = entry.line_id();
            if id < start {
                continue;
            }
            if id > end {
                break;
            }
            let cells = skip_wire_leads(entry.wire_cells(), &mut skip);
            if cells.is_empty() {
                continue;
            }
            if let Some((last_id, last_cells)) = lines.last_mut()
                && *last_id == id
            {
                last_cells.extend(cells);
            } else {
                lines.push((id, cells));
                if skip == 0 && lines.len() >= max_lines {
                    return Ok(lines);
                }
            }
        }
        for row in 0..self.core.primary.rows() {
            let Some(id) = self.core.primary.line_id(row) else {
                continue;
            };
            if id < start || id > end {
                continue;
            }
            if lines.iter().any(|(existing, _)| *existing == id) {
                continue;
            }
            let Some(cells) = self.core.primary.cell_row(row) else {
                continue;
            };
            let content_end = cells
                .iter()
                .rposition(|cell| cell.role != CellRole::Empty)
                .map_or(0, |index| index + 1);
            let mut wire = Vec::new();
            for cell in &cells[..content_end] {
                match cell.role {
                    CellRole::Continuation => {
                        wire.push(HistoryWireCell {
                            text: String::new(),
                            width: 0,
                            style: cell.style,
                            continuation: true,
                        });
                    }
                    CellRole::Empty => {
                        wire.push(HistoryWireCell {
                            text: " ".into(),
                            width: 1,
                            style: cell.style,
                            continuation: false,
                        });
                    }
                    CellRole::Lead => {
                        let text = if cell.overflow {
                            "\u{FFFD}".to_owned()
                        } else {
                            self.core.grapheme_store.get(cell.store_id).map_or_else(
                                || cell.character.to_string(),
                                |bytes| String::from_utf8_lossy(bytes).into_owned(),
                            )
                        };
                        wire.push(HistoryWireCell {
                            text,
                            width: cell.width.max(1),
                            style: cell.style,
                            continuation: false,
                        });
                    }
                }
            }
            lines.push((id, skip_wire_leads(wire, &mut skip)));
            if let Some((_, cells)) = lines.last()
                && cells.is_empty()
            {
                lines.pop();
            }
            if skip == 0 && lines.len() >= max_lines {
                break;
            }
        }
        Ok(lines)
    }

    /// Returns complete canonical source units for a bounded primary-history
    /// range. This projection preserves multi-scalar grapheme payloads and
    /// source anchors; callers that only need legacy scalar cells can use
    /// [`Self::primary_history_range`].
    pub fn primary_history_units_range(
        &self,
        start: LineId,
        end: LineId,
        max_units: usize,
    ) -> Vec<HistoryUnitView> {
        let mut units = self
            .core
            .primary
            .history()
            .source_units(start, end, max_units)
            .into_iter()
            .collect::<Vec<_>>();
        if units.len() >= max_units {
            return units;
        }
        let mut visible_offsets = std::collections::HashMap::<LineId, u32>::new();
        for row in 0..self.core.primary.rows() {
            let Some(line_id) = self.core.primary.line_id(row) else {
                continue;
            };
            if line_id < start || line_id > end {
                continue;
            }
            let Some(cells) = self.core.primary.cell_row(row) else {
                continue;
            };
            let content_end = cells
                .iter()
                .rposition(|cell| cell.role != CellRole::Empty)
                .map_or(0, |index| index + 1);
            let unit_offset = visible_offsets.entry(line_id).or_default();
            for (col, cell) in cells[..content_end].iter().enumerate() {
                let CellRole::Lead = cell.role else {
                    continue;
                };
                let text = if cell.overflow {
                    "\u{FFFD}".to_owned()
                } else {
                    self.core.grapheme_store.get(cell.store_id).map_or_else(
                        || cell.character.to_string(),
                        |bytes| String::from_utf8_lossy(bytes).into_owned(),
                    )
                };
                let anchor =
                    self.core
                        .primary
                        .cell_anchor(col as u16, row)
                        .unwrap_or(HistoryAnchor {
                            line_id,
                            unit_offset: *unit_offset,
                        });
                if !units.iter().any(|unit| unit.anchor == anchor) {
                    units.push(HistoryUnitView {
                        anchor,
                        text,
                        width: cell.width.max(1),
                        style: cell.style,
                        // Live viewport rows are not sealed HistoryStore
                        // records; treat them as ephemeral wrap fragments.
                        break_after: HistoryBreakAfter::SoftWrap,
                    });
                    if units.len() >= max_units {
                        return units;
                    }
                }
                *unit_offset = (*unit_offset).saturating_add(1);
            }
        }
        units
    }

    /// Derives width-specific rows from canonical retained history. The
    /// projection is bounded by `max_rows` and does not rewrite source text.
    /// Each lead cell carries the complete grapheme UTF-8 payload.
    pub fn primary_history_reflow(&self, cols: u16, max_rows: usize) -> Vec<ReflowRow> {
        self.core.primary.history().reflow(cols, max_rows)
    }

    pub fn primary_history_reflow_uncached(&self, cols: u16, max_rows: usize) -> Vec<ReflowRow> {
        self.core.primary.history().reflow_uncached(cols, max_rows)
    }

    pub fn primary_history_resident_bytes(&self) -> usize {
        self.core.primary.history().resident_bytes()
    }

    pub fn primary_history_derived_cache_bytes(&self) -> usize {
        self.core.primary.history().derived_cache_bytes()
    }

    pub fn drop_primary_history_derived_cache(&mut self) {
        self.core.primary.history_mut().drop_derived_cache();
    }

    pub fn primary_history_eviction_generation(&self) -> u64 {
        self.core.primary.history().eviction_generation()
    }

    pub fn primary_history_oldest_segment_age(&self) -> Option<u64> {
        self.core.primary.history().oldest_segment_age()
    }

    pub fn evict_oldest_primary_history_segment(&mut self) -> usize {
        self.core.primary.history_mut().evict_oldest_segment()
    }

    /// Resolves a retained source anchor without conflating an evicted source
    /// with an invalid line or unit offset.
    pub fn primary_history_unit(&self, anchor: HistoryAnchor) -> HistoryAnchorResolution {
        self.core.primary.history().resolve_anchor(anchor)
    }

    pub fn primary_history_search(&self, needle: &str, max_matches: usize) -> Vec<HistoryMatch> {
        self.core.primary.history().search(needle, max_matches)
    }

    pub fn primary_history_selection(
        &self,
        start: HistoryAnchor,
        end: HistoryAnchor,
    ) -> Result<Vec<HistoryUnitView>, HistoryRangeError> {
        if start > end {
            return Err(HistoryRangeError::Unrepresentable);
        }
        // Eviction of either endpoint is explicit (SPEC-010 §11). Active-only
        // anchors resolve as Invalid/not-in-store and may still be copied from
        // the live grid below.
        if matches!(
            self.primary_history_unit(start),
            HistoryAnchorResolution::Unavailable
        ) || matches!(
            self.primary_history_unit(end),
            HistoryAnchorResolution::Unavailable
        ) {
            return Err(HistoryRangeError::Stale);
        }

        let mut selected = match self.core.primary.history().selection(start, end) {
            Ok(units) => units,
            Err(HistoryRangeError::Stale) => Vec::new(),
            Err(other) => return Err(other),
        };

        for row in 0..self.core.primary.rows() {
            let Some(cells) = self.core.primary.cell_row(row) else {
                continue;
            };
            let content_end = cells
                .iter()
                .rposition(|cell| cell.role != CellRole::Empty)
                .map_or(0, |index| index + 1);
            let break_after = self
                .core
                .primary
                .row_break_after(row)
                .unwrap_or(HistoryBreakAfter::HardBreak);
            for (col, cell) in cells[..content_end].iter().enumerate() {
                let CellRole::Lead = cell.role else {
                    continue;
                };
                let Some(anchor) = self.core.primary.cell_anchor(col as u16, row) else {
                    continue;
                };
                if anchor < start || anchor > end {
                    continue;
                }
                if selected.iter().any(|unit| unit.anchor == anchor) {
                    continue;
                }
                if selected.len() >= crate::history::HISTORY_SELECTION_UNIT_CAP {
                    return Err(HistoryRangeError::Unrepresentable);
                }
                let text = if cell.overflow {
                    "\u{FFFD}".to_owned()
                } else {
                    self.core.grapheme_store.get(cell.store_id).map_or_else(
                        || cell.character.to_string(),
                        |bytes| String::from_utf8_lossy(bytes).into_owned(),
                    )
                };
                selected.push(HistoryUnitView {
                    anchor,
                    text,
                    width: cell.width.max(1),
                    style: cell.style,
                    break_after,
                });
            }
        }
        selected.sort_by_key(|unit| unit.anchor);
        if selected.is_empty() {
            return Err(HistoryRangeError::Stale);
        }
        Ok(selected)
    }

    pub fn copy_history_range(
        &self,
        start: HistoryAnchor,
        end: HistoryAnchor,
    ) -> Result<String, HistoryRangeError> {
        let units = self.primary_history_selection(start, end)?;
        format_history_copy(&units)
    }
}

fn skip_wire_leads(cells: Vec<HistoryWireCell>, skip: &mut u32) -> Vec<HistoryWireCell> {
    if *skip == 0 {
        return cells;
    }
    let mut index = 0usize;
    while index < cells.len() && *skip > 0 {
        if cells[index].continuation {
            index += 1;
            continue;
        }
        *skip = skip.saturating_sub(1);
        index += 1;
        while index < cells.len() && cells[index].continuation {
            index += 1;
        }
    }
    cells[index..].to_vec()
}
