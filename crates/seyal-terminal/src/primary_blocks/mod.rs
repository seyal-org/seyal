//! Canonical native-Block primary storage (#1484 / ADR-009).
//!
//! Live PTY window payload lives only on RegionStore window page 0
//! (`grid_cells` / `grid_anchors`); `VTWindowMap` holds non-owning generational
//! handles into that page. Sealed off-window payload transfers once into the
//! shared HistoryStore; RegionGrid `sealed_rows` index LineId + column
//! ownership only. Block timeline metadata remains Runtime-owned.
//!
//! Internal helpers remain reachable for tests and the remaining G5–G9 cutover
//! (selection/copy hit-testing, viewport plans); allow unused until those
//! call sites land in this same Issue.
#![allow(dead_code)]
//!
//! Production `Screen` no longer owns a parallel `Vec<Cell>` — print/overwrite
//! and clear-unit go through `PrimaryBlockEngine::write_at` /
//! `set_window_cell`. Bulk erase/scroll remint of window generations continues
//! in the same cutover.

pub(crate) mod attribution;
mod engine;
mod region_store;
pub(crate) mod types;
mod viewport;
mod vt_window_map;

pub use attribution::{MarkerKind, PrimaryEvent, StreamSequence, UnitVersion};
pub(crate) use engine::PrimaryBlockEngine;
pub use types::{BlockId, CellHandle, RegionId};
pub use viewport::{BlockContentRef, OnWindowRegionSpan, VisibleBlockSpan};
