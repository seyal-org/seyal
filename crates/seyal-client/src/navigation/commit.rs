//! Atomic `Navigate(address)` commit (SPEC-022 §4).
//!
//! Resolve with the N1 resolver, then apply workspace/tab/pane focus in one
//! transition. Rejection leaves shell focus unchanged. No focus history,
//! window activation, presentation, binding, or PTY mutation (N2 scope).

use crate::shell::ShellState;

use super::{
    resolve, ExecutionInventory, NavigationPrincipal, NavigationRejection, ResolvedTarget,
    ResourceAddress,
};

/// Atomically navigate to `address`.
///
/// On success: activate owning Workspace, select owning Tab, set focused Pane
/// (SPEC-022 R4.1). On any failure: no focus fields change (R4.2). Already-active
/// targets succeed as a no-op (R4.4). Never mutates presentation, bindings, or
/// PTY state (R4.3).
pub fn navigate(
    address: ResourceAddress,
    shell: &mut ShellState,
    inventory: &impl ExecutionInventory,
    principal: NavigationPrincipal<'_>,
) -> Result<ResolvedTarget, NavigationRejection> {
    let target = resolve(address, shell, inventory, principal)?;
    let (workspace, tab, pane) = match target {
        ResolvedTarget::Workspace { workspace } => focus_triple_for_workspace(shell, workspace)?,
        ResolvedTarget::Tab { workspace, tab } => {
            let pane = focused_pane_of_tab(shell, workspace, tab)?;
            (workspace, tab, pane)
        }
        ResolvedTarget::Pane {
            workspace,
            tab,
            pane,
        } => (workspace, tab, pane),
    };

    let before = shell.focus_checkpoint();
    if before.active_workspace == workspace
        && before.active_tab == tab
        && before.focused_pane == pane
    {
        // Already active: success no-op (R4.4).
        return Ok(target);
    }

    // Re-validate composition immediately before the write so a Tab/Pane
    // destroyed after resolve cannot partially apply (R4.2 / §12 item 11).
    if !shell.contains_workspace(workspace) {
        return Err(NavigationRejection::UnknownWorkspace);
    }
    let Some(tab_owner) = shell.workspace_of_tab(tab) else {
        return Err(NavigationRejection::UnknownTab);
    };
    let Some((pane_workspace, pane_tab)) = shell.location_of_pane(pane) else {
        return Err(NavigationRejection::UnknownPane);
    };
    if tab_owner != workspace
        || pane_workspace != workspace
        || pane_tab != tab
        || !shell.tab_contains_leaf(workspace, tab, pane)
    {
        return Err(NavigationRejection::NotComposed);
    }

    shell
        .commit_focus(workspace, tab, pane)
        .expect("composition re-validated immediately above");
    Ok(target)
}

fn focus_triple_for_workspace(
    shell: &ShellState,
    workspace: seyal_core::WorkspaceId,
) -> Result<
    (
        seyal_core::WorkspaceId,
        seyal_core::TabId,
        seyal_core::PaneId,
    ),
    NavigationRejection,
> {
    let checkpoint = shell
        .workspace_focus(workspace)
        .ok_or(NavigationRejection::UnknownWorkspace)?;
    Ok((workspace, checkpoint.active_tab, checkpoint.focused_pane))
}

fn focused_pane_of_tab(
    shell: &ShellState,
    workspace: seyal_core::WorkspaceId,
    tab: seyal_core::TabId,
) -> Result<seyal_core::PaneId, NavigationRejection> {
    shell
        .tab_focused_pane(workspace, tab)
        .ok_or(NavigationRejection::UnknownTab)
}
