//! Per-display Block viewport index + plan (#1484 Findings A+B+C).
//!
//! Runtime publishes canonical Block content metrics; each `LocalDisplayClient`
//! owns an independent order-statistic height index, scroll anchor, and
//! `BlockViewportPlan`. This is not TerminalState authority.

mod index;
mod plan;

pub use index::{BlockViewportIndex, VisibleBlockSpan, WorkCounter};
pub use plan::{
    BlockViewportPlan, BlockViewportPlanRequest, ClippedGridRange, ScrollAnchor, VisiblePlanItem,
};
