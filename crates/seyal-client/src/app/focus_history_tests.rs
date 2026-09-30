//! ApplicationRoot focus-history wiring (SPEC-022 §6 / N3) and K8 bindings.

use seyal_core::{PaneId, TabId, WorkspaceId};

use super::*;
use crate::keybinding::{BindingContext, WorkspaceCommand, WorkspaceCommandId};
use crate::navigation::ResourceAddress;
use crate::shell::{ShellPaneSeed, ShellTabSeed, ShellWorkspaceSeed};

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
    let shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: w1,
            name: "A".into(),
            detail: None,
            attention: false,
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
        w1,
        true,
        true,
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
fn builtin_focus_history_back_and_forward_dispatch_committed_seq() {
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
    assert_eq!(root.snapshot().shell.focused_pane, p2);
    let seq_at_p2 = root
        .snapshot()
        .focus_history_seq
        .expect("cursor after navigate");

    // Dispatch reads FocusSeq from the committed snapshot (not a stale caller value).
    root.invoke_workspace_command(
        WorkspaceCommand {
            id: WorkspaceCommandId::FocusHistoryBack,
            ordinal: None,
        },
        BindingContext::APP,
    )
    .unwrap();
    assert_eq!(root.snapshot().shell.focused_pane, p1);
    assert_ne!(
        root.snapshot()
            .focus_history_seq
            .expect("cursor after back"),
        seq_at_p2
    );

    root.invoke_workspace_command(
        WorkspaceCommand {
            id: WorkspaceCommandId::FocusHistoryForward,
            ordinal: None,
        },
        BindingContext::APP,
    )
    .unwrap();
    assert_eq!(root.snapshot().shell.focused_pane, p2);

    // At the forward end → HistoryUnavailable (SPEC-022 rejection surfaces).
    assert_eq!(
        root.invoke_workspace_command(
            WorkspaceCommand {
                id: WorkspaceCommandId::FocusHistoryForward,
                ordinal: None,
            },
            BindingContext::APP,
        ),
        Err(AppError::NavigationHistoryUnavailable)
    );
    assert_eq!(root.snapshot().shell.focused_pane, p2);
}

#[test]
fn focus_history_binding_empty_history_is_action_unavailable() {
    let mut root = ApplicationRoot::new();
    assert!(root.snapshot().focus_history_seq.is_none());
    assert_eq!(
        root.invoke_workspace_command(
            WorkspaceCommand {
                id: WorkspaceCommandId::FocusHistoryBack,
                ordinal: None,
            },
            BindingContext::APP,
        ),
        Err(AppError::ActionUnavailable)
    );
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

#[test]
fn tab_select_next_previous_wrap_and_pane_focus_cycles_leaf_order() {
    let (mut root, _, t1, p1, p2) = two_pane_root();
    root.apply(AppAction::CreateTab).unwrap();
    let t_new = root.snapshot().shell.active_tab;
    assert_ne!(t_new, t1);
    // At last tab, next wraps to first.
    root.select_tab_relative(1).unwrap();
    assert_eq!(root.snapshot().shell.active_tab, t1);
    // Previous wraps back to last.
    root.select_tab_relative(-1).unwrap();
    assert_eq!(root.snapshot().shell.active_tab, t_new);

    root.apply(AppAction::SelectTab { id: t1 }).unwrap();
    root.apply(AppAction::FocusPane { id: p1 }).unwrap();
    root.focus_pane_relative(1).unwrap();
    assert_eq!(root.snapshot().shell.focused_pane, p2);
    root.focus_pane_relative(1).unwrap();
    assert_eq!(root.snapshot().shell.focused_pane, p1);
}
