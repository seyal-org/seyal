//! W2b presentation-removal tests (ADR-018 §3.1 / §3.2 / §3.3a).

use super::*;
use seyal_core::WindowId;

fn workspace_a() -> WorkspaceId {
    WorkspaceId::m001_default()
}

fn workspace_b() -> WorkspaceId {
    WorkspaceId::from_bytes([0x11; 16])
}

fn other_workspace() -> WorkspaceId {
    workspace_b()
}

fn seed_two_workspaces() -> ShellState {
    let first_tab = TabId::new();
    let first_pane = PaneId::new();
    let second_tab = TabId::new();
    let second_pane = PaneId::new();
    let first_window = WindowId::new();
    let second_window = WindowId::new();
    ShellState::from_workspaces(
        vec![
            ShellWorkspaceSeed {
                id: workspace_a(),
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
                id: workspace_b(),
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
        workspace_a(),
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

fn seed_single_window() -> (ShellState, WindowId, TabId, PaneId) {
    let window = WindowId::new();
    let tab = TabId::new();
    let pane = PaneId::new();
    let shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: workspace_a(),
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
                        id: pane,
                        title: "P".into(),
                        allows_implicit_execution_bootstrap: false,
                    },
                }],
            }],
        }],
        workspace_a(),
        true,
        true,
    )
    .expect("fixture");
    (shell, window, tab, pane)
}

fn seed_two_windows() -> (ShellState, WindowId, WindowId, TabId, TabId, TabId) {
    let w1 = WindowId::new();
    let w2 = WindowId::new();
    let t1 = TabId::new();
    let t2 = TabId::new();
    let t3 = TabId::new();
    let shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: workspace_a(),
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
        workspace_a(),
        true,
        true,
    )
    .expect("fixture");
    (shell, w1, w2, t1, t2, t3)
}

fn containment_gen(shell: &ShellState) -> u64 {
    shell.containment_generation()
}

#[test]
fn close_pane_unbinds_collapses_and_keeps_sibling_focus() {
    let (mut shell, _window, _tab, pane) = seed_single_window();
    let execution = ExecutionId::new();
    shell
        .apply(ShellAction::BindExecution { pane, execution })
        .unwrap();
    shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Right,
                    containment_generation: shell.containment_generation(),
        })
        .unwrap();
    let created = shell.snapshot().focused_pane;
    shell.apply(ShellAction::FocusPane { id: pane }).unwrap();
    shell
        .apply(ShellAction::ClosePane {
            id: pane,
            containment_generation: containment_gen(&shell),
        })
        .unwrap();
    let snap = shell.snapshot();
    assert_eq!(snap.layout, LayoutDescription::Single);
    assert_eq!(snap.focused_pane, created);
    assert_eq!(shell.live_unpresented(workspace_a()), vec![execution]);
    assert!(shell.take_effects().is_empty());
}

#[test]
fn close_pane_of_only_pane_is_close_tab() {
    let (mut shell, window, tab, pane) = seed_single_window();
    let execution = ExecutionId::new();
    shell
        .apply(ShellAction::BindExecution { pane, execution })
        .unwrap();
    let last_active = shell.snapshot().last_active_workspace;
    shell
        .apply(ShellAction::ClosePane {
            id: pane,
            containment_generation: containment_gen(&shell),
        })
        .unwrap();
    let snap = shell.snapshot();
    assert_eq!(snap.active_window, None);
    assert!(snap.windows.is_empty());
    assert_eq!(snap.last_active_workspace, last_active);
    assert_eq!(shell.live_unpresented(workspace_a()), vec![execution]);
    assert_eq!(
        shell.take_effects(),
        vec![ShellNativeEffect::DestroyWindowRealization { window }]
    );
    let _ = tab;
}

#[test]
fn close_tab_unbinds_all_leaves_and_selects_successor() {
    let (mut shell, _w1, _w2, t1, t2, _t3) = seed_two_windows();
    let execution = ExecutionId::new();
    // Bind on t1's pane, then select t1 and close it while t2 follows.
    shell.apply(ShellAction::SelectTab { id: t1 }).unwrap();
    let pane = shell.snapshot().focused_pane;
    shell
        .apply(ShellAction::BindExecution { pane, execution })
        .unwrap();
    assert_eq!(shell.snapshot().active_tab, t1);
    shell
        .apply(ShellAction::CloseTab {
            id: t1,
            containment_generation: containment_gen(&shell),
        })
        .unwrap();
    let snap = shell.snapshot();
    assert_eq!(snap.active_tab, t2);
    assert_eq!(shell.live_unpresented(workspace_a()), vec![execution]);
    assert!(shell
        .take_effects()
        .iter()
        .all(|effect| !matches!(effect, ShellNativeEffect::TerminateExecution { .. })));
}

#[test]
fn close_non_active_tab_leaves_active_tab_unchanged() {
    let (mut shell, _w1, _w2, t1, t2, _t3) = seed_two_windows();
    shell.apply(ShellAction::SelectTab { id: t1 }).unwrap();
    shell
        .apply(ShellAction::CloseTab {
            id: t2,
            containment_generation: containment_gen(&shell),
        })
        .unwrap();
    assert_eq!(shell.snapshot().active_tab, t1);
}

#[test]
fn close_tab_of_only_tab_is_close_window_not_last_tab_refusal() {
    let (mut shell, window, tab, pane) = seed_single_window();
    let execution = ExecutionId::new();
    shell
        .apply(ShellAction::BindExecution { pane, execution })
        .unwrap();
    shell
        .apply(ShellAction::CloseTab {
            id: tab,
            containment_generation: containment_gen(&shell),
        })
        .unwrap();
    assert_eq!(shell.snapshot().active_window, None);
    assert_eq!(shell.live_unpresented(workspace_a()), vec![execution]);
    assert_eq!(
        shell.take_effects(),
        vec![ShellNativeEffect::DestroyWindowRealization { window }]
    );
}

#[test]
fn close_active_window_activates_mru_successor_with_destroy_before_order_front() {
    let (mut shell, w1, w2, _t1, _t2, t3) = seed_two_windows();
    // Touch w2 so it is most-recent after w1 is closed.
    shell.apply(ShellAction::SelectWindow { id: w2 }).unwrap();
    shell.apply(ShellAction::SelectWindow { id: w1 }).unwrap();
    let _ = shell.take_effects();
    let execution = ExecutionId::new();
    let pane = shell.snapshot().focused_pane;
    shell
        .apply(ShellAction::BindExecution { pane, execution })
        .unwrap();
    shell
        .apply(ShellAction::CloseWindow {
            id: w1,
            containment_generation: containment_gen(&shell),
        })
        .unwrap();
    let snap = shell.snapshot();
    assert_eq!(snap.active_window, Some(w2));
    assert_eq!(snap.active_tab, t3);
    assert_eq!(shell.live_unpresented(workspace_a()), vec![execution]);
    assert_eq!(
        shell.take_effects(),
        vec![
            ShellNativeEffect::DestroyWindowRealization { window: w1 },
            ShellNativeEffect::OrderFrontMakeKey { window: w2 },
        ]
    );
}

#[test]
fn close_non_active_window_leaves_product_active_unchanged() {
    let (mut shell, w1, w2, t1, _t2, _t3) = seed_two_windows();
    shell.apply(ShellAction::SelectWindow { id: w1 }).unwrap();
    let _ = shell.take_effects();
    shell
        .apply(ShellAction::CloseWindow {
            id: w2,
            containment_generation: containment_gen(&shell),
        })
        .unwrap();
    let snap = shell.snapshot();
    assert_eq!(snap.active_window, Some(w1));
    assert_eq!(snap.active_tab, t1);
    assert_eq!(
        shell.take_effects(),
        vec![ShellNativeEffect::DestroyWindowRealization { window: w2 }]
    );
}

#[test]
fn close_last_window_is_zero_window_reentry_without_quit() {
    let (mut shell, window, _tab, pane) = seed_single_window();
    let execution = ExecutionId::new();
    let last_active = shell.snapshot().last_active_workspace;
    shell
        .apply(ShellAction::BindExecution { pane, execution })
        .unwrap();
    shell
        .apply(ShellAction::CloseWindow {
            id: window,
            containment_generation: containment_gen(&shell),
        })
        .unwrap();
    let snap = shell.snapshot();
    assert_eq!(snap.active_window, None);
    assert!(snap.windows.is_empty());
    assert_eq!(snap.last_active_workspace, last_active);
    assert_eq!(shell.live_unpresented(workspace_a()), vec![execution]);
    assert_eq!(
        shell.take_effects(),
        vec![ShellNativeEffect::DestroyWindowRealization { window }]
    );

    shell
        .apply(ShellAction::CreateWindow {
            workspace: workspace_a(),
            containment_generation: containment_gen(&shell),
        })
        .expect("create from zero");
    assert!(shell.snapshot().active_window.is_some());

    // Re-enter zero, then ActivateWorkspace create path.
    let window = shell.snapshot().active_window.expect("window");
    shell
        .apply(ShellAction::CloseWindow {
            id: window,
            containment_generation: containment_gen(&shell),
        })
        .unwrap();
    shell
        .apply(ShellAction::ActivateWorkspace {
            workspace: workspace_a(),
            containment_generation: containment_gen(&shell),
        })
        .expect("activate create from zero");
    assert!(shell.snapshot().active_window.is_some());
}

#[test]
fn close_active_window_prefers_same_workspace_mru_then_any() {
    let w_a1 = WindowId::new();
    let w_a2 = WindowId::new();
    let w_b = WindowId::new();
    let t_a1 = TabId::new();
    let t_a2 = TabId::new();
    let t_b = TabId::new();
    let mut shell = ShellState::from_workspaces(
        vec![
            ShellWorkspaceSeed {
                id: workspace_a(),
                name: "A".into(),
                detail: None,
                attention: false,
                active_window: w_a1,
                windows: vec![
                    ShellWindowSeed {
                        id: w_a1,
                        active_tab: t_a1,
                        tabs: vec![ShellTabSeed {
                            id: t_a1,
                            title: "A1".into(),
                            attention: false,
                            pane: ShellPaneSeed {
                                id: PaneId::new(),
                                title: "P".into(),
                                allows_implicit_execution_bootstrap: false,
                            },
                        }],
                    },
                    ShellWindowSeed {
                        id: w_a2,
                        active_tab: t_a2,
                        tabs: vec![ShellTabSeed {
                            id: t_a2,
                            title: "A2".into(),
                            attention: false,
                            pane: ShellPaneSeed {
                                id: PaneId::new(),
                                title: "P".into(),
                                allows_implicit_execution_bootstrap: false,
                            },
                        }],
                    },
                ],
            },
            ShellWorkspaceSeed {
                id: workspace_b(),
                name: "B".into(),
                detail: None,
                attention: false,
                active_window: w_b,
                windows: vec![ShellWindowSeed {
                    id: w_b,
                    active_tab: t_b,
                    tabs: vec![ShellTabSeed {
                        id: t_b,
                        title: "B".into(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: PaneId::new(),
                            title: "P".into(),
                            allows_implicit_execution_bootstrap: false,
                        },
                    }],
                }],
            },
        ],
        workspace_a(),
        true,
        true,
    )
    .unwrap();
    shell.apply(ShellAction::SelectWindow { id: w_a2 }).unwrap();
    shell.apply(ShellAction::SelectWindow { id: w_a1 }).unwrap();
    let _ = shell.take_effects();
    shell
        .apply(ShellAction::CloseWindow {
            id: w_a1,
            containment_generation: containment_gen(&shell),
        })
        .unwrap();
    assert_eq!(shell.snapshot().active_window, Some(w_a2));

    shell
        .apply(ShellAction::CloseWindow {
            id: w_a2,
            containment_generation: containment_gen(&shell),
        })
        .unwrap();
    assert_eq!(shell.snapshot().active_window, Some(w_b));
    assert_eq!(shell.snapshot().active_workspace, workspace_b());
}

#[test]
fn unknown_and_duplicate_removals_reject_without_retarget() {
    let (mut shell, window, _w2, _t1, _t2, _t3) = seed_two_windows();
    let before = shell.containment_fingerprint();
    assert_eq!(
        shell.apply(ShellAction::CloseWindow {
            id: WindowId::new(),
            containment_generation: containment_gen(&shell),
        }),
        Err(ShellError::UnknownWindow)
    );
    assert_eq!(shell.containment_fingerprint(), before);
    assert_eq!(
        shell.apply(ShellAction::CloseTab {
            id: TabId::new(),
            containment_generation: containment_gen(&shell),
        }),
        Err(ShellError::UnknownTab)
    );
    assert_eq!(
        shell.apply(ShellAction::ClosePane {
            id: PaneId::new(),
            containment_generation: containment_gen(&shell),
        }),
        Err(ShellError::UnknownPane)
    );

    shell
        .apply(ShellAction::CloseWindow {
            id: window,
            containment_generation: containment_gen(&shell),
        })
        .unwrap();
    let after = shell.containment_fingerprint();
    assert_eq!(
        shell.apply(ShellAction::CloseWindow {
            id: window,
            containment_generation: containment_gen(&shell),
        }),
        Err(ShellError::UnknownWindow)
    );
    assert_eq!(shell.containment_fingerprint(), after);
}

#[test]
fn structural_close_rejects_stale_generation_atomically() {
    let (mut shell, window, _w2, tab, _t2, _t3) = seed_two_windows();
    let pane = shell.snapshot().focused_pane;
    let stale = containment_gen(&shell);
    shell
        .apply(ShellAction::CreateTab {
            window,
            containment_generation: stale,
        })
        .unwrap();
    let before = shell.containment_fingerprint();
    assert_eq!(
        shell.apply(ShellAction::CloseWindow {
            id: window,
            containment_generation: stale,
        }),
        Err(ShellError::StaleContainment)
    );
    assert_eq!(shell.containment_fingerprint(), before);
    assert_eq!(
        shell.apply(ShellAction::CloseTab {
            id: tab,
            containment_generation: stale,
        }),
        Err(ShellError::StaleContainment)
    );
    assert_eq!(
        shell.apply(ShellAction::ClosePane {
            id: pane,
            containment_generation: stale,
        }),
        Err(ShellError::StaleContainment)
    );
}

#[test]
fn no_removal_variant_emits_terminate_execution() {
    let (mut shell, w1, w2, t1, t2, _t3) = seed_two_windows();
    let e1 = ExecutionId::from_bytes([0x01; 16]);
    let e2 = ExecutionId::from_bytes([0x02; 16]);
    shell.apply(ShellAction::SelectTab { id: t1 }).unwrap();
    let p1 = shell.snapshot().focused_pane;
    shell
        .apply(ShellAction::BindExecution {
            pane: p1,
            execution: e1,
        })
        .unwrap();
    shell.apply(ShellAction::SelectTab { id: t2 }).unwrap();
    let p2 = shell.snapshot().focused_pane;
    shell
        .apply(ShellAction::BindExecution {
            pane: p2,
            execution: e2,
        })
        .unwrap();
    let _ = shell.take_effects();

    shell
        .apply(ShellAction::ClosePane {
            id: p2,
            containment_generation: containment_gen(&shell),
        })
        .unwrap();
    assert!(shell
        .take_effects()
        .iter()
        .all(|effect| !matches!(effect, ShellNativeEffect::TerminateExecution { .. })));

    shell
        .apply(ShellAction::CloseTab {
            id: t1,
            containment_generation: containment_gen(&shell),
        })
        .unwrap();
    assert!(shell
        .take_effects()
        .iter()
        .all(|effect| !matches!(effect, ShellNativeEffect::TerminateExecution { .. })));

    shell
        .apply(ShellAction::CloseWindow {
            id: w2,
            containment_generation: containment_gen(&shell),
        })
        .unwrap();
    let effects = shell.take_effects();
    assert!(effects
        .iter()
        .all(|effect| !matches!(effect, ShellNativeEffect::TerminateExecution { .. })));
    assert!(effects
        .iter()
        .any(|effect| matches!(effect, ShellNativeEffect::DestroyWindowRealization { .. })));
    let _ = w1;
}

#[test]
fn close_enablement_admits_hierarchical_removal_while_a_window_exists() {
    let mut shell = seed_two_workspaces();
    let single = shell.snapshot();
    assert!(single.allows_tab_close);
    assert!(single.allows_pane_close);

    create_tab(&mut shell).expect("tabs allowed");
    shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Right,
                    containment_generation: shell.containment_generation(),
        })
        .expect("splits allowed");
    let two_panes = shell.snapshot();
    assert!(two_panes.allows_pane_close);
    shell
        .apply(ShellAction::ClosePane {
            id: two_panes.focused_pane,
            containment_generation: shell.containment_generation(),
        })
        .expect("close split pane");
    assert!(shell.snapshot().allows_pane_close);
    assert!(shell.snapshot().allows_tab_close);
}

#[test]
fn select_create_close_tabs_are_authoritative() {
    let mut shell = seed_two_workspaces();
    let before = shell.snapshot();
    let before_window = before.active_window.expect("active window");
    create_tab(&mut shell).expect("tabs allowed");
    let after_create = shell.snapshot();
    assert_eq!(after_create.tabs.len(), 2);
    assert_ne!(after_create.active_tab, before.active_tab);
    let created = after_create.active_tab;
    shell
        .apply(ShellAction::SelectTab {
            id: before.active_tab,
        })
        .expect("select original");
    assert_eq!(shell.snapshot().active_tab, before.active_tab);
    shell
        .apply(ShellAction::CloseTab {
            id: created,
            containment_generation: shell.containment_generation(),
        })
        .expect("close created");
    assert_eq!(shell.snapshot().tabs.len(), 1);
    assert_eq!(shell.snapshot().active_tab, before.active_tab);
    // Sole remaining Tab removes its Window in the same atomic step (ADR-018 §3.2).
    // The other Workspace's Window becomes product-active.
    shell
        .apply(ShellAction::CloseTab {
            id: before.active_tab,
            containment_generation: shell.containment_generation(),
        })
        .expect("sole tab removes window");
    let after = shell.snapshot();
    assert!(!after
        .windows
        .iter()
        .any(|window| window.id == before_window));
    assert_eq!(after.active_workspace, other_workspace());
    assert!(after.active_window.is_some());
}

#[test]
fn split_focus_and_close_panes() {
    let mut shell = seed_two_workspaces();
    let original = shell.snapshot().focused_pane;
    shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Right,
                    containment_generation: shell.containment_generation(),
        })
        .expect("split");
    let snap = shell.snapshot();
    assert_eq!(snap.layout, LayoutDescription::SplitRight);
    assert_eq!(snap.tabs[0].pane_count, 2);
    assert_ne!(snap.focused_pane, original);
    let created = snap.focused_pane;
    shell
        .apply(ShellAction::FocusPane { id: original })
        .expect("focus original");
    assert_eq!(shell.snapshot().focused_pane, original);
    shell
        .apply(ShellAction::SplitPane {
            id: original,
            axis: SplitAxis::Down,
                    containment_generation: shell.containment_generation(),
        })
        .expect("nested split");
    assert_eq!(shell.snapshot().tabs[0].pane_count, 3);
    assert_eq!(shell.snapshot().layout, LayoutDescription::SplitRight);
    shell
        .apply(ShellAction::ClosePane {
            id: created,
            containment_generation: shell.containment_generation(),
        })
        .expect("close first split");
    assert_eq!(shell.snapshot().tabs[0].pane_count, 2);
    let remaining_new = shell.snapshot().focused_pane;
    shell
        .apply(ShellAction::ClosePane {
            id: remaining_new,
            containment_generation: shell.containment_generation(),
        })
        .expect("close nested");
    let snap = shell.snapshot();
    assert_eq!(snap.layout, LayoutDescription::Single);
    assert_eq!(snap.focused_pane, original);
    // Sole remaining Pane removes its Tab (and therefore its Window) atomically.
    let removed_window = snap.active_window.expect("active window");
    shell
        .apply(ShellAction::ClosePane {
            id: original,
            containment_generation: shell.containment_generation(),
        })
        .expect("sole pane removes tab/window");
    let after = shell.snapshot();
    assert!(!after
        .windows
        .iter()
        .any(|window| window.id == removed_window));
    assert_eq!(after.active_workspace, other_workspace());
}

#[test]
fn execution_bound_pane_close_unbinds_without_terminate() {
    let mut shell = seed_two_workspaces();
    let bound = shell.snapshot().focused_pane;
    let execution = ExecutionId::from_bytes([7; 16]);
    let workspace = shell.snapshot().active_workspace;
    shell
        .apply(ShellAction::BindExecution {
            pane: bound,
            execution,
        })
        .expect("bind");
    shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Right,
                    containment_generation: shell.containment_generation(),
        })
        .expect("split");
    let created = shell.snapshot().focused_pane;
    assert!(shell.snapshot().allows_pane_close);
    shell
        .apply(ShellAction::FocusPane { id: bound })
        .expect("focus bound");
    assert!(shell.snapshot().allows_pane_close);
    shell
        .apply(ShellAction::ClosePane {
            id: bound,
            containment_generation: shell.containment_generation(),
        })
        .expect("close bound pane");
    assert_eq!(shell.snapshot().tabs[0].pane_count, 1);
    assert_eq!(shell.snapshot().focused_pane, created);
    assert_eq!(shell.live_unpresented(workspace), vec![execution]);
    assert!(shell
        .take_effects()
        .iter()
        .all(|effect| !matches!(effect, ShellNativeEffect::TerminateExecution { .. })));
}
