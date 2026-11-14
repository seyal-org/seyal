//! Content-side Block region handles (#1484).
//!
//! Display `BlockViewportIndex` lives in `seyal-client` per displayHandle
//! (Findings A+B). This module only publishes RegionId content refs and
//! on-window span metadata for Runtime content/damage.

use super::types::{BlockId, RegionId};

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

/// One Block span inside a bounded viewport window (row units = line_spine rows).
/// Built on demand from content heights; not a retained display index.
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
