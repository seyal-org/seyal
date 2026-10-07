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
        let product_window = self.product_active_window_id();
        let workspace = self
            .workspace(self.active_workspace)
            .expect("active Workspace must exist");
        let window_and_tab = product_window.and_then(|id| {
            let ws = self.find_window(id)?;
            let window = ws.window(id)?;
            let tab = ws.tab(window.active_tab)?;
            Some((window, tab))
        });

        let nil_tab = TabId::from_bytes([0; 16]);
        let nil_pane = PaneId::from_bytes([0; 16]);
        let (
            tabs,
            active_tab,
            focused_pane,
            panes,
            tree,
            layout,
            allows_tab_close,
            allows_pane_close,
        ) = if let Some((window, tab)) = window_and_tab {
            (
                self.tabs_for_workspace_projection(workspace),
                window.active_tab,
                tab.focused,
                tab.root
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
                tab.root.clone(),
                tab.root.layout_description(),
                true,
                true,
            )
        } else {
            (
                Vec::new(),
                nil_tab,
                nil_pane,
                Vec::new(),
                PaneTree::Leaf(nil_pane),
                LayoutDescription::Single,
                false,
                false,
            )
        };

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
            active_window: product_window,
            containment_generation: self.containment_generation,
            tabs,
            active_tab,
            focused_pane,
            panes,
            tree,
            layout,
            last_error: self.last_error,
            allows_tab_creation: self.allows_tab_creation && product_window.is_some(),
            allows_window_creation: self.allows_window_creation,
            allows_pane_splitting: self.allows_pane_splitting && product_window.is_some(),
            allows_tab_close,
            allows_pane_close,
        }
    }

    fn tabs_for_workspace_projection(&self, workspace: &Workspace) -> Vec<TabSnapshot> {
        workspace
            .tabs()
            .map(|item| TabSnapshot {
                id: item.id,
                title: item.title.clone(),
                attention: item.attention,
                pane_count: item.panes.len(),
            })
            .collect()
    }

    fn ordered_window_snapshots(&self, product_window: Option<WindowId>) -> Vec<WindowSnapshot> {
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
    product_window: Option<WindowId>,
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

fn tab_snapshot(tab: &Tab, window: &Window, product_window: Option<WindowId>) -> WindowTabSnapshot {
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
    product_window: Option<WindowId>,
) -> PaneLeafSnapshot {
    PaneLeafSnapshot {
        id: pane.id,
        title: pane.title.clone(),
        execution: pane.execution,
        allows_implicit_bootstrap: pane.allows_implicit_execution_bootstrap,
        presentation_tier: tier_for_leaf(pane.id, tab, window, product_window),
    }
}

/// ADR-018 §5 product-derivable tier. Occlusion/miniaturize are host *inputs*;
/// the host never invents the tier itself.
fn tier_for_leaf(
    pane: PaneId,
    tab: &Tab,
    window: &Window,
    product_window: Option<WindowId>,
) -> PresentationTier {
    if window.occluded || window.miniaturized || tab.id != window.active_tab {
        return PresentationTier::Hidden;
    }
    if product_window == Some(window.id) && pane == tab.focused {
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
            Self::CrossWorkspaceAdopt => 18,
            Self::ExecutionNotUnpresented => 19,
            Self::WindowCreationUnavailable => 20,
        }
    }
}

impl ShellState {
    /// Record host-forwarded occlusion. Does not bump containment generation.
    pub(crate) fn set_window_occluded(
        &mut self,
        id: WindowId,
        occluded: bool,
    ) -> Result<(), ShellError> {
        self.window_mut(id)?.occluded = occluded;
        Ok(())
    }

    /// Record host-forwarded miniaturize. Does not bump containment generation.
    pub(crate) fn set_window_miniaturized(
        &mut self,
        id: WindowId,
        miniaturized: bool,
    ) -> Result<(), ShellError> {
        self.window_mut(id)?.miniaturized = miniaturized;
        Ok(())
    }

    fn window_mut(&mut self, id: WindowId) -> Result<&mut Window, ShellError> {
        for workspace in &mut self.workspaces {
            if let Some(window) = workspace.window_mut(id) {
                return Ok(window);
            }
        }
        Err(ShellError::UnknownWindow)
    }
}
