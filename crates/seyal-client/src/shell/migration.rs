//! SPEC-032 §3.5 one-time migration off the outer Seyal Tab layer.

use seyal_core::{TabId, WindowId, WorkspaceId};

use super::ShellState;

impl ShellState {
    /// Collapse each window to its active Seyal Tab's pane tree.
    ///
    /// Executions bound only on non-active tabs become live-unpresented and keep
    /// their `WorkspaceId`. Non-active tab layouts are discarded.
    pub fn migrate_seyal_tabs_to_sidebar_containment(&mut self) {
        let targets: Vec<(WorkspaceId, WindowId, TabId)> = self
            .workspaces
            .iter()
            .flat_map(|workspace| {
                workspace.windows.iter().filter_map(move |window| {
                    if window.tabs.len() <= 1 {
                        return None;
                    }
                    Some((workspace.id, window.id, window.active_tab))
                })
            })
            .collect();
        for (workspace_id, window_id, keep) in targets {
            let removed = {
                let Some(window) = self
                    .workspaces
                    .iter_mut()
                    .find(|workspace| workspace.id == workspace_id)
                    .and_then(|workspace| workspace.window_mut(window_id))
                else {
                    continue;
                };
                let mut kept = Vec::new();
                let mut discarded = Vec::new();
                for tab in std::mem::take(&mut window.tabs) {
                    if tab.id == keep {
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
                    window.active_tab = keep;
                }
                window.tabs = kept;
                discarded
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
        self.purge_missing_panes();
    }

    #[cfg(test)]
    pub(crate) fn unpresented_executions_for_test(&self) -> Vec<(ExecutionId, WorkspaceId)> {
        self.unpresented
            .iter()
            .map(|(execution, workspace)| (*execution, *workspace))
            .collect()
    }
}
