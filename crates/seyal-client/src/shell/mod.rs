//! Portable Workspace / Tab / Pane composition for headed hosts.
//!
//! This module is product UI authority for navigation, split trees, focus, and
//! pane→execution binding metadata. It is not a Workspace database, PTY owner,
//! VT/grid, Runtime registry, or renderer. Hosts dispatch [`ShellAction`] values
//! and render [`ShellSnapshot`]. Do not call this from the PTY→VT→damage path.

mod actions;
mod close;
mod effects;
mod focus_history;
mod inventory;
mod snapshot;
mod tree;
mod unpresented;
mod workspace;

#[cfg(test)]
mod close_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod unpresented_tests;
#[cfg(test)]
mod w2a_tests;
#[cfg(test)]
mod w5_tests;
#[cfg(test)]
mod w7_matrix_tests;
#[cfg(test)]
mod window_admission_tests;

use std::collections::BTreeMap;
use std::fmt;

use seyal_core::{ExecutionId, PaneId, TabId, WindowId, WorkspaceId};

use crate::pane_layout::SplitRatio;

pub use effects::ShellNativeEffect;
pub use inventory::{
    NavigationInventory, PaneNavItem, SessionNavItem, TabNavItem, WorkspaceNavItem,
};
pub use snapshot::{PaneLeafSnapshot, WindowSnapshot, WindowTabSnapshot};
pub use tree::{LayoutDescription, PaneTree, SplitAxis};
pub use unpresented::unpresented_palette_label;
pub use workspace::{ShellPaneSeed, ShellTabSeed, ShellWindowSeed, ShellWorkspaceSeed};

use focus_history::FocusHistory;
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
    /// Retained for ABI/FFI stability; bound Pane close is now detach-only
    /// disposition (ADR-017 §6.1) and no longer produced by [`ShellState`].
    CannotCloseBoundPane,
    NoSplitDivider,
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
    /// Adopt/terminate of an execution that is not live-unpresented here.
    ExecutionNotUnpresented,
    /// Extra Window create rejected when headed admission is off.
    WindowCreationUnavailable,
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
            Self::CannotCloseBoundPane => "A Pane bound to an execution cannot be closed.",
            Self::NoSplitDivider => "This Pane does not lead a split divider.",
            Self::ExecutionAlreadyBound => "This Pane is already bound to an execution.",
            Self::EmptyShell => "Shell requires at least one Workspace.",
            Self::EmptyWindow => "A Window requires at least one Tab.",
            Self::EmptyTab => "A Tab requires at least one Pane.",
            Self::UnknownWindow => "Unknown Window.",
            Self::StaleContainment => "This structural action is stale.",
            Self::MoveWouldNotChangeContainment => {
                "Moving this Tab would not change Window containment."
            }
            Self::CrossWorkspaceMove => "A Tab cannot move across Workspaces.",
            Self::CrossWorkspaceAdopt => "An execution cannot be adopted across Workspaces.",
            Self::ExecutionNotUnpresented => {
                "This execution is not a live-unpresented execution in this Workspace."
            }
            Self::WindowCreationUnavailable => {
                "Creating another Window is unavailable until window close exists."
            }
        }
    }
}

impl fmt::Display for ShellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

/// Portable focus triple used by atomic navigation commit checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FocusCheckpoint {
    pub active_workspace: WorkspaceId,
    pub active_tab: TabId,
    pub focused_pane: PaneId,
}

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
    SelectWindow {
        id: WindowId,
    },
    SelectTab {
        id: TabId,
    },
    CreateTab {
        window: WindowId,
        containment_generation: u64,
    },
    CycleWindow {
        direction: CycleDirection,
    },
    CycleTab {
        direction: CycleDirection,
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
    CloseWindow {
        id: WindowId,
        containment_generation: u64,
    },
    CloseTab {
        id: TabId,
        containment_generation: u64,
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
        containment_generation: u64,
    },
    FocusPane {
        id: PaneId,
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
    /// Rebind a live-unpresented `ExecutionId` into a Pane leaf (same id; fresh
    /// `AttachmentId` comes from Runtime attach).
    AdoptExecution {
        pane: PaneId,
        execution: ExecutionId,
    },
    /// Explicit disposition. Queues [`ShellNativeEffect::TerminateExecution`];
    /// never implied by tab/window destruction (W2b/W4b).
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
    /// `None` in the zero-Window state (ADR-018 §3.3a).
    pub active_window: Option<WindowId>,
    /// ADR-018 §6 fence. Bumps only on Window/Tab/PaneTree membership change.
    pub containment_generation: u64,
    /// Active-Window Tab projection (compatible with pre-W3 hosts).
    /// Empty when there is no product-active Window.
    pub tabs: Vec<TabSnapshot>,
    /// Placeholder identity when [`Self::active_window`] is `None`.
    pub active_tab: TabId,
    /// Placeholder identity when [`Self::active_window`] is `None`.
    pub focused_pane: PaneId,
    pub panes: Vec<PaneSnapshot>,
    pub tree: PaneTree,
    pub layout: LayoutDescription,
    pub last_error: Option<ShellError>,
    /// Whether `CreateTab`/`SplitFocused` would currently be accepted.
    /// Hosts use this to omit the control rather than show one that always
    /// fails closed (mirrors the palette's own omission of "New Tab").
    pub allows_tab_creation: bool,
    pub allows_window_creation: bool,
    pub allows_pane_splitting: bool,
    /// Whether `CloseTab` of the active Tab / `ClosePane` of the focused Pane
    /// would currently be accepted. Hierarchical close always admits the active
    /// Tab / focused Pane while a Window exists (last Tab → Window, last Pane →
    /// Tab). Hosts read these instead of re-deriving the rule from counts.
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
    containment_generation: u64,
    focus_history: FocusHistory,
    allows_pane_splitting: bool,
    allows_tab_creation: bool,
    allows_window_creation: bool,
    last_error: Option<ShellError>,
    next_tab_ordinal: u32,
    /// Execution released by the most recent successful `ClosePane`, if any.
    /// Portable provisioning records it as unreferenced (ADR-017 §6.1).
    last_released_execution: Option<(PaneId, ExecutionId)>,
    /// Pane ids removed by the most recent successful `CloseTab` (detach-only).
    last_removed_tab_panes: Vec<PaneId>,
    /// ADR-018 §2.4 effects from the last successful commit (drained by the host path).
    pending_effects: Vec<ShellNativeEffect>,
    /// Live executions with no Pane binding in this headed session (ADR-018 §3.3).
    unpresented: BTreeMap<ExecutionId, WorkspaceId>,
}

impl ShellState {
    /// Production composition: one Runtime default workspace, one Tab, one
    /// Pane. Tab creation is enabled on the live create→attach→bind path
    /// (C2b / #1175); pane splitting stays fail-closed until C3.
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
        let mut shell = Self {
            active_workspace: workspace.id,
            last_active_workspace: workspace.id,
            workspaces: vec![workspace],
            containment_generation: 0,
            focus_history: FocusHistory::default(),
            allows_pane_splitting: false,
            allows_tab_creation: true,
            allows_window_creation: true,
            last_error: None,
            next_tab_ordinal: 2,
            last_released_execution: None,
            last_removed_tab_panes: Vec::new(),
            pending_effects: Vec::new(),
            unpresented: BTreeMap::new(),
        };
        shell.seed_focus_from_product();
        shell
    }

    /// Test/host fixture with explicit policy flags. Requires a non-empty set.
    pub fn from_workspaces(
        workspaces: Vec<ShellWorkspaceSeed>,
        active_workspace: WorkspaceId,
        allows_pane_splitting: bool,
        allows_tab_creation: bool,
        allows_window_creation: bool,
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
        let mut shell = Self {
            workspaces,
            active_workspace,
            last_active_workspace: active_workspace,
            containment_generation: 0,
            focus_history: FocusHistory::default(),
            allows_pane_splitting,
            allows_tab_creation,
            allows_window_creation,
            last_error: None,
            next_tab_ordinal: 2,
            last_released_execution: None,
            last_removed_tab_panes: Vec::new(),
            pending_effects: Vec::new(),
            unpresented: BTreeMap::new(),
        };
        shell.seed_focus_from_product();
        Ok(shell)
    }

    pub fn last_error(&self) -> Option<ShellError> {
        self.last_error
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

    pub fn containment_generation(&self) -> u64 {
        self.containment_generation
    }

    /// Clone without `last_error` for atomic rejection equality checks.
    pub fn containment_fingerprint(&self) -> ShellState {
        let mut clone = self.clone();
        clone.last_error = None;
        clone
    }

    /// Drain §2.4 effects produced by the last successful commit(s).
    pub fn take_effects(&mut self) -> Vec<ShellNativeEffect> {
        std::mem::take(&mut self.pending_effects)
    }

    pub(super) fn push_effect(&mut self, effect: ShellNativeEffect) {
        self.pending_effects.push(effect);
    }

    pub fn last_active_workspace(&self) -> WorkspaceId {
        self.last_active_workspace
    }

    /// Product-active Window of the product-active Workspace.
    pub fn product_window_id(&self) -> Result<WindowId, ShellError> {
        self.workspace(self.active_workspace)?
            .active_window()
            .map(|window| window.id)
            .ok_or(ShellError::UnknownWindow)
    }

    /// Host-originated New Tab: target the product Window with the live fence.
    pub(crate) fn apply_product_create_tab(&mut self) -> Result<(), ShellError> {
        let window = self.product_window_id()?;
        let containment_generation = self.containment_generation;
        self.apply(ShellAction::CreateTab {
            window,
            containment_generation,
        })
    }

    /// Host-originated workspace activation with the live fence.
    pub(crate) fn apply_activate_workspace(
        &mut self,
        workspace: WorkspaceId,
    ) -> Result<(), ShellError> {
        let containment_generation = self.containment_generation;
        self.apply(ShellAction::ActivateWorkspace {
            workspace,
            containment_generation,
        })
    }

    /// Test-only: write a Pane binding without the one-to-one check so the
    /// navigation resolver can still prove `AmbiguousTarget` fail-closed.
    #[cfg(test)]
    pub(crate) fn overwrite_pane_execution_for_test(
        &mut self,
        pane: PaneId,
        execution: ExecutionId,
    ) -> Result<(), ShellError> {
        self.pane_mut(pane)?.execution = Some(execution);
        Ok(())
    }

    pub fn allows_pane_splitting(&self) -> bool {
        self.allows_pane_splitting
    }

    pub fn allows_tab_creation(&self) -> bool {
        self.allows_tab_creation
    }

    pub fn allows_window_creation(&self) -> bool {
        self.allows_window_creation
    }

    /// Test harness: flip CreateWindow admission without rebuilding the shell.
    #[cfg(test)]
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn set_allows_window_creation_for_test(&mut self, allowed: bool) {
        self.allows_window_creation = allowed;
    }

    /// Test-only policy flip. Called from macOS C2 harness suites via
    /// `ApplicationRoot::enable_tab_creation_for_test`.
    #[cfg(test)]
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn set_allows_tab_creation_for_test(&mut self, allowed: bool) {
        self.allows_tab_creation = allowed;
    }

    pub fn focused_pane_allows_implicit_bootstrap(&self) -> bool {
        self.focused_pane()
            .is_ok_and(|pane| pane.allows_implicit_execution_bootstrap)
    }

    pub fn pane_execution(&self, pane: PaneId) -> Result<Option<ExecutionId>, ShellError> {
        Ok(self.pane(pane)?.execution)
    }

    /// Take the execution released by the last successful `ClosePane`, if any.
    pub fn take_released_execution(&mut self) -> Option<(PaneId, ExecutionId)> {
        self.last_released_execution.take()
    }

    /// Take Pane ids removed by the last successful `CloseTab` (detach-only).
    pub fn take_removed_tab_panes(&mut self) -> Vec<PaneId> {
        std::mem::take(&mut self.last_removed_tab_panes)
    }

    /// Clear a Pane→execution binding without removing the Pane (explicit
    /// terminate). Presentation close uses [`ShellAction::ClosePane`] /
    /// [`ShellAction::CloseTab`] instead.
    pub fn release_execution(&mut self, pane: PaneId) -> Result<Option<ExecutionId>, ShellError> {
        let pane = self.pane_mut(pane)?;
        Ok(pane.execution.take())
    }

    /// Whether `id` is a current Workspace in this shell.
    pub fn contains_workspace(&self, id: WorkspaceId) -> bool {
        self.workspaces.iter().any(|workspace| workspace.id == id)
    }

    /// Owning Workspace of a current Tab, if any.
    pub fn workspace_of_tab(&self, id: TabId) -> Option<WorkspaceId> {
        self.workspaces
            .iter()
            .find_map(|workspace| workspace.tab(id).map(|_| workspace.id))
    }

    /// Owning Workspace and Tab of a current Pane, if any.
    pub fn location_of_pane(&self, id: PaneId) -> Option<(WorkspaceId, TabId)> {
        for workspace in &self.workspaces {
            for tab in workspace.tabs() {
                if tab.panes.contains_key(&id) {
                    return Some((workspace.id, tab.id));
                }
            }
        }
        None
    }

    /// Whether `pane` is currently a leaf of `tab` inside `workspace`.
    pub fn tab_contains_leaf(&self, workspace: WorkspaceId, tab: TabId, pane: PaneId) -> bool {
        self.workspaces
            .iter()
            .find(|item| item.id == workspace)
            .and_then(|item| item.tab(tab))
            .is_some_and(|item| item.root.contains_leaf(pane))
    }

    /// Every current Pane bound to `execution`, across all Workspaces and Tabs.
    pub fn panes_bound_to(&self, execution: ExecutionId) -> Vec<(WorkspaceId, TabId, PaneId)> {
        let mut bound = Vec::new();
        for workspace in &self.workspaces {
            for tab in workspace.tabs() {
                for pane in tab.panes.values() {
                    if pane.execution == Some(execution) {
                        bound.push((workspace.id, tab.id, pane.id));
                    }
                }
            }
        }
        bound
    }

    /// Stable-order inventory for goto enumeration (SPEC-022 §7.6 / N4).
    pub fn navigation_inventory(&self) -> NavigationInventory {
        inventory::build(&self.workspaces, self.active_workspace)
    }

    /// Test helper: rename a Workspace's display name without changing identity.
    #[cfg(test)]
    pub fn rename_workspace_for_test(
        &mut self,
        id: WorkspaceId,
        name: impl Into<String>,
    ) -> Result<(), ShellError> {
        let workspace = self.workspace_mut(id)?;
        workspace.name = name.into();
        Ok(())
    }

    /// Test helper: rename a Tab's display title without changing identity.
    #[cfg(test)]
    pub fn rename_tab_for_test(
        &mut self,
        id: TabId,
        title: impl Into<String>,
    ) -> Result<(), ShellError> {
        let workspace_id = self.workspace_of_tab(id).ok_or(ShellError::UnknownTab)?;
        let workspace = self.workspace_mut(workspace_id)?;
        let tab = workspace.tab_mut(id).ok_or(ShellError::UnknownTab)?;
        tab.title = title.into();
        Ok(())
    }

    /// Test helper: rename a Pane's display title without changing identity.
    #[cfg(test)]
    pub fn rename_pane_for_test(
        &mut self,
        id: PaneId,
        title: impl Into<String>,
    ) -> Result<(), ShellError> {
        let (workspace_id, tab_id) = self.location_of_pane(id).ok_or(ShellError::UnknownPane)?;
        let workspace = self.workspace_mut(workspace_id)?;
        let tab = workspace.tab_mut(tab_id).ok_or(ShellError::UnknownTab)?;
        let pane = tab.panes.get_mut(&id).ok_or(ShellError::UnknownPane)?;
        pane.title = title.into();
        Ok(())
    }

    /// Current focus triple for equality / no-op checks (SPEC-022 R4.4).
    pub fn focus_checkpoint(&self) -> FocusCheckpoint {
        let snap = self.snapshot();
        FocusCheckpoint {
            active_workspace: snap.active_workspace,
            active_tab: snap.active_tab,
            focused_pane: snap.focused_pane,
        }
    }

    /// Active Tab and focused Pane currently recorded for `workspace`.
    pub fn workspace_focus(&self, workspace: WorkspaceId) -> Option<FocusCheckpoint> {
        let workspace = self.workspace(workspace).ok()?;
        let tab = workspace.tab(workspace.active_tab_id().ok()?)?;
        Some(FocusCheckpoint {
            active_workspace: workspace.id,
            active_tab: tab.id,
            focused_pane: tab.focused,
        })
    }

    /// Focused Pane of `tab` inside `workspace`, if that composition exists.
    pub fn tab_focused_pane(&self, workspace: WorkspaceId, tab: TabId) -> Option<PaneId> {
        self.workspace(workspace)
            .ok()
            .and_then(|item| item.tab(tab))
            .map(|item| item.focused)
    }

    /// Atomically set Workspace + Tab + Pane focus. Validates composition
    /// before any write so a failure leaves prior focus unchanged.
    pub fn commit_focus(
        &mut self,
        workspace: WorkspaceId,
        tab: TabId,
        pane: PaneId,
    ) -> Result<(), ShellError> {
        if !self.contains_workspace(workspace) {
            return Err(ShellError::UnknownWorkspace);
        }
        let Some(owner) = self.workspace_of_tab(tab) else {
            return Err(ShellError::UnknownTab);
        };
        if owner != workspace {
            return Err(ShellError::UnknownTab);
        }
        let Some((pane_workspace, pane_tab)) = self.location_of_pane(pane) else {
            return Err(ShellError::UnknownPane);
        };
        if pane_workspace != workspace
            || pane_tab != tab
            || !self.tab_contains_leaf(workspace, tab, pane)
        {
            return Err(ShellError::UnknownPane);
        }
        let previous_product = self
            .workspace(self.active_workspace)
            .ok()
            .and_then(|item| item.active_window);
        self.active_workspace = workspace;
        self.last_active_workspace = workspace;
        let workspace_mut = self.workspace_mut(workspace)?;
        workspace_mut.select_tab(tab)?;
        workspace_mut.active_tab_mut()?.focused = pane;
        self.focus_history.record(pane);
        self.last_error = None;
        if let Some((_, window, _)) = self.find_tab_location(tab)
            && previous_product != Some(window)
        {
            self.push_effect(ShellNativeEffect::OrderFrontMakeKey { window });
        }
        Ok(())
    }

    pub fn snapshot(&self) -> ShellSnapshot {
        self.build_snapshot()
    }

    pub fn apply(&mut self, action: ShellAction) -> Result<(), ShellError> {
        let mut next = self.clone();
        // Effects belong to this commit only; do not replay prior drained ones.
        next.pending_effects.clear();
        let result = next.dispatch(action);
        match result {
            Ok(()) => {
                next.last_error = None;
                *self = next;
                Ok(())
            }
            Err(error) => {
                self.last_error = Some(error);
                Err(error)
            }
        }
    }

    fn dispatch(&mut self, action: ShellAction) -> Result<(), ShellError> {
        match action {
            ShellAction::CreateWindow { .. }
            | ShellAction::ActivateWorkspace { .. }
            | ShellAction::SelectWindow { .. }
            | ShellAction::SelectTab { .. }
            | ShellAction::CreateTab { .. }
            | ShellAction::CycleWindow { .. }
            | ShellAction::CycleTab { .. }
            | ShellAction::MoveTabBefore { .. }
            | ShellAction::MoveTabToWindow { .. }
            | ShellAction::MoveTabToNewWindow { .. } => self.dispatch_w2a(action),
            ShellAction::CloseWindow { .. }
            | ShellAction::CloseTab { .. }
            | ShellAction::ClosePane { .. } => self.dispatch_close(action),
            ShellAction::SplitFocused { axis } => {
                let focused = self.focused_pane_id()?;
                self.split_pane(focused, axis).map(|_| ())
            }
            ShellAction::SplitPane { id, axis } => self.split_pane(id, axis).map(|_| ()),
            ShellAction::FocusPane { id } => self.focus_pane(id),
            ShellAction::SetSplitRatio { pane, ratio } => self.set_split_ratio(pane, ratio),
            ShellAction::BindExecution { pane, execution } => self.bind_execution(pane, execution),
            ShellAction::RecordUnpresented { .. }
            | ShellAction::ForgetUnpresented { .. }
            | ShellAction::AdoptExecution { .. }
            | ShellAction::TerminateExecution { .. } => self.dispatch_unpresented(action),
        }
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
                ratio: SplitRatio::HALF,
            },
        );
        tab.focused = id;
        self.bump_containment_generation();
        Ok(id)
    }

    fn focus_pane(&mut self, id: PaneId) -> Result<(), ShellError> {
        let workspace = self.workspace_mut(self.active_workspace)?;
        let tab = workspace.active_tab_mut()?;
        if !tab.panes.contains_key(&id) {
            return Err(ShellError::UnknownPane);
        }
        tab.focused = id;
        self.focus_history.record(id);
        Ok(())
    }

    fn set_split_ratio(&mut self, pane: PaneId, ratio: SplitRatio) -> Result<(), ShellError> {
        let workspace = self.workspace_mut(self.active_workspace)?;
        let tab = workspace.active_tab_mut()?;
        if !tab.panes.contains_key(&pane) {
            return Err(ShellError::UnknownPane);
        }
        if tab.root.set_ratio(pane, ratio) {
            Ok(())
        } else {
            Err(ShellError::NoSplitDivider)
        }
    }

    fn bind_execution(
        &mut self,
        pane_id: PaneId,
        execution: ExecutionId,
    ) -> Result<(), ShellError> {
        if !self.panes_bound_to(execution).is_empty() {
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

    fn focused_pane_id(&self) -> Result<PaneId, ShellError> {
        Ok(self.focused_pane()?.id)
    }

    fn focused_pane(&self) -> Result<&Pane, ShellError> {
        let workspace = self.workspace(self.active_workspace)?;
        let tab = workspace
            .tab(workspace.active_tab_id()?)
            .ok_or(ShellError::UnknownTab)?;
        tab.panes.get(&tab.focused).ok_or(ShellError::UnknownPane)
    }

    /// Pane lookup is by id across every Tab: a create result may complete for a
    /// background Tab's leaf after focus moved on (ADR-017 request-id correlation).
    fn pane(&self, id: PaneId) -> Result<&Pane, ShellError> {
        self.workspaces
            .iter()
            .flat_map(|workspace| workspace.tabs())
            .find_map(|tab| tab.panes.get(&id))
            .ok_or(ShellError::UnknownPane)
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

    fn seed_focus_from_product(&mut self) {
        if let Ok(pane) = self.focused_pane_id() {
            self.focus_history.record(pane);
        }
    }

    pub(super) fn purge_missing_panes(&mut self) {
        let live: Vec<_> = self
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.tabs())
            .flat_map(|tab| tab.panes.keys().copied())
            .collect();
        self.focus_history.purge_if(|pane| !live.contains(&pane));
    }
}
