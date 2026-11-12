//! Non-owning PTY coordinate map into RegionStore cell handles.

use super::region_store::{CellLookup, RegionStore};
use super::types::{CellHandle, RegionId};
use crate::TerminalError;

#[derive(Clone, Debug)]
struct MappedCell {
    handle: CellHandle,
    region: RegionId,
}

/// PTY-sized address space. Does not own cell payload.
pub(crate) struct VTWindowMap {
    cols: u16,
    rows: u16,
    cells: Vec<Option<MappedCell>>,
}

impl VTWindowMap {
    pub(crate) fn new(cols: u16, rows: u16) -> Result<Self, TerminalError> {
        if cols == 0 || rows == 0 {
            return Err(TerminalError::InvalidSize);
        }
        let len = usize::from(cols)
            .checked_mul(usize::from(rows))
            .ok_or(TerminalError::InvalidSize)?;
        Ok(Self {
            cols,
            rows,
            cells: vec![None; len],
        })
    }

    pub(crate) fn cols(&self) -> u16 {
        self.cols
    }

    pub(crate) fn rows(&self) -> u16 {
        self.rows
    }

    fn index(&self, col: u16, row: u16) -> Option<usize> {
        if col >= self.cols || row >= self.rows {
            return None;
        }
        Some(usize::from(row) * usize::from(self.cols) + usize::from(col))
    }

    pub(crate) fn set(
        &mut self,
        col: u16,
        row: u16,
        handle: CellHandle,
        region: RegionId,
    ) -> Result<(), TerminalError> {
        let idx = self.index(col, row).ok_or(TerminalError::InvalidSize)?;
        self.cells[idx] = Some(MappedCell { handle, region });
        Ok(())
    }

    pub(crate) fn handle_at(&self, col: u16, row: u16) -> Option<(CellHandle, RegionId)> {
        let idx = self.index(col, row)?;
        self.cells[idx]
            .as_ref()
            .map(|m| (m.handle, m.region))
    }

    pub(crate) fn resolve(
        &self,
        store: &RegionStore,
        col: u16,
        row: u16,
    ) -> Option<(CellLookup, RegionId)> {
        let (handle, region) = self.handle_at(col, row)?;
        Some((store.get(handle), region))
    }

    pub(crate) fn replace_handle(
        &mut self,
        col: u16,
        row: u16,
        handle: CellHandle,
    ) -> Result<(), TerminalError> {
        let idx = self.index(col, row).ok_or(TerminalError::InvalidSize)?;
        let entry = self.cells[idx]
            .as_mut()
            .ok_or(TerminalError::InvalidSize)?;
        entry.handle = handle;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primary_blocks::types::RegionKind;
    use crate::{Cell, Color};

    #[test]
    fn map_is_non_owning_and_tracks_region() {
        let mut store = RegionStore::new(8).unwrap();
        let region = store.alloc_region(RegionKind::Output, None).unwrap();
        let handle = store.alloc_cell(Cell::blank(Color::Default)).unwrap();
        let mut map = VTWindowMap::new(4, 2).unwrap();
        map.set(1, 0, handle, region).unwrap();
        let (lookup, mapped_region) = map.resolve(&store, 1, 0).unwrap();
        assert_eq!(mapped_region, region);
        assert!(matches!(lookup, CellLookup::Live(_)));
    }

    #[test]
    fn overwrite_updates_handle_generation_in_map() {
        let mut store = RegionStore::new(8).unwrap();
        let region = store.alloc_region(RegionKind::Unstructured, None).unwrap();
        let handle = store.alloc_cell(Cell::blank(Color::Default)).unwrap();
        let mut map = VTWindowMap::new(2, 1).unwrap();
        map.set(0, 0, handle, region).unwrap();
        let new_handle = store
            .overwrite(handle, Cell::blank(Color::Default))
            .unwrap();
        map.replace_handle(0, 0, new_handle).unwrap();
        let (lookup, _) = map.resolve(&store, 0, 0).unwrap();
        assert!(matches!(lookup, CellLookup::Live(_)));
        // Old handle must not resolve through the map anymore.
        assert!(matches!(
            store.get(handle),
            CellLookup::Stale(_)
        ));
    }
}
