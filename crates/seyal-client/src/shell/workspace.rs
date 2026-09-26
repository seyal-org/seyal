//! Workspace / Tab / Pane composition seeds and private shell inventory types.

use std::collections::HashMap;

use seyal_core::{ExecutionId, PaneId, TabId, WorkspaceId};

use super::tree::PaneTree;
use super::ShellError;

#[derive(Clone, Debug)]
pub(super) struct Pane {
    pub(super) id: PaneId,
    pub(super) title: String,
    pub(super) execution: Option<ExecutionId>,
    pub(super) allows_implicit_execution_bootstrap: bool,
}

#[derive(Clone, Debug)]
pub(super) struct Tab {
    pub(super) id: TabId,
    pub(super) title: String,
    pub(super) attention: bool,
    pub(super) panes: HashMap<PaneId, Pane>,
    pub(super) root: PaneTree,
    pub(super) focused: PaneId,
}

#[derive(Clone, Debug)]
pub(super) struct Workspace {
    pub(super) id: WorkspaceId,
    pub(super) name: String,
    pub(super) detail: Option<String>,
    pub(super) attention: bool,
    pub(super) tabs: Vec<Tab>,
    pub(super) active_tab: TabId,
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
    pub(super) fn from_seed(seed: ShellWorkspaceSeed) -> Self {
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

    pub(super) fn tab(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|tab| tab.id == id)
    }

    /// The last Tab of a Workspace cannot be closed.
    pub(super) fn allows_tab_close(&self) -> bool {
        self.tabs.len() > 1
    }

    pub(super) fn active_tab_mut(&mut self) -> Result<&mut Tab, ShellError> {
        let id = self.active_tab;
        self.tabs
            .iter_mut()
            .find(|tab| tab.id == id)
            .ok_or(ShellError::UnknownTab)
    }
}

impl Tab {
    pub(super) fn with_pane(id: TabId, title: String, pane: Pane) -> Self {
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
    pub(super) fn allows_pane_close(&self) -> bool {
        self.panes.len() > 1
    }
}
