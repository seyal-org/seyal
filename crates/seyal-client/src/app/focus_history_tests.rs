//! ApplicationRoot focus-history wiring (SPEC-022 §6 / N3).

use seyal_core::{PaneId, TabId, WindowId, WorkspaceId};

use super::*;
use crate::keybinding::{WorkspaceCommand, WorkspaceCommandId};
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

#[test]
fn failed_split_rolls_back_its_history_entry_with_destroyed_pane() {
    let (root_with_history, _workspace, _tab, _p1, _p2) = two_pane_root();
    // Keep the same shell layout while exercising rollback with an empty
    // history, so R6.7a's restored-successor commit is observable.
    let mut root = ApplicationRoot::with_shell(root_with_history.shell.clone());
    root.provisioning_mut().seed_next_request_id(u64::MAX);

    assert!(root
        .apply(AppAction::SplitFocused {
            axis: SplitAxis::Right,
        })
        .is_err());

    let surviving_panes = root
        .snapshot()
        .shell
        .panes
        .into_iter()
        .map(|pane| pane.id)
        .collect::<std::collections::HashSet<_>>();
    assert!(root
        .focus_history
        .entries()
        .iter()
        .all(|entry| match entry.target {
            ResourceAddress::Pane { pane, .. } => surviving_panes.contains(&pane),
            _ => false,
        }));
    // R6.7a: the restored focus is committed after purging the transient leaf.
    let focus = root.shell.focus_checkpoint();
    assert_eq!(
        root.focus_history.cursor_target(),
        Some(ResourceAddress::Pane {
            workspace: focus.active_workspace,
            tab: focus.active_tab,
            pane: focus.focused_pane,
        })
    );
}

#[test]
fn successful_split_records_new_focused_leaf() {
    let (mut root, workspace, tab, _p1, _p2) = two_pane_root();
    root.apply(AppAction::SplitFocused {
        axis: SplitAxis::Right,
    })
    .unwrap();
    let pane = root.snapshot().shell.focused_pane;
    assert_eq!(
        root.focus_history.cursor_target(),
        Some(ResourceAddress::Pane {
            workspace,
            tab,
            pane
        })
    );
}

#[test]
fn open_attention_reveal_records_exact_target_in_history() {
    let (mut root, workspace, tab, p1, p2) = two_pane_root();
    let id = AttentionId::new("history-reveal");
    let mut item = crate::chrome::AttentionItem::projection(
        id.clone(),
        "Attention",
        "target pane",
        Some(workspace),
        Some(tab),
        None,
    );
    item.resource_address = Some(crate::navigation::pack_resource_address(
        ResourceAddress::Pane {
            workspace,
            tab,
            pane: p2,
        },
    ));
    root.apply(AppAction::ReplaceChrome {
        fence: root.fence(),
        agents: vec![],
        attention: vec![item],
    })
    .unwrap();
    root.apply(AppAction::OpenAttention {
        fence: root.fence(),
        id,
    })
    .unwrap();
    assert_eq!(root.snapshot().shell.focused_pane, p2);
    assert_eq!(
        root.focus_history.cursor_target(),
        Some(ResourceAddress::Pane {
            workspace,
            tab,
            pane: p2,
        })
    );
    let seq = root.snapshot().focus_history_seq.unwrap();
    root.apply(AppAction::HistoryBack {
        fence: root.fence(),
        observed: seq,
    })
    .unwrap();
    assert_eq!(root.snapshot().shell.focused_pane, p1);
}

fn evidence_exec(tag: u8) -> BindingEvidence {
    BindingEvidence {
        execution: ExecutionId::from_bytes([tag; 16]),
        attachment: AttachmentId::from_bytes([tag.wrapping_add(1); 16]),
        controller: true,
        pty_generation: 1,
        alternate_screen: false,
    }
}

fn two_tab_bound_root() -> (
    ApplicationRoot,
    TabId,
    TabId,
    PaneId,
    PaneId,
    ExecutionId,
    ExecutionId,
) {
    let w1 = WorkspaceId::m001_default();
    let t1 = TabId::from_bytes([0x01; 16]);
    let t2 = TabId::from_bytes([0x11; 16]);
    let p1 = PaneId::from_bytes([0x03; 16]);
    let p2 = PaneId::from_bytes([0x13; 16]);
    let win1 = WindowId::from_bytes([0x02; 16]);
    let exec1 = ExecutionId::from_bytes([0x21; 16]);
    let exec2 = ExecutionId::from_bytes([0x22; 16]);
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
                tabs: vec![
                    ShellTabSeed {
                        id: t1,
                        title: "T1".into(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: p1,
                            title: "P1".into(),
                            allows_implicit_execution_bootstrap: true,
                        },
                    },
                    ShellTabSeed {
                        id: t2,
                        title: "T2".into(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: p2,
                            title: "P2".into(),
                            allows_implicit_execution_bootstrap: true,
                        },
                    },
                ],
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
        evidence: BindingEvidence {
            execution: exec1,
            attachment: AttachmentId::from_bytes([0x31; 16]),
            controller: true,
            pty_generation: 1,
            alternate_screen: false,
        },
    })
    .unwrap();
    root.apply(AppAction::SelectTab { id: t2 }).unwrap();
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: BindingEvidence {
            execution: exec2,
            attachment: AttachmentId::from_bytes([0x32; 16]),
            controller: true,
            pty_generation: 1,
            alternate_screen: false,
        },
    })
    .unwrap();
    root.apply(AppAction::SelectTab { id: t1 }).unwrap();
    assert_eq!(root.snapshot().shell.focused_pane, p1);
    assert_eq!(root.snapshot().execution, Some(exec1));
    (root, t1, t2, p1, p2, exec1, exec2)
}

#[test]
fn open_attention_packed_reveal_activates_focused_tab_authority() {
    let (mut root, _t1, t2, _p1, p2, _exec1, exec2) = two_tab_bound_root();
    let workspace = root.snapshot().shell.active_workspace;
    let id = AttentionId::new("packed-authority-target");
    let mut item = crate::chrome::AttentionItem::projection(
        id.clone(),
        "Attention",
        "target execution",
        Some(workspace),
        Some(t2),
        None,
    );
    item.resource_address = Some(crate::navigation::pack_resource_address(
        ResourceAddress::Pane {
            workspace,
            tab: t2,
            pane: p2,
        },
    ));
    root.apply(AppAction::ReplaceChrome {
        fence: root.fence(),
        agents: vec![],
        attention: vec![item],
    })
    .unwrap();

    root.apply(AppAction::OpenAttention {
        fence: root.fence(),
        id,
    })
    .unwrap();

    assert_eq!(root.snapshot().shell.focused_pane, p2);
    assert_eq!(root.snapshot().execution, Some(exec2));
}

#[test]
fn open_attention_fallback_tab_focus_activates_focused_tab_authority() {
    let (mut root, _t1, t2, _p1, p2, _exec1, exec2) = two_tab_bound_root();
    let workspace = root.snapshot().shell.active_workspace;
    let id = AttentionId::new("fallback-authority-target");
    let item = crate::chrome::AttentionItem::projection(
        id.clone(),
        "Attention",
        "target execution",
        Some(workspace),
        Some(t2),
        None,
    );
    root.apply(AppAction::ReplaceChrome {
        fence: root.fence(),
        agents: vec![],
        attention: vec![item],
    })
    .unwrap();

    root.apply(AppAction::OpenAttention {
        fence: root.fence(),
        id,
    })
    .unwrap();

    assert_eq!(root.snapshot().shell.focused_pane, p2);
    assert_eq!(root.snapshot().execution, Some(exec2));
}

#[test]
fn history_back_and_forward_move_input_authority_with_tab_focus() {
    let (mut root, _t1, _t2, p1, p2, exec1, exec2) = two_tab_bound_root();
    let seq = root.snapshot().focus_history_seq.expect("cursor seq");
    root.apply(AppAction::HistoryBack {
        fence: root.fence(),
        observed: seq,
    })
    .unwrap();
    assert_eq!(root.snapshot().shell.focused_pane, p2);
    assert_eq!(root.snapshot().execution, Some(exec2));
    let seq = root.snapshot().focus_history_seq.expect("after back");
    root.apply(AppAction::HistoryForward {
        fence: root.fence(),
        observed: seq,
    })
    .unwrap();
    assert_eq!(root.snapshot().shell.focused_pane, p1);
    assert_eq!(root.snapshot().execution, Some(exec1));
}

#[test]
fn history_traverses_unbound_pane_without_mutating_sibling_binding() {
    let w1 = WorkspaceId::m001_default();
    let t1 = TabId::from_bytes([0x01; 16]);
    let t2 = TabId::from_bytes([0x11; 16]);
    let p1 = PaneId::from_bytes([0x03; 16]);
    let p2 = PaneId::from_bytes([0x13; 16]);
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
                tabs: vec![
                    ShellTabSeed {
                        id: t1,
                        title: "T1".into(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: p1,
                            title: "P1".into(),
                            allows_implicit_execution_bootstrap: true,
                        },
                    },
                    ShellTabSeed {
                        id: t2,
                        title: "T2".into(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: p2,
                            title: "P2".into(),
                            allows_implicit_execution_bootstrap: true,
                        },
                    },
                ],
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
        evidence: evidence_exec(0x21),
    })
    .unwrap();
    let bound_execution = ExecutionId::from_bytes([0x21; 16]);
    let bound_authority = root.pane_authorities.get(&p1).copied();
    assert!(bound_authority.is_some());
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
            tab: t2,
            pane: p2,
        },
    })
    .unwrap();
    assert_eq!(root.snapshot().shell.focused_pane, p2);
    assert!(root.snapshot().execution.is_none());
    assert!(root.pane_regions().iter().all(|region| !region.live));
    assert_eq!(
        root.apply(AppAction::SubmitInput {
            fence: root.fence(),
            text: "must fail closed".into(),
        }),
        Err(AppError::UnboundUnauthorized)
    );
    assert_eq!(root.pane_authorities.get(&p1).copied(), bound_authority);
    assert!(root.pane_authorities.contains_key(&p1));

    // Back/Forward traverse from the unbound Pane to its bound sibling and
    // back, preserving the sibling attachment throughout.
    let seq = root.snapshot().focus_history_seq.expect("cursor seq");
    root.apply(AppAction::HistoryBack {
        fence: root.fence(),
        observed: seq,
    })
    .unwrap();
    assert_eq!(root.snapshot().shell.focused_pane, p1);
    assert_eq!(root.snapshot().execution, Some(bound_execution));
    assert_eq!(root.pane_authorities.get(&p1).copied(), bound_authority);

    let seq = root.snapshot().focus_history_seq.expect("cursor seq");
    root.apply(AppAction::HistoryForward {
        fence: root.fence(),
        observed: seq,
    })
    .unwrap();
    assert_eq!(root.snapshot().shell.focused_pane, p2);
    assert!(root.snapshot().execution.is_none());
    assert_eq!(root.pane_authorities.get(&p1).copied(), bound_authority);
    assert_eq!(
        root.apply(AppAction::SubmitInput {
            fence: root.fence(),
            text: "must fail closed".into(),
        }),
        Err(AppError::UnboundUnauthorized)
    );
}

#[test]
fn workspace_command_history_back_uses_committed_cursor_seq() {
    let (mut root, _t1, _t2, _p1, p2, _exec1, exec2) = two_tab_bound_root();
    let observed = root.snapshot().focus_history_seq.expect("cursor seq");
    let route = root.keybinding_route_context(false);
    root.invoke_workspace_command(
        WorkspaceCommand {
            id: WorkspaceCommandId::FocusHistoryBack,
            ordinal: None,
        },
        route,
    )
    .expect("focus_history.back");
    assert_eq!(root.snapshot().shell.focused_pane, p2);
    assert_eq!(root.snapshot().execution, Some(exec2));
    assert_ne!(root.snapshot().focus_history_seq, Some(observed));
}

#[test]
fn focus_pane_onto_unbound_leaf_clears_authority() {
    let (mut root, _w1, _t1, p1, p2) = two_pane_root();
    assert_eq!(root.snapshot().shell.focused_pane, p1);
    assert!(root.snapshot().execution.is_some());
    let sibling_authority = root.pane_authorities.get(&p1).copied();
    root.apply(AppAction::FocusPane { id: p2 }).unwrap();
    assert_eq!(root.snapshot().shell.focused_pane, p2);
    assert!(root.snapshot().execution.is_none());
    assert_eq!(root.pane_authorities.get(&p1).copied(), sibling_authority);
    assert_eq!(
        root.apply(AppAction::SubmitInput {
            fence: root.fence(),
            text: "must fail closed".into(),
        }),
        Err(AppError::UnboundUnauthorized)
    );
    root.apply(AppAction::FocusPane { id: p1 }).unwrap();
    assert!(root.snapshot().execution.is_some());
}

#[test]
fn split_focus_on_unbound_leaf_fails_closed_and_retains_sibling_binding() {
    let (mut root, _workspace, _tab, p1, _p2) = two_pane_root();
    let sibling_authority = root.pane_authorities.get(&p1).copied();
    root.apply(AppAction::SplitFocused {
        axis: SplitAxis::Right,
    })
    .unwrap();
    let focused = root.snapshot().shell.focused_pane;
    assert_ne!(focused, p1);
    assert_eq!(root.fence().pane, focused);
    assert!(root.snapshot().execution.is_none());
    assert!(root.pane_regions().iter().all(|region| !region.live));
    assert_eq!(root.pane_authorities.get(&p1).copied(), sibling_authority);
    assert_eq!(
        root.apply(AppAction::SubmitInput {
            fence: root.fence(),
            text: "must fail closed".into(),
        }),
        Err(AppError::UnboundUnauthorized)
    );
    root.apply(AppAction::FocusPane { id: p1 }).unwrap();
    assert!(root.snapshot().execution.is_some());
}

#[test]
fn closing_unbound_focused_pane_restores_retained_sibling_authority() {
    let (mut root, _workspace, _tab, p1, _p2) = two_pane_root();
    let sibling_authority = root.pane_authorities.get(&p1).copied();
    let sibling_execution = root.snapshot().execution.expect("bound sibling");
    root.apply(AppAction::SplitFocused {
        axis: SplitAxis::Right,
    })
    .unwrap();
    let unbound = root.snapshot().shell.focused_pane;
    assert!(root.snapshot().execution.is_none());

    root.apply(AppAction::ClosePane { id: unbound }).unwrap();

    assert_eq!(root.snapshot().shell.focused_pane, p1);
    assert_eq!(root.snapshot().execution, Some(sibling_execution));
    assert_eq!(root.authority, sibling_authority);
    assert_eq!(root.pane_authorities.get(&p1).copied(), sibling_authority);
}

#[test]
fn selecting_bound_tab_restores_its_portable_input_authority() {
    let (mut root, _t1, t2, _p1, _p2, _exec1, exec2) = two_tab_bound_root();
    root.apply(AppAction::SelectTab { id: t2 }).unwrap();
    assert_eq!(root.snapshot().execution, Some(exec2));
}

#[test]
fn history_back_ensures_composer_for_reactivated_bound_pane() {
    let (mut root, _t1, _t2, _p1, p2, _exec1, _exec2) = two_tab_bound_root();
    // Model a pane whose product composer projection has not yet been
    // materialized, while its binding already matches the history target.
    root.composer = crate::composer::ComposerState::new();
    root.authority = root.pane_authorities.get(&p2).copied();
    let observed = root.snapshot().focus_history_seq.expect("cursor seq");

    root.apply(AppAction::HistoryBack {
        fence: root.fence(),
        observed,
    })
    .unwrap();

    assert_eq!(root.snapshot().shell.focused_pane, p2);
    assert!(root.snapshot().composer.is_some());
}
