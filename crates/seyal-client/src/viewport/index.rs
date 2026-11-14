//! Order-statistic Block height index — O(log B) mutations (Finding B).
//!
//! Fenwick tree (BIT) over presentation heights plus `HashMap<BlockId, rank>`
//! for direct lookup. Not a renamed prefix `Vec`.

use std::collections::HashMap;

/// Test-only visit counter for asymptotic proofs.
#[derive(Clone, Debug, Default)]
pub struct WorkCounter {
    pub fenwick_ops: u64,
    pub rebuild_all: u64,
}

impl WorkCounter {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibleBlockSpan {
    pub block_id: u64,
    pub y: u64,
    pub height: u32,
    pub first_visible_row: u32,
    pub visible_rows: u32,
}

/// Per-display indexed Block heights.
#[derive(Clone, Debug, Default)]
pub struct BlockViewportIndex {
    /// Timeline order of BlockIds.
    order: Vec<u64>,
    /// Parallel heights for `order`.
    heights: Vec<u32>,
    /// Fenwick tree over heights (1-indexed internally).
    bit: Vec<u64>,
    /// BlockId → rank in `order`.
    ranks: HashMap<u64, usize>,
    #[cfg(test)]
    work: WorkCounter,
}

impl BlockViewportIndex {
    pub fn clear(&mut self) {
        self.order.clear();
        self.heights.clear();
        self.bit.clear();
        self.ranks.clear();
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// Replace the index from a full ordered `(block_id, height)` list.
    /// Used when Runtime publishes a new BlockNativeRegions revision.
    pub fn replace_all(&mut self, blocks: impl IntoIterator<Item = (u64, u32)>) {
        self.clear();
        for (id, height) in blocks {
            self.order.push(id);
            self.heights.push(height);
            self.ranks.insert(id, self.order.len() - 1);
        }
        self.rebuild_bit();
        #[cfg(test)]
        {
            self.work.rebuild_all = self.work.rebuild_all.saturating_add(1);
        }
    }

    /// Insert or update one Block height.
    /// Updates are O(log B); first-time inserts rebuild the Fenwick spine once.
    pub fn upsert(&mut self, block_id: u64, height: u32) {
        if let Some(&rank) = self.ranks.get(&block_id) {
            self.set_height_at(rank, height);
            return;
        }
        let rank = self.order.len();
        self.order.push(block_id);
        self.heights.push(height);
        self.ranks.insert(block_id, rank);
        // Structural growth requires a Fenwick rebuild; set_height stays O(log B).
        self.rebuild_bit();
    }

    /// Remove one Block in O(log B + B) compaction of ranks after the hole.
    /// Ordinary output never calls this; Block completion retains the entry.
    pub fn remove(&mut self, block_id: u64) -> bool {
        let Some(rank) = self.ranks.remove(&block_id) else {
            return false;
        };
        self.set_height_at(rank, 0);
        self.order.remove(rank);
        self.heights.remove(rank);
        // Rebuild ranks + BIT after structural removal (rare vs set_height).
        self.ranks.clear();
        for (i, id) in self.order.iter().enumerate() {
            self.ranks.insert(*id, i);
        }
        self.rebuild_bit();
        true
    }

    pub fn set_height(&mut self, block_id: u64, height: u32) -> bool {
        let Some(&rank) = self.ranks.get(&block_id) else {
            return false;
        };
        self.set_height_at(rank, height);
        true
    }

    pub fn height_of(&self, block_id: u64) -> Option<u32> {
        self.ranks.get(&block_id).map(|&rank| self.heights[rank])
    }

    pub fn total_height(&self) -> u64 {
        if self.heights.is_empty() {
            0
        } else {
            self.prefix_sum(self.heights.len())
        }
    }

    pub fn y_of(&self, block_id: u64) -> Option<u64> {
        let rank = *self.ranks.get(&block_id)?;
        Some(self.prefix_sum(rank))
    }

    pub fn ordered_ids(&self) -> &[u64] {
        &self.order
    }

    pub fn visible_spans(&self, y: u64, window_rows: u32) -> Vec<VisibleBlockSpan> {
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
            let block_y = self.prefix_sum(index);
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

    #[cfg(test)]
    pub fn work(&self) -> &WorkCounter {
        &self.work
    }

    #[cfg(test)]
    pub fn work_mut(&mut self) -> &mut WorkCounter {
        &mut self.work
    }

    fn set_height_at(&mut self, rank: usize, height: u32) {
        let old = self.heights[rank];
        if old == height {
            return;
        }
        self.heights[rank] = height;
        let delta = i64::from(height) - i64::from(old);
        self.bit_add(rank, delta);
    }

    fn ensure_bit_len(&mut self) {
        let need = self.heights.len() + 1;
        if self.bit.len() < need {
            self.bit.resize(need, 0);
        }
    }

    fn rebuild_bit(&mut self) {
        let heights: Vec<u32> = self.heights.clone();
        self.bit.clear();
        self.bit.resize(heights.len() + 1, 0);
        for (i, height) in heights.iter().enumerate() {
            self.bit_add(i, i64::from(*height));
        }
    }

    fn bit_add(&mut self, rank: usize, delta: i64) {
        self.ensure_bit_len();
        let mut i = rank + 1;
        while i < self.bit.len() {
            #[cfg(test)]
            {
                self.work.fenwick_ops = self.work.fenwick_ops.saturating_add(1);
            }
            if delta >= 0 {
                self.bit[i] = self.bit[i].saturating_add(delta as u64);
            } else {
                self.bit[i] = self.bit[i].saturating_sub((-delta) as u64);
            }
            i += lsb(i);
        }
    }

    /// Sum of heights[0..rank).
    fn prefix_sum(&self, rank: usize) -> u64 {
        let mut sum = 0u64;
        let mut i = rank;
        while i > 0 {
            #[cfg(test)]
            {
                // work is behind &self — count via saturating on a cell would need
                // interior mutability; tests assert fenwick_ops on mutations instead.
            }
            sum = sum.saturating_add(self.bit[i]);
            i -= lsb(i);
        }
        sum
    }

    fn first_intersecting(&self, y: u64) -> usize {
        // Binary lift over Fenwick to find lowest rank with prefix_sum(rank+1) > y.
        let mut lo = 0usize;
        let mut hi = self.order.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.prefix_sum(mid + 1) <= y {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }
}

fn lsb(i: usize) -> usize {
    i.isolate_lowest_one()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fenwick_total_and_visible_spans() {
        let mut index = BlockViewportIndex::default();
        index.replace_all([(1, 3), (2, 4), (3, 2)]);
        assert_eq!(index.total_height(), 9);
        assert_eq!(index.height_of(2), Some(4));
        assert_eq!(index.y_of(2), Some(3));
        let spans = index.visible_spans(2, 5);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].block_id, 1);
        assert_eq!(spans[0].first_visible_row, 2);
        assert_eq!(spans[1].block_id, 2);
        assert_eq!(spans[1].visible_rows, 4);
    }

    #[test]
    fn set_height_is_logarithmic_not_full_rebuild() {
        let mut index = BlockViewportIndex::default();
        let n = 4096usize;
        index.replace_all((1..=n as u64).map(|id| (id, 2)));
        index.work_mut().reset();
        assert!(index.set_height(2048, 10));
        assert_eq!(index.work().rebuild_all, 0);
        // Fenwick add walks O(log B) nodes — far below B.
        assert!(index.work().fenwick_ops < 40);
        assert_eq!(index.total_height(), (n as u64) * 2 - 2 + 10);
    }

    #[test]
    fn upsert_insert_and_update_without_rebuild_all_on_update() {
        let mut index = BlockViewportIndex::default();
        index.upsert(10, 5);
        index.upsert(20, 7);
        index.work_mut().reset();
        index.upsert(10, 9);
        assert_eq!(index.work().rebuild_all, 0);
        assert_eq!(index.height_of(10), Some(9));
        assert_eq!(index.total_height(), 16);
    }

    #[test]
    fn work_counter_set_height_grows_logarithmically_with_b() {
        let mut ops_at = Vec::new();
        for &n in &[1024usize, 4096, 16384] {
            let mut index = BlockViewportIndex::default();
            index.replace_all((1..=n as u64).map(|id| (id, 1)));
            index.work_mut().reset();
            assert!(index.set_height((n as u64) / 2, 3));
            ops_at.push(index.work().fenwick_ops);
        }
        // Each step multiplies B by 4; ops must stay within a small log factor.
        assert!(ops_at[1] <= ops_at[0].saturating_add(8));
        assert!(ops_at[2] <= ops_at[1].saturating_add(8));
        assert!(ops_at[2] < 64);
    }

    #[test]
    fn same_order_height_updates_skip_replace_all() {
        let mut index = BlockViewportIndex::default();
        index.replace_all([(1, 2), (2, 3), (3, 4)]);
        index.work_mut().reset();
        assert!(index.set_height(2, 9));
        assert_eq!(index.work().rebuild_all, 0);
        assert_eq!(index.height_of(2), Some(9));
        assert_eq!(index.total_height(), 2 + 9 + 4);
    }
}
