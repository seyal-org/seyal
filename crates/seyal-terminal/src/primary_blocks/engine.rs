//! PrimaryBlockEngine — BlockList + RegionStore + VTWindowMap composition.

use super::attribution::{AttributionJournal, MarkerKind, PrimaryEvent, UnitVersion};
use super::region_store::{CellLookup, RegionStore};
use super::types::{BlockContent, BlockId, RegionId, RegionKind};
use super::viewport::{BlockContentRef, OnWindowRegionSpan, VisibleBlockSpan};
use super::vt_window_map::VTWindowMap;
use crate::{Cell, LineId, TerminalError};
use std::collections::HashMap;

/// Production journal capacity: enough for one full 16 KiB reactor read.
/// Exact sizing is refined with mutation-density measurements; this floor
/// keeps overflow unreachable under normal F05 production testing.
/// Production journal covers one reactor read (16 KiB) plus marker/rewrite
/// margin so tall bursts do not hit capacity on the trusted path (#1484 F05/F06).
pub(crate) const PRODUCTION_ATTRIBUTION_JOURNAL_CAPACITY: usize = 64 * 1024;

pub(crate) struct PrimaryBlockEngine {
    pub(crate) store: RegionStore,
    pub(crate) window: VTWindowMap,
    pub(crate) journal: AttributionJournal,
    pub(crate) blocks: Vec<BlockContent>,
    pub(crate) unstructured: RegionId,
    /// Region that new VT writes join until the next accepted boundary.
    active_region: RegionId,
    /// Nested Block output regions to restore when an inner Block ends.
    /// Host (local) stays Running across session Blocks; ending the session
    /// must return writes to the host output region, not unstructured.
    active_region_stack: Vec<RegionId>,
    /// Output region opened at OSC CommandStart before Runtime assigns BlockId.
    pending_output: Option<RegionId>,
    /// Prompt/command header region opened at OSC PromptStart before BlockId
    /// assignment (#1484 Finding F / D4).
    pending_header: Option<RegionId>,
    /// Last content generation per (LineId, unit_offset). Overwrite mints anew.
    unit_generations: HashMap<(LineId, u64), u64>,
}

impl PrimaryBlockEngine {
    pub(crate) fn new(cols: u16, rows: u16) -> Result<Self, TerminalError> {
        let mut store = RegionStore::with_window(cols, rows)?;
        let unstructured = store.alloc_region(RegionKind::Unstructured, None)?;
        store.paint_window_region(unstructured);
        let mut window = VTWindowMap::new(cols, rows)?;
        Self::map_full_window(&store, &mut window, cols, rows)?;
        Ok(Self {
            store,
            window,
            journal: AttributionJournal::production(PRODUCTION_ATTRIBUTION_JOURNAL_CAPACITY),
            blocks: Vec::new(),
            unstructured,
            active_region: unstructured,
            active_region_stack: Vec::new(),
            pending_output: None,
            pending_header: None,
            unit_generations: HashMap::new(),
        })
    }

    fn push_active_region(&mut self) {
        if self.active_region != self.unstructured {
            self.active_region_stack.push(self.active_region);
        }
    }

    fn restore_active_region(&mut self) {
        self.active_region = self.active_region_stack.pop().unwrap_or(self.unstructured);
    }

    fn map_full_window(
        store: &RegionStore,
        window: &mut VTWindowMap,
        cols: u16,
        rows: u16,
    ) -> Result<(), TerminalError> {
        for row in 0..rows {
            for col in 0..cols {
                let (handle, region) = store.window_handle_at(col, row)?;
                window.set(col, row, handle, region)?;
            }
        }
        Ok(())
    }

    pub(crate) fn open_block(&mut self, id: BlockId) -> Result<&BlockContent, TerminalError> {
        self.begin_block_output(id)?;
        Ok(self.blocks.last().expect("just pushed"))
    }

    pub(crate) fn unstructured_region(&self) -> RegionId {
        self.unstructured
    }

    pub(crate) fn active_region(&self) -> RegionId {
        self.active_region
    }

    pub(crate) fn set_active_region(&mut self, region: RegionId) {
        self.active_region = region;
    }

    /// OSC PromptStart: open a provisional prompt/command header region so
    /// prompt bytes own the header RegionId before Runtime assigns BlockId (D4).
    pub(crate) fn arm_prompt_start_header(&mut self) -> Result<RegionId, TerminalError> {
        if let Some(region) = self.pending_header {
            self.active_region = region;
            return Ok(region);
        }
        self.push_active_region();
        let region = self
            .store
            .alloc_region(RegionKind::PromptCommand, None)?;
        self.pending_header = Some(region);
        self.active_region = region;
        Ok(region)
    }

    /// OSC CommandStart: open a provisional output region so same-read bytes
    /// after the marker do not stay unstructured while Runtime validates.
    pub(crate) fn arm_command_start_output(&mut self) -> Result<RegionId, TerminalError> {
        if let Some(region) = self.pending_output {
            self.active_region = region;
            return Ok(region);
        }
        // Preserve the outer Block (e.g. SSH host) or pending header across the arm.
        self.push_active_region();
        let region = self.store.alloc_region(RegionKind::Output, None)?;
        self.pending_output = Some(region);
        self.active_region = region;
        Ok(region)
    }

    /// Open Block regions and direct subsequent VT writes into `output`.
    /// Reuses provisional OSC-armed header/output regions when present.
    pub(crate) fn begin_block_output(
        &mut self,
        id: BlockId,
    ) -> Result<BlockContent, TerminalError> {
        let header = if let Some(pending) = self.pending_header.take() {
            if let Some(grid) = self.store.region_mut(pending) {
                grid.owner = Some(id);
            }
            pending
        } else {
            self.store
                .alloc_region(RegionKind::PromptCommand, Some(id))?
        };
        let output = if let Some(pending) = self.pending_output.take() {
            if let Some(grid) = self.store.region_mut(pending) {
                grid.owner = Some(id);
            }
            // Arm already pushed the outer region and set active to `pending`.
            pending
        } else {
            self.push_active_region();
            self.store.alloc_region(RegionKind::Output, Some(id))?
        };
        let block = BlockContent { id, header, output };
        self.blocks.push(block.clone());
        self.active_region = output;
        Ok(block)
    }

    /// Drop UnitVersion and sealed ownership indexes for evicted LineIds (D6).
    pub(crate) fn reclaim_evicted_line_ids(&mut self, evicted: &[LineId]) {
        if evicted.is_empty() {
            return;
        }
        for line_id in evicted {
            self.unit_generations.retain(|(lid, _), _| lid != line_id);
        }
        self.store.reclaim_sealed_line_ids(evicted);
    }

    pub(crate) fn block_content_refs(&self) -> Vec<BlockContentRef> {
        // Content order only — display viewport indexes live in seyal-client (A).
        self.blocks
            .iter()
            .map(|block| BlockContentRef {
                block_id: block.id,
                header: block.header,
                output: block.output,
            })
            .collect()
    }

    /// Sealed ownership rows for `region` (LineId + owned columns).
    pub(crate) fn sealed_rows(
        &self,
        region: RegionId,
    ) -> &[crate::primary_blocks::types::SealedRow] {
        self.store.sealed_rows(region)
    }

    /// Sealed ownership row count for `region` (u16-saturated).
    pub(crate) fn sealed_row_count(&self, region: RegionId) -> u32 {
        self.store.sealed_row_count(region)
    }

    /// Index one window row's RegionId ownership onto RegionGrids (no cell copy).
    /// HistoryStore remains the sole sealed payload owner (#1484 / ADR-009).
    pub(crate) fn seal_window_row_indexed(
        &mut self,
        row: u16,
        line_id: LineId,
    ) -> Result<(), TerminalError> {
        let columns = self.store.window_row_regions(row)?;
        self.store.record_sealed_row_ownership(line_id, &columns)
    }

    /// Live window cells currently owned by `region` (col, row, cell).
    pub(crate) fn live_region_cells(&self, region: RegionId) -> Vec<(u16, u16, Cell)> {
        let cols = self.store.window_cols();
        let rows = self.store.window_rows();
        let mut out = Vec::new();
        for row in 0..rows {
            for col in 0..cols {
                if self.store.region_at_window(col, row) == Some(region) {
                    let idx = self.store.window_index(col, row).expect("in range");
                    out.push((col, row, self.store.grid_cells[idx]));
                }
            }
        }
        out
    }

    /// One-pass on-window output spans for every Block (contiguous row runs).
    /// Gaps inside a Block's ownership collapse to first..last inclusive.
    /// Complexity: O(window cells + B), not O(B × window cells).
    pub(crate) fn on_window_region_spans(&self) -> Vec<OnWindowRegionSpan> {
        let rows = self.store.window_rows();
        let cols = self.store.window_cols();
        let mut by_output: HashMap<RegionId, (Option<u16>, Option<u16>)> =
            HashMap::with_capacity(self.blocks.len());
        for block in &self.blocks {
            by_output.insert(block.output, (None, None));
        }
        for row in 0..rows {
            for col in 0..cols {
                let Some(region) = self.store.region_at_window(col, row) else {
                    continue;
                };
                let Some(entry) = by_output.get_mut(&region) else {
                    continue;
                };
                if entry.0.is_none() {
                    entry.0 = Some(row);
                }
                entry.1 = Some(row);
            }
        }
        self.blocks
            .iter()
            .map(|block| {
                let (first, last) = by_output
                    .get(&block.output)
                    .copied()
                    .unwrap_or((None, None));
                let (first_row, row_count) = match (first, last) {
                    (Some(start), Some(end)) => {
                        (start, end.saturating_sub(start).saturating_add(1))
                    }
                    _ => (0, 0),
                };
                OnWindowRegionSpan {
                    block_id: block.id,
                    header: block.header,
                    output: block.output,
                    first_row,
                    row_count,
                }
            })
            .collect()
    }

    /// End the innermost Block output region. Restores a nested outer Block
    /// (host under session) when present; otherwise returns to unstructured.
    pub(crate) fn end_block_output(&mut self) {
        self.pending_output = None;
        self.pending_header = None;
        self.restore_active_region();
    }

    /// Drop a provisional PromptStart/CommandStart arm that never became a Block.
    /// No-op when no pending region is armed.
    pub(crate) fn cancel_pending_output(&mut self) {
        let had_output = self.pending_output.take().is_some();
        let had_header = self.pending_header.take().is_some();
        if had_output {
            self.restore_active_region();
        }
        if had_header {
            self.restore_active_region();
        }
    }

    /// Replace the live window grid after a prepared resize commit.
    ///
    /// `regions` must match `cells`/`anchors` length. Callers that reflow the
    /// primary window must thread RegionId ownership through resize — resetting
    /// to unstructured and leaving glyphs orphaned blanks Block AX (#1484).
    pub(crate) fn replace_window_grid(
        &mut self,
        cols: u16,
        rows: u16,
        cells: Vec<crate::Cell>,
        anchors: Vec<Option<crate::HistoryAnchor>>,
        regions: Vec<RegionId>,
    ) -> Result<(), TerminalError> {
        self.store.reset_window(cols, rows, self.unstructured)?;
        self.store.grid_cells = cells;
        self.store.grid_anchors = anchors;
        let expected = usize::from(cols)
            .checked_mul(usize::from(rows))
            .ok_or(TerminalError::InvalidSize)?;
        if self.store.grid_cells.len() != expected
            || self.store.grid_anchors.len() != expected
            || regions.len() != expected
        {
            return Err(TerminalError::InvalidSize);
        }
        self.store.grid_regions = regions;
        self.window = VTWindowMap::new(cols, rows)?;
        Self::map_full_window(&self.store, &mut self.window, cols, rows)?;
        self.unit_generations.clear();
        Ok(())
    }

    /// Write one canonical window cell through RegionStore + VTWindowMap.
    /// Overwrite mints a new `UnitVersion` so prior Block ownership cannot stick.
    pub(crate) fn write_at(
        &mut self,
        col: u16,
        row: u16,
        cell: Cell,
        region: RegionId,
        line_id: LineId,
        unit_offset: u64,
    ) -> Result<UnitVersion, TerminalError> {
        let key = (line_id, unit_offset);
        let content_generation = self
            .unit_generations
            .get(&key)
            .copied()
            .unwrap_or(0)
            .saturating_add(1);
        self.unit_generations.insert(key, content_generation);
        let version = UnitVersion {
            line_id,
            unit_offset,
            content_generation,
        };

        let handle = self.store.write_window(col, row, cell, region)?;
        self.window.set(col, row, handle, region)?;
        self.store.note_line(region, line_id)?;
        let sequence = self.journal.next_sequence();
        if self
            .journal
            .push(PrimaryEvent::CellMutation {
                sequence,
                version,
                handle,
                region,
            })
            .is_err()
        {
            match self.journal.policy() {
                // F06 fault-inject: classify unresolved ownership as Unstructured.
                super::attribution::JournalOverflowPolicy::FaultInject => {
                    self.pending_output = None;
                    self.pending_header = None;
                    self.active_region_stack.clear();
                    self.active_region = self.unstructured;
                }
                // Production: cell already written under the Block region; the
                // refused journal admission must not forge StreamSequence nor
                // force-clear VT ownership (#1484 F05/F06).
                super::attribution::JournalOverflowPolicy::ProductionUnreachable => {}
            }
        }
        Ok(version)
    }

    /// Content-derived presentation height for one Block (header + output spines).
    /// Display viewport indexes are owned by seyal-client per displayHandle (A/B).
    pub(crate) fn content_height_of(&self, block_id: BlockId) -> u32 {
        let Some(block) = self.blocks.iter().find(|block| block.id == block_id) else {
            return 0;
        };
        let header = self
            .store
            .region(block.header)
            .map(|grid| grid.line_spine.len())
            .unwrap_or(0);
        let output = self
            .store
            .region(block.output)
            .map(|grid| grid.line_spine.len())
            .unwrap_or(0);
        u32::try_from(header.saturating_add(output)).unwrap_or(u32::MAX)
    }

    pub(crate) fn content_total_height(&self) -> u64 {
        self.blocks
            .iter()
            .map(|block| u64::from(self.content_height_of(block.id)))
            .sum()
    }

    /// Compatibility shim: content height scan for tests during A/B cutover.
    pub(crate) fn visible_block_spans(&self, y: u64, window_rows: u32) -> Vec<VisibleBlockSpan> {
        // Build a throwaway prefix scan without retaining display authority.
        let mut prefix = 0u64;
        let window_end = y.saturating_add(u64::from(window_rows));
        let mut out = Vec::new();
        for block in &self.blocks {
            let height = self.content_height_of(block.id);
            let block_y = prefix;
            let block_end = block_y.saturating_add(u64::from(height));
            prefix = block_end;
            if height == 0 || block_end <= y || block_y >= window_end {
                continue;
            }
            let first_visible = y.saturating_sub(block_y).min(u64::from(height)) as u32;
            let visible_end = window_end.min(block_end);
            let visible_rows = (visible_end - (block_y + u64::from(first_visible))) as u32;
            if visible_rows > 0 {
                out.push(VisibleBlockSpan {
                    block_id: block.id,
                    y: block_y,
                    height,
                    first_visible_row: first_visible,
                    visible_rows,
                });
            }
        }
        out
    }

    pub(crate) fn viewport_total_height(&self) -> u64 {
        self.content_total_height()
    }

    pub(crate) fn viewport_height_of(&self, block_id: BlockId) -> u32 {
        self.content_height_of(block_id)
    }

    /// Blank / continuation / erase that is not a source-unit lead write.
    pub(crate) fn set_window_cell(
        &mut self,
        col: u16,
        row: u16,
        cell: Cell,
        region: RegionId,
    ) -> Result<(), TerminalError> {
        let handle = self.store.write_window(col, row, cell, region)?;
        self.window.set(col, row, handle, region)?;
        Ok(())
    }

    /// After a raw `grid_cells` bulk fill, remint generations + VTWindowMap for
    /// each slot in `[start_col, end_col)` on `row` so handles cannot alias
    /// pre-fill content.
    pub(crate) fn remint_row_range(
        &mut self,
        row: u16,
        start_col: u16,
        end_col: u16,
        region: RegionId,
    ) -> Result<(), TerminalError> {
        let end = end_col.min(self.store.window_cols());
        for col in start_col..end {
            let idx = self
                .store
                .window_index(col, row)
                .ok_or(TerminalError::InvalidSize)?;
            let handle = self.store.bump_window_slot(idx, region)?;
            self.window.set(col, row, handle, region)?;
        }
        Ok(())
    }

    pub(crate) fn remint_full_window(&mut self, region: RegionId) -> Result<(), TerminalError> {
        let rows = self.store.window_rows();
        let cols = self.store.window_cols();
        for row in 0..rows {
            self.remint_row_range(row, 0, cols, region)?;
        }
        Ok(())
    }

    /// Remint a contiguous window-slot range after a raw linear fill.
    pub(crate) fn remint_linear_range(
        &mut self,
        start_idx: usize,
        end_idx: usize,
        region: RegionId,
    ) -> Result<(), TerminalError> {
        let cols = usize::from(self.store.window_cols());
        if cols == 0 || end_idx > self.store.grid_cells.len() || start_idx > end_idx {
            return Err(TerminalError::InvalidSize);
        }
        for idx in start_idx..end_idx {
            let handle = self.store.bump_window_slot(idx, region)?;
            let col = (idx % cols) as u16;
            let row = (idx / cols) as u16;
            self.window.set(col, row, handle, region)?;
        }
        Ok(())
    }

    /// Remint generations/VTWindowMap after a bulk cell move, keeping each
    /// slot's existing RegionId (#1484: scroll/ICH/DCH must not wipe Block ownership).
    pub(crate) fn remint_linear_range_retain_regions(
        &mut self,
        start_idx: usize,
        end_idx: usize,
    ) -> Result<(), TerminalError> {
        let cols = usize::from(self.store.window_cols());
        if cols == 0 || end_idx > self.store.grid_cells.len() || start_idx > end_idx {
            return Err(TerminalError::InvalidSize);
        }
        let fallback = self.unstructured_region();
        for idx in start_idx..end_idx {
            let region = self
                .store
                .grid_regions
                .get(idx)
                .copied()
                .unwrap_or(fallback);
            let handle = self.store.bump_window_slot(idx, region)?;
            let col = (idx % cols) as u16;
            let row = (idx / cols) as u16;
            self.window.set(col, row, handle, region)?;
        }
        Ok(())
    }

    pub(crate) fn live_cell(&self, col: u16, row: u16) -> Option<(Cell, RegionId)> {
        let (lookup, region) = self.window.resolve(&self.store, col, row)?;
        match lookup {
            CellLookup::Live(cell) => Some((cell, region)),
            CellLookup::Stale(_) => None,
        }
    }

    /// Journal a trusted-shell boundary candidate in wire order with cell
    /// mutations. Runtime still validates nonce/ExecutionId before opening a
    /// Block; `token_accepted` here means the VT OSC token parsed cleanly.
    ///
    /// Returns `Err` when the journal refuses admission. Callers must treat
    /// journal + `shell_events` as one atomic barrier (#1484 Finding F / D1):
    /// never enqueue the Runtime-visible shell event after a refused journal.
    pub(crate) fn record_shell_boundary(
        &mut self,
        marker: MarkerKind,
        version: UnitVersion,
        column: u16,
        block: Option<BlockId>,
        token_accepted: bool,
    ) -> Result<(), ()> {
        let sequence = self.journal.next_sequence();
        self.journal
            .push_shell_boundary(PrimaryEvent::ShellBoundaryCandidate {
                sequence,
                marker,
                block,
                version,
                column,
                token_accepted,
            })
    }

    pub(crate) fn unit_generation_at(&self, line_id: LineId, unit_offset: u64) -> u64 {
        self.unit_generations
            .get(&(line_id, unit_offset))
            .copied()
            .unwrap_or(0)
    }
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
