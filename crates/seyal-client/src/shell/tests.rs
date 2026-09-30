use super::*;
use seyal_core::WindowId;

fn other_workspace() -> WorkspaceId {
    WorkspaceId::from_bytes([0x11; 16])
}

fn seed_two_workspaces() -> ShellState {
    let first_tab = TabId::new();
    let first_pane = PaneId::new();
    let second_tab = TabId::new();
    let second_pane = PaneId::new();
    let first = WorkspaceId::m001_default();
    let second = other_workspace();
    let first_window = WindowId::new();
    let second_window = WindowId::new();
    ShellState::from_workspaces(
        vec![
            ShellWorkspaceSeed {
                id: first,
                name: "Seyal OSS".to_owned(),
                detail: Some("~/Projects/seyal".to_owned()),
                attention: false,
                active_window: first_window,
                windows: vec![ShellWindowSeed {
                    id: first_window,
                    active_tab: first_tab,
                    tabs: vec![ShellTabSeed {
                        id: first_tab,
                        title: "Core Terminal".to_owned(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: first_pane,
                            title: "Pane 1".to_owned(),
                            allows_implicit_execution_bootstrap: true,
                        },
                    }],
                }],
            },
            ShellWorkspaceSeed {
                id: second,
                name: "Payments".to_owned(),
                detail: Some("~/Projects/payments".to_owned()),
                attention: true,
                active_window: second_window,
                windows: vec![ShellWindowSeed {
                    id: second_window,
                    active_tab: second_tab,
                    tabs: vec![ShellTabSeed {
                        id: second_tab,
                        title: "API".to_owned(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: second_pane,
                            title: "Pane 1".to_owned(),
                            allows_implicit_execution_bootstrap: false,
                        },
                    }],
                }],
            },
        ],
        first,
        true,
        true,
    )
    .expect("fixture")
}

fn create_tab(shell: &mut ShellState) -> Result<(), ShellError> {
    let snap = shell.snapshot();
    shell.apply(ShellAction::CreateTab {
        window: snap.active_window.expect("active window"),
        containment_generation: snap.containment_generation,
    })
}

fn activate_workspace(shell: &mut ShellState, id: WorkspaceId) -> Result<(), ShellError> {
    let generation = shell.containment_generation();
    shell.apply(ShellAction::ActivateWorkspace {
        workspace: id,
        containment_generation: generation,
    })
}

fn assert_rejection_atomic(shell: &ShellState, before: &ShellState) {
    assert_eq!(
        shell.containment_fingerprint(),
        before.containment_fingerprint()
    );
}

#[test]
fn production_shell_is_single_pane_and_fail_closed() {
    let mut shell = ShellState::m001_local("/tmp/seyal");
    let snap = shell.snapshot();
    assert_eq!(snap.workspaces.len(), 1);
    assert_eq!(snap.workspaces[0].id, WorkspaceId::m001_default());
    assert_eq!(snap.tabs.len(), 1);
    assert_eq!(snap.layout, LayoutDescription::Single);
    assert_eq!(snap.panes.len(), 1);
    assert_eq!(snap.panes[0].title, "Pane 1");
    assert!(snap.panes[0].allows_implicit_bootstrap);
    // W4b: production admits tab creation; pane splitting stays fail-closed.
    assert!(shell.allows_tab_creation());
    assert!(!shell.allows_pane_splitting());
    // Hierarchical close is always admitted while a Window exists (W2b).
    assert!(snap.allows_tab_close);
    assert!(snap.allows_pane_close);
    create_tab(&mut shell).expect("W4b admits CreateTab");
    assert_eq!(shell.snapshot().tabs.len(), 2);
    let focused = shell.snapshot().focused_pane;
    assert_eq!(
        shell.apply(ShellAction::SplitPane {
            id: focused,
            axis: SplitAxis::Right
        }),
        Err(ShellError::PaneSplitUnavailable)
    );
    // Rejected split leaves tab inventory and single-pane layout unchanged.
    assert_eq!(shell.snapshot().tabs.len(), 2);
    assert_eq!(shell.snapshot().layout, LayoutDescription::Single);
}

#[test]
fn workspace_activation_switches_tab_inventory() {
    let mut shell = seed_two_workspaces();
    let first = shell.snapshot();
    activate_workspace(&mut shell, other_workspace()).expect("activate second");
    let second = shell.snapshot();
    assert_eq!(second.active_workspace, other_workspace());
    assert_eq!(second.tabs[0].title, "API");
    assert_ne!(second.active_tab, first.active_tab);
    assert!(second.workspaces[1].attention);
}

#[test]
fn stale_identities_fail_closed_and_leave_state() {
    let mut shell = seed_two_workspaces();
    let before = shell.containment_fingerprint();
    assert_eq!(
        activate_workspace(&mut shell, WorkspaceId::from_bytes([0xff; 16])),
        Err(ShellError::UnknownWorkspace)
    );
    assert_rejection_atomic(&shell, &before);
    assert_eq!(
        shell.apply(ShellAction::SelectTab { id: TabId::new() }),
        Err(ShellError::UnknownTab)
    );
    assert_eq!(
        shell.apply(ShellAction::FocusPane { id: PaneId::new() }),
        Err(ShellError::UnknownPane)
    );
    assert_eq!(
        shell.snapshot().active_workspace,
        before.snapshot().active_workspace
    );
    assert_eq!(shell.snapshot().active_tab, before.snapshot().active_tab);
    assert_eq!(
        shell.snapshot().focused_pane,
        before.snapshot().focused_pane
    );
}

#[test]
fn execution_bind_is_one_shot_and_does_not_own_pty() {
    let mut shell = ShellState::m001_local(".");
    let pane = shell.snapshot().focused_pane;
    let execution = ExecutionId::new();
    shell
        .apply(ShellAction::BindExecution { pane, execution })
        .expect("bind");
    assert_eq!(shell.pane_execution(pane).unwrap(), Some(execution));
    assert_eq!(
        shell.apply(ShellAction::BindExecution {
            pane,
            execution: ExecutionId::new()
        }),
        Err(ShellError::ExecutionAlreadyBound)
    );
    assert_eq!(shell.pane_execution(pane).unwrap(), Some(execution));
    assert!(shell.focused_pane_allows_implicit_bootstrap());
}

#[test]
fn bind_rejects_second_leaf_for_same_execution() {
    let mut shell = seed_two_workspaces();
    create_tab(&mut shell).expect("tab");
    let first_pane = shell.snapshot().panes[0].id;
    // Select original tab's pane via creating then selecting.
    let tabs = shell.snapshot().tabs;
    let original_tab = tabs[0].id;
    let created_tab = tabs[1].id;
    shell
        .apply(ShellAction::SelectTab { id: original_tab })
        .unwrap();
    let execution = ExecutionId::new();
    let pane_a = shell.snapshot().focused_pane;
    shell
        .apply(ShellAction::BindExecution {
            pane: pane_a,
            execution,
        })
        .unwrap();
    shell
        .apply(ShellAction::SelectTab { id: created_tab })
        .unwrap();
    let pane_b = shell.snapshot().focused_pane;
    assert_ne!(pane_a, pane_b);
    let before = shell.containment_fingerprint();
    assert_eq!(
        shell.apply(ShellAction::BindExecution {
            pane: pane_b,
            execution,
        }),
        Err(ShellError::ExecutionAlreadyBound)
    );
    assert_rejection_atomic(&shell, &before);
    let _ = first_pane;
}

#[test]
fn snapshots_are_deterministic_for_identical_state() {
    let shell = seed_two_workspaces();
    assert_eq!(shell.snapshot(), shell.snapshot());
}

#[test]
fn empty_shell_is_rejected() {
    assert_eq!(
        ShellState::from_workspaces(Vec::new(), WorkspaceId::m001_default(), true, true).err(),
        Some(ShellError::EmptyShell)
    );
}

#[test]
fn pane_tree_walk_is_preorder() {
    let a = PaneId::new();
    let b = PaneId::new();
    let tree = PaneTree::Leaf(a).replacing(
        a,
        PaneTree::Split {
            axis: SplitAxis::Right,
            first: Box::new(PaneTree::Leaf(a)),
            second: Box::new(PaneTree::Leaf(b)),
        },
    );
    assert_eq!(tree.pane_ids(), vec![a, b]);
    assert_eq!(tree.layout_description(), LayoutDescription::SplitRight);
}

// --- W2a: create / select / cycle / move / fencing ---

fn seed_two_windows_one_workspace() -> (ShellState, WindowId, WindowId, TabId, TabId, TabId) {
    let workspace = WorkspaceId::m001_default();
    let w1 = WindowId::new();
    let w2 = WindowId::new();
    let t1 = TabId::new();
    let t2 = TabId::new();
    let t3 = TabId::new();
    let shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: workspace,
            name: "Local".into(),
            detail: None,
            attention: false,
            active_window: w1,
            windows: vec![
                ShellWindowSeed {
                    id: w1,
                    active_tab: t1,
                    tabs: vec![
                        ShellTabSeed {
                            id: t1,
                            title: "A".into(),
                            attention: false,
                            pane: ShellPaneSeed {
                                id: PaneId::new(),
                                title: "P".into(),
                                allows_implicit_execution_bootstrap: false,
                            },
                        },
                        ShellTabSeed {
                            id: t2,
                            title: "B".into(),
                            attention: false,
                            pane: ShellPaneSeed {
                                id: PaneId::new(),
                                title: "P".into(),
                                allows_implicit_execution_bootstrap: false,
                            },
                        },
                    ],
                },
                ShellWindowSeed {
                    id: w2,
                    active_tab: t3,
                    tabs: vec![ShellTabSeed {
                        id: t3,
                        title: "C".into(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: PaneId::new(),
                            title: "P".into(),
                            allows_implicit_execution_bootstrap: false,
                        },
                    }],
                },
            ],
        }],
        workspace,
        true,
        true,
    )
    .expect("fixture");
    (shell, w1, w2, t1, t2, t3)
}

#[test]
fn create_window_bumps_generation_and_selects_new_window() {
    let mut shell = seed_two_workspaces();
    let before_gen = shell.containment_generation();
    let workspace = shell.snapshot().active_workspace;
    shell
        .apply(ShellAction::CreateWindow {
            workspace,
            containment_generation: before_gen,
        })
        .expect("create");
    let snap = shell.snapshot();
    assert_eq!(snap.containment_generation, before_gen + 1);
    assert_eq!(snap.active_workspace, workspace);
    // New window is product-active with one tab; workspace has the prior tab too.
    assert_eq!(snap.workspaces[0].tab_count, 2);
    assert_eq!(snap.tabs.len(), 2);
}

#[test]
fn structural_actions_reject_stale_generation_atomically() {
    let (mut shell, w1, w2, t1, _t2, t3) = seed_two_windows_one_workspace();
    let generation = shell.containment_generation();
    // Bump generation with a real structural action.
    shell
        .apply(ShellAction::CreateTab {
            window: w1,
            containment_generation: generation,
        })
        .unwrap();
    let stale = generation;
    let before = shell.containment_fingerprint();
    let structural = [
        ShellAction::CreateWindow {
            workspace: WorkspaceId::m001_default(),
            containment_generation: stale,
        },
        ShellAction::CreateTab {
            window: w1,
            containment_generation: stale,
        },
        ShellAction::MoveTabBefore {
            tab: t1,
            before: None,
            window: w2,
            containment_generation: stale,
        },
        ShellAction::MoveTabToWindow {
            tab: t1,
            window: w2,
            containment_generation: stale,
        },
        ShellAction::MoveTabToNewWindow {
            tab: t1,
            containment_generation: stale,
        },
    ];
    for action in structural {
        assert_eq!(shell.apply(action), Err(ShellError::StaleContainment));
        assert_rejection_atomic(&shell, &before);
        assert_eq!(shell.last_error(), Some(ShellError::StaleContainment));
    }
    let _ = t3;
}

#[test]
fn selection_actions_accept_live_identities_across_generations() {
    let (mut shell, w1, w2, t1, t2, t3) = seed_two_windows_one_workspace();
    let generation = shell.containment_generation();
    shell
        .apply(ShellAction::CreateTab {
            window: w1,
            containment_generation: generation,
        })
        .unwrap();
    assert!(shell.containment_generation() > generation);
    shell
        .apply(ShellAction::SelectWindow { id: w2 })
        .expect("select window");
    assert_eq!(shell.snapshot().active_window, Some(w2));
    shell
        .apply(ShellAction::SelectTab { id: t1 })
        .expect("select tab");
    assert_eq!(shell.snapshot().active_tab, t1);
    shell
        .apply(ShellAction::CycleWindow {
            direction: CycleDirection::Next,
        })
        .expect("cycle window");
    shell
        .apply(ShellAction::CycleTab {
            direction: CycleDirection::Previous,
        })
        .expect("cycle tab");
    let _ = (t2, t3);
}

#[test]
fn activate_workspace_raise_ignores_stale_generation() {
    let mut shell = seed_two_workspaces();
    let generation = shell.containment_generation();
    shell
        .apply(ShellAction::CreateWindow {
            workspace: WorkspaceId::m001_default(),
            containment_generation: generation,
        })
        .unwrap();
    let stale = generation;
    shell
        .apply(ShellAction::ActivateWorkspace {
            workspace: other_workspace(),
            containment_generation: stale,
        })
        .expect("raise with stale gen");
    assert_eq!(shell.snapshot().active_workspace, other_workspace());
}

#[test]
fn activate_workspace_create_rejects_stale_generation() {
    let first = WorkspaceId::m001_default();
    let empty = WorkspaceId::from_bytes([0x33; 16]);
    let window = WindowId::new();
    let tab = TabId::new();
    let mut shell = ShellState::from_workspaces(
        vec![
            ShellWorkspaceSeed {
                id: first,
                name: "Local".into(),
                detail: None,
                attention: false,
                active_window: window,
                windows: vec![ShellWindowSeed {
                    id: window,
                    active_tab: tab,
                    tabs: vec![ShellTabSeed {
                        id: tab,
                        title: "T".into(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: PaneId::new(),
                            title: "P".into(),
                            allows_implicit_execution_bootstrap: false,
                        },
                    }],
                }],
            },
            ShellWorkspaceSeed {
                id: empty,
                name: "Empty".into(),
                detail: None,
                attention: false,
                active_window: WindowId::new(),
                windows: Vec::new(),
            },
        ],
        first,
        true,
        true,
    )
    .unwrap();
    let generation = shell.containment_generation();
    shell
        .apply(ShellAction::CreateTab {
            window,
            containment_generation: generation,
        })
        .unwrap();
    let before = shell.containment_fingerprint();
    assert_eq!(
        shell.apply(ShellAction::ActivateWorkspace {
            workspace: empty,
            containment_generation: generation, // stale
        }),
        Err(ShellError::StaleContainment)
    );
    assert_rejection_atomic(&shell, &before);
    shell
        .apply(ShellAction::ActivateWorkspace {
            workspace: empty,
            containment_generation: shell.containment_generation(),
        })
        .expect("create path");
    assert_eq!(shell.snapshot().active_workspace, empty);
    assert_eq!(shell.snapshot().workspaces[1].tab_count, 1);
}

#[test]
fn move_only_tab_to_other_window_destroys_source() {
    let (mut shell, w1, w2, t1, t2, t3) = seed_two_windows_one_workspace();
    // Move t3 (only tab in w2) onto w1.
    let generation = shell.containment_generation();
    let focused_before = {
        shell.apply(ShellAction::SelectTab { id: t3 }).unwrap();
        shell.snapshot().focused_pane
    };
    shell
        .apply(ShellAction::MoveTabToWindow {
            tab: t3,
            window: w1,
            containment_generation: generation,
        })
        .expect("move only tab");
    let snap = shell.snapshot();
    assert_eq!(snap.active_window, Some(w1));
    assert_eq!(snap.active_tab, t3);
    assert_eq!(snap.focused_pane, focused_before);
    assert_eq!(snap.containment_generation, generation + 1);
    assert_eq!(snap.workspaces[0].tab_count, 3);
    // w2 is gone — select must fail.
    assert_eq!(
        shell.apply(ShellAction::SelectWindow { id: w2 }),
        Err(ShellError::UnknownWindow)
    );
    let _ = (t1, t2);
}

#[test]
fn move_tab_to_new_window_rejects_only_tab() {
    let (mut shell, _w1, w2, _t1, _t2, t3) = seed_two_windows_one_workspace();
    let generation = shell.containment_generation();
    let before = shell.containment_fingerprint();
    assert_eq!(
        shell.apply(ShellAction::MoveTabToNewWindow {
            tab: t3,
            containment_generation: generation,
        }),
        Err(ShellError::MoveWouldNotChangeContainment)
    );
    assert_rejection_atomic(&shell, &before);
    let _ = w2;
}

#[test]
fn move_tab_to_new_window_splits_off_tab() {
    let (mut shell, w1, _w2, t1, t2, _t3) = seed_two_windows_one_workspace();
    let generation = shell.containment_generation();
    shell
        .apply(ShellAction::MoveTabToNewWindow {
            tab: t2,
            containment_generation: generation,
        })
        .expect("move to new");
    let snap = shell.snapshot();
    assert_eq!(snap.active_tab, t2);
    assert_ne!(snap.active_window, Some(w1));
    assert_eq!(snap.containment_generation, generation + 1);
    // Original window still has t1 as its only remaining seeded tab from {t1,t2}.
    shell.apply(ShellAction::SelectWindow { id: w1 }).unwrap();
    assert_eq!(shell.snapshot().active_tab, t1);
    assert_eq!(
        shell
            .snapshot()
            .tabs
            .iter()
            .filter(|tab| tab.id == t1)
            .count(),
        1
    );
    // t2 lives in the new window, still present in the derived workspace list.
    assert!(shell.snapshot().tabs.iter().any(|tab| tab.id == t2));
}

#[test]
fn move_tab_before_noop_skips_generation_bump() {
    let (mut shell, w1, _w2, t1, t2, _) = seed_two_windows_one_workspace();
    let generation = shell.containment_generation();
    // t1 is already before t2.
    shell
        .apply(ShellAction::MoveTabBefore {
            tab: t1,
            before: Some(t2),
            window: w1,
            containment_generation: generation,
        })
        .expect("noop");
    assert_eq!(shell.containment_generation(), generation);
}

#[test]
fn move_rejects_unknown_before_anchor() {
    let (mut shell, w1, _w2, t1, _, _) = seed_two_windows_one_workspace();
    let generation = shell.containment_generation();
    let before = shell.containment_fingerprint();
    assert_eq!(
        shell.apply(ShellAction::MoveTabBefore {
            tab: t1,
            before: Some(TabId::new()),
            window: w1,
            containment_generation: generation,
        }),
        Err(ShellError::UnknownTab)
    );
    assert_rejection_atomic(&shell, &before);
}

#[test]
fn move_rejects_cross_workspace() {
    let mut shell = seed_two_workspaces();
    let snap = shell.snapshot();
    let tab = snap.active_tab;
    activate_workspace(&mut shell, other_workspace()).unwrap();
    let other_window = shell.snapshot().active_window.expect("active window");
    let generation = shell.containment_generation();
    let before = shell.containment_fingerprint();
    assert_eq!(
        shell.apply(ShellAction::MoveTabToWindow {
            tab,
            window: other_window,
            containment_generation: generation,
        }),
        Err(ShellError::CrossWorkspaceMove)
    );
    assert_rejection_atomic(&shell, &before);
}

#[test]
fn reorder_and_move_property_no_zero_tab_window() {
    let (mut shell, w1, w2, t1, t2, t3) = seed_two_windows_one_workspace();
    let tabs = [t1, t2, t3];
    let windows = [w1, w2];
    // Exhaustive small sequences of MoveTabBefore / MoveTabToWindow / MoveTabToNewWindow.
    for &tab in &tabs {
        for &window in &windows {
            for before in [None, Some(t1), Some(t2), Some(t3)] {
                let generation = shell.containment_generation();
                let _ = shell.apply(ShellAction::MoveTabBefore {
                    tab,
                    before,
                    window,
                    containment_generation: generation,
                });
                assert_no_empty_window(&shell);
            }
            let generation = shell.containment_generation();
            let _ = shell.apply(ShellAction::MoveTabToWindow {
                tab,
                window,
                containment_generation: generation,
            });
            assert_no_empty_window(&shell);
        }
        let generation = shell.containment_generation();
        let _ = shell.apply(ShellAction::MoveTabToNewWindow {
            tab,
            containment_generation: generation,
        });
        assert_no_empty_window(&shell);
    }
}

fn assert_no_empty_window(shell: &ShellState) {
    assert!(shell.every_window_has_a_tab());
}

#[test]
fn selection_does_not_bump_containment_generation() {
    let (mut shell, w1, w2, t1, _, _) = seed_two_windows_one_workspace();
    let generation = shell.containment_generation();
    shell.apply(ShellAction::SelectWindow { id: w2 }).unwrap();
    shell.apply(ShellAction::SelectTab { id: t1 }).unwrap();
    shell
        .apply(ShellAction::CycleWindow {
            direction: CycleDirection::Next,
        })
        .unwrap();
    shell
        .apply(ShellAction::CycleTab {
            direction: CycleDirection::Next,
        })
        .unwrap();
    assert_eq!(shell.containment_generation(), generation);
    let _ = w1;
}

#[test]
fn same_window_selection_does_not_emit_order_front() {
    let (mut shell, w1, w2, t1, t2, _) = seed_two_windows_one_workspace();
    let _ = shell.take_effects();
    // Cross-window selection emits exactly one raise.
    shell.apply(ShellAction::SelectWindow { id: w2 }).unwrap();
    assert_eq!(
        shell.take_effects(),
        [ShellNativeEffect::OrderFrontMakeKey { window: w2 }]
    );
    // Return to w1 (one raise), then same-window tab selection must not emit.
    shell.apply(ShellAction::SelectWindow { id: w1 }).unwrap();
    assert_eq!(
        shell.take_effects(),
        [ShellNativeEffect::OrderFrontMakeKey { window: w1 }]
    );
    for _ in 0..8 {
        shell.apply(ShellAction::SelectTab { id: t2 }).unwrap();
        shell.apply(ShellAction::SelectTab { id: t1 }).unwrap();
        assert!(
            shell.take_effects().is_empty(),
            "same-window SelectTab must not emit OrderFrontMakeKey"
        );
    }
}
