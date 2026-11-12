//! Stable identities for native Block regions.

use crate::LineId;

/// Runtime-issued Block identity. Never reused within an ExecutionId.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BlockId(pub u64);

/// Region identity within one PrimaryBlockEngine / RegionStore.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RegionId(pub u64);

/// Generational locator into RegionStore mutable pages (non-owning).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CellHandle {
    pub page: u64,
    pub slot: u32,
    pub generation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RegionKind {
    PromptCommand,
    Output,
    Unstructured,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RegionState {
    Writable,
    PartiallySealed,
    Sealed,
}

/// First-class Block content ownership (not a history line range).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BlockContent {
    pub id: BlockId,
    pub header: RegionId,
    pub output: RegionId,
}

/// One sealed off-window row owned by a RegionGrid (spill handles, not window).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SealedRow {
    pub line_id: LineId,
    pub handles: Vec<CellHandle>,
}

/// RegionGrid owns content identity through RegionStore runs/handles.
#[derive(Clone, Debug)]
pub(crate) struct RegionGrid {
    pub id: RegionId,
    pub owner: Option<BlockId>,
    pub kind: RegionKind,
    pub state: RegionState,
    /// Ordered source runs keyed by LineId; payload lives in RegionStore.
    pub line_spine: Vec<LineId>,
    /// Sealed spill rows in chronological order (scroll/ED2 retention).
    pub sealed_rows: Vec<SealedRow>,
}
