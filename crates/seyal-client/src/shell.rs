//! Portable Workspace / Tab / Pane composition for headed hosts.
//!
//! This module is product UI authority for navigation, split trees, focus, and
//! pane→execution binding metadata. It is not a Workspace database, PTY owner,
//! VT/grid, Runtime registry, or renderer. Hosts dispatch [`ShellAction`] values
//! and render [`ShellSnapshot`]. Do not call this from the PTY→VT→damage path.

use std::collections::HashMap;
use std::fmt;

use seyal_core::{ExecutionId, PaneId, TabId, WorkspaceId};

/// Horizontal or vertical split of one Tab's pane tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitAxis {
    Right,
    Down,
}

/// Recursive pane layout for one Tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaneTree {
    Leaf(PaneId),
    Split {
        axis: SplitAxis,
        first: Box<PaneTree>,
        second: Box<PaneTree>,
    },
}

impl PaneTree {
    fn replacing(&self, target: PaneId, replacement: PaneTree) -> PaneTree {
        match self {
            Self::Leaf(id) if *id == target => replacement,
            Self::Leaf(_) => self.clone(),
            Self::Split {
                axis,
                first,
                second,
            } => Self::Split {
                axis: *axis,
                first: Box::new(first.replacing(target, replacement.clone())),
                second: Box::new(second.replacing(target, replacement)),
            },
        }
    }

    fn removing(&self, target: PaneId) -> Option<PaneTree> {
        match self {
            Self::Leaf(id) => {
                if *id == target {
                    None
                } else {
                    Some(self.clone())
                }
            }
            Self::Split {
                axis,
                first,
                second,
            } => match (first.removing(target), second.removing(target)) {
                (Some(left), Some(right)) => Some(Self::Split {
                    axis: *axis,
                    first: Box::new(left),
                    second: Box::new(right),
                }),
                (Some(left), None) => Some(left),
                (None, Some(right)) => Some(right),
                (None, None) => None,
            },
        }
    }

    fn first_pane(&self) -> Option<PaneId> {
        match self {
            Self::Leaf(id) => Some(*id),
            Self::Split { first, second, .. } => first.first_pane().or_else(|| second.first_pane()),
        }
    }

    fn pane_ids(&self) -> Vec<PaneId> {
        match self {
            Self::Leaf(id) => vec![*id],
            Self::Split { first, second, .. } => {
                let mut ids = first.pane_ids();
                ids.extend(second.pane_ids());
                ids
            }
        }
    }

    fn layout_description(&self) -> LayoutDescription {
        match self {
            Self::Leaf(_) => LayoutDescription::Single,
            Self::Split {
                axis: SplitAxis::Right,
                ..
            } => LayoutDescription::SplitRight,
            Self::Split {
                axis: SplitAxis::Down,
                ..
            } => LayoutDescription::SplitDown,
        }
    }
}

/// Host-visible summary of a Tab's pane tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutDescription {
    Single,
    SplitRight,
    SplitDown,
}

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

#[derive(Clone, Debug)]
struct Pane {
    id: PaneId,
    title: String,
    execution: Option<ExecutionId>,
    allows_implicit_execution_bootstrap: bool,
}

#[derive(Clone, Debug)]
struct Tab {
    id: TabId,
    title: String,
    attention: bool,
    panes: HashMap<PaneId, Pane>,
    root: PaneTree,
    focused: PaneId,
}

#[derive(Clone, Debug)]
struct Workspace {
    id: WorkspaceId,
    name: String,
    detail: Option<String>,
    attention: bool,
    tabs: Vec<Tab>,
    active_tab: TabId,
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

/// Constructor input for tests and future hosts. Not a persistence schema.
pub struct ShellWorkspaceSeed {
    pub id: WorkspaceId,
    pub name: String,
    pub detail: Option<String>,
    pub attention: bool,
    pub tabs: Vec<ShellTabSeed>,
    pub active_tab: TabId,
}

pub struct ShellTabSeed {
    pub id: TabId,
    pub title: String,
    pub attention: bool,
    pub pane: ShellPaneSeed,
}

pub struct ShellPaneSeed {
    pub id: PaneId,
    pub title: String,
    pub allows_implicit_execution_bootstrap: bool,
}

impl Workspace {
    fn from_seed(seed: ShellWorkspaceSeed) -> Self {
        let tabs: Vec<Tab> = seed
            .tabs
            .into_iter()
            .map(|tab| {
                let pane = Pane {
                    id: tab.pane.id,
                    title: tab.pane.title,
                    execution: None,
                    allows_implicit_execution_bootstrap: tab
                        .pane
                        .allows_implicit_execution_bootstrap,
                };
                let mut composed = Tab::with_pane(tab.id, tab.title, pane);
                composed.attention = tab.attention;
                composed
            })
            .collect();
        Self {
            id: seed.id,
            name: seed.name,
            detail: seed.detail,
            attention: seed.attention,
            active_tab: seed.active_tab,
            tabs,
        }
    }

    fn tab(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|tab| tab.id == id)
    }

    /// The last Tab of a Workspace cannot be closed.
    fn allows_tab_close(&self) -> bool {
        self.tabs.len() > 1
    }

    fn active_tab_mut(&mut self) -> Result<&mut Tab, ShellError> {
        let id = self.active_tab;
        self.tabs
            .iter_mut()
            .find(|tab| tab.id == id)
            .ok_or(ShellError::UnknownTab)
    }
}

impl Tab {
    fn with_pane(id: TabId, title: String, pane: Pane) -> Self {
        let pane_id = pane.id;
        let mut panes = HashMap::new();
        panes.insert(pane_id, pane);
        Self {
            id,
            title,
            attention: false,
            panes,
            root: PaneTree::Leaf(pane_id),
            focused: pane_id,
        }
    }

    /// The last Pane of a Tab cannot be closed.
    fn allows_pane_close(&self) -> bool {
        self.panes.len() > 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn other_workspace() -> WorkspaceId {
        WorkspaceId::from_bytes([0x11; 16])
    }

    fn seed_two_workspaces() -> ShellState {
        let first_tab = TabId::new();
        let first_pane = PaneId::new();
        let second_tab = TabId::new();
        let second_pane = PaneId::new();
        let first = WorkspaceId::m001_default();
        let second = other_workspace();
        ShellState::from_workspaces(
            vec![
                ShellWorkspaceSeed {
                    id: first,
                    name: "Seyal OSS".to_owned(),
                    detail: Some("~/Projects/seyal".to_owned()),
                    attention: false,
                    active_tab: first_tab,
                    tabs: vec![ShellTabSeed {
                        id: first_tab,
                        title: "Core Terminal".to_owned(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: first_pane,
                            title: "Pane 1".to_owned(),
                            allows_implicit_execution_bootstrap: true,
                        },
                    }],
                },
                ShellWorkspaceSeed {
                    id: second,
                    name: "Payments".to_owned(),
                    detail: Some("~/Projects/payments".to_owned()),
                    attention: true,
                    active_tab: second_tab,
                    tabs: vec![ShellTabSeed {
                        id: second_tab,
                        title: "API".to_owned(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: second_pane,
                            title: "Pane 1".to_owned(),
                            allows_implicit_execution_bootstrap: false,
                        },
                    }],
                },
            ],
            first,
            true,
            true,
        )
        .expect("fixture")
    }

    #[test]
    fn production_shell_is_single_pane_and_fail_closed() {
        let mut shell = ShellState::m001_local("/tmp/seyal");
        let snap = shell.snapshot();
        assert_eq!(snap.workspaces.len(), 1);
        assert_eq!(snap.workspaces[0].id, WorkspaceId::m001_default());
        assert_eq!(snap.tabs.len(), 1);
        assert_eq!(snap.layout, LayoutDescription::Single);
        assert_eq!(snap.panes.len(), 1);
        assert_eq!(snap.panes[0].title, "Pane 1");
        assert!(snap.panes[0].allows_implicit_bootstrap);
        assert!(!shell.allows_tab_creation());
        assert!(!shell.allows_pane_splitting());
        assert!(!snap.allows_tab_close);
        assert!(!snap.allows_pane_close);
        assert_eq!(
            shell.apply(ShellAction::CreateTab),
            Err(ShellError::TabCreationUnavailable)
        );
        assert_eq!(shell.last_error(), Some(ShellError::TabCreationUnavailable));
        let focused = snap.focused_pane;
        assert_eq!(
            shell.apply(ShellAction::SplitPane {
                id: focused,
                axis: SplitAxis::Right
            }),
            Err(ShellError::PaneSplitUnavailable)
        );
        assert_eq!(shell.snapshot().tabs.len(), 1);
        assert_eq!(shell.snapshot().layout, LayoutDescription::Single);
    }

    #[test]
    fn close_enablement_is_projected_from_the_same_rule_close_enforces() {
        let mut shell = seed_two_workspaces();
        let single = shell.snapshot();
        assert!(!single.allows_tab_close);
        assert!(!single.allows_pane_close);

        shell.apply(ShellAction::CreateTab).expect("tabs allowed");
        let two_tabs = shell.snapshot();
        assert!(two_tabs.allows_tab_close);
        assert!(!two_tabs.allows_pane_close);

        shell
            .apply(ShellAction::SplitFocused {
                axis: SplitAxis::Right,
            })
            .expect("splits allowed");
        let two_panes = shell.snapshot();
        assert!(two_panes.allows_pane_close);

        shell
            .apply(ShellAction::ClosePane {
                id: two_panes.focused_pane,
            })
            .expect("close split pane");
        assert!(!shell.snapshot().allows_pane_close);
        shell
            .apply(ShellAction::CloseTab {
                id: two_tabs.active_tab,
            })
            .expect("close created tab");
        let closed = shell.snapshot();
        assert!(!closed.allows_tab_close);
        assert_eq!(
            shell.apply(ShellAction::CloseTab {
                id: closed.active_tab
            }),
            Err(ShellError::CannotCloseLastTab)
        );
        assert_eq!(
            shell.apply(ShellAction::ClosePane {
                id: closed.focused_pane
            }),
            Err(ShellError::CannotCloseLastPane)
        );
    }

    #[test]
    fn select_create_close_tabs_are_authoritative() {
        let mut shell = seed_two_workspaces();
        let before = shell.snapshot();
        shell.apply(ShellAction::CreateTab).expect("tabs allowed");
        let after_create = shell.snapshot();
        assert_eq!(after_create.tabs.len(), 2);
        assert_ne!(after_create.active_tab, before.active_tab);
        let created = after_create.active_tab;
        shell
            .apply(ShellAction::SelectTab {
                id: before.active_tab,
            })
            .expect("select original");
        assert_eq!(shell.snapshot().active_tab, before.active_tab);
        shell
            .apply(ShellAction::CloseTab { id: created })
            .expect("close created");
        assert_eq!(shell.snapshot().tabs.len(), 1);
        assert_eq!(shell.snapshot().active_tab, before.active_tab);
        assert_eq!(
            shell.apply(ShellAction::CloseTab {
                id: before.active_tab
            }),
            Err(ShellError::CannotCloseLastTab)
        );
    }

    #[test]
    fn split_focus_and_close_panes() {
        let mut shell = seed_two_workspaces();
        let original = shell.snapshot().focused_pane;
        shell
            .apply(ShellAction::SplitFocused {
                axis: SplitAxis::Right,
            })
            .expect("split");
        let snap = shell.snapshot();
        assert_eq!(snap.layout, LayoutDescription::SplitRight);
        assert_eq!(snap.tabs[0].pane_count, 2);
        assert_ne!(snap.focused_pane, original);
        let created = snap.focused_pane;
        shell
            .apply(ShellAction::FocusPane { id: original })
            .expect("focus original");
        assert_eq!(shell.snapshot().focused_pane, original);
        shell
            .apply(ShellAction::SplitPane {
                id: original,
                axis: SplitAxis::Down,
            })
            .expect("nested split");
        assert_eq!(shell.snapshot().tabs[0].pane_count, 3);
        assert_eq!(shell.snapshot().layout, LayoutDescription::SplitRight);
        shell
            .apply(ShellAction::ClosePane { id: created })
            .expect("close first split");
        assert_eq!(shell.snapshot().tabs[0].pane_count, 2);
        let remaining_new = shell.snapshot().focused_pane;
        shell
            .apply(ShellAction::ClosePane { id: remaining_new })
            .expect("close nested");
        let snap = shell.snapshot();
        assert_eq!(snap.layout, LayoutDescription::Single);
        assert_eq!(snap.focused_pane, original);
        assert_eq!(
            shell.apply(ShellAction::ClosePane { id: original }),
            Err(ShellError::CannotCloseLastPane)
        );
    }

    #[test]
    fn workspace_selection_switches_tab_inventory() {
        let mut shell = seed_two_workspaces();
        let first = shell.snapshot();
        shell
            .apply(ShellAction::SelectWorkspace {
                id: other_workspace(),
            })
            .expect("select second workspace");
        let second = shell.snapshot();
        assert_eq!(second.active_workspace, other_workspace());
        assert_eq!(second.tabs[0].title, "API");
        assert_ne!(second.active_tab, first.active_tab);
        assert!(second.workspaces[1].attention);
    }

    #[test]
    fn stale_identities_fail_closed_and_leave_state() {
        let mut shell = seed_two_workspaces();
        let before = shell.snapshot();
        assert_eq!(
            shell.apply(ShellAction::SelectWorkspace {
                id: WorkspaceId::from_bytes([0xff; 16])
            }),
            Err(ShellError::UnknownWorkspace)
        );
        assert_eq!(
            shell.apply(ShellAction::SelectTab { id: TabId::new() }),
            Err(ShellError::UnknownTab)
        );
        assert_eq!(
            shell.apply(ShellAction::FocusPane { id: PaneId::new() }),
            Err(ShellError::UnknownPane)
        );
        assert_eq!(shell.snapshot().active_workspace, before.active_workspace);
        assert_eq!(shell.snapshot().active_tab, before.active_tab);
        assert_eq!(shell.snapshot().focused_pane, before.focused_pane);
    }

    #[test]
    fn execution_bind_is_one_shot_and_does_not_own_pty() {
        let mut shell = ShellState::m001_local(".");
        let pane = shell.snapshot().focused_pane;
        let execution = ExecutionId::new();
        shell
            .apply(ShellAction::BindExecution { pane, execution })
            .expect("bind");
        assert_eq!(shell.pane_execution(pane).unwrap(), Some(execution));
        assert_eq!(
            shell.apply(ShellAction::BindExecution {
                pane,
                execution: ExecutionId::new()
            }),
            Err(ShellError::ExecutionAlreadyBound)
        );
        assert_eq!(shell.pane_execution(pane).unwrap(), Some(execution));
        assert!(shell.focused_pane_allows_implicit_bootstrap());
    }

    #[test]
    fn snapshots_are_deterministic_for_identical_state() {
        let shell = seed_two_workspaces();
        assert_eq!(shell.snapshot(), shell.snapshot());
    }

    #[test]
    fn empty_shell_is_rejected() {
        assert_eq!(
            ShellState::from_workspaces(Vec::new(), WorkspaceId::m001_default(), true, true).err(),
            Some(ShellError::EmptyShell)
        );
    }

    #[test]
    fn pane_tree_walk_is_preorder() {
        let a = PaneId::new();
        let b = PaneId::new();
        let tree = PaneTree::Leaf(a).replacing(
            a,
            PaneTree::Split {
                axis: SplitAxis::Right,
                first: Box::new(PaneTree::Leaf(a)),
                second: Box::new(PaneTree::Leaf(b)),
            },
        );
        assert_eq!(tree.pane_ids(), vec![a, b]);
        assert_eq!(tree.layout_description(), LayoutDescription::SplitRight);
    }
}
