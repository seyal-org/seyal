//! Portable Workspace / Tab / Pane composition for headed hosts.
//!
//! This module is product UI authority for navigation, split trees, focus, and
//! pane→execution binding metadata. It is not a Workspace database, PTY owner,
//! VT/grid, Runtime registry, or renderer. Hosts dispatch [`ShellAction`] values
//! and render [`ShellSnapshot`]. Do not call this from the PTY→VT→damage path.

mod actions;
mod effects;
mod snapshot;
mod tree;
mod unpresented;
mod workspace;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod unpresented_tests;

use std::collections::BTreeMap;
use std::fmt;

use seyal_core::{ExecutionId, PaneId, TabId, WindowId, WorkspaceId};

pub use effects::ShellNativeEffect;
pub use snapshot::{PaneLeafSnapshot, WindowSnapshot, WindowTabSnapshot};
pub use tree::{LayoutDescription, PaneTree, SplitAxis};
pub use unpresented::unpresented_palette_label;
pub use workspace::{ShellPaneSeed, ShellTabSeed, ShellWindowSeed, ShellWorkspaceSeed};

use workspace::{Pane, Tab, Window, Workspace};

/// ADR-018 §5 presentation tier for one Pane leaf (product-derived in W3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentationTier {
    Focused,
    Visible,
    Hidden,
    Unpresented,
}

/// Why a [`ShellAction`] was rejected. The previous containment state is unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellError {
    UnknownWorkspace,
    UnknownTab,
    UnknownPane,
    TabCreationUnavailable,
    PaneSplitUnavailable,
    CannotCloseLastTab,
    CannotCloseLastPane,
    CannotCloseBoundPane,
    ExecutionAlreadyBound,
    EmptyShell,
    EmptyWindow,
    EmptyTab,
    UnknownWindow,
    StaleContainment,
    MoveWouldNotChangeContainment,
    CrossWorkspaceMove,
    /// Adopt naming an execution owned by a different Workspace (ADR-017).
    CrossWorkspaceAdopt,
    /// Adopt/terminate of an execution that is not live-unpresented here
    /// (unknown, retired, or finalized).
    ExecutionNotUnpresented,
}

impl ShellError {
    fn message(self) -> &'static str {
        match self {
            Self::UnknownWorkspace => "Unknown Workspace.",
            Self::UnknownTab => "Unknown Tab.",
            Self::UnknownPane => "Unknown Pane.",
            Self::TabCreationUnavailable => {
                "Creating tabs is unavailable until a distinct execution route is available."
            }
            Self::PaneSplitUnavailable => {
                "Splitting panes is unavailable until a distinct execution route is available."
            }
            Self::CannotCloseLastTab => "The last Tab cannot be closed.",
            Self::CannotCloseLastPane => "The last Pane cannot be closed.",
            Self::CannotCloseBoundPane => {
                "A Pane bound to an execution cannot be closed until execution disposition is available."
            }
            Self::ExecutionAlreadyBound => "This execution is already bound to a Pane.",
            Self::EmptyShell => "Shell requires at least one Workspace.",
            Self::EmptyWindow => "A Window requires at least one Tab.",
            Self::EmptyTab => "A Tab requires at least one Pane.",
            Self::UnknownWindow => "Unknown Window.",
            Self::StaleContainment => "Containment generation is stale.",
            Self::MoveWouldNotChangeContainment => {
                "Moving this Tab would not change containment."
            }
            Self::CrossWorkspaceMove => "Tabs cannot move across Workspaces.",
            Self::CrossWorkspaceAdopt => {
                "An execution cannot be adopted across Workspaces."
            }
            Self::ExecutionNotUnpresented => {
                "This execution is not a live-unpresented execution in this Workspace."
            }
        }
    }
}

impl fmt::Display for ShellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

/// Direction for [`ShellAction::CycleWindow`] / [`ShellAction::CycleTab`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CycleDirection {
    Next,
    Previous,
}

/// Typed host → Rust command. One action is one coarse transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellAction {
    ActivateWorkspace {
        workspace: WorkspaceId,
        containment_generation: u64,
    },
    SelectWindow {
        id: WindowId,
    },
    SelectTab {
        id: TabId,
    },
    CycleWindow {
        direction: CycleDirection,
    },
    CycleTab {
        direction: CycleDirection,
    },
    CreateWindow {
        workspace: WorkspaceId,
        containment_generation: u64,
    },
    CreateTab {
        window: WindowId,
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
    },
    SplitFocused {
        axis: SplitAxis,
    },
    SplitPane {
        id: PaneId,
        axis: SplitAxis,
    },
    ClosePane {
        id: PaneId,
    },
    FocusPane {
        id: PaneId,
    },
    BindExecution {
        pane: PaneId,
        execution: ExecutionId,
    },
    /// Record a Runtime-owned live execution with no Pane binding (ADR-018 §3.3).
    /// Reducer tests construct Unpresented state with this; W2b will emit it on unbind.
    RecordUnpresented {
        execution: ExecutionId,
        workspace: WorkspaceId,
    },
    /// Drop a previously recorded live-unpresented entry after Runtime retirement.
    ForgetUnpresented {
        execution: ExecutionId,
    },
    /// Rebind a live-unpresented `ExecutionId` into a Pane leaf (same id; fresh
    /// `AttachmentId` is allocated on the Runtime attach path).
    AdoptExecution {
        pane: PaneId,
        execution: ExecutionId,
    },
    /// Explicit disposition. Queues [`ShellNativeEffect::TerminateExecution`];
    /// never produced by presentation close (W2b/W4b).
    TerminateExecution {
        execution: ExecutionId,
    },
}

/// Read-only projection for native hosts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellSnapshot {
    pub workspaces: Vec<WorkspaceSnapshot>,
    /// Ordered Windows across Workspaces (`workspace order`, then window order).
    pub windows: Vec<WindowSnapshot>,
    pub active_workspace: WorkspaceId,
    pub last_active_workspace: WorkspaceId,
    pub active_window: WindowId,
    pub containment_generation: u64,
    /// Active-Window Tab projection (compatible with pre-W3 hosts).
    pub tabs: Vec<TabSnapshot>,
    pub active_tab: TabId,
    pub focused_pane: PaneId,
    pub panes: Vec<PaneSnapshot>,
    pub tree: PaneTree,
    pub layout: LayoutDescription,
    pub last_error: Option<ShellError>,
    /// Whether `CreateTab`/`SplitFocused` would currently be accepted.
    /// Hosts use this to omit the control rather than show one that always
    /// fails closed (mirrors the palette's own omission of "New Tab").
    pub allows_tab_creation: bool,
    pub allows_pane_splitting: bool,
    /// Whether `CloseTab` of the active Tab / `ClosePane` of the focused
    /// Pane would currently be accepted (the last Tab/Pane cannot close, and
    /// neither can an execution-bound Pane).
    /// Hosts read these instead of re-deriving the rule from counts.
    pub allows_tab_close: bool,
    pub allows_pane_close: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneSnapshot {
    pub id: PaneId,
    pub title: String,
    pub execution: Option<ExecutionId>,
    pub allows_implicit_bootstrap: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceSnapshot {
    pub id: WorkspaceId,
    pub name: String,
    pub detail: Option<String>,
    pub attention: bool,
    pub tab_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabSnapshot {
    pub id: TabId,
    pub title: String,
    pub attention: bool,
    pub pane_count: usize,
}

/// Authoritative headed composition state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellState {
    workspaces: Vec<Workspace>,
    active_workspace: WorkspaceId,
    last_active_workspace: WorkspaceId,
    /// Most-recently-active Window order (index 0 = most recent).
    window_mru: Vec<WindowId>,
    containment_generation: u64,
    allows_pane_splitting: bool,
    allows_tab_creation: bool,
    last_error: Option<ShellError>,
    next_tab_ordinal: u32,
    /// ADR-018 §2.4 effects from the last successful commit (drained by the host path).
    pending_effects: Vec<ShellNativeEffect>,
    /// Live executions with no Pane binding in this headed session (ADR-018 §3.3).
    /// Keyed by `ExecutionId` so enumeration order is stable.
    unpresented: BTreeMap<ExecutionId, WorkspaceId>,
}

impl ShellState {
    /// M001 production composition: one Runtime default workspace, one Tab,
    /// one Pane. Extra tabs/splits stay fail-closed until a distinct execution
    /// route exists.
    pub fn m001_local(detail: impl Into<String>) -> Self {
        let pane = Pane {
            id: PaneId::new(),
            title: "Pane 1".to_owned(),
            execution: None,
            allows_implicit_execution_bootstrap: true,
        };
        let tab = Tab::with_pane(TabId::new(), "Terminal".to_owned(), pane);
        let tab_id = tab.id;
        let window_id = WindowId::new();
        let window = Window::try_new(window_id, WorkspaceId::m001_default(), vec![tab], tab_id)
            .expect("the production window has one tab");
        let workspace = Workspace {
            id: WorkspaceId::m001_default(),
            name: "Local".to_owned(),
            detail: Some(detail.into()),
            attention: false,
            windows: vec![window],
            active_window: Some(window_id),
        };
        Self {
            active_workspace: workspace.id,
            last_active_workspace: workspace.id,
            window_mru: vec![window_id],
            containment_generation: 0,
            workspaces: vec![workspace],
            allows_pane_splitting: false,
            allows_tab_creation: false,
            last_error: None,
            next_tab_ordinal: 2,
            pending_effects: Vec::new(),
            unpresented: BTreeMap::new(),
        }
    }

    /// Test/host fixture with explicit policy flags. Requires a non-empty set.
    pub fn from_workspaces(
        workspaces: Vec<ShellWorkspaceSeed>,
        active_workspace: WorkspaceId,
        allows_pane_splitting: bool,
        allows_tab_creation: bool,
    ) -> Result<Self, ShellError> {
        if workspaces.is_empty() {
            return Err(ShellError::EmptyShell);
        }
        if !workspaces.iter().any(|seed| seed.id == active_workspace) {
            return Err(ShellError::UnknownWorkspace);
        }
        let workspaces = workspaces
            .into_iter()
            .map(Workspace::from_seed)
            .collect::<Result<Vec<_>, _>>()?;
        let active = workspaces
            .iter()
            .find(|workspace| workspace.id == active_workspace)
            .expect("active workspace validated above");
        let (product_workspace, active_window) = if let Some(window) = active.active_window {
            (active_workspace, window)
        } else {
            let window = workspaces
                .iter()
                .find_map(|workspace| workspace.windows.first().map(|window| window.id))
                .ok_or(ShellError::EmptyWindow)?;
            let workspace_id = workspaces
                .iter()
                .find(|workspace| workspace.window(window).is_some())
                .map(|workspace| workspace.id)
                .expect("window belongs to a seeded workspace");
            (workspace_id, window)
        };
        let mut window_mru = Vec::new();
        window_mru.push(active_window);
        for workspace in &workspaces {
            for window in &workspace.windows {
                if window.id != active_window {
                    window_mru.push(window.id);
                }
            }
        }
        Ok(Self {
            workspaces,
            active_workspace: product_workspace,
            last_active_workspace: product_workspace,
            window_mru,
            containment_generation: 0,
            allows_pane_splitting,
            allows_tab_creation,
            last_error: None,
            next_tab_ordinal: 2,
            pending_effects: Vec::new(),
            unpresented: BTreeMap::new(),
        })
    }

    pub fn last_error(&self) -> Option<ShellError> {
        self.last_error
    }

    pub fn containment_generation(&self) -> u64 {
        self.containment_generation
    }

    /// Drain §2.4 effects produced by the last successful commit(s).
    pub fn take_effects(&mut self) -> Vec<ShellNativeEffect> {
        std::mem::take(&mut self.pending_effects)
    }

    pub(super) fn push_effect(&mut self, effect: ShellNativeEffect) {
        self.pending_effects.push(effect);
    }

    pub fn allows_pane_splitting(&self) -> bool {
        self.allows_pane_splitting
    }

    pub fn allows_tab_creation(&self) -> bool {
        self.allows_tab_creation
    }

    pub fn focused_pane_allows_implicit_bootstrap(&self) -> bool {
        self.focused_pane()
            .is_ok_and(|pane| pane.allows_implicit_execution_bootstrap)
    }

    pub fn pane_execution(&self, pane: PaneId) -> Result<Option<ExecutionId>, ShellError> {
        Ok(self.pane(pane)?.execution)
    }

    /// Containment identity for atomicity checks (excludes `last_error`).
    pub fn containment_fingerprint(&self) -> ShellState {
        let mut clone = self.clone();
        clone.last_error = None;
        clone
    }

    pub fn snapshot(&self) -> ShellSnapshot {
        self.build_snapshot()
    }

    pub fn apply(&mut self, action: ShellAction) -> Result<(), ShellError> {
        let mut next = self.clone();
        next.last_error = None;
        // Effects belong to this commit only; do not replay prior drained ones.
        next.pending_effects.clear();
        let result = next.apply_inner(action);
        match result {
            Ok(()) => {
                *self = next;
                Ok(())
            }
            Err(error) => {
                self.last_error = Some(error);
                Err(error)
            }
        }
    }

    fn apply_inner(&mut self, action: ShellAction) -> Result<(), ShellError> {
        match action {
            ShellAction::ActivateWorkspace { .. }
            | ShellAction::SelectWindow { .. }
            | ShellAction::SelectTab { .. }
            | ShellAction::CycleWindow { .. }
            | ShellAction::CycleTab { .. }
            | ShellAction::CreateWindow { .. }
            | ShellAction::CreateTab { .. }
            | ShellAction::MoveTabBefore { .. }
            | ShellAction::MoveTabToWindow { .. }
            | ShellAction::MoveTabToNewWindow { .. } => self.dispatch_w2a(action),
            ShellAction::CloseTab { id } => self.close_tab(id),
            ShellAction::SplitFocused { axis } => {
                let focused = self.focused_pane_id()?;
                self.split_pane(focused, axis).map(|_| ())
            }
            ShellAction::SplitPane { id, axis } => self.split_pane(id, axis).map(|_| ()),
            ShellAction::ClosePane { id } => self.close_pane(id),
            ShellAction::FocusPane { id } => self.focus_pane(id),
            ShellAction::BindExecution { pane, execution } => self.bind_execution(pane, execution),
            ShellAction::RecordUnpresented { .. }
            | ShellAction::ForgetUnpresented { .. }
            | ShellAction::AdoptExecution { .. }
            | ShellAction::TerminateExecution { .. } => self.dispatch_unpresented(action),
        }
    }

    fn close_tab(&mut self, id: TabId) -> Result<(), ShellError> {
        let workspace = self.workspace_mut(self.active_workspace)?;
        workspace.close_tab(id)?;
        self.bump_containment_generation();
        Ok(())
    }

    fn split_pane(&mut self, pane_id: PaneId, axis: SplitAxis) -> Result<PaneId, ShellError> {
        if !self.allows_pane_splitting {
            return Err(ShellError::PaneSplitUnavailable);
        }
        let workspace = self.workspace_mut(self.active_workspace)?;
        let tab = workspace.active_tab_mut()?;
        if !tab.panes.contains_key(&pane_id) {
            return Err(ShellError::UnknownPane);
        }
        let pane_count = tab.panes.len();
        let pane = Pane {
            id: PaneId::new(),
            title: format!("Pane {}", pane_count + 1),
            execution: None,
            allows_implicit_execution_bootstrap: false,
        };
        let id = pane.id;
        tab.panes.insert(id, pane);
        tab.root = tab.root.replacing(
            pane_id,
            PaneTree::Split {
                axis,
                first: Box::new(PaneTree::Leaf(pane_id)),
                second: Box::new(PaneTree::Leaf(id)),
            },
        );
        tab.focused = id;
        self.bump_containment_generation();
        Ok(id)
    }

    fn close_pane(&mut self, pane_id: PaneId) -> Result<(), ShellError> {
        let workspace = self.workspace_mut(self.active_workspace)?;
        let tab = workspace.active_tab_mut()?;
        if !tab.allows_pane_close() {
            return Err(ShellError::CannotCloseLastPane);
        }
        let Some(pane) = tab.panes.get(&pane_id) else {
            return Err(ShellError::UnknownPane);
        };
        // Closing would orphan the bound execution's authority; what happens
        // to that execution is the unaccepted provisioning/disposition
        // contract (#994), so fail closed instead of inventing it here.
        if pane.execution.is_some() {
            return Err(ShellError::CannotCloseBoundPane);
        }
        let Some(root) = tab.root.removing(pane_id) else {
            return Err(ShellError::CannotCloseLastPane);
        };
        tab.root = root;
        tab.panes.remove(&pane_id);
        if tab.focused == pane_id || !tab.panes.contains_key(&tab.focused) {
            tab.focused = tab
                .root
                .first_pane()
                .expect("remaining Pane tree must contain a Pane");
        }
        self.bump_containment_generation();
        Ok(())
    }

    fn focus_pane(&mut self, id: PaneId) -> Result<(), ShellError> {
        let workspace = self.workspace_mut(self.active_workspace)?;
        let tab = workspace.active_tab_mut()?;
        if !tab.panes.contains_key(&id) {
            return Err(ShellError::UnknownPane);
        }
        tab.focused = id;
        Ok(())
    }

    fn bind_execution(
        &mut self,
        pane_id: PaneId,
        execution: ExecutionId,
    ) -> Result<(), ShellError> {
        if self.execution_is_bound(execution) {
            return Err(ShellError::ExecutionAlreadyBound);
        }
        let pane = self.pane_mut(pane_id)?;
        if pane.execution.is_some() {
            return Err(ShellError::ExecutionAlreadyBound);
        }
        pane.execution = Some(execution);
        self.unpresented.remove(&execution);
        Ok(())
    }

    pub(super) fn execution_is_bound(&self, execution: ExecutionId) -> bool {
        self.workspaces.iter().any(|workspace| {
            workspace.tabs().any(|tab| {
                tab.panes
                    .values()
                    .any(|pane| pane.execution == Some(execution))
            })
        })
    }

    fn focused_pane_id(&self) -> Result<PaneId, ShellError> {
        Ok(self.focused_pane()?.id)
    }

    fn focused_pane(&self) -> Result<&Pane, ShellError> {
        let workspace = self.workspace(self.active_workspace)?;
        let tab_id = workspace.active_tab_id()?;
        let tab = workspace.tab(tab_id).ok_or(ShellError::UnknownTab)?;
        tab.panes.get(&tab.focused).ok_or(ShellError::UnknownPane)
    }

    fn pane(&self, id: PaneId) -> Result<&Pane, ShellError> {
        for workspace in &self.workspaces {
            for tab in workspace.tabs() {
                if let Some(pane) = tab.panes.get(&id) {
                    return Ok(pane);
                }
            }
        }
        Err(ShellError::UnknownPane)
    }

    fn pane_mut(&mut self, id: PaneId) -> Result<&mut Pane, ShellError> {
        for workspace in &mut self.workspaces {
            for window in &mut workspace.windows {
                for tab in &mut window.tabs {
                    if let Some(pane) = tab.panes.get_mut(&id) {
                        return Ok(pane);
                    }
                }
            }
        }
        Err(ShellError::UnknownPane)
    }

    fn workspace(&self, id: WorkspaceId) -> Result<&Workspace, ShellError> {
        self.workspaces
            .iter()
            .find(|workspace| workspace.id == id)
            .ok_or(ShellError::UnknownWorkspace)
    }

    fn workspace_mut(&mut self, id: WorkspaceId) -> Result<&mut Workspace, ShellError> {
        self.workspaces
            .iter_mut()
            .find(|workspace| workspace.id == id)
            .ok_or(ShellError::UnknownWorkspace)
    }

    #[cfg(test)]
    pub(super) fn every_window_has_a_tab(&self) -> bool {
        self.workspaces.iter().all(|workspace| {
            workspace
                .windows
                .iter()
                .all(|window| !window.tabs.is_empty())
        })
    }
}
