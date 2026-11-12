//! Sole physical allocator for primary cells and retained source units.

use super::types::{BlockId, CellHandle, RegionGrid, RegionId, RegionKind, RegionState};
use crate::{Cell, HistoryAnchor, LineId, TerminalError};

/// Live PTY window cells always live on page 0; spill/sealed pages start at 1.
pub(crate) const WINDOW_PAGE: u64 = 0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StaleHandle {
    GenerationMismatch,
    Evicted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CellLookup {
    Live(Cell),
    Stale(StaleHandle),
}

struct PageSlot {
    generation: u32,
    cell: Cell,
    live: bool,
    region: RegionId,
}

/// Bounded mutable page pool + region index.
///
/// Sealed off-window Block payload is transferred once into the shared
/// HistoryStore (ADR-009). [`RegionGrid::sealed_rows`] retains only LineId +
/// column ownership so Flow can paint by RegionId without a second cell copy.
/// Spill `pages` remain available for non-Block scratch; Block seal must not
/// dual-write cell payloads here (#1484).
///
/// The active PTY window's canonical cell payload lives only in `grid_cells` /
/// `grid_anchors` on [`WINDOW_PAGE`]. Generational `grid_gens` make window
/// handles stale on overwrite.
pub(crate) struct RegionStore {
    next_region: u64,
    next_spill_page: u64,
    pages: Vec<Vec<PageSlot>>,
    page_slots: u32,
    regions: Vec<RegionGrid>,
    window_cols: u16,
    window_rows: u16,
    /// Sole live primary-window cell payload (cols*rows).
    pub(crate) grid_cells: Vec<Cell>,
    /// Source anchors parallel to `grid_cells`.
    pub(crate) grid_anchors: Vec<Option<HistoryAnchor>>,
    grid_gens: Vec<u32>,
    /// Per-window-slot RegionId ownership; must move with cells on scroll/ICH/DCH.
    pub(crate) grid_regions: Vec<RegionId>,
}

impl RegionStore {
    pub(crate) fn new(page_slots: u32) -> Result<Self, TerminalError> {
        if page_slots == 0 {
            return Err(TerminalError::InvalidSize);
        }
        Ok(Self {
            next_region: 1,
            next_spill_page: 1,
            pages: Vec::new(),
            page_slots,
            regions: Vec::new(),
            window_cols: 0,
            window_rows: 0,
            grid_cells: Vec::new(),
            grid_anchors: Vec::new(),
            grid_gens: Vec::new(),
            grid_regions: Vec::new(),
        })
    }

    pub(crate) fn with_window(cols: u16, rows: u16) -> Result<Self, TerminalError> {
        let slots = u32::from(cols)
            .checked_mul(u32::from(rows))
            .ok_or(TerminalError::InvalidSize)?;
        let mut store = Self::new(slots.max(1))?;
        store.reset_window(cols, rows, RegionId(0))?;
        Ok(store)
    }

    /// Replace the live window raster. `default_region` must already exist when
    /// non-zero; RegionId(0) is a construction sentinel cleared by the engine.
    pub(crate) fn reset_window(
        &mut self,
        cols: u16,
        rows: u16,
        default_region: RegionId,
    ) -> Result<(), TerminalError> {
        if cols == 0 || rows == 0 {
            return Err(TerminalError::InvalidSize);
        }
        let len = usize::from(cols)
            .checked_mul(usize::from(rows))
            .ok_or(TerminalError::InvalidSize)?;
        self.window_cols = cols;
        self.window_rows = rows;
        self.grid_cells = vec![Cell::default(); len];
        self.grid_anchors = vec![None; len];
        self.grid_gens = vec![1; len];
        self.grid_regions = vec![default_region; len];
        Ok(())
    }

    pub(crate) fn window_cols(&self) -> u16 {
        self.window_cols
    }

    /// Spill page count (0 when Block seal uses HistoryStore-only ownership).
    pub(crate) fn spill_page_count(&self) -> usize {
        self.pages.len()
    }

    pub(crate) fn window_rows(&self) -> u16 {
        self.window_rows
    }

    pub(crate) fn window_index(&self, col: u16, row: u16) -> Option<usize> {
        if col >= self.window_cols || row >= self.window_rows {
            return None;
        }
        Some(usize::from(row) * usize::from(self.window_cols) + usize::from(col))
    }

    pub(crate) fn alloc_region(
        &mut self,
        kind: RegionKind,
        owner: Option<BlockId>,
    ) -> Result<RegionId, TerminalError> {
        let id = RegionId(self.next_region);
        self.next_region = self
            .next_region
            .checked_add(1)
            .ok_or(TerminalError::LineIdentityExhausted)?;
        self.regions.push(RegionGrid {
            id,
            owner,
            kind,
            state: RegionState::Writable,
            line_spine: Vec::new(),
            sealed_rows: Vec::new(),
        });
        Ok(id)
    }

    /// Spill/off-window allocation only. Live window cells use [`write_window`].
    pub(crate) fn alloc_cell(&mut self, cell: Cell) -> Result<CellHandle, TerminalError> {
        self.alloc_spill_cell(cell, RegionId(0))
    }

    /// Spill a cell with explicit Block/unstructured region ownership.
    pub(crate) fn alloc_spill_cell(
        &mut self,
        cell: Cell,
        region: RegionId,
    ) -> Result<CellHandle, TerminalError> {
        if self.pages.is_empty()
            || self.pages.last().map(|p| p.len()).unwrap_or(0) >= self.page_slots as usize
        {
            self.pages
                .push(Vec::with_capacity(self.page_slots as usize));
            if self.pages.len() > 1 {
                self.next_spill_page = self
                    .next_spill_page
                    .checked_add(1)
                    .ok_or(TerminalError::LineIdentityExhausted)?;
            }
        }
        let page_index = self.pages.len() - 1;
        let page = self.next_spill_page;
        let slot = self.pages[page_index].len() as u32;
        self.pages[page_index].push(PageSlot {
            generation: 1,
            cell,
            live: true,
            region,
        });
        Ok(CellHandle {
            page,
            slot,
            generation: 1,
        })
    }

    pub(crate) fn spill_region(&self, handle: CellHandle) -> Option<RegionId> {
        if handle.page == WINDOW_PAGE {
            return self.grid_regions.get(handle.slot as usize).copied();
        }
        let spill_index = handle.page.saturating_sub(1) as usize;
        self.pages
            .get(spill_index)
            .and_then(|page| page.get(handle.slot as usize))
            .filter(|slot| slot.live && slot.generation == handle.generation)
            .map(|slot| slot.region)
    }

    /// Index one sealed window row's per-column RegionId ownership onto RegionGrids.
    /// Does not copy cell payloads — HistoryStore is the sole sealed owner (#1484).
    pub(crate) fn record_sealed_row_ownership(
        &mut self,
        line_id: LineId,
        column_regions: &[(u16, RegionId)],
    ) -> Result<(), TerminalError> {
        use super::types::SealedRow;
        use std::collections::BTreeMap;
        let mut by_region: BTreeMap<RegionId, Vec<u16>> = BTreeMap::new();
        for (col, region) in column_regions {
            by_region.entry(*region).or_default().push(*col);
        }
        for (region, mut cols) in by_region {
            cols.sort_unstable();
            cols.dedup();
            let grid = self
                .regions
                .iter_mut()
                .find(|r| r.id == region)
                .ok_or(TerminalError::InvalidSize)?;
            if grid.line_spine.last().copied() != Some(line_id) {
                grid.line_spine.push(line_id);
            }
            grid.sealed_rows.push(SealedRow { line_id, cols });
            if grid.state == RegionState::Writable {
                grid.state = RegionState::PartiallySealed;
            }
        }
        Ok(())
    }

    /// Capture live-window RegionId per column for `row` (no spill copy).
    pub(crate) fn window_row_regions(
        &self,
        row: u16,
    ) -> Result<Vec<(u16, RegionId)>, TerminalError> {
        if row >= self.window_rows {
            return Err(TerminalError::InvalidSize);
        }
        let mut out = Vec::with_capacity(usize::from(self.window_cols));
        for col in 0..self.window_cols {
            let idx = self
                .window_index(col, row)
                .ok_or(TerminalError::InvalidSize)?;
            out.push((col, self.grid_regions[idx]));
        }
        Ok(out)
    }

    /// Sealed ownership rows for one region (LineId + owned columns).
    pub(crate) fn sealed_rows(&self, region: RegionId) -> &[super::types::SealedRow] {
        self.region(region)
            .map(|grid| grid.sealed_rows.as_slice())
            .unwrap_or(&[])
    }

    /// Number of sealed ownership rows for one region (u32-saturated).
    pub(crate) fn sealed_row_count(&self, region: RegionId) -> u32 {
        u32::try_from(self.sealed_rows(region).len()).unwrap_or(u32::MAX)
    }

    /// Write a live window cell in place; bumps generation and records region.
    pub(crate) fn write_window(
        &mut self,
        col: u16,
        row: u16,
        cell: Cell,
        region: RegionId,
    ) -> Result<CellHandle, TerminalError> {
        let idx = self
            .window_index(col, row)
            .ok_or(TerminalError::InvalidSize)?;
        self.grid_cells[idx] = cell;
        self.bump_window_slot(idx, region)
    }

    /// Bump generation for an already-updated window slot (bulk fill remint).
    pub(crate) fn bump_window_slot(
        &mut self,
        idx: usize,
        region: RegionId,
    ) -> Result<CellHandle, TerminalError> {
        if idx >= self.grid_cells.len() {
            return Err(TerminalError::InvalidSize);
        }
        let generation = self.grid_gens[idx].saturating_add(1).max(1);
        self.grid_gens[idx] = generation;
        self.grid_regions[idx] = region;
        Ok(CellHandle {
            page: WINDOW_PAGE,
            slot: idx as u32,
            generation,
        })
    }

    /// Current window handle without mutating (for map bootstrap).
    pub(crate) fn window_handle_at(
        &self,
        col: u16,
        row: u16,
    ) -> Result<(CellHandle, RegionId), TerminalError> {
        let idx = self
            .window_index(col, row)
            .ok_or(TerminalError::InvalidSize)?;
        Ok((
            CellHandle {
                page: WINDOW_PAGE,
                slot: idx as u32,
                generation: self.grid_gens[idx],
            },
            self.grid_regions[idx],
        ))
    }

    pub(crate) fn region_at_window(&self, col: u16, row: u16) -> Option<RegionId> {
        let idx = self.window_index(col, row)?;
        Some(self.grid_regions[idx])
    }

    pub(crate) fn get(&self, handle: CellHandle) -> CellLookup {
        if handle.page == WINDOW_PAGE {
            let Some(cell) = self.grid_cells.get(handle.slot as usize) else {
                return CellLookup::Stale(StaleHandle::Evicted);
            };
            let Some(generation) = self.grid_gens.get(handle.slot as usize) else {
                return CellLookup::Stale(StaleHandle::Evicted);
            };
            if *generation != handle.generation {
                return CellLookup::Stale(StaleHandle::GenerationMismatch);
            }
            return CellLookup::Live(*cell);
        }
        let spill_index = handle.page.saturating_sub(1) as usize;
        let Some(page) = self.pages.get(spill_index) else {
            return CellLookup::Stale(StaleHandle::Evicted);
        };
        let Some(slot) = page.get(handle.slot as usize) else {
            return CellLookup::Stale(StaleHandle::Evicted);
        };
        if !slot.live {
            return CellLookup::Stale(StaleHandle::Evicted);
        }
        if slot.generation != handle.generation {
            return CellLookup::Stale(StaleHandle::GenerationMismatch);
        }
        CellLookup::Live(slot.cell)
    }

    pub(crate) fn overwrite(
        &mut self,
        handle: CellHandle,
        cell: Cell,
    ) -> Result<CellHandle, StaleHandle> {
        if handle.page == WINDOW_PAGE {
            let idx = handle.slot as usize;
            let generation = self
                .grid_gens
                .get(idx)
                .copied()
                .ok_or(StaleHandle::Evicted)?;
            if generation != handle.generation {
                return Err(StaleHandle::GenerationMismatch);
            }
            let region = self.grid_regions[idx];
            return self
                .write_window(
                    (idx % usize::from(self.window_cols)) as u16,
                    (idx / usize::from(self.window_cols)) as u16,
                    cell,
                    region,
                )
                .map_err(|_| StaleHandle::Evicted);
        }
        let spill_index = handle.page.saturating_sub(1) as usize;
        let page = self
            .pages
            .get_mut(spill_index)
            .ok_or(StaleHandle::Evicted)?;
        let slot = page
            .get_mut(handle.slot as usize)
            .ok_or(StaleHandle::Evicted)?;
        if !slot.live {
            return Err(StaleHandle::Evicted);
        }
        if slot.generation != handle.generation {
            return Err(StaleHandle::GenerationMismatch);
        }
        slot.generation = slot.generation.saturating_add(1);
        slot.cell = cell;
        Ok(CellHandle {
            page: handle.page,
            slot: handle.slot,
            generation: slot.generation,
        })
    }

    pub(crate) fn note_line(
        &mut self,
        region: RegionId,
        line: LineId,
    ) -> Result<(), TerminalError> {
        let grid = self
            .regions
            .iter_mut()
            .find(|r| r.id == region)
            .ok_or(TerminalError::InvalidSize)?;
        if grid.line_spine.last().copied() != Some(line) {
            grid.line_spine.push(line);
        }
        Ok(())
    }

    pub(crate) fn region(&self, id: RegionId) -> Option<&RegionGrid> {
        self.regions.iter().find(|r| r.id == id)
    }

    pub(crate) fn region_mut(&mut self, id: RegionId) -> Option<&mut RegionGrid> {
        self.regions.iter_mut().find(|r| r.id == id)
    }

    /// Paint every window slot with `default_region` and generation 1.
    pub(crate) fn paint_window_region(&mut self, default_region: RegionId) {
        self.grid_regions.fill(default_region);
        self.grid_gens.fill(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Color;

    #[test]
    fn overwrite_bumps_generation_and_stales_old_handle() {
        let mut store = RegionStore::new(4).unwrap();
        let handle = store.alloc_cell(Cell::blank(Color::Default)).unwrap();
        let new_handle = store
            .overwrite(handle, Cell::blank(Color::Default))
            .unwrap();
        assert_ne!(handle.generation, new_handle.generation);
        assert!(matches!(
            store.get(handle),
            CellLookup::Stale(StaleHandle::GenerationMismatch)
        ));
        assert!(matches!(store.get(new_handle), CellLookup::Live(_)));
    }

    #[test]
    fn window_write_is_sole_live_payload_path() {
        let mut store = RegionStore::with_window(2, 1).unwrap();
        let region = store
            .alloc_region(RegionKind::Output, Some(BlockId(1)))
            .unwrap();
        store.paint_window_region(region);
        let h1 = store
            .write_window(
                0,
                0,
                Cell::lead_inline('A', 1, crate::Style::default()),
                region,
            )
            .unwrap();
        let h2 = store
            .write_window(
                0,
                0,
                Cell::lead_inline('B', 1, crate::Style::default()),
                region,
            )
            .unwrap();
        assert_eq!(store.grid_cells[0].character, 'B');
        assert!(matches!(
            store.get(h1),
            CellLookup::Stale(StaleHandle::GenerationMismatch)
        ));
        assert!(matches!(store.get(h2), CellLookup::Live(_)));
        assert_eq!(store.region_at_window(0, 0), Some(region));
    }

    #[test]
    fn region_grid_records_owner_and_line_spine() {
        let mut store = RegionStore::new(4).unwrap();
        let region = store
            .alloc_region(RegionKind::Output, Some(BlockId(7)))
            .unwrap();
        store.note_line(region, LineId(3)).unwrap();
        store.note_line(region, LineId(3)).unwrap();
        store.note_line(region, LineId(4)).unwrap();
        let grid = store.region(region).unwrap();
        assert_eq!(grid.owner, Some(BlockId(7)));
        assert_eq!(grid.line_spine, vec![LineId(3), LineId(4)]);
    }

    #[test]
    fn sealed_row_ownership_indexes_columns_without_spill_copy() {
        let mut store = RegionStore::with_window(3, 1).unwrap();
        let unstructured = store.alloc_region(RegionKind::Unstructured, None).unwrap();
        let output = store
            .alloc_region(RegionKind::Output, Some(BlockId(3)))
            .unwrap();
        store.paint_window_region(unstructured);
        store
            .write_window(
                0,
                0,
                Cell::lead_inline('A', 1, crate::Style::default()),
                output,
            )
            .unwrap();
        store
            .write_window(
                1,
                0,
                Cell::lead_inline('B', 1, crate::Style::default()),
                output,
            )
            .unwrap();
        let columns = store.window_row_regions(0).unwrap();
        assert_eq!(columns.len(), 3);
        assert_eq!(columns[0].1, output);
        assert_eq!(columns[1].1, output);
        assert_eq!(columns[2].1, unstructured);
        store
            .record_sealed_row_ownership(LineId(9), &columns)
            .unwrap();
        let grid = store.region(output).unwrap();
        assert_eq!(grid.state, RegionState::PartiallySealed);
        assert_eq!(grid.sealed_rows.len(), 1);
        assert_eq!(grid.sealed_rows[0].line_id, LineId(9));
        assert_eq!(grid.sealed_rows[0].cols, vec![0, 1]);
        assert_eq!(store.sealed_row_count(output), 1);
        assert_eq!(
            store.spill_page_count(),
            0,
            "ownership index must not allocate spill pages"
        );
    }
}
