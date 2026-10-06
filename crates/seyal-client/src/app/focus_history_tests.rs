//! ApplicationRoot focus-history wiring (SPEC-022 §6 / N3).

use seyal_core::{PaneId, TabId, WindowId, WorkspaceId};

use super::*;
use crate::navigation::{FocusSeq, ResourceAddress};
use crate::shell::{ShellPaneSeed, ShellTabSeed, ShellWindowSeed, ShellWorkspaceSeed};

fn evidence(pty_generation: u64, controller: bool, alternate_screen: bool) -> BindingEvidence {
    BindingEvidence {
        execution: ExecutionId::from_bytes([0x11; 16]),
        attachment: AttachmentId::from_bytes([0x12; 16]),
        controller,
        pty_generation,
        alternate_screen,
    }
}

fn two_pane_root() -> (ApplicationRoot, WorkspaceId, TabId, PaneId, PaneId) {
    let w1 = WorkspaceId::m001_default();
    let t1 = TabId::from_bytes([0x01; 16]);
    let p1 = PaneId::from_bytes([0x03; 16]);
    let win1 = WindowId::from_bytes([0x02; 16]);
    let shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: w1,
            name: "A".into(),
            detail: None,
            attention: false,
            active_window: win1,
            windows: vec![ShellWindowSeed {
                id: win1,
                active_tab: t1,
                tabs: vec![ShellTabSeed {
                    id: t1,
                    title: "T1".into(),
                    attention: false,
                    pane: ShellPaneSeed {
                        id: p1,
                        title: "P1".into(),
                        allows_implicit_execution_bootstrap: true,
                    },
                }],
            }],
        }],
        w1,
        true,
        true,
        false,
    )
    .expect("fixture");
    let mut root = ApplicationRoot::with_shell(shell);
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: evidence(1, true, false),
    })
    .unwrap();
    root.apply(AppAction::SplitFocused {
        axis: SplitAxis::Right,
    })
    .unwrap();
    let p2_live = root.snapshot().shell.focused_pane;
    assert_ne!(p2_live, p1);
    // Refocus p1 so Navigate to p2 is a real transition.
    root.apply(AppAction::FocusPane { id: p1 }).unwrap();
    (root, w1, t1, p1, p2_live)
}

#[test]
fn navigate_records_and_back_moves_focus() {
    let (mut root, w1, t1, p1, p2) = two_pane_root();
    root.apply(AppAction::Navigate {
        fence: root.fence(),
        address: ResourceAddress::Pane {
            workspace: w1,
            tab: t1,
            pane: p1,
        },
    })
    .unwrap();
    root.apply(AppAction::Navigate {
        fence: root.fence(),
        address: ResourceAddress::Pane {
            workspace: w1,
            tab: t1,
            pane: p2,
        },
    })
    .unwrap();
    let seq = root.snapshot().focus_history_seq.expect("cursor seq");
    root.apply(AppAction::HistoryBack {
        fence: root.fence(),
        observed: seq,
    })
    .unwrap();
    assert_eq!(root.snapshot().shell.focused_pane, p1);
}

#[test]
fn select_tab_and_focus_pane_record_focus_history() {
    let (mut root, w1, t1, p1, p2) = two_pane_root();
    // two_pane_root already FocusPane'd p1 (recorded). Navigate/focus to p2.
    root.apply(AppAction::FocusPane { id: p2 }).unwrap();
    assert_eq!(root.snapshot().shell.focused_pane, p2);
    let seq = root.snapshot().focus_history_seq.expect("p2 recorded");
    root.apply(AppAction::HistoryBack {
        fence: root.fence(),
        observed: seq,
    })
    .unwrap();
    assert_eq!(root.snapshot().shell.focused_pane, p1);

    root.apply(AppAction::CreateTab).unwrap();
    let t_new = root.snapshot().shell.active_tab;
    assert_ne!(t_new, t1);
    root.apply(AppAction::SelectTab { id: t1 }).unwrap();
    assert_eq!(root.snapshot().shell.active_tab, t1);
    assert_eq!(root.snapshot().shell.focused_pane, p1);
    assert!(root.snapshot().focus_history_seq.is_some());
    let _ = w1;
}

#[test]
fn stale_history_back_rejects_without_focus_move() {
    let (mut root, w1, t1, p1, p2) = two_pane_root();
    root.apply(AppAction::Navigate {
        fence: root.fence(),
        address: ResourceAddress::Pane {
            workspace: w1,
            tab: t1,
            pane: p1,
        },
    })
    .unwrap();
    root.apply(AppAction::Navigate {
        fence: root.fence(),
        address: ResourceAddress::Pane {
            workspace: w1,
            tab: t1,
            pane: p2,
        },
    })
    .unwrap();
    let focused = root.snapshot().shell.focused_pane;
    assert_eq!(
        root.apply(AppAction::HistoryBack {
            fence: root.fence(),
            observed: FocusSeq::from_raw(999),
        }),
        Err(AppError::NavigationStaleHistoryCursor)
    );
    assert_eq!(root.snapshot().shell.focused_pane, focused);
}

#[test]
fn close_focused_pane_purges_then_commits_successor() {
    let (mut root, w1, t1, p1, p2) = two_pane_root();
    root.apply(AppAction::Navigate {
        fence: root.fence(),
        address: ResourceAddress::Pane {
            workspace: w1,
            tab: t1,
            pane: p1,
        },
    })
    .unwrap();
    root.apply(AppAction::Navigate {
        fence: root.fence(),
        address: ResourceAddress::Pane {
            workspace: w1,
            tab: t1,
            pane: p2,
        },
    })
    .unwrap();
    // Close focused p2: R6.7a purge → reposition → successor commit (p1).
    root.apply(AppAction::ClosePane { id: p2 }).unwrap();
    assert_eq!(root.snapshot().shell.focused_pane, p1);
    // Destroyed p2 is gone before any Back; Back from sole surviving entry is unavailable.
    let seq = root
        .snapshot()
        .focus_history_seq
        .expect("successor recorded");
    assert_eq!(
        root.apply(AppAction::HistoryBack {
            fence: root.fence(),
            observed: seq,
        }),
        Err(AppError::NavigationHistoryUnavailable)
    );
    assert_eq!(root.snapshot().shell.focused_pane, p1);
}
