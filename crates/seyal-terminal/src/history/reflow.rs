//! Width-derived reflow over retained history.

use super::store::HistoryStore;
use super::types::{
    HistoryAnchor, HistoryBreakAfter, HistoryWireCell, HISTORY_PER_EXECUTION_DERIVED_INDEX_CAP,
};
use crate::LineId;
use std::mem::size_of;

#[derive(Clone, Debug)]
pub(super) struct ReflowCache {
    pub(super) columns: u16,
    pub(super) max_rows: usize,
    pub(super) eviction_generation: u64,
    pub(super) rows: Vec<ReflowRow>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReflowRow {
    pub source_line_id: Option<LineId>,
    pub anchors: Vec<HistoryAnchor>,
    pub cells: Vec<HistoryWireCell>,
    pub break_after: Option<HistoryBreakAfter>,
    pub unavailable: bool,
}

pub(super) fn reflow_rows_allocated_bytes(rows: &Vec<ReflowRow>) -> usize {
    size_of::<Vec<ReflowRow>>()
        .saturating_add(rows.capacity().saturating_mul(size_of::<ReflowRow>()))
        .saturating_add(
            rows.iter()
                .map(|row| {
                    row.cells
                        .capacity()
                        .saturating_mul(size_of::<HistoryWireCell>())
                        .saturating_add(
                            row.cells
                                .iter()
                                .map(|cell| cell.text.capacity())
                                .sum::<usize>(),
                        )
                        .saturating_add(
                            row.anchors
                                .capacity()
                                .saturating_mul(size_of::<HistoryAnchor>()),
                        )
                })
                .sum::<usize>(),
        )
}


impl HistoryStore {
    pub(crate) fn reflow(&self, cols: u16, max_rows: usize) -> Vec<ReflowRow> {
        self.reflow_from(cols, max_rows, 0)
    }

    pub(crate) fn reflow_from(
        &self,
        cols: u16,
        max_rows: usize,
        start_col: usize,
    ) -> Vec<ReflowRow> {
        let generation = self.eviction_generation;
        if start_col == 0
            && let Some(cache) = self.reflow_cache.borrow().as_ref()
            && (cache.columns, cache.max_rows, cache.eviction_generation)
                == (cols, max_rows, generation)
        {
            return cache.rows.clone();
        }
        let rows = self.reflow_uncached_from(cols, max_rows, start_col);
        if start_col == 0 {
            let estimated = reflow_rows_allocated_bytes(&rows);
            if estimated <= HISTORY_PER_EXECUTION_DERIVED_INDEX_CAP {
                *self.reflow_cache.borrow_mut() = Some(ReflowCache {
                    columns: cols,
                    max_rows,
                    eviction_generation: generation,
                    rows: rows.clone(),
                });
            }
        }
        rows
    }

    pub(crate) fn reflow_uncached(&self, cols: u16, max_rows: usize) -> Vec<ReflowRow> {
        self.reflow_uncached_from(cols, max_rows, 0)
    }

    pub(crate) fn reflow_uncached_from(
        &self,
        cols: u16,
        max_rows: usize,
        start_col: usize,
    ) -> Vec<ReflowRow> {
        if cols == 0 || max_rows == 0 {
            return Vec::new();
        }
        let width = usize::from(cols);
        let mut rows = Vec::new();
        let mut current = ReflowRow::default();
        let mut used = start_col.min(width);
        let mut previous_break = None;
        for line in self.entries() {
            let joins = previous_break == Some(HistoryBreakAfter::SoftWrap);
            if !joins && !current.cells.is_empty() {
                rows.push(std::mem::take(&mut current));
                used = 0;
                if rows.len() >= max_rows {
                    break;
                }
            }
            for (unit_index, unit) in line.units().enumerate() {
                let unit_width = usize::from(unit.width().max(1));
                if used > 0 && used + unit_width > width {
                    if !current.cells.is_empty() {
                        current.break_after = Some(HistoryBreakAfter::SoftWrap);
                        rows.push(std::mem::take(&mut current));
                        if rows.len() >= max_rows {
                            return rows;
                        }
                    }
                    used = 0;
                }
                let anchor = HistoryAnchor {
                    line_id: line.line_id(),
                    unit_offset: line.start_offset().saturating_add(unit_index as u32),
                };
                if unit_width > width {
                    if !current.cells.is_empty() || !current.anchors.is_empty() {
                        current.break_after = Some(HistoryBreakAfter::SoftWrap);
                        rows.push(std::mem::take(&mut current));
                        if rows.len() >= max_rows {
                            return rows;
                        }
                    }
                    rows.push(ReflowRow {
                        source_line_id: Some(line.line_id()),
                        anchors: vec![anchor],
                        cells: Vec::new(),
                        break_after: Some(HistoryBreakAfter::SoftWrap),
                        unavailable: true,
                    });
                    used = 0;
                    if rows.len() >= max_rows {
                        return rows;
                    }
                    continue;
                }
                current.source_line_id.get_or_insert(line.line_id());
                current.anchors.push(anchor);
                current.cells.push(unit.reflow_wire_cell());
                if unit_width == 2 {
                    current
                        .cells
                        .push(HistoryWireCell::continuation_placeholder(unit.style()));
                }
                used = used.saturating_add(unit_width);
            }
            previous_break = Some(line.break_after());
            if line.break_after() == HistoryBreakAfter::HardBreak {
                if current.cells.is_empty() && current.anchors.is_empty() {
                    if let Some(last) = rows.last_mut().filter(|row| row.unavailable) {
                        last.break_after = Some(HistoryBreakAfter::HardBreak);
                    } else {
                        current.source_line_id = Some(line.line_id());
                        current.break_after = Some(HistoryBreakAfter::HardBreak);
                        rows.push(std::mem::take(&mut current));
                    }
                } else {
                    current.break_after = Some(HistoryBreakAfter::HardBreak);
                    rows.push(std::mem::take(&mut current));
                }
                used = 0;
                if rows.len() >= max_rows {
                    break;
                }
                previous_break = None;
            }
        }
        if !current.cells.is_empty() && rows.len() < max_rows {
            current.break_after = previous_break;
            rows.push(current);
        }
        rows
    }

}
