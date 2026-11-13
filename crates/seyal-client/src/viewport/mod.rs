//! Per-display Block viewport index (#1484 Findings A+B).
//!
//! Runtime publishes canonical Block content metrics; each `LocalDisplayClient`
//! owns an independent order-statistic height index and scroll anchor. This is
//! not TerminalState authority.

mod index;

pub use index::{BlockViewportIndex, VisibleBlockSpan, WorkCounter};
