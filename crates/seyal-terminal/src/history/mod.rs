//! Canonical retained primary history and width-derived reflow.
//!
//! History owns source text units, rather than the physical rows used by the
//! active screen.  It deliberately has no persistence or renderer dependency;
//! sealed segments are the bounded handoff seam for those consumers.

mod eviction;
mod query;
mod reflow;
mod store;
mod types;
mod wrap;

#[cfg(test)]
mod tests;

pub use reflow::ReflowRow;
pub use types::{
    HistoryAnchor, HistoryAnchorResolution, HistoryBreakAfter, HistoryMatch, HistoryRangeError,
    HistoryUnitView, HistoryWireCell, HISTORY_PER_EXECUTION_BYTE_CAP,
    HISTORY_PER_EXECUTION_DERIVED_INDEX_CAP, HISTORY_RUNTIME_AGGREGATE_BYTE_CAP,
    HISTORY_RUNTIME_DERIVED_INDEX_CAP, HISTORY_SEGMENT_PAYLOAD_TARGET, HISTORY_SELECTION_UNIT_CAP,
    HISTORY_TAIL_PAYLOAD_LIMIT,
};

pub(crate) use store::HistoryStore;
pub(crate) use types::{range_entirely_before, HistoryLine, HistoryLineRef, HistoryUnit};
