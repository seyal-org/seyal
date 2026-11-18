//! SPEC-032 §3.5 one-time migration off the outer Seyal Tab layer.

#[cfg(test)]
use seyal_core::ExecutionId;
use seyal_core::{WindowId, WorkspaceId};

use super::ShellState;

impl ShellState {
    /// Collapse each workspace to one pane tree on one presenting window.
    ///
    /// Executions bound only on discarded tabs/windows become live-unpresented
    /// and keep their `WorkspaceId`. Non-active tab layouts and extra storage
    /// windows are discarded (SPEC-032 §3.5 / #1490).
    pub fn migrate_seyal_tabs_to_sidebar_containment(&mut self) {
        let workspace_ids: Vec<WorkspaceId> = self
            .workspaces
            .iter()
            .map(|workspace| workspace.id)
            .collect();
        for workspace_id in workspace_ids {
            self.migrate_workspace_to_single_tree(workspace_id);
        }
        self.purge_missing_panes();
    }

    fn migrate_workspace_to_single_tree(&mut self, workspace_id: WorkspaceId) {
        let keep_window = {
            let Ok(workspace) = self.workspace(workspace_id) else {
                return;
            };
            workspace
                .presenting_window
                .filter(|window| workspace.window(*window).is_some())
                .or(workspace.active_window)
                .or_else(|| workspace.windows.first().map(|window| window.id))
        };
        let Some(keep_window) = keep_window else {
            return;
        };
        let keep_tab = self
            .workspace(workspace_id)
            .ok()
            .and_then(|workspace| workspace.window(keep_window))
            .map(|window| window.active_tab);

        let other_windows: Vec<WindowId> = self
            .workspace(workspace_id)
            .ok()
            .map(|workspace| {
                workspace
                    .windows
                    .iter()
                    .map(|window| window.id)
                    .filter(|id| *id != keep_window)
                    .collect()
            })
            .unwrap_or_default();
        for window_id in other_windows {
            if let Ok(released) = self.window_released_executions(window_id) {
                for (execution, workspace) in released {
                    self.unpresented.insert(execution, workspace);
                }
            }
            let _ = self.remove_window_record(window_id);
        }

        if let Some(keep_tab) = keep_tab {
            let removed = {
                let Some(window) = self
                    .workspaces
                    .iter_mut()
                    .find(|workspace| workspace.id == workspace_id)
                    .and_then(|workspace| workspace.window_mut(keep_window))
                else {
                    return;
                };
                if window.tabs.len() > 1 {
                    let mut kept = Vec::new();
                    let mut discarded = Vec::new();
                    for tab in std::mem::take(&mut window.tabs) {
                        if tab.id == keep_tab {
                            kept.push(tab);
                        } else {
                            discarded.push(tab);
                        }
                    }
                    if kept.is_empty() {
                        if let Some(first) = discarded.first_mut() {
                            let fallback = first.id;
                            window.active_tab = fallback;
                            kept.push(discarded.remove(0));
                        }
                    } else {
                        window.active_tab = keep_tab;
                    }
                    window.tabs = kept;
                    discarded
                } else {
                    Vec::new()
                }
            };
            for tab in removed {
                for pane in tab.panes.values() {
                    for terminal in &pane.terminal_tabs {
                        if let Some(execution) = terminal.execution {
                            self.unpresented.insert(execution, workspace_id);
                        }
                    }
                }
            }
        }

        if let Ok(workspace) = self.workspace_mut(workspace_id) {
            if let Some(window) = workspace.window_mut(keep_window) {
                window.open_workspaces = vec![workspace_id];
                window.selected_workspace = workspace_id;
            }
            workspace.active_window = Some(keep_window);
            workspace.presenting_window = Some(keep_window);
        }
    }

    #[cfg(test)]
    pub(crate) fn unpresented_executions_for_test(&self) -> Vec<(ExecutionId, WorkspaceId)> {
        self.unpresented
            .iter()
            .map(|(execution, workspace)| (*execution, *workspace))
            .collect()
    }
}
