//! ADR-018 §2.1 multi-window product snapshot projection (W3).

use seyal_core::{ExecutionId, PaneId, TabId, WindowId, WorkspaceId};

use super::tree::{LayoutDescription, PaneTree};
use super::workspace::{Pane, Tab, Window, Workspace};
use super::{
    PresentationTier, ShellError, ShellSnapshot, ShellState, TabSnapshot, WorkspaceSnapshot,
};

/// One Window in the ordered §2.1 snapshot list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowSnapshot {
    pub id: WindowId,
    pub workspace: WorkspaceId,
    pub title: String,
    pub attention: bool,
    pub tabs: Vec<WindowTabSnapshot>,
    pub active_tab: TabId,
}

/// Per-Window Tab with its PaneTree, focus, and leaf bindings/tiers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowTabSnapshot {
    pub id: TabId,
    pub title: String,
    pub attention: bool,
    pub focused_pane: PaneId,
    pub panes: Vec<PaneLeafSnapshot>,
    pub tree: PaneTree,
    pub layout: LayoutDescription,
}

/// One Pane leaf with presentation tier and execution binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneLeafSnapshot {
    pub id: PaneId,
    pub title: String,
    pub execution: Option<ExecutionId>,
    pub allows_implicit_bootstrap: bool,
    pub presentation_tier: PresentationTier,
}

impl ShellState {
    pub(super) fn build_snapshot(&self) -> ShellSnapshot {
        let product_window = self.product_active_window();
        let workspace = self
            .workspace(self.active_workspace)
            .expect("active Workspace must exist");
        let window = workspace
            .active_window()
            .expect("product-active Window must exist for snapshot");
        let tab = workspace
            .tab(window.active_tab)
            .expect("active Tab must exist");
        ShellSnapshot {
            workspaces: self
                .workspaces
                .iter()
                .map(|item| WorkspaceSnapshot {
                    id: item.id,
                    name: item.name.clone(),
                    detail: item.detail.clone(),
                    attention: item.attention,
                    tab_count: item.tab_count(),
                })
                .collect(),
            windows: self.ordered_window_snapshots(product_window),
            active_workspace: self.active_workspace,
            last_active_workspace: self.last_active_workspace,
            active_window: window.id,
            containment_generation: self.containment_generation,
            tabs: workspace
                .tabs()
                .map(|item| TabSnapshot {
                    id: item.id,
                    title: item.title.clone(),
                    attention: item.attention,
                    pane_count: item.panes.len(),
                })
                .collect(),
            active_tab: window.active_tab,
            focused_pane: tab.focused,
            panes: tab
                .root
                .pane_ids()
                .into_iter()
                .filter_map(|id| {
                    tab.panes.get(&id).map(|pane| super::PaneSnapshot {
                        id: pane.id,
                        title: pane.title.clone(),
                        execution: pane.execution,
                        allows_implicit_bootstrap: pane.allows_implicit_execution_bootstrap,
                    })
                })
                .collect(),
            tree: tab.root.clone(),
            layout: tab.root.layout_description(),
            last_error: self.last_error,
            allows_tab_creation: self.allows_tab_creation,
            allows_window_creation: self.allows_window_creation,
            allows_pane_splitting: self.allows_pane_splitting,
            allows_tab_close: workspace.allows_tab_close(),
            allows_pane_close: tab.allows_focused_pane_close(),
        }
    }

    pub(super) fn product_active_window(&self) -> WindowId {
        self.workspace(self.active_workspace)
            .ok()
            .and_then(|workspace| workspace.active_window)
            .expect("product-active Window must exist")
    }

    fn ordered_window_snapshots(&self, product_window: WindowId) -> Vec<WindowSnapshot> {
        let mut windows = Vec::new();
        for workspace in &self.workspaces {
            for window in &workspace.windows {
                windows.push(window_snapshot(workspace, window, product_window));
            }
        }
        windows
    }
}

fn window_snapshot(
    workspace: &Workspace,
    window: &Window,
    product_window: WindowId,
) -> WindowSnapshot {
    let active_tab = window
        .tabs
        .iter()
        .find(|tab| tab.id == window.active_tab)
        .expect("active Tab must belong to its Window");
    let attention = window.tabs.iter().any(|tab| tab.attention);
    WindowSnapshot {
        id: window.id,
        workspace: workspace.id,
        title: active_tab.title.clone(),
        attention,
        tabs: window
            .tabs
            .iter()
            .map(|tab| tab_snapshot(tab, window, product_window))
            .collect(),
        active_tab: window.active_tab,
    }
}

fn tab_snapshot(tab: &Tab, window: &Window, product_window: WindowId) -> WindowTabSnapshot {
    WindowTabSnapshot {
        id: tab.id,
        title: tab.title.clone(),
        attention: tab.attention,
        focused_pane: tab.focused,
        panes: tab
            .root
            .pane_ids()
            .into_iter()
            .filter_map(|id| {
                tab.panes
                    .get(&id)
                    .map(|pane| leaf_snapshot(pane, tab, window, product_window))
            })
            .collect(),
        tree: tab.root.clone(),
        layout: tab.root.layout_description(),
    }
}

fn leaf_snapshot(
    pane: &Pane,
    tab: &Tab,
    window: &Window,
    product_window: WindowId,
) -> PaneLeafSnapshot {
    PaneLeafSnapshot {
        id: pane.id,
        title: pane.title.clone(),
        execution: pane.execution,
        allows_implicit_bootstrap: pane.allows_implicit_execution_bootstrap,
        presentation_tier: tier_for_leaf(pane.id, tab, window, product_window),
    }
}

/// Product-derivable ADR-018 §5 tier (occlusion stays host input until W4a/W5).
fn tier_for_leaf(
    pane: PaneId,
    tab: &Tab,
    window: &Window,
    product_window: WindowId,
) -> PresentationTier {
    if tab.id != window.active_tab {
        return PresentationTier::Hidden;
    }
    if window.id == product_window && pane == tab.focused {
        PresentationTier::Focused
    } else {
        PresentationTier::Visible
    }
}

impl ShellError {
    /// Bounded non-secret FFI / host error number for shell `last_error`.
    pub fn error_number(self) -> u32 {
        match self {
            Self::UnknownWorkspace => 1,
            Self::UnknownTab => 2,
            Self::UnknownPane => 3,
            Self::TabCreationUnavailable => 4,
            Self::PaneSplitUnavailable => 5,
            Self::CannotCloseLastTab => 6,
            Self::CannotCloseLastPane => 7,
            Self::CannotCloseBoundPane => 8,
            Self::NoSplitDivider => 17,
            Self::ExecutionAlreadyBound => 9,
            Self::EmptyShell => 10,
            Self::EmptyWindow => 11,
            Self::EmptyTab => 12,
            Self::UnknownWindow => 13,
            Self::StaleContainment => 14,
            Self::MoveWouldNotChangeContainment => 15,
            Self::CrossWorkspaceMove => 16,
            Self::WindowCreationUnavailable => 19,
        }
    }
}
