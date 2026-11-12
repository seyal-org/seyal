//! Canonical native-Block primary storage (#1484 / ADR-009).
//!
//! `RegionStore` is the sole physical allocator of primary cells and retained
//! source units. The live PTY window payload lives only on window page 0
//! (`grid_cells` / `grid_anchors`); `VTWindowMap` holds non-owning generational
//! handles into that page. Block timeline metadata remains Runtime-owned;
//! this module stores RegionGrid content keyed by Runtime-issued BlockIds.
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
