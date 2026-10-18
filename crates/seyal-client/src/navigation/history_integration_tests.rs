//! SPEC-022 §6 / N3 integration: Navigate records history; Back/Forward apply-only.

use crate::shell::{ShellPaneSeed, ShellState, ShellTabSeed, ShellWindowSeed, ShellWorkspaceSeed};
use seyal_core::{PaneId, TabId, WindowId, WorkspaceId};

use super::{
    history_back, history_forward, navigate, EmptyExecutionInventory, FocusHistory, FocusSeq,
    NavigateHistory, NavigationPrincipal, NavigationRejection, ResourceAddress,
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
    let win1 = WindowId::new();
    let win2 = WindowId::new();
    let shell = ShellState::from_workspaces(
        vec![
            ShellWorkspaceSeed {
                id: w1,
                name: "Alpha".to_owned(),
                detail: Some("shared-label".to_owned()),
                attention: false,
                active_window: win1,
                windows: vec![ShellWindowSeed {
                    id: win1,
                    active_tab: t1,
                    tabs: vec![
                        ShellTabSeed {
                            id: t1,
                            title: "Tab One".to_owned(),
                            attention: false,
                            pane: ShellPaneSeed {
                                id: p1,
                                title: "Pane A".to_owned(),
                                allows_implicit_execution_bootstrap: true,
                            },
                        },
                        ShellTabSeed {
                            id: t2,
                            title: "Tab Two".to_owned(),
                            attention: false,
                            pane: ShellPaneSeed {
                                id: p2,
                                title: "Pane B".to_owned(),
                                allows_implicit_execution_bootstrap: false,
                            },
                        },
                    ],
                }],
            },
            ShellWorkspaceSeed {
                id: w2,
                name: "Alpha".to_owned(),
                detail: Some("shared-label".to_owned()),
                attention: false,
                active_window: win2,
                windows: vec![ShellWindowSeed {
                    id: win2,
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
                }],
            },
        ],
        w1,
        true,
        true,
        false,
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

#[test]
fn rejected_back_leaves_history_entries_cursor_and_seq_unchanged() {
    // SPEC-022 §12.22a / R6.9: rejection must not mutate history, including
    // the dead history entry. Cursor moves only after successful navigate.
    let (mut shell, w1, _, t1, p1) = seed_shell();
    let mut history = FocusHistory::new();
    history.record_user_commit(ResourceAddress::Pane {
        workspace: w1,
        tab: t1,
        pane: PaneId::from_bytes([0xcd; 16]),
    });
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
    let before_len = history.len();
    let before_cursor = history.cursor_index();
    let before_seq = history.cursor_seq();
    let entries_before: Vec<_> = history
        .entries()
        .iter()
        .map(|e| (e.seq, e.target))
        .collect();
    let focused_before = shell.focus_checkpoint();
    let head = history.cursor_seq().unwrap();
    assert_eq!(
        history_back(
            head,
            &mut history,
            &mut shell,
            &EmptyExecutionInventory,
            NavigationPrincipal::local_user(),
        ),
        Err(NavigationRejection::UnknownPane)
    );
    assert_eq!(history.len(), before_len);
    assert_eq!(history.cursor_index(), before_cursor);
    assert_eq!(history.cursor_seq(), before_seq);
    let entries_after: Vec<_> = history
        .entries()
        .iter()
        .map(|e| (e.seq, e.target))
        .collect();
    assert_eq!(entries_after, entries_before);
    assert_eq!(shell.focus_checkpoint(), focused_before);
}
