//! Typed host-to-Rust shell commands and their shared value types.

use seyal_core::{ExecutionId, PaneId, TabId, WindowId, WorkspaceId};

use crate::pane_layout::SplitRatio;

use super::{MoveSide, SplitAxis};

/// Window/tab cycle direction within the Rust-owned order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CycleDirection {
    Next,
    Previous,
}

/// Typed host → Rust command. One action is one coarse transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellAction {
    CreateWindow {
        workspace: WorkspaceId,
        containment_generation: u64,
    },
    ActivateWorkspace {
        workspace: WorkspaceId,
        containment_generation: u64,
    },
    /// Selection actions carry the snapshot generation for complete host action context,
    /// but acceptance is fenced only by live identity (ADR-018 §6).
    SelectWindow {
        id: WindowId,
        containment_generation: u64,
    },
    SelectTab {
        id: TabId,
        containment_generation: u64,
    },
    CreateTab {
        window: WindowId,
        containment_generation: u64,
    },
    CycleWindow {
        direction: CycleDirection,
        containment_generation: u64,
    },
    CycleTab {
        direction: CycleDirection,
        containment_generation: u64,
    },
    MoveTabBefore {
        tab: TabId,
        before: Option<TabId>,
        window: WindowId,
        containment_generation: u64,
    },
    MoveTabToWindow {
        tab: TabId,
        window: WindowId,
        containment_generation: u64,
    },
    MoveTabToNewWindow {
        tab: TabId,
        containment_generation: u64,
    },
    CloseTab {
        id: TabId,
        containment_generation: u64,
    },
    SplitFocused {
        axis: SplitAxis,
        containment_generation: u64,
    },
    SplitPane {
        id: PaneId,
        axis: SplitAxis,
        containment_generation: u64,
    },
    ClosePane {
        id: PaneId,
        containment_generation: u64,
    },
    FocusPane {
        id: PaneId,
    },
    ZoomPane {
        id: PaneId,
    },
    Unzoom,
    SwapPanes {
        a: PaneId,
        b: PaneId,
        containment_generation: u64,
    },
    MovePaneBeside {
        pane: PaneId,
        neighbor: PaneId,
        side: MoveSide,
        containment_generation: u64,
    },
    /// Resize the Split whose divider follows `pane` (see `PaneTree`).
    SetSplitRatio {
        pane: PaneId,
        ratio: SplitRatio,
    },
    BindExecution {
        pane: PaneId,
        execution: ExecutionId,
    },
    /// Record a live execution with no Pane binding (tests / Runtime sync).
    RecordUnpresented {
        execution: ExecutionId,
        workspace: WorkspaceId,
    },
    /// Drop a previously recorded live-unpresented entry after Runtime retirement.
    ForgetUnpresented {
        execution: ExecutionId,
    },
    /// Rebind a live-unpresented execution into a Pane leaf.
    AdoptExecution {
        pane: PaneId,
        execution: ExecutionId,
    },
    /// Explicit disposition; never implied by Tab/Window destruction.
    TerminateExecution {
        execution: ExecutionId,
    },
}
