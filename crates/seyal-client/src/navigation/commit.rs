//! Atomic `Navigate(address)` commit (SPEC-022 §4 / §5) with optional history record.
//!
//! Resolve with the N1 resolver, then apply workspace/tab/pane focus in one
//! transition. When the target Tab lives in a non-active Window, emit exactly
//! one WindowActivation (`OrderFrontMakeKey`) naming that WindowId. User-initiated
//! commits record Pane-granularity history behind this path (R4.1 / R6.5);
//! traversal apply-only does not. Rejection leaves shell focus unchanged.
//! Commit never mutates presentation, bindings, or the PTY.

use crate::shell::ShellState;

use super::history::FocusHistory;
use super::{
    resolve, ExecutionInventory, NavigationPrincipal, NavigationRejection, ResolvedTarget,
    ResourceAddress,
};

/// Whether a successful Navigate should record focus history (SPEC-022 R6.5).
pub enum NavigateHistory<'a> {
    /// User-initiated commit: truncate/append per R6.4–R6.5 after focus apply.
    Record(&'a mut FocusHistory),
    /// Back/Forward apply step: focus/activation only; no history append.
    ApplyOnly,
}

/// Atomically navigate to `address`.
///
/// On success: activate owning Workspace, select owning Tab, set focused Pane
/// (SPEC-022 R4.1). When the Tab's Window is not product-active, emit one
/// WindowActivation effect (R5.2), then optionally record history. On any
/// failure: no focus fields change (R4.2). Already-active targets succeed as a
/// no-op for focus and still run cursor-equality history dedup (R4.4 / R6.4).
/// Never mutates presentation, bindings, or PTY state (R4.3). Never reparents
/// a Tab (R5.3).
pub fn navigate(
    address: ResourceAddress,
    shell: &mut ShellState,
    inventory: &impl ExecutionInventory,
    principal: NavigationPrincipal<'_>,
    history: NavigateHistory<'_>,
) -> Result<ResolvedTarget, NavigationRejection> {
    let target = resolve(address, shell, inventory, principal)?;
    let (workspace, tab, pane) = match target {
        ResolvedTarget::Workspace { workspace } => {
            focus_triple_for_workspace(shell, workspace)?
        }
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

    let pane_address = ResourceAddress::Pane {
        workspace,
        tab,
        pane,
    };

    let before = shell.focus_checkpoint();
    if before.active_workspace == workspace
        && before.active_tab == tab
        && before.focused_pane == pane
    {
        // Already active: success no-op for focus (R4.4). No activation effect.
        // History still sees the commit so cursor-equality dedup applies (R6.4).
        record_if_needed(history, pane_address);
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

    let prior_window = shell.active_window_id();
    let target_window = shell
        .window_of_tab(tab)
        .ok_or(NavigationRejection::UnknownTab)?;

    shell
        .commit_focus(workspace, tab, pane)
        .expect("composition re-validated immediately above");

    if target_window != prior_window {
        // SPEC-022 R5.2: exactly one WindowActivation naming Rust's placement.
        shell
            .emit_window_activation(target_window)
            .expect("target window re-validated via window_of_tab");
    }
    record_if_needed(history, pane_address);
    Ok(target)
}

/// Back one history entry, then apply-only Navigate (R6.5 / R6.8 / R6.9).
pub fn history_back(
    observed: super::history::FocusSeq,
    history: &mut FocusHistory,
    shell: &mut ShellState,
    inventory: &impl ExecutionInventory,
    principal: NavigationPrincipal<'_>,
) -> Result<ResolvedTarget, NavigationRejection> {
    let target = history.prepare_back(observed)?;
    match navigate(
        target,
        shell,
        inventory,
        principal,
        NavigateHistory::ApplyOnly,
    ) {
        Ok(resolved) => Ok(resolved),
        Err(error) => {
            history.remove_cursor_entry();
            Err(error)
        }
    }
}

/// Forward one history entry, then apply-only Navigate (R6.5 / R6.8 / R6.9).
pub fn history_forward(
    observed: super::history::FocusSeq,
    history: &mut FocusHistory,
    shell: &mut ShellState,
    inventory: &impl ExecutionInventory,
    principal: NavigationPrincipal<'_>,
) -> Result<ResolvedTarget, NavigationRejection> {
    let target = history.prepare_forward(observed)?;
    match navigate(
        target,
        shell,
        inventory,
        principal,
        NavigateHistory::ApplyOnly,
    ) {
        Ok(resolved) => Ok(resolved),
        Err(error) => {
            history.remove_cursor_entry();
            Err(error)
        }
    }
}

fn record_if_needed(history: NavigateHistory<'_>, pane_address: ResourceAddress) {
    if let NavigateHistory::Record(store) = history {
        store.record_user_commit(pane_address);
    }
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
