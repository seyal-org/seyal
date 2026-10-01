//! SPEC-022 §6 / N3 integration: Navigate records history; Back/Forward apply-only.

use crate::shell::{ShellPaneSeed, ShellState, ShellTabSeed, ShellWorkspaceSeed};
use seyal_core::{PaneId, TabId, WorkspaceId};

use super::{
    history_back, history_forward, navigate, EmptyExecutionInventory, FocusHistory, FocusSeq,
    NavigateHistory, NavigationPrincipal, NavigationRejection, ResourceAddress, WorkspaceAccess,
};

fn workspace_b() -> WorkspaceId {
    WorkspaceId::from_bytes([0x22; 16])
}

fn seed_shell() -> (ShellState, WorkspaceId, WorkspaceId, TabId, PaneId) {
    let w1 = WorkspaceId::m001_default();
    let w2 = workspace_b();
    let t1 = TabId::new();
    let t2 = TabId::new();
    let p1 = PaneId::new();
    let p2 = PaneId::new();
    let t_other = TabId::new();
    let p_other = PaneId::new();
    let shell = ShellState::from_workspaces(
        vec![
            ShellWorkspaceSeed {
                id: w1,
                name: "Alpha".to_owned(),
                detail: Some("shared-label".to_owned()),
                attention: false,
                active_tab: t1,
                tabs: vec![
                    ShellTabSeed {
                        id: t1,
                        title: "Tab One".to_owned(),
                        attention: false,
                        pane: crate::shell::ShellPaneSeed {
                            id: p1,
                            title: "Pane A".to_owned(),
                            allows_implicit_execution_bootstrap: true,
                        },
                    },
                    ShellTabSeed {
                        id: t2,
                        title: "Tab Two".to_owned(),
                        attention: false,
                        pane: crate::shell::ShellPaneSeed {
                            id: p2,
                            title: "Pane B".to_owned(),
                            allows_implicit_execution_bootstrap: false,
                        },
                    },
                ],
            },
            ShellWorkspaceSeed {
                id: w2,
                name: "Alpha".to_owned(),
                detail: Some("shared-label".to_owned()),
                attention: false,
                active_tab: t_other,
                tabs: vec![ShellTabSeed {
                    id: t_other,
                    title: "Other".to_owned(),
                    attention: false,
                    pane: ShellPaneSeed {
                        id: p_other,
                        title: "Pane C".to_owned(),
                        allows_implicit_execution_bootstrap: false,
                    },
                }],
            },
        ],
        w1,
        true,
        true,
    )
    .expect("seed");
    (shell, w1, w2, t1, p1)
}

#[test]
fn navigate_records_history_and_back_forward_apply_without_append() {
    let (mut shell, w1, w2, t1, p1) = seed_shell();
    let mut history = FocusHistory::new();
    let w2_focus = shell.workspace_focus(w2).expect("w2");
    let p2_addr = ResourceAddress::Pane {
        workspace: w2,
        tab: w2_focus.active_tab,
        pane: w2_focus.focused_pane,
    };
    let p1_addr = ResourceAddress::Pane {
        workspace: w1,
        tab: t1,
        pane: p1,
    };

    navigate(
        p1_addr,
        &mut shell,
        &EmptyExecutionInventory,
        NavigationPrincipal::local_user(),
        NavigateHistory::Record(&mut history),
    )
    .expect("record p1");
    navigate(
        p2_addr,
        &mut shell,
        &EmptyExecutionInventory,
        NavigationPrincipal::local_user(),
        NavigateHistory::Record(&mut history),
    )
    .expect("record p2");
    assert_eq!(history.len(), 2);
    let head = history.cursor_seq().unwrap();

    history_back(
        head,
        &mut history,
        &mut shell,
        &EmptyExecutionInventory,
        NavigationPrincipal::local_user(),
    )
    .expect("back");
    assert_eq!(shell.snapshot().focused_pane, p1);
    assert_eq!(history.len(), 2, "traversal must not append");
    assert!(history.can_go_forward());

    let mid = history.cursor_seq().unwrap();
    history_forward(
        mid,
        &mut history,
        &mut shell,
        &EmptyExecutionInventory,
        NavigationPrincipal::local_user(),
    )
    .expect("forward");
    assert_eq!(shell.snapshot().focused_pane, w2_focus.focused_pane);
    assert_eq!(history.len(), 2);
}

#[test]
fn stale_history_cursor_rejects_without_focus_move() {
    let (mut shell, w1, w2, t1, p1) = seed_shell();
    let mut history = FocusHistory::new();
    let w2_focus = shell.workspace_focus(w2).expect("w2");
    navigate(
        ResourceAddress::Pane {
            workspace: w1,
            tab: t1,
            pane: p1,
        },
        &mut shell,
        &EmptyExecutionInventory,
        NavigationPrincipal::local_user(),
        NavigateHistory::Record(&mut history),
    )
    .unwrap();
    navigate(
        ResourceAddress::Pane {
            workspace: w2,
            tab: w2_focus.active_tab,
            pane: w2_focus.focused_pane,
        },
        &mut shell,
        &EmptyExecutionInventory,
        NavigationPrincipal::local_user(),
        NavigateHistory::Record(&mut history),
    )
    .unwrap();
    let focused_before = shell.snapshot().focused_pane;
    let seq = history.cursor_seq();
    let len = history.len();
    assert_eq!(
        history_back(
            FocusSeq::from_raw(1),
            &mut history,
            &mut shell,
            &EmptyExecutionInventory,
            NavigationPrincipal::local_user(),
        ),
        Err(NavigationRejection::StaleHistoryCursor)
    );
    assert_eq!(shell.snapshot().focused_pane, focused_before);
    assert_eq!(history.cursor_seq(), seq);
    assert_eq!(history.len(), len);
}

#[test]
fn back_applies_pane_address_without_stored_window() {
    // §12.23b: entries store no window; apply-time placement is the one window.
    let (mut shell, w1, w2, t1, p1) = seed_shell();
    let mut history = FocusHistory::new();
    let w2_focus = shell.workspace_focus(w2).expect("w2");
    navigate(
        ResourceAddress::Pane {
            workspace: w1,
            tab: t1,
            pane: p1,
        },
        &mut shell,
        &EmptyExecutionInventory,
        NavigationPrincipal::local_user(),
        NavigateHistory::Record(&mut history),
    )
    .unwrap();
    navigate(
        ResourceAddress::Pane {
            workspace: w2,
            tab: w2_focus.active_tab,
            pane: w2_focus.focused_pane,
        },
        &mut shell,
        &EmptyExecutionInventory,
        NavigationPrincipal::local_user(),
        NavigateHistory::Record(&mut history),
    )
    .unwrap();
    let entry = &history.entries()[0];
    assert!(matches!(entry.target, ResourceAddress::Pane { .. }));
    let head = history.cursor_seq().unwrap();
    history_back(
        head,
        &mut history,
        &mut shell,
        &EmptyExecutionInventory,
        NavigationPrincipal::local_user(),
    )
    .expect("back applies address in current placement");
    assert_eq!(shell.snapshot().active_workspace, w1);
    assert_eq!(shell.snapshot().focused_pane, p1);
}

fn targets(history: &FocusHistory) -> Vec<ResourceAddress> {
    history.entries().iter().map(|entry| entry.target).collect()
}

fn pane_addr(workspace: WorkspaceId, tab: TabId, pane: PaneId) -> ResourceAddress {
    ResourceAddress::Pane {
        workspace,
        tab,
        pane,
    }
}

/// History `[A, B, C]` with the shell focused on C. `B` does not resolve.
fn history_with_dead_middle() -> (
    ShellState,
    FocusHistory,
    ResourceAddress,
    ResourceAddress,
    ResourceAddress,
    PaneId,
) {
    let (shell, w1, _, t1, p1) = seed_shell();
    let t2 = shell
        .snapshot()
        .tabs
        .iter()
        .find(|tab| tab.id != t1)
        .expect("second tab")
        .id;
    let p2 = shell.tab_focused_pane(w1, t2).expect("pane A");
    let addr_a = pane_addr(w1, t2, p2);
    let addr_b = pane_addr(w1, t1, PaneId::from_bytes([0xab; 16]));
    let addr_c = pane_addr(w1, t1, p1);
    let mut history = FocusHistory::new();
    history.record_user_commit(addr_a);
    history.record_user_commit(addr_b);
    history.record_user_commit(addr_c);
    (shell, history, addr_a, addr_b, addr_c, p1)
}

#[test]
fn resolution_failure_on_back_keeps_cursor_on_focused_entry() {
    let (mut shell, mut history, addr_a, _, addr_c, p1) = history_with_dead_middle();
    assert_eq!(shell.snapshot().focused_pane, p1);
    let seq_c = history.cursor_seq().unwrap();
    assert_eq!(
        history_back(
            seq_c,
            &mut history,
            &mut shell,
            &EmptyExecutionInventory,
            NavigationPrincipal::local_user(),
        ),
        Err(NavigationRejection::UnknownPane)
    );
    assert_eq!(shell.snapshot().focused_pane, p1);
    assert_eq!(targets(&history), vec![addr_a, addr_c]);
    assert_eq!(history.cursor_target(), Some(addr_c));
    assert_eq!(history.cursor_index(), Some(1));
}

#[test]
fn resolution_failure_on_forward_keeps_cursor_on_focused_entry() {
    let (mut shell, mut history, addr_a, _, addr_c, _) = history_with_dead_middle();
    let t2_pane = match addr_a {
        ResourceAddress::Pane { tab, pane, .. } => (tab, pane),
        _ => unreachable!(),
    };
    shell
        .commit_focus(shell.snapshot().active_workspace, t2_pane.0, t2_pane.1)
        .expect("focus A");
    let head = history.cursor_seq().unwrap();
    history.prepare_back(head).unwrap();
    let on_dead = history.cursor_seq().unwrap();
    history.prepare_back(on_dead).unwrap();
    let seq_a = history.cursor_seq().unwrap();
    assert_eq!(history.cursor_target(), Some(addr_a));
    assert_eq!(
        history_forward(
            seq_a,
            &mut history,
            &mut shell,
            &EmptyExecutionInventory,
            NavigationPrincipal::local_user(),
        ),
        Err(NavigationRejection::UnknownPane)
    );
    assert_eq!(shell.snapshot().focused_pane, t2_pane.1);
    assert_eq!(targets(&history), vec![addr_a, addr_c]);
    assert_eq!(history.cursor_target(), Some(addr_a));
    assert_eq!(history.cursor_index(), Some(0));
}

#[test]
fn denied_traversal_restores_history_without_deleting() {
    let (mut shell, mut history, addr_a, addr_b, addr_c, p1) = history_with_dead_middle();
    let seq_c = history.cursor_seq().unwrap();
    let before = targets(&history);
    assert_eq!(before, vec![addr_a, addr_b, addr_c]);
    let denied = NavigationPrincipal {
        local_navigation: true,
        workspaces: WorkspaceAccess::Only(&[]),
    };
    assert_eq!(
        history_back(
            seq_c,
            &mut history,
            &mut shell,
            &EmptyExecutionInventory,
            denied,
        ),
        Err(NavigationRejection::NavigationDenied)
    );
    assert_eq!(shell.snapshot().focused_pane, p1);
    assert_eq!(targets(&history), before);
    assert_eq!(history.cursor_seq(), Some(seq_c));
    assert_eq!(history.len(), 3);
}

#[test]
fn history_unavailable_at_end_does_not_delete() {
    let (mut shell, mut history, addr_a, addr_b, addr_c, p1) = history_with_dead_middle();
    let head = history.cursor_seq().unwrap();
    history.prepare_back(head).unwrap();
    let on_dead = history.cursor_seq().unwrap();
    history.prepare_back(on_dead).unwrap();
    let seq_a = history.cursor_seq().unwrap();
    let before = targets(&history);
    assert_eq!(before, vec![addr_a, addr_b, addr_c]);
    assert_eq!(
        history_back(
            seq_a,
            &mut history,
            &mut shell,
            &EmptyExecutionInventory,
            NavigationPrincipal::local_user(),
        ),
        Err(NavigationRejection::HistoryUnavailable)
    );
    assert_eq!(shell.snapshot().focused_pane, p1);
    assert_eq!(targets(&history), before);
    assert_eq!(history.cursor_seq(), Some(seq_a));

    // Return the cursor to the head without apply; Forward is then unavailable.
    let mid = history.prepare_forward(seq_a).unwrap();
    assert_eq!(mid, addr_b);
    let on_b = history.cursor_seq().unwrap();
    let at_c = history.prepare_forward(on_b).unwrap();
    assert_eq!(at_c, addr_c);
    let seq_c = history.cursor_seq().unwrap();
    assert_eq!(
        history_forward(
            seq_c,
            &mut history,
            &mut shell,
            &EmptyExecutionInventory,
            NavigationPrincipal::local_user(),
        ),
        Err(NavigationRejection::HistoryUnavailable)
    );
    assert_eq!(targets(&history), before);
    assert_eq!(history.cursor_seq(), Some(seq_c));
    assert_eq!(shell.snapshot().focused_pane, p1);
}
