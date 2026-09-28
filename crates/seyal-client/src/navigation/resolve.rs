//! Pure resolution of [`ResourceAddress`] against shell and execution inventory.

use seyal_core::{ExecutionId, PaneId, TabId, WorkspaceId};

use crate::shell::ShellState;

use super::ResourceAddress;

/// Typed rejection taxonomy from SPEC-022 R3.4. Exhaustive and ordered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavigationRejection {
    UnsupportedKind,
    NavigationDenied,
    UnknownWorkspace,
    UnknownTab,
    UnknownPane,
    UnknownExecution,
    NotComposed,
    TargetTerminated,
    TargetUnbound,
    AmbiguousTarget,
}

/// Successful resolution target. An [`ResourceAddress::Execution`] with exactly
/// one bound Pane resolves to that Pane (SPEC-022 R3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolvedTarget {
    Workspace {
        workspace: WorkspaceId,
    },
    Tab {
        workspace: WorkspaceId,
        tab: TabId,
    },
    Pane {
        workspace: WorkspaceId,
        tab: TabId,
        pane: PaneId,
    },
}

/// Presence of an `ExecutionId` in the Runtime inventory (borrowed; not owned
/// by this module).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionPresence {
    /// Execution is live.
    Live,
    /// Execution has exited and the inventory still holds its exited record.
    ExitedHeld,
}

/// Borrowed Runtime inventory view for execution-address resolution.
pub trait ExecutionInventory {
    fn presence(&self, execution: ExecutionId) -> Option<ExecutionPresence>;
}

/// Empty inventory: every execution is absent (UnknownExecution when unbound).
#[derive(Clone, Copy, Debug, Default)]
pub struct EmptyExecutionInventory;

impl ExecutionInventory for EmptyExecutionInventory {
    fn presence(&self, _execution: ExecutionId) -> Option<ExecutionPresence> {
        None
    }
}

impl<F> ExecutionInventory for F
where
    F: Fn(ExecutionId) -> Option<ExecutionPresence>,
{
    fn presence(&self, execution: ExecutionId) -> Option<ExecutionPresence> {
        self(execution)
    }
}

/// Workspace access set for the requesting principal (ADR-007 §11 / SPEC-022 R3.4).
#[derive(Clone, Copy, Debug)]
pub enum WorkspaceAccess<'a> {
    /// M003 local user: authorized for every local Workspace.
    AllLocal,
    /// Explicit allow-list. A `WorkspaceId` outside the set is denied whether
    /// or not it exists.
    Only(&'a [WorkspaceId]),
}

/// Requesting principal for navigation authorization.
#[derive(Clone, Copy, Debug)]
pub struct NavigationPrincipal<'a> {
    /// Local navigation authority required for `Execution` addresses.
    pub local_navigation: bool,
    pub workspaces: WorkspaceAccess<'a>,
}

impl NavigationPrincipal<'static> {
    /// M003 sole principal: local user authorized for every local Workspace.
    pub const fn local_user() -> Self {
        Self {
            local_navigation: true,
            workspaces: WorkspaceAccess::AllLocal,
        }
    }
}

impl<'a> NavigationPrincipal<'a> {
    fn allows_workspace(self, workspace: WorkspaceId) -> bool {
        match self.workspaces {
            WorkspaceAccess::AllLocal => true,
            WorkspaceAccess::Only(allowed) => allowed.contains(&workspace),
        }
    }
}

/// Resolve `address` against current authoritative state. Never mutates
/// `shell`, bindings, focus, history, or presentation (SPEC-022 R3.6).
pub fn resolve(
    address: ResourceAddress,
    shell: &ShellState,
    inventory: &impl ExecutionInventory,
    principal: NavigationPrincipal<'_>,
) -> Result<ResolvedTarget, NavigationRejection> {
    match address {
        ResourceAddress::Workspace { workspace } => {
            authorize_workspace(principal, workspace)?;
            if !shell.contains_workspace(workspace) {
                return Err(NavigationRejection::UnknownWorkspace);
            }
            Ok(ResolvedTarget::Workspace { workspace })
        }
        ResourceAddress::Tab { workspace, tab } => {
            authorize_workspace(principal, workspace)?;
            if !shell.contains_workspace(workspace) {
                return Err(NavigationRejection::UnknownWorkspace);
            }
            let Some(owner) = shell.workspace_of_tab(tab) else {
                return Err(NavigationRejection::UnknownTab);
            };
            if owner != workspace {
                return Err(NavigationRejection::NotComposed);
            }
            Ok(ResolvedTarget::Tab { workspace, tab })
        }
        ResourceAddress::Pane {
            workspace,
            tab,
            pane,
        } => {
            authorize_workspace(principal, workspace)?;
            if !shell.contains_workspace(workspace) {
                return Err(NavigationRejection::UnknownWorkspace);
            }
            let Some(tab_owner) = shell.workspace_of_tab(tab) else {
                return Err(NavigationRejection::UnknownTab);
            };
            let Some((pane_workspace, pane_tab)) = shell.location_of_pane(pane) else {
                return Err(NavigationRejection::UnknownPane);
            };
            // Components exist but do not currently compose (R3.2).
            if tab_owner != workspace
                || pane_workspace != workspace
                || pane_tab != tab
                || !shell.tab_contains_leaf(workspace, tab, pane)
            {
                return Err(NavigationRejection::NotComposed);
            }
            Ok(ResolvedTarget::Pane {
                workspace,
                tab,
                pane,
            })
        }
        ResourceAddress::Execution { execution } => {
            if !principal.local_navigation {
                return Err(NavigationRejection::NavigationDenied);
            }
            let bound = shell.panes_bound_to(execution);
            match bound.len() {
                0 => match inventory.presence(execution) {
                    Some(ExecutionPresence::ExitedHeld) => {
                        Err(NavigationRejection::TargetTerminated)
                    }
                    Some(ExecutionPresence::Live) => Err(NavigationRejection::TargetUnbound),
                    None => Err(NavigationRejection::UnknownExecution),
                },
                1 => {
                    let (workspace, tab, pane) = bound[0];
                    if !principal.allows_workspace(workspace) {
                        return Err(NavigationRejection::NavigationDenied);
                    }
                    Ok(ResolvedTarget::Pane {
                        workspace,
                        tab,
                        pane,
                    })
                }
                _ => Err(NavigationRejection::AmbiguousTarget),
            }
        }
    }
}

fn authorize_workspace(
    principal: NavigationPrincipal<'_>,
    workspace: WorkspaceId,
) -> Result<(), NavigationRejection> {
    if principal.allows_workspace(workspace) {
        Ok(())
    } else {
        Err(NavigationRejection::NavigationDenied)
    }
}
