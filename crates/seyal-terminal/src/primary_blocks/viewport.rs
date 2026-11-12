//! Derived Block viewport index — no cell ownership (ADR-009 / #1484 Phase D).
//!
//! Heights come from RegionGrid `line_spine` lengths (header + output). The
//! Fenwick / prefix structure supports O(log n) first-visible lookup and
//! O(visible) traversal without scanning every Block on ordinary ticks.

use super::region_store::RegionStore;
use super::types::{BlockContent, BlockId, RegionId};

/// One Flow-visible Block content handle (regions only; no cell copies).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockContentRef {
    pub block_id: BlockId,
    pub header: RegionId,
    pub output: RegionId,
}

/// Contiguous live-window rows owned by a Block's output RegionId.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OnWindowRegionSpan {
    pub block_id: BlockId,
    pub header: RegionId,
    pub output: RegionId,
    /// Inclusive first VT window row currently owned by `output`.
    pub first_row: u16,
    /// Number of contiguous window rows from `first_row` that contain at least
    /// one cell owned by `output`. Zero means the Block is off-window.
    pub row_count: u16,
}

/// Stable order + incremental height index for Flow projection.
#[derive(Clone, Debug, Default)]
pub(crate) struct BlockViewportIndex {
    /// BlockId order matches engine.blocks after rebuild.
    order: Vec<BlockId>,
    /// Parallel heights (presentation rows) for `order`.
    heights: Vec<u32>,
    /// Prefix sums: `prefix[i]` = sum(heights[0..i]); length = order.len() + 1.
    prefix: Vec<u64>,
}

/// One Block span inside a bounded viewport window (row units = line_spine rows).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibleBlockSpan {
    pub block_id: BlockId,
    /// Absolute transcript Y of the Block's first row.
    pub y: u64,
    /// Total Block height in presentation rows.
    pub height: u32,
    /// First row of this Block that intersects the query window (0..height).
    pub first_visible_row: u32,
    /// Number of Block rows that intersect the query window.
    pub visible_rows: u32,
}

impl BlockViewportIndex {
    pub(crate) fn rebuild(&mut self, blocks: &[BlockContent], store: &RegionStore) {
        self.order = blocks.iter().map(|block| block.id).collect();
        self.heights = blocks
            .iter()
            .map(|block| block_height(store, block))
            .collect();
        self.rebuild_prefix();
    }

    /// Update one Block's height after a dirty line_spine change. Returns false
    /// when `block_id` is unknown (caller should `rebuild`).
    pub(crate) fn set_height(&mut self, block_id: BlockId, height: u32) -> bool {
        let Some(index) = self.order.iter().position(|id| *id == block_id) else {
            return false;
        };
        if self.heights[index] == height {
            return true;
        }
        self.heights[index] = height;
        // Incremental prefix repair from the changed index forward.
        for i in index..self.heights.len() {
            self.prefix[i + 1] = self.prefix[i].saturating_add(u64::from(self.heights[i]));
        }
        true
    }

    pub(crate) fn ordered_ids(&self) -> &[BlockId] {
        &self.order
    }

    pub(crate) fn total_height(&self) -> u64 {
        *self.prefix.last().unwrap_or(&0)
    }

    pub(crate) fn height_of(&self, block_id: BlockId) -> Option<u32> {
        self.order
            .iter()
            .position(|id| *id == block_id)
            .map(|index| self.heights[index])
    }

    /// First Block whose span intersects `[y, y + window_rows)`. Empty when the
    /// window is empty or past the end. Traverses only intersecting Blocks.
    pub(crate) fn visible_spans(&self, y: u64, window_rows: u32) -> Vec<VisibleBlockSpan> {
        if window_rows == 0 || self.order.is_empty() {
            return Vec::new();
        }
        let window_end = y.saturating_add(u64::from(window_rows));
        let total = self.total_height();
        if y >= total {
            return Vec::new();
        }
        let mut index = self.first_intersecting(y);
        let mut out = Vec::new();
        while index < self.order.len() {
            let block_y = self.prefix[index];
            if block_y >= window_end {
                break;
            }
            let height = self.heights[index];
            let block_end = block_y.saturating_add(u64::from(height));
            if height == 0 || block_end <= y {
                index += 1;
                continue;
            }
            let first_visible = y.saturating_sub(block_y).min(u64::from(height)) as u32;
            let visible_end = window_end.min(block_end);
            let visible_rows = (visible_end - (block_y + u64::from(first_visible))) as u32;
            if visible_rows > 0 {
                out.push(VisibleBlockSpan {
                    block_id: self.order[index],
                    y: block_y,
                    height,
                    first_visible_row: first_visible,
                    visible_rows,
                });
            }
            index += 1;
        }
        out
    }

    fn rebuild_prefix(&mut self) {
        self.prefix.clear();
        self.prefix.reserve(self.heights.len() + 1);
        self.prefix.push(0);
        let mut sum = 0u64;
        for height in &self.heights {
            sum = sum.saturating_add(u64::from(*height));
            self.prefix.push(sum);
        }
    }

    /// Lowest index with `prefix[index+1] > y` (Block that owns absolute row `y`).
    fn first_intersecting(&self, y: u64) -> usize {
        let mut lo = 0usize;
        let mut hi = self.order.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.prefix[mid + 1] <= y {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }
}

fn block_height(store: &RegionStore, block: &BlockContent) -> u32 {
    let header = store
        .region(block.header)
        .map(|grid| grid.line_spine.len())
        .unwrap_or(0);
    let output = store
        .region(block.output)
        .map(|grid| grid.line_spine.len())
        .unwrap_or(0);
    u32::try_from(header.saturating_add(output)).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primary_blocks::types::{RegionKind, RegionState};
    use crate::LineId;

    fn store_with_spines(spines: &[(BlockId, usize, usize)]) -> (RegionStore, Vec<BlockContent>) {
        let mut store = RegionStore::new(8).unwrap();
        let mut blocks = Vec::new();
        for (id, header_lines, output_lines) in spines {
            let header = store
                .alloc_region(RegionKind::PromptCommand, Some(*id))
                .unwrap();
            let output = store.alloc_region(RegionKind::Output, Some(*id)).unwrap();
            for i in 0..*header_lines {
                store.note_line(header, LineId(i as u64 + 1)).unwrap();
            }
            for i in 0..*output_lines {
                store
                    .note_line(output, LineId(100 + i as u64))
                    .unwrap();
            }
            let _ = RegionState::Writable;
            blocks.push(BlockContent {
                id: *id,
                header,
                output,
            });
        }
        (store, blocks)
    }

    #[test]
    fn prefix_heights_and_visible_window() {
        let (store, blocks) = store_with_spines(&[
            (BlockId(1), 1, 2), // height 3
            (BlockId(2), 0, 4), // height 4
            (BlockId(3), 1, 1), // height 2
        ]);
        let mut index = BlockViewportIndex::default();
        index.rebuild(&blocks, &store);
        assert_eq!(index.total_height(), 9);
        assert_eq!(index.height_of(BlockId(2)), Some(4));

        // Window covering the last row of block1 + all of block2.
        let spans = index.visible_spans(2, 5);
        assert_eq!(
            spans,
            vec![
                VisibleBlockSpan {
                    block_id: BlockId(1),
                    y: 0,
                    height: 3,
                    first_visible_row: 2,
                    visible_rows: 1,
                },
                VisibleBlockSpan {
                    block_id: BlockId(2),
                    y: 3,
                    height: 4,
                    first_visible_row: 0,
                    visible_rows: 4,
                },
            ]
        );
    }

    #[test]
    fn set_height_repairs_prefix_without_full_rebuild() {
        let (store, blocks) = store_with_spines(&[(BlockId(1), 1, 1), (BlockId(2), 1, 1)]);
        let mut index = BlockViewportIndex::default();
        index.rebuild(&blocks, &store);
        assert_eq!(index.total_height(), 4);
        assert!(index.set_height(BlockId(1), 10));
        assert_eq!(index.total_height(), 12);
        let spans = index.visible_spans(9, 2);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].block_id, BlockId(1));
        assert_eq!(spans[0].first_visible_row, 9);
        assert_eq!(spans[1].block_id, BlockId(2));
    }

    #[test]
    fn empty_and_past_end_return_no_spans() {
        let mut index = BlockViewportIndex::default();
        assert!(index.visible_spans(0, 10).is_empty());
        let (store, blocks) = store_with_spines(&[(BlockId(1), 0, 2)]);
        index.rebuild(&blocks, &store);
        assert!(index.visible_spans(2, 4).is_empty());
        assert!(index.visible_spans(0, 0).is_empty());
    }
}
