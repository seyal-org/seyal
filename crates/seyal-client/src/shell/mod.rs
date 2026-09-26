//! Portable Workspace / Tab / Pane composition for headed hosts.
//!
//! This module is product UI authority for navigation, split trees, focus, and
//! pane→execution binding metadata. It is not a Workspace database, PTY owner,
//! VT/grid, Runtime registry, or renderer. Hosts dispatch [`ShellAction`] values
//! and render [`ShellSnapshot`]. Do not call this from the PTY→VT→damage path.

mod tree;
mod workspace;

#[cfg(test)]
mod tests;

use std::fmt;

use seyal_core::{ExecutionId, PaneId, TabId, WorkspaceId};

pub use tree::{LayoutDescription, PaneTree, SplitAxis};
pub use workspace::{ShellPaneSeed, ShellTabSeed, ShellWorkspaceSeed};

use workspace::{Pane, Tab, Workspace};

/// Why a [`ShellAction`] was rejected. The previous state is unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellError {
    UnknownWorkspace,
    UnknownTab,
    UnknownPane,
    TabCreationUnavailable,
    PaneSplitUnavailable,
    CannotCloseLastTab,
    CannotCloseLastPane,
    ExecutionAlreadyBound,
    EmptyShell,
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
            Self::ExecutionAlreadyBound => "This Pane is already bound to an execution.",
            Self::EmptyShell => "Shell requires at least one Workspace.",
        }
    }
}

impl fmt::Display for ShellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

/// Typed host → Rust command. One action is one coarse transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellAction {
    SelectWorkspace {
        id: WorkspaceId,
    },
    SelectTab {
        id: TabId,
    },
    CreateTab,
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
}

/// Read-only projection for native hosts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellSnapshot {
    pub workspaces: Vec<WorkspaceSnapshot>,
    pub active_workspace: WorkspaceId,
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
    /// Pane would currently be accepted (the last Tab/Pane cannot close).
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
#[derive(Clone, Debug)]
pub struct ShellState {
    workspaces: Vec<Workspace>,
    active_workspace: WorkspaceId,
    allows_pane_splitting: bool,
    allows_tab_creation: bool,
    last_error: Option<ShellError>,
    next_tab_ordinal: u32,
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
        let workspace = Workspace {
            id: WorkspaceId::m001_default(),
            name: "Local".to_owned(),
            detail: Some(detail.into()),
            attention: false,
            active_tab: tab.id,
            tabs: vec![tab],
        };
        Self {
            active_workspace: workspace.id,
            workspaces: vec![workspace],
            allows_pane_splitting: false,
            allows_tab_creation: false,
            last_error: None,
            next_tab_ordinal: 2,
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
        let workspaces = workspaces.into_iter().map(Workspace::from_seed).collect();
        Ok(Self {
            workspaces,
            active_workspace,
            allows_pane_splitting,
            allows_tab_creation,
            last_error: None,
            next_tab_ordinal: 2,
        })
    }

    pub fn last_error(&self) -> Option<ShellError> {
        self.last_error
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

    pub fn snapshot(&self) -> ShellSnapshot {
        let workspace = self
            .workspace(self.active_workspace)
            .expect("active Workspace must exist");
        let tab = workspace
            .tab(workspace.active_tab)
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
                    tab_count: item.tabs.len(),
                })
                .collect(),
            active_workspace: self.active_workspace,
            tabs: workspace
                .tabs
                .iter()
                .map(|item| TabSnapshot {
                    id: item.id,
                    title: item.title.clone(),
                    attention: item.attention,
                    pane_count: item.panes.len(),
                })
                .collect(),
            active_tab: workspace.active_tab,
            focused_pane: tab.focused,
            panes: tab
                .root
                .pane_ids()
                .into_iter()
                .filter_map(|id| {
                    tab.panes.get(&id).map(|pane| PaneSnapshot {
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
            allows_pane_splitting: self.allows_pane_splitting,
            allows_tab_close: workspace.allows_tab_close(),
            allows_pane_close: tab.allows_pane_close(),
        }
    }

    pub fn apply(&mut self, action: ShellAction) -> Result<(), ShellError> {
        let result = match action {
            ShellAction::SelectWorkspace { id } => self.select_workspace(id),
            ShellAction::SelectTab { id } => self.select_tab(id),
            ShellAction::CreateTab => self.create_tab().map(|_| ()),
            ShellAction::CloseTab { id } => self.close_tab(id),
            ShellAction::SplitFocused { axis } => {
                let focused = self.focused_pane_id()?;
                self.split_pane(focused, axis).map(|_| ())
            }
            ShellAction::SplitPane { id, axis } => self.split_pane(id, axis).map(|_| ()),
            ShellAction::ClosePane { id } => self.close_pane(id),
            ShellAction::FocusPane { id } => self.focus_pane(id),
            ShellAction::BindExecution { pane, execution } => self.bind_execution(pane, execution),
        };
        match result {
            Ok(()) => {
                self.last_error = None;
                Ok(())
            }
            Err(error) => {
                self.last_error = Some(error);
                Err(error)
            }
        }
    }

    fn select_workspace(&mut self, id: WorkspaceId) -> Result<(), ShellError> {
        if !self.workspaces.iter().any(|workspace| workspace.id == id) {
            return Err(ShellError::UnknownWorkspace);
        }
        self.active_workspace = id;
        Ok(())
    }

    fn select_tab(&mut self, id: TabId) -> Result<(), ShellError> {
        let workspace = self.workspace_mut(self.active_workspace)?;
        if workspace.tab(id).is_none() {
            return Err(ShellError::UnknownTab);
        }
        workspace.active_tab = id;
        Ok(())
    }

    fn create_tab(&mut self) -> Result<TabId, ShellError> {
        if !self.allows_tab_creation {
            return Err(ShellError::TabCreationUnavailable);
        }
        let ordinal = self.next_tab_ordinal;
        self.next_tab_ordinal = self.next_tab_ordinal.saturating_add(1);
        let pane = Pane {
            id: PaneId::new(),
            title: "Pane 1".to_owned(),
            execution: None,
            allows_implicit_execution_bootstrap: false,
        };
        let tab = Tab::with_pane(TabId::new(), format!("Terminal {ordinal}"), pane);
        let id = tab.id;
        let workspace = self.workspace_mut(self.active_workspace)?;
        workspace.active_tab = id;
        workspace.tabs.push(tab);
        Ok(id)
    }

    fn close_tab(&mut self, id: TabId) -> Result<(), ShellError> {
        let workspace = self.workspace_mut(self.active_workspace)?;
        if !workspace.allows_tab_close() {
            return Err(ShellError::CannotCloseLastTab);
        }
        let Some(index) = workspace.tabs.iter().position(|tab| tab.id == id) else {
            return Err(ShellError::UnknownTab);
        };
        workspace.tabs.remove(index);
        if workspace.active_tab == id {
            let replacement = index.min(workspace.tabs.len() - 1);
            workspace.active_tab = workspace.tabs[replacement].id;
        }
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
        Ok(id)
    }

    fn close_pane(&mut self, pane_id: PaneId) -> Result<(), ShellError> {
        let workspace = self.workspace_mut(self.active_workspace)?;
        let tab = workspace.active_tab_mut()?;
        if !tab.allows_pane_close() {
            return Err(ShellError::CannotCloseLastPane);
        }
        if !tab.panes.contains_key(&pane_id) {
            return Err(ShellError::UnknownPane);
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
        let pane = self.pane_mut(pane_id)?;
        if pane.execution.is_some() {
            return Err(ShellError::ExecutionAlreadyBound);
        }
        pane.execution = Some(execution);
        Ok(())
    }

    fn focused_pane_id(&self) -> Result<PaneId, ShellError> {
        Ok(self.focused_pane()?.id)
    }

    fn focused_pane(&self) -> Result<&Pane, ShellError> {
        let workspace = self.workspace(self.active_workspace)?;
        let tab = workspace
            .tab(workspace.active_tab)
            .ok_or(ShellError::UnknownTab)?;
        tab.panes.get(&tab.focused).ok_or(ShellError::UnknownPane)
    }

    fn pane(&self, id: PaneId) -> Result<&Pane, ShellError> {
        let workspace = self.workspace(self.active_workspace)?;
        let tab = workspace
            .tab(workspace.active_tab)
            .ok_or(ShellError::UnknownTab)?;
        tab.panes.get(&id).ok_or(ShellError::UnknownPane)
    }

    fn pane_mut(&mut self, id: PaneId) -> Result<&mut Pane, ShellError> {
        let workspace = self.workspace_mut(self.active_workspace)?;
        let tab = workspace.active_tab_mut()?;
        tab.panes.get_mut(&id).ok_or(ShellError::UnknownPane)
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
}
