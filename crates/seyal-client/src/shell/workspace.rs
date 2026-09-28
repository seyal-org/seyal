//! Workspace / Tab / Pane composition seeds and private shell inventory types.

use std::collections::HashMap;

use seyal_core::{ExecutionId, PaneId, TabId, WindowId, WorkspaceId};

use super::tree::PaneTree;
use super::ShellError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Pane {
    pub(super) id: PaneId,
    pub(super) title: String,
    pub(super) execution: Option<ExecutionId>,
    pub(super) allows_implicit_execution_bootstrap: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Tab {
    pub(super) id: TabId,
    pub(super) title: String,
    pub(super) attention: bool,
    pub(super) panes: HashMap<PaneId, Pane>,
    pub(super) root: PaneTree,
    pub(super) focused: PaneId,
}

/// One Window inside a Workspace. `workspace_id` is fixed at construction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Window {
    pub(super) id: WindowId,
    /// Stored with the window. Shell tests read it; the library build does not.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) workspace_id: WorkspaceId,
    pub(super) attention: bool,
    pub(super) tabs: Vec<Tab>,
    pub(super) active_tab: TabId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Workspace {
    pub(super) id: WorkspaceId,
    pub(super) name: String,
    pub(super) detail: Option<String>,
    pub(super) attention: bool,
    pub(super) windows: Vec<Window>,
    /// None only when this Workspace has zero Windows (ADR-018 §3.3a / ActivateWorkspace create).
    pub(super) active_window: Option<WindowId>,
}

/// Constructor input for tests and future hosts. Not a persistence schema.
pub struct ShellWorkspaceSeed {
    pub id: WorkspaceId,
    pub name: String,
    pub detail: Option<String>,
    pub attention: bool,
    pub windows: Vec<ShellWindowSeed>,
    /// Ignored when `windows` is empty; otherwise must name a seeded Window.
    pub active_window: WindowId,
}

pub struct ShellWindowSeed {
    pub id: WindowId,
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

impl Window {
    pub(super) fn try_new(
        id: WindowId,
        workspace_id: WorkspaceId,
        tabs: Vec<Tab>,
        active_tab: TabId,
    ) -> Result<Self, ShellError> {
        if tabs.is_empty() || tabs.iter().any(|tab| tab.panes.is_empty()) {
            return Err(if tabs.is_empty() {
                ShellError::EmptyWindow
            } else {
                ShellError::EmptyTab
            });
        }
        if !tabs.iter().any(|tab| tab.id == active_tab) {
            return Err(ShellError::UnknownTab);
        }
        Ok(Self {
            id,
            workspace_id,
            attention: false,
            tabs,
            active_tab,
        })
    }

    pub(super) fn tab_index(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id == id)
    }
}

impl Workspace {
    pub(super) fn from_seed(seed: ShellWorkspaceSeed) -> Result<Self, ShellError> {
        if seed.windows.is_empty() {
            return Ok(Self {
                id: seed.id,
                name: seed.name,
                detail: seed.detail,
                attention: seed.attention,
                windows: Vec::new(),
                active_window: None,
            });
        }
        let mut windows = Vec::with_capacity(seed.windows.len());
        for window in seed.windows {
            let tabs = window
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
            windows.push(Window::try_new(
                window.id,
                seed.id,
                tabs,
                window.active_tab,
            )?);
        }
        if !windows.iter().any(|window| window.id == seed.active_window) {
            return Err(ShellError::UnknownWindow);
        }
        Ok(Self {
            id: seed.id,
            name: seed.name,
            detail: seed.detail,
            attention: seed.attention,
            active_window: Some(seed.active_window),
            windows,
        })
    }

    pub(super) fn tab_count(&self) -> usize {
        self.windows.iter().map(|window| window.tabs.len()).sum()
    }

    /// Workspace tab order is window order, then tab order inside each window.
    pub(super) fn tabs(&self) -> impl Iterator<Item = &Tab> {
        self.windows.iter().flat_map(|window| window.tabs.iter())
    }

    pub(super) fn tab(&self, id: TabId) -> Option<&Tab> {
        self.tabs().find(|tab| tab.id == id)
    }

    pub(super) fn window(&self, id: WindowId) -> Option<&Window> {
        self.windows.iter().find(|window| window.id == id)
    }

    pub(super) fn window_mut(&mut self, id: WindowId) -> Option<&mut Window> {
        self.windows.iter_mut().find(|window| window.id == id)
    }

    pub(super) fn window_index(&self, id: WindowId) -> Option<usize> {
        self.windows.iter().position(|window| window.id == id)
    }

    pub(super) fn active_window(&self) -> Option<&Window> {
        let id = self.active_window?;
        self.windows.iter().find(|window| window.id == id)
    }

    pub(super) fn active_window_mut(&mut self) -> Result<&mut Window, ShellError> {
        let id = self.active_window.ok_or(ShellError::UnknownWindow)?;
        self.windows
            .iter_mut()
            .find(|window| window.id == id)
            .ok_or(ShellError::UnknownWindow)
    }

    pub(super) fn active_tab_id(&self) -> Result<TabId, ShellError> {
        Ok(self
            .active_window()
            .ok_or(ShellError::UnknownWindow)?
            .active_tab)
    }

    /// The active Window's last Tab cannot be closed. Closing it would leave a
    /// zero-Tab Window, which is not a W1 close-window path.
    pub(super) fn allows_tab_close(&self) -> bool {
        self.active_window()
            .is_some_and(|window| window.tabs.len() > 1)
    }

    pub(super) fn select_tab(&mut self, id: TabId) -> Result<(), ShellError> {
        let window_id = self
            .windows
            .iter()
            .find(|window| window.tabs.iter().any(|tab| tab.id == id))
            .map(|window| window.id)
            .ok_or(ShellError::UnknownTab)?;
        self.active_window = Some(window_id);
        self.active_window_mut()?.active_tab = id;
        Ok(())
    }

    pub(super) fn push_tab_on_window(
        &mut self,
        window_id: WindowId,
        tab: Tab,
    ) -> Result<(), ShellError> {
        let window = self
            .window_mut(window_id)
            .ok_or(ShellError::UnknownWindow)?;
        window.active_tab = tab.id;
        window.tabs.push(tab);
        Ok(())
    }

    pub(super) fn close_tab(&mut self, id: TabId) -> Result<(), ShellError> {
        // One Tab in the Workspace fails closed before the id is resolved, so a
        // stale id and the real last Tab report the same cause.
        if self.tab_count() <= 1 {
            return Err(ShellError::CannotCloseLastTab);
        }
        let Some(window_index) = self
            .windows
            .iter()
            .position(|window| window.tabs.iter().any(|tab| tab.id == id))
        else {
            return Err(ShellError::UnknownTab);
        };
        if self.windows[window_index].tabs.len() <= 1 {
            return Err(ShellError::CannotCloseLastTab);
        }
        let Some(tab_index) = self.windows[window_index]
            .tabs
            .iter()
            .position(|tab| tab.id == id)
        else {
            return Err(ShellError::UnknownTab);
        };
        self.windows[window_index].tabs.remove(tab_index);
        if self.windows[window_index].active_tab == id {
            let replacement = tab_index.min(self.windows[window_index].tabs.len() - 1);
            self.windows[window_index].active_tab = self.windows[window_index].tabs[replacement].id;
        }
        Ok(())
    }

    pub(super) fn active_tab_mut(&mut self) -> Result<&mut Tab, ShellError> {
        let tab_id = self.active_tab_id()?;
        self.active_window_mut()?
            .tabs
            .iter_mut()
            .find(|tab| tab.id == tab_id)
            .ok_or(ShellError::UnknownTab)
    }

    /// Remove `tab` from its Window. Destroys the Window when it was the only Tab.
    /// Returns `(removed_tab, destroyed_source_window)`.
    pub(super) fn take_tab(&mut self, tab: TabId) -> Result<(Tab, Option<WindowId>), ShellError> {
        let window_index = self
            .windows
            .iter()
            .position(|window| window.tabs.iter().any(|item| item.id == tab))
            .ok_or(ShellError::UnknownTab)?;
        let tab_index = self.windows[window_index]
            .tab_index(tab)
            .expect("tab located above");
        let removed = self.windows[window_index].tabs.remove(tab_index);
        if self.windows[window_index].tabs.is_empty() {
            let destroyed = self.windows.remove(window_index).id;
            if self.active_window == Some(destroyed) {
                self.active_window = self.windows.first().map(|window| window.id);
            }
            return Ok((removed, Some(destroyed)));
        }
        if self.windows[window_index].active_tab == tab {
            let replacement = tab_index.min(self.windows[window_index].tabs.len() - 1);
            self.windows[window_index].active_tab = self.windows[window_index].tabs[replacement].id;
        }
        Ok((removed, None))
    }

    /// Insert `tab` before `before` in `window` (`None` appends). Makes it active.
    pub(super) fn insert_tab_before(
        &mut self,
        window_id: WindowId,
        tab: Tab,
        before: Option<TabId>,
    ) -> Result<(), ShellError> {
        let window = self
            .window_mut(window_id)
            .ok_or(ShellError::UnknownWindow)?;
        let insert_at = match before {
            None => window.tabs.len(),
            Some(before_id) => window.tab_index(before_id).ok_or(ShellError::UnknownTab)?,
        };
        window.active_tab = tab.id;
        window.tabs.insert(insert_at, tab);
        Ok(())
    }

    pub(super) fn push_window(&mut self, window: Window) {
        self.active_window = Some(window.id);
        self.windows.push(window);
    }

    #[cfg(test)]
    pub(super) fn window_workspace(&self, id: WindowId) -> Option<WorkspaceId> {
        self.windows
            .iter()
            .find(|window| window.id == id)
            .map(|window| window.workspace_id)
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

    /// `ClosePane` of the focused Pane would be accepted: not the last Pane
    /// and not bound to an execution.
    pub(super) fn allows_focused_pane_close(&self) -> bool {
        self.allows_pane_close()
            && self
                .panes
                .get(&self.focused)
                .is_some_and(|pane| pane.execution.is_none())
    }

    #[cfg(test)]
    fn without_panes(id: TabId) -> Self {
        Self {
            id,
            title: "empty".to_owned(),
            attention: false,
            panes: HashMap::new(),
            root: PaneTree::Leaf(PaneId::new()),
            focused: PaneId::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::ShellState;

    fn tab_seed(id: TabId, title: &str) -> ShellTabSeed {
        ShellTabSeed {
            id,
            title: title.to_owned(),
            attention: false,
            pane: ShellPaneSeed {
                id: PaneId::new(),
                title: "Pane 1".to_owned(),
                allows_implicit_execution_bootstrap: true,
            },
        }
    }

    #[test]
    fn derived_tab_order_is_window_order_then_tab_order() {
        let workspace_id = WorkspaceId::m001_default();
        let first_window = WindowId::new();
        let second_window = WindowId::new();
        let first = TabId::new();
        let second = TabId::new();
        let third = TabId::new();
        let shell = ShellState::from_workspaces(
            vec![ShellWorkspaceSeed {
                id: workspace_id,
                name: "Local".to_owned(),
                detail: None,
                attention: false,
                active_window: first_window,
                windows: vec![
                    ShellWindowSeed {
                        id: first_window,
                        active_tab: first,
                        tabs: vec![tab_seed(first, "One"), tab_seed(second, "Two")],
                    },
                    ShellWindowSeed {
                        id: second_window,
                        active_tab: third,
                        tabs: vec![tab_seed(third, "Three")],
                    },
                ],
            }],
            workspace_id,
            false,
            false,
        )
        .expect("two windows");
        let titles: Vec<_> = shell
            .snapshot()
            .tabs
            .into_iter()
            .map(|tab| tab.title)
            .collect();
        assert_eq!(titles, ["One", "Two", "Three"]);
    }

    #[test]
    fn a_window_stays_bound_to_its_workspace() {
        let left = WorkspaceId::m001_default();
        let right = WorkspaceId::from_bytes([0x22; 16]);
        let left_window = WindowId::new();
        let right_window = WindowId::new();
        let left_tab = TabId::new();
        let right_tab = TabId::new();
        let left_workspace = Workspace::from_seed(ShellWorkspaceSeed {
            id: left,
            name: "Left".to_owned(),
            detail: None,
            attention: false,
            active_window: left_window,
            windows: vec![ShellWindowSeed {
                id: left_window,
                active_tab: left_tab,
                tabs: vec![tab_seed(left_tab, "Left")],
            }],
        })
        .expect("left");
        let right_workspace = Workspace::from_seed(ShellWorkspaceSeed {
            id: right,
            name: "Right".to_owned(),
            detail: None,
            attention: false,
            active_window: right_window,
            windows: vec![ShellWindowSeed {
                id: right_window,
                active_tab: right_tab,
                tabs: vec![tab_seed(right_tab, "Right")],
            }],
        })
        .expect("right");
        assert_eq!(left_workspace.window_workspace(left_window), Some(left));
        assert_eq!(right_workspace.window_workspace(right_window), Some(right));
        assert_eq!(left_workspace.window_workspace(right_window), None);
    }

    #[test]
    fn empty_window_and_empty_tab_fail_closed() {
        let window = WindowId::new();
        let tab = TabId::new();
        assert!(matches!(
            Window::try_new(window, WorkspaceId::m001_default(), Vec::new(), tab),
            Err(ShellError::EmptyWindow)
        ));
        assert!(matches!(
            Window::try_new(
                window,
                WorkspaceId::m001_default(),
                vec![Tab::without_panes(tab)],
                tab,
            ),
            Err(ShellError::EmptyTab)
        ));
    }

    #[test]
    fn empty_workspace_seed_is_allowed_for_activate_create() {
        let workspace = Workspace::from_seed(ShellWorkspaceSeed {
            id: WorkspaceId::m001_default(),
            name: "Empty".to_owned(),
            detail: None,
            attention: false,
            active_window: WindowId::new(),
            windows: Vec::new(),
        })
        .expect("empty workspace");
        assert!(workspace.windows.is_empty());
        assert_eq!(workspace.active_window, None);
    }
}
