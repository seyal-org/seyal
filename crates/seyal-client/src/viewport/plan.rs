//! Rust-owned `BlockViewportPlan` (ADR-009 / Finding C).
//!
//! Built per displayHandle from the client BlockViewportIndex + bounded
//! content metrics. Metal consumes this plan; Swift must not invent geometry.

use super::index::{BlockViewportIndex, VisibleBlockSpan};

/// Stable source-relative scroll anchor for one display attachment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollAnchor {
    /// Live end of the indexed transcript.
    LiveEnd,
    /// Absolute presentation Y within the client index.
    AbsoluteY(u64),
    /// Block-relative: BlockId + row within that Block's height.
    BlockRow { block_id: u64, row: u32 },
}

/// One visible Block (or clipped portion) in the plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisiblePlanItem {
    pub block_id: u64,
    pub header_region: u64,
    pub output_region: u64,
    pub y: u64,
    pub height: u32,
    pub first_visible_row: u32,
    pub visible_rows: u32,
    pub sealed_rows: u32,
    pub live_first_row: u16,
    pub live_row_count: u16,
}

/// Clipped grid range for Metal page prepare.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClippedGridRange {
    pub block_id: u64,
    pub region_id: u64,
    pub first_row: u32,
    pub row_count: u32,
    pub sealed: bool,
}

/// Bounded batched viewport render plan (storage-contract §4 / ADR-009).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockViewportPlan {
    pub execution_id: u64,
    pub display_handle: u64,
    pub presentation_epoch: u64,
    pub source_generation: u64,
    pub block_revision: u64,
    pub content_generation: u64,
    pub scroll_anchor: ScrollAnchor,
    /// Absolute presentation Y of the visible window origin.
    pub scroll_y: u64,
    /// Visible window height in presentation rows.
    pub window_rows: u32,
    pub total_document_extent: u64,
    pub visible_items: Vec<VisiblePlanItem>,
    pub clipping: Vec<ClippedGridRange>,
}

/// Inputs for [`BlockViewportPlan::build`] (keeps the builder under Clippy's
/// argument limit without inventing a second plan authority).
pub struct BlockViewportPlanRequest<'a> {
    pub execution_id: u64,
    pub display_handle: u64,
    pub presentation_epoch: u64,
    pub source_generation: u64,
    pub block_revision: u64,
    pub content_generation: u64,
    pub scroll_anchor: ScrollAnchor,
    pub index: &'a BlockViewportIndex,
    pub window_rows: u32,
    pub records: &'a [(u64, u64, u64, u32, u16, u16)],
}

impl BlockViewportPlan {
    /// Build a plan for `[scroll_y, scroll_y + window_rows)` from the client index
    /// and the latest native content map records.
    pub fn build(req: BlockViewportPlanRequest<'_>) -> Self {
        let scroll_y = match req.scroll_anchor {
            ScrollAnchor::LiveEnd => req
                .index
                .total_height()
                .saturating_sub(u64::from(req.window_rows)),
            ScrollAnchor::AbsoluteY(y) => y,
            ScrollAnchor::BlockRow { block_id, row } => req
                .index
                .y_of(block_id)
                .unwrap_or(0)
                .saturating_add(u64::from(row)),
        };
        let spans = req.index.visible_spans(scroll_y, req.window_rows);
        let mut visible_items = Vec::with_capacity(spans.len());
        let mut clipping = Vec::with_capacity(spans.len());
        for span in spans {
            let meta = req
                .records
                .iter()
                .find(|(id, _, _, _, _, _)| *id == span.block_id);
            let (header, output, sealed, live_first, live_count) = meta
                .map(|(_, h, o, sealed, lf, lc)| (*h, *o, *sealed, *lf, *lc))
                .unwrap_or((0, 0, 0, 0, 0));
            visible_items.push(VisiblePlanItem {
                block_id: span.block_id,
                header_region: header,
                output_region: output,
                y: span.y,
                height: span.height,
                first_visible_row: span.first_visible_row,
                visible_rows: span.visible_rows,
                sealed_rows: sealed,
                live_first_row: live_first,
                live_row_count: live_count,
            });
            if output != 0 && span.visible_rows > 0 && span.first_visible_row < sealed {
                // Visible sealed rows ± bounded overscan (SPEC-008); never the
                // full Block. Overscan stays inside [0, sealed).
                const OVERSCAN: u32 = 16;
                let sealed_remaining = sealed - span.first_visible_row;
                let sealed_vis = span.visible_rows.min(sealed_remaining);
                if sealed_vis > 0 {
                    let first = span.first_visible_row.saturating_sub(OVERSCAN);
                    let end = (span.first_visible_row + sealed_vis)
                        .saturating_add(OVERSCAN)
                        .min(sealed);
                    let row_count = end.saturating_sub(first);
                    if row_count > 0 {
                        clipping.push(ClippedGridRange {
                            block_id: span.block_id,
                            region_id: output,
                            first_row: first,
                            row_count,
                            sealed: true,
                        });
                    }
                }
            }
        }
        Self {
            execution_id: req.execution_id,
            display_handle: req.display_handle,
            presentation_epoch: req.presentation_epoch,
            source_generation: req.source_generation,
            block_revision: req.block_revision,
            content_generation: req.content_generation,
            scroll_anchor: req.scroll_anchor,
            scroll_y,
            window_rows: req.window_rows,
            total_document_extent: req.index.total_height(),
            visible_items,
            clipping,
        }
    }

    pub fn from_spans_for_test(spans: &[VisibleBlockSpan]) -> Self {
        Self {
            execution_id: 0,
            display_handle: 0,
            presentation_epoch: 0,
            source_generation: 0,
            block_revision: 0,
            content_generation: 0,
            scroll_anchor: ScrollAnchor::AbsoluteY(0),
            scroll_y: 0,
            window_rows: 0,
            total_document_extent: spans
                .iter()
                .map(|s| s.y.saturating_add(u64::from(s.height)))
                .max()
                .unwrap_or(0),
            visible_items: spans
                .iter()
                .map(|span| VisiblePlanItem {
                    block_id: span.block_id,
                    header_region: 0,
                    output_region: 0,
                    y: span.y,
                    height: span.height,
                    first_visible_row: span.first_visible_row,
                    visible_rows: span.visible_rows,
                    sealed_rows: 0,
                    live_first_row: 0,
                    live_row_count: 0,
                })
                .collect(),
            clipping: Vec::new(),
        }
    }

    /// Clipping range for one Block, if the current plan covers it.
    pub fn clip_for(&self, block_id: u64) -> Option<ClippedGridRange> {
        self.clipping
            .iter()
            .copied()
            .find(|c| c.block_id == block_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport::BlockViewportIndex;

    #[test]
    fn plan_binds_display_handle_and_visible_items() {
        let mut index = BlockViewportIndex::default();
        index.replace_all([(1, 10), (2, 20), (3, 5)]);
        let records = [
            (1, 10, 11, 8, 0, 0),
            (2, 20, 21, 15, 0, 2),
            (3, 30, 31, 5, 0, 0),
        ];
        let plan = BlockViewportPlan::build(BlockViewportPlanRequest {
            execution_id: 7,
            display_handle: 99,
            presentation_epoch: 3,
            source_generation: 4,
            block_revision: 5,
            content_generation: 6,
            scroll_anchor: ScrollAnchor::AbsoluteY(8),
            index: &index,
            window_rows: 10,
            records: &records,
        });
        assert_eq!(plan.execution_id, 7);
        assert_eq!(plan.display_handle, 99);
        assert_eq!(plan.presentation_epoch, 3);
        assert_eq!(plan.total_document_extent, 35);
        assert_eq!(plan.scroll_y, 8);
        assert_eq!(plan.window_rows, 10);
        assert!(!plan.visible_items.is_empty());
        assert_eq!(plan.visible_items[0].block_id, 1);
        assert!(plan.visible_items.iter().any(|item| item.block_id == 2));
        assert!(plan.clip_for(2).is_some());
    }

    #[test]
    fn two_display_plans_keep_independent_scroll_anchors() {
        // Adversarial proof #1: one content map, two display indexes/plans.
        let records = [(1u64, 10u64, 11u64, 20u32, 0u16, 0u16)];
        let mut index_a = BlockViewportIndex::default();
        index_a.replace_all([(1, 40)]);
        let mut index_b = BlockViewportIndex::default();
        index_b.replace_all([(1, 40)]);
        let plan_a = BlockViewportPlan::build(BlockViewportPlanRequest {
            execution_id: 1,
            display_handle: 101,
            presentation_epoch: 1,
            source_generation: 1,
            block_revision: 1,
            content_generation: 1,
            scroll_anchor: ScrollAnchor::AbsoluteY(0),
            index: &index_a,
            window_rows: 10,
            records: &records,
        });
        let plan_b = BlockViewportPlan::build(BlockViewportPlanRequest {
            execution_id: 1,
            display_handle: 202,
            presentation_epoch: 1,
            source_generation: 1,
            block_revision: 1,
            content_generation: 1,
            scroll_anchor: ScrollAnchor::LiveEnd,
            index: &index_b,
            window_rows: 10,
            records: &records,
        });
        assert_eq!(plan_a.display_handle, 101);
        assert_eq!(plan_b.display_handle, 202);
        assert_eq!(plan_a.scroll_y, 0);
        assert_eq!(plan_b.scroll_y, 30);
        assert_ne!(
            plan_a.visible_items[0].first_visible_row,
            plan_b.visible_items[0].first_visible_row
        );
        // Mutating one index must not affect the other plan's source index.
        assert!(index_a.set_height(1, 50));
        assert_eq!(index_b.height_of(1), Some(40));
        assert_eq!(index_a.total_height(), 50);
    }
}
