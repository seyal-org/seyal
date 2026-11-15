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

/// Presentation policy in terminal-cell rows. The Rust plan owns the gap;
/// Swift only converts row geometry to drawable pixels.
pub const BLOCK_SPACING_ROWS: u32 = 1;

/// Semantic role of a Block-owned RegionGrid clip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BlockRegionRole {
    Header = 0,
    Output = 1,
}

/// Rust hit-test result for one row in the Flow canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockHit {
    pub block_id: u64,
    pub region_id: u64,
    pub region_row: u32,
    pub role: BlockRegionRole,
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
    pub header_rows: u32,
    pub live_first_row: u16,
    pub live_row_count: u16,
}

/// Clipped grid range for Metal page prepare.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClippedGridRange {
    pub block_id: u64,
    pub region_id: u64,
    /// First source row in this RegionGrid.
    pub first_row: u32,
    pub row_count: u32,
    /// Absolute presentation row for `first_row` in the document.
    pub presentation_y: u64,
    pub role: BlockRegionRole,
    pub sealed: bool,
}

/// Rust-owned solid-surface geometry for one visible portion of a Block.
/// Coordinates remain in presentation rows until the platform adapter maps
/// them to drawable pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockSurfaceRange {
    pub block_id: u64,
    pub presentation_y: u64,
    pub row_count: u32,
    pub role: BlockSurfaceRole,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BlockSurfaceRole {
    Card = 0,
    Header = 1,
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
    /// Rows of bottom padding needed to dock short content at the composer.
    pub alignment_padding_rows: u32,
    pub visible_items: Vec<VisiblePlanItem>,
    pub clipping: Vec<ClippedGridRange>,
    pub surfaces: Vec<BlockSurfaceRange>,
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
    pub records: &'a [(u64, u64, u64, u32, u32, u16, u16)],
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
            ScrollAnchor::BlockRow { block_id, row } => {
                let height = req.index.height_of(block_id).unwrap_or(0);
                let row = row.min(height.saturating_sub(1));
                req.index
                    .y_of(block_id)
                    .unwrap_or(0)
                    .saturating_add(u64::from(row))
            }
        };
        let spans = req.index.visible_spans(scroll_y, req.window_rows);
        // Build one ID index so every visible span resolves its metadata in
        // expected O(1), rather than rescanning the full Block list (O(V×B)).
        // Keep the first matching record to preserve the former `.find()`
        // behavior even if a malformed input contains duplicate BlockIds.
        let mut record_index_by_id = std::collections::HashMap::with_capacity(req.records.len());
        for (record_index, record) in req.records.iter().enumerate() {
            record_index_by_id.entry(record.0).or_insert(record_index);
        }
        let mut visible_items = Vec::with_capacity(spans.len());
        let mut clipping = Vec::with_capacity(spans.len());
        let mut surfaces = Vec::with_capacity(spans.len().saturating_mul(2));
        for span in spans {
            let meta = record_index_by_id
                .get(&span.block_id)
                .map(|&record_index| &req.records[record_index]);
            let (header, output, header_rows, sealed, live_first, live_count) = meta
                .map(|(_, h, o, hr, sealed, lf, lc)| (*h, *o, *hr, *sealed, *lf, *lc))
                .unwrap_or((0, 0, 0, 0, 0, 0));
            let output_capacity = span
                .height
                .saturating_sub(header_rows.saturating_add(BLOCK_SPACING_ROWS));
            let live_count =
                u16::try_from(u32::from(live_count).min(output_capacity.saturating_sub(sealed)))
                    .unwrap_or(u16::MAX);
            visible_items.push(VisiblePlanItem {
                block_id: span.block_id,
                header_region: header,
                output_region: output,
                y: span.y,
                height: span.height,
                first_visible_row: span.first_visible_row,
                visible_rows: span.visible_rows,
                sealed_rows: sealed,
                header_rows,
                live_first_row: live_first,
                live_row_count: live_count,
            });
            // A card ends before its trailing Rust-owned spacing row. Emit
            // complete body geometry plus a header overlay as render intent.
            let card_end = span.height.saturating_sub(BLOCK_SPACING_ROWS);
            let card_first = span.first_visible_row.min(card_end);
            let card_visible_end = span
                .first_visible_row
                .saturating_add(span.visible_rows)
                .min(card_end);
            if card_visible_end > card_first {
                surfaces.push(BlockSurfaceRange {
                    block_id: span.block_id,
                    presentation_y: span.y.saturating_add(u64::from(card_first)),
                    row_count: card_visible_end - card_first,
                    role: BlockSurfaceRole::Card,
                });
                let header_first = card_first.min(header_rows);
                let header_end = card_visible_end.min(header_rows);
                if header_end > header_first {
                    surfaces.push(BlockSurfaceRange {
                        block_id: span.block_id,
                        presentation_y: span.y.saturating_add(u64::from(header_first)),
                        row_count: header_end - header_first,
                        role: BlockSurfaceRole::Header,
                    });
                }
            }
            if span.visible_rows > 0 {
                let visible_end = span.first_visible_row.saturating_add(span.visible_rows);
                let header_first = span.first_visible_row.min(header_rows);
                let header_end = visible_end.min(header_rows);
                if header != 0 && header_end > header_first {
                    clipping.push(ClippedGridRange {
                        block_id: span.block_id,
                        region_id: header,
                        first_row: header_first,
                        row_count: header_end - header_first,
                        presentation_y: span.y.saturating_add(u64::from(header_first)),
                        role: BlockRegionRole::Header,
                        sealed: true,
                    });
                }

                if output != 0 {
                    // The indexed span includes one Rust-owned inter-Block row.
                    // Keep it out of the output RegionGrid extent and leave it
                    // unpainted for the Metal compositor to render as spacing.
                    let output_extent = span
                        .height
                        .saturating_sub(header_rows.saturating_add(BLOCK_SPACING_ROWS))
                        .min(sealed);
                    let output_first = span.first_visible_row.saturating_sub(header_rows);
                    let output_end = visible_end.saturating_sub(header_rows).min(output_extent);
                    if output_end > output_first {
                        // Visible output ± bounded overscan. RegionGrid row IDs,
                        // not Swift cards or LineId ranges, select the content.
                        const OVERSCAN: u32 = 16;
                        let first = output_first.saturating_sub(OVERSCAN).min(output_extent);
                        let end = output_end.saturating_add(OVERSCAN).min(output_extent);
                        clipping.push(ClippedGridRange {
                            block_id: span.block_id,
                            region_id: output,
                            first_row: first,
                            row_count: end - first,
                            presentation_y: span
                                .y
                                .saturating_add(u64::from(header_rows))
                                .saturating_add(u64::from(first)),
                            role: BlockRegionRole::Output,
                            sealed: first < sealed,
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
            alignment_padding_rows: u32::try_from(
                u64::from(req.window_rows).saturating_sub(req.index.total_height()),
            )
            .unwrap_or(u32::MAX),
            visible_items,
            clipping,
            surfaces,
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
            alignment_padding_rows: 0,
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
                    header_rows: 0,
                    live_first_row: 0,
                    live_row_count: 0,
                })
                .collect(),
            clipping: Vec::new(),
            surfaces: Vec::new(),
        }
    }

    /// Resolve a visible canvas row to its canonical Block RegionGrid.
    /// Gap rows and short-content bottom padding are intentionally not hits.
    pub fn hit_test_viewport_row(&self, viewport_row: u32) -> Option<BlockHit> {
        if viewport_row >= self.window_rows {
            return None;
        }
        let row = u64::from(viewport_row.saturating_sub(self.alignment_padding_rows))
            .saturating_add(self.scroll_y);
        let item = self
            .visible_items
            .iter()
            .find(|item| row >= item.y && row < item.y.saturating_add(u64::from(item.height)))?;
        let block_row = u32::try_from(row.saturating_sub(item.y)).ok()?;
        if block_row < item.header_rows {
            return Some(BlockHit {
                block_id: item.block_id,
                region_id: item.header_region,
                region_row: block_row,
                role: BlockRegionRole::Header,
            });
        }
        let output_end = item.height.saturating_sub(BLOCK_SPACING_ROWS);
        if block_row < output_end && item.output_region != 0 {
            return Some(BlockHit {
                block_id: item.block_id,
                region_id: item.output_region,
                region_row: block_row - item.header_rows,
                role: BlockRegionRole::Output,
            });
        }
        None
    }

    /// Clipping range for one Block, if the current plan covers it.
    pub fn clip_for(&self, block_id: u64) -> Option<ClippedGridRange> {
        self.clipping
            .iter()
            .copied()
            .find(|c| c.block_id == block_id)
    }

    pub fn clip_for_region(&self, region_id: u64) -> Option<ClippedGridRange> {
        self.clipping
            .iter()
            .copied()
            .find(|clip| clip.region_id == region_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport::BlockViewportIndex;

    #[test]
    fn native_block_plan_clips_header_output_and_hit_tests_spacing() {
        let mut index = BlockViewportIndex::default();
        index.replace_all([(7, 9)]);
        let records = [(7u64, 70u64, 71u64, 3u32, 4u32, 0u16, 1u16)];
        let plan = BlockViewportPlan::build(BlockViewportPlanRequest {
            execution_id: 1,
            display_handle: 1,
            presentation_epoch: 1,
            source_generation: 1,
            block_revision: 1,
            content_generation: 1,
            scroll_anchor: ScrollAnchor::AbsoluteY(0),
            index: &index,
            window_rows: 9,
            records: &records,
        });

        let item = plan.visible_items.first().expect("visible Block");
        assert_eq!(item.height, 9);
        assert_eq!(item.header_rows, 3);
        assert_eq!(
            plan.surfaces,
            [
                BlockSurfaceRange {
                    block_id: 7,
                    presentation_y: 0,
                    row_count: 8,
                    role: BlockSurfaceRole::Card,
                },
                BlockSurfaceRange {
                    block_id: 7,
                    presentation_y: 0,
                    row_count: 3,
                    role: BlockSurfaceRole::Header,
                },
            ],
            "Rust provides card and header geometry; the trailing spacing row is outside the card"
        );
        let header = plan.clip_for_region(70).expect("header clip");
        assert_eq!(header.first_row, 0);
        assert_eq!(header.row_count, 3);
        let output = plan.clip_for_region(71).expect("output clip");
        assert_eq!(output.first_row, 0);
        assert_eq!(
            output.row_count, 4,
            "only immutable sealed rows use HISTORY"
        );
        assert_eq!(
            item.height, 9,
            "Rust plan includes one explicit inter-Block spacing row"
        );
        assert_eq!(
            plan.hit_test_viewport_row(0),
            Some(BlockHit {
                block_id: 7,
                region_id: 70,
                region_row: 0,
                role: BlockRegionRole::Header,
            })
        );
        assert_eq!(
            plan.hit_test_viewport_row(3),
            Some(BlockHit {
                block_id: 7,
                region_id: 71,
                region_row: 0,
                role: BlockRegionRole::Output,
            })
        );
        assert_eq!(
            plan.hit_test_viewport_row(8),
            None,
            "spacing belongs to layout, not content"
        );
    }

    #[test]
    fn plan_metadata_lookup_matches_linear_order_for_unsorted_records() {
        let mut index = BlockViewportIndex::default();
        index.replace_all([(10, 5), (20, 6), (30, 7)]);
        let ordered_records = [
            (10u64, 100u64, 101u64, 1u32, 2u32, 0u16, 1u16),
            (20, 200, 201, 2, 3, 0, 1),
            (30, 300, 301, 3, 4, 0, 1),
        ];
        let unsorted_records = [ordered_records[2], ordered_records[0], ordered_records[1]];

        let build = |records: &[(u64, u64, u64, u32, u32, u16, u16)]| {
            BlockViewportPlan::build(BlockViewportPlanRequest {
                execution_id: 1,
                display_handle: 2,
                presentation_epoch: 3,
                source_generation: 4,
                block_revision: 5,
                content_generation: 6,
                scroll_anchor: ScrollAnchor::AbsoluteY(0),
                index: &index,
                window_rows: 18,
                records,
            })
        };

        assert_eq!(
            build(&ordered_records),
            build(&unsorted_records),
            "ID-indexed metadata lookup must produce the same plan regardless of record order"
        );
    }

    #[test]
    fn plan_metadata_lookup_preserves_first_duplicate_record() {
        let mut index = BlockViewportIndex::default();
        index.replace_all([(7, 4)]);
        let records = [
            (7u64, 70u64, 71u64, 1u32, 1u32, 0u16, 1u16),
            (7u64, 700u64, 701u64, 2u32, 0u32, 0u16, 0u16),
        ];
        let plan = BlockViewportPlan::build(BlockViewportPlanRequest {
            execution_id: 1,
            display_handle: 1,
            presentation_epoch: 1,
            source_generation: 1,
            block_revision: 1,
            content_generation: 1,
            scroll_anchor: ScrollAnchor::AbsoluteY(0),
            index: &index,
            window_rows: 4,
            records: &records,
        });

        assert_eq!(plan.visible_items[0].header_region, 70);
        assert_eq!(plan.visible_items[0].output_region, 71);
        assert_eq!(plan.visible_items[0].header_rows, 1);
    }

    #[test]
    fn live_only_scrolled_block_keeps_its_card_surface_without_history_clips() {
        let mut index = BlockViewportIndex::default();
        index.replace_all([(7, 9)]);
        let records = [(7u64, 70u64, 71u64, 3u32, 0u32, 0u16, 5u16)];
        let plan = BlockViewportPlan::build(BlockViewportPlanRequest {
            execution_id: 1,
            display_handle: 1,
            presentation_epoch: 1,
            source_generation: 1,
            block_revision: 1,
            content_generation: 1,
            scroll_anchor: ScrollAnchor::AbsoluteY(4),
            index: &index,
            window_rows: 3,
            records: &records,
        });

        assert!(
            plan.clipping.is_empty(),
            "live output has no sealed HISTORY clip"
        );
        assert_eq!(plan.visible_items[0].live_row_count, 5);
        assert_eq!(
            plan.surfaces,
            [BlockSurfaceRange {
                block_id: 7,
                presentation_y: 4,
                row_count: 3,
                role: BlockSurfaceRole::Card,
            }],
            "Rust must retain card geometry after the header scrolls above the viewport"
        );
    }

    #[test]
    fn plan_binds_display_handle_and_visible_items() {
        let mut index = BlockViewportIndex::default();
        index.replace_all([(1, 11), (2, 21), (3, 6)]);
        let records = [
            (1, 10, 11, 0, 8, 0, 0),
            (2, 20, 21, 0, 15, 0, 2),
            (3, 30, 31, 0, 5, 0, 0),
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
        assert_eq!(plan.total_document_extent, 38);
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
        let records = [(1u64, 10u64, 11u64, 0u32, 20u32, 0u16, 0u16)];
        let mut index_a = BlockViewportIndex::default();
        index_a.replace_all([(1, 41)]);
        let mut index_b = BlockViewportIndex::default();
        index_b.replace_all([(1, 41)]);
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
        assert_eq!(plan_b.scroll_y, 31);
        assert_ne!(
            plan_a.visible_items[0].first_visible_row,
            plan_b.visible_items[0].first_visible_row
        );
        // Mutating one index must not affect the other plan's source index.
        assert!(index_a.set_height(1, 50));
        assert_eq!(index_b.height_of(1), Some(41));
        assert_eq!(index_a.total_height(), 50);
    }

    #[test]
    fn plan_clips_short_completed_block_with_zero_sealed_rows() {
        // printf-sized Block: all content still on-window, sealed_rows == 0.
        let mut index = BlockViewportIndex::default();
        index.replace_all([(7, 2)]);
        let records = [(7u64, 70u64, 71u64, 0u32, 0u32, 0u16, 1u16)];
        let plan = BlockViewportPlan::build(BlockViewportPlanRequest {
            execution_id: 1,
            display_handle: 1,
            presentation_epoch: 1,
            source_generation: 1,
            block_revision: 1,
            content_generation: 1,
            scroll_anchor: ScrollAnchor::LiveEnd,
            index: &index,
            window_rows: 24,
            records: &records,
        });
        let item = plan.visible_items.first().expect("live-only Block");
        assert_eq!(item.sealed_rows, 0);
        assert_eq!(item.live_first_row, 0);
        assert_eq!(item.live_row_count, 1);
        assert!(plan.clip_for(7).is_none(), "live rows bypass HISTORY fetch");
    }

    #[test]
    fn absolute_y_selects_onset_while_live_end_selects_tail() {
        // G5: AppKit mid-scroll AbsoluteY must paint onset HISTORY, not LiveEnd.
        let mut index = BlockViewportIndex::default();
        index.replace_all([(1, 41), (2, 41)]);
        let records = [
            (1u64, 10u64, 11u64, 0u32, 40u32, 0u16, 0u16),
            (2u64, 20u64, 21u64, 0u32, 40u32, 0u16, 0u16),
        ];
        let onset = BlockViewportPlan::build(BlockViewportPlanRequest {
            execution_id: 1,
            display_handle: 1,
            presentation_epoch: 1,
            source_generation: 1,
            block_revision: 1,
            content_generation: 1,
            scroll_anchor: ScrollAnchor::AbsoluteY(0),
            index: &index,
            window_rows: 10,
            records: &records,
        });
        let live = BlockViewportPlan::build(BlockViewportPlanRequest {
            execution_id: 1,
            display_handle: 1,
            presentation_epoch: 1,
            source_generation: 1,
            block_revision: 1,
            content_generation: 1,
            scroll_anchor: ScrollAnchor::LiveEnd,
            index: &index,
            window_rows: 10,
            records: &records,
        });
        assert_eq!(onset.scroll_y, 0);
        assert_eq!(live.scroll_y, 72);
        assert!(onset.clip_for(1).is_some());
        assert!(onset.clip_for(2).is_none());
        assert!(live.clip_for(2).is_some());
        assert!(live.clip_for(1).is_none());
    }

    #[test]
    fn plan_does_not_request_history_when_live_end_is_past_sealed() {
        // 100k-class live end: viewport sits entirely in the live window past sealed.
        let mut index = BlockViewportIndex::default();
        index.replace_all([(9, 101)]);
        let records = [(9u64, 90u64, 91u64, 0u32, 80u32, 0u16, 20u16)];
        let plan = BlockViewportPlan::build(BlockViewportPlanRequest {
            execution_id: 1,
            display_handle: 1,
            presentation_epoch: 1,
            source_generation: 1,
            block_revision: 1,
            content_generation: 1,
            scroll_anchor: ScrollAnchor::LiveEnd,
            index: &index,
            window_rows: 10,
            records: &records,
        });
        assert_eq!(plan.scroll_y, 91);
        let item = &plan.visible_items[0];
        assert!(item.first_visible_row >= 80);
        assert!(
            plan.clip_for(9).is_none(),
            "the live-end window is entirely on-window and must not request HISTORY"
        );
    }
}
