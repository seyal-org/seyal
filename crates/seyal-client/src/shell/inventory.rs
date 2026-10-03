//! Stable-order navigation inventory for goto enumeration (SPEC-022 §7.6 / N4).

use std::collections::HashSet;

use seyal_core::{ExecutionId, PaneId, TabId, WorkspaceId};

use super::workspace::Workspace;

/// Stable-order goto enumeration input derived from authoritative shell state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavigationInventory {
    pub workspaces: Vec<WorkspaceNavItem>,
    pub tabs: Vec<TabNavItem>,
    pub panes: Vec<PaneNavItem>,
    pub sessions: Vec<SessionNavItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceNavItem {
    pub id: WorkspaceId,
    pub name: String,
    pub detail: Option<String>,
    pub attention: bool,
    pub active: bool,
    pub tab_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabNavItem {
    pub workspace: WorkspaceId,
    pub workspace_name: String,
    pub id: TabId,
    pub title: String,
    pub attention: bool,
    pub active: bool,
    pub pane_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneNavItem {
    pub workspace: WorkspaceId,
    pub workspace_name: String,
    pub tab: TabId,
    pub tab_title: String,
    pub id: PaneId,
    pub title: String,
    pub focused: bool,
    pub execution: Option<ExecutionId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionNavItem {
    pub execution: ExecutionId,
    pub workspace: WorkspaceId,
    pub workspace_name: String,
    pub tab: TabId,
    pub tab_title: String,
    pub pane: PaneId,
    pub pane_title: String,
    pub focused: bool,
}

/// Order is Workspace registration order, then Tab order, then Pane tree leaf
/// order. Sessions are bound executions from that walk (one row per execution).
pub(super) fn build(
    workspaces: &[Workspace],
    active_workspace: WorkspaceId,
) -> NavigationInventory {
    let mut out_workspaces = Vec::new();
    let mut tabs = Vec::new();
    let mut panes = Vec::new();
    let mut sessions = Vec::new();
    let mut seen_executions = HashSet::new();

    for workspace in workspaces {
        let active_tab = workspace.active_tab_id();
        out_workspaces.push(WorkspaceNavItem {
            id: workspace.id,
            name: workspace.name.clone(),
            detail: workspace.detail.clone(),
            attention: workspace.attention,
            active: workspace.id == active_workspace,
            tab_count: workspace.tab_count(),
        });
        for tab in workspace.tabs() {
            let focused = tab.focused;
            tabs.push(TabNavItem {
                workspace: workspace.id,
                workspace_name: workspace.name.clone(),
                id: tab.id,
                title: tab.title.clone(),
                attention: tab.attention,
                active: workspace.id == active_workspace && tab.id == active_tab,
                pane_count: tab.panes.len(),
            });
            for pane_id in tab.root.pane_ids() {
                let Some(pane) = tab.panes.get(&pane_id) else {
                    continue;
                };
                let focused_here =
                    workspace.id == active_workspace && tab.id == active_tab && pane.id == focused;
                panes.push(PaneNavItem {
                    workspace: workspace.id,
                    workspace_name: workspace.name.clone(),
                    tab: tab.id,
                    tab_title: tab.title.clone(),
                    id: pane.id,
                    title: pane.title.clone(),
                    focused: focused_here,
                    execution: pane.execution,
                });
                if let Some(execution) = pane.execution
                    && seen_executions.insert(execution)
                {
                    sessions.push(SessionNavItem {
                        execution,
                        workspace: workspace.id,
                        workspace_name: workspace.name.clone(),
                        tab: tab.id,
                        tab_title: tab.title.clone(),
                        pane: pane.id,
                        pane_title: pane.title.clone(),
                        focused: focused_here,
                    });
                }
            }
        }
    }

    NavigationInventory {
        workspaces: out_workspaces,
        tabs,
        panes,
        sessions,
    }
}
