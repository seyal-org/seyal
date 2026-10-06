//! W2a reducer tests: non-close window/tab actions and containment fencing.

use seyal_core::{ExecutionId, PaneId, TabId, WindowId, WorkspaceId};

use super::*;

fn other_workspace() -> WorkspaceId {
    WorkspaceId::from_bytes([0x11; 16])
}

fn tab_seed(id: TabId, title: &str) -> ShellTabSeed {
    ShellTabSeed {
        id,
        title: title.to_owned(),
        attention: false,
        pane: ShellPaneSeed {
            id: PaneId::new(),
            title: "Pane 1".to_owned(),
            allows_implicit_execution_bootstrap: true,
        },
    }
}

fn two_tab_window(window: WindowId, first: TabId, second: TabId) -> ShellWindowSeed {
    ShellWindowSeed {
        id: window,
        active_tab: first,
        tabs: vec![tab_seed(first, "One"), tab_seed(second, "Two")],
    }
}

fn containment_key(shell: &ShellState) -> (u64, WorkspaceId, TabId, usize, Option<WindowId>) {
    let snap = shell.snapshot();
    (
        snap.containment_generation,
        snap.active_workspace,
        snap.active_tab,
        snap.tabs.len(),
        shell.product_window_id().ok(),
    )
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
                detail: None,
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
                detail: None,
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
        false,
        true,
    )
    .expect("fixture")
}

#[test]
fn structural_actions_reject_stale_generation_and_keep_containment() {
    let mut shell = seed_two_workspaces();
    let window = shell.product_window_id().unwrap();
    let stale = shell.containment_generation();
    shell.apply_product_create_tab().unwrap();
    let before = containment_key(&shell);
    let actions = [
        ShellAction::CreateWindow {
            workspace: WorkspaceId::m001_default(),
            containment_generation: stale,
        },
        ShellAction::CreateTab {
            window,
            containment_generation: stale,
        },
        ShellAction::MoveTabToNewWindow {
            tab: shell.snapshot().active_tab,
            containment_generation: stale,
        },
    ];
    for action in actions {
        let mut probe = shell.clone();
        // Force create-path ActivateWorkspace by using an empty workspace clone below.
        let err = probe.apply(action).unwrap_err();
        assert_eq!(err, ShellError::StaleContainment, "{action:?}");
        assert_eq!(containment_key(&probe), before);
        assert_eq!(probe.last_error(), Some(ShellError::StaleContainment));
    }
}

#[test]
fn activate_workspace_create_path_is_generation_fenced() {
    let empty_id = WorkspaceId::from_bytes([0x33; 16]);
    let window = WindowId::new();
    let tab = TabId::new();
    let mut shell = ShellState::from_workspaces(
        vec![
            ShellWorkspaceSeed {
                id: WorkspaceId::m001_default(),
                name: "Local".to_owned(),
                detail: None,
                attention: false,
                active_window: window,
                windows: vec![ShellWindowSeed {
                    id: window,
                    active_tab: tab,
                    tabs: vec![tab_seed(tab, "One")],
                }],
            },
            ShellWorkspaceSeed {
                id: empty_id,
                name: "Empty".to_owned(),
                detail: None,
                attention: false,
                active_window: WindowId::new(),
                windows: Vec::new(),
            },
        ],
        WorkspaceId::m001_default(),
        false,
        true,
    )
    .expect("empty workspace fixture");
    let stale = 99;
    let before = containment_key(&shell);
    assert_eq!(
        shell.apply(ShellAction::ActivateWorkspace {
            workspace: empty_id,
            containment_generation: stale,
        }),
        Err(ShellError::StaleContainment)
    );
    assert_eq!(containment_key(&shell), before);
    shell
        .apply_activate_workspace(empty_id)
        .expect("create when empty");
    assert_eq!(shell.snapshot().active_workspace, empty_id);
    assert!(shell.product_window_id().is_ok());
    assert_eq!(shell.snapshot().containment_generation, 1);
}

#[test]
fn activate_workspace_raise_accepts_stale_generation() {
    let mut shell = seed_two_workspaces();
    shell.apply_product_create_tab().unwrap();
    let stale = 0;
    assert_ne!(shell.containment_generation(), stale);
    shell
        .apply(ShellAction::ActivateWorkspace {
            workspace: other_workspace(),
            containment_generation: stale,
        })
        .expect("raise is identity-fenced");
    assert_eq!(shell.snapshot().active_workspace, other_workspace());
    assert_eq!(shell.snapshot().tabs[0].title, "API");
}

#[test]
fn selection_actions_accept_live_identities_across_generations() {
    let mut shell = seed_two_workspaces();
    let original_tab = shell.snapshot().active_tab;
    let original_window = shell.product_window_id().unwrap();
    shell.apply_product_create_tab().unwrap();
    let created = shell.snapshot().active_tab;
    assert_ne!(shell.containment_generation(), 0);
    shell
        .apply(ShellAction::SelectTab { id: original_tab })
        .expect("select original tab after generation bump");
    assert_eq!(shell.snapshot().active_tab, original_tab);
    shell
        .apply(ShellAction::SelectWindow {
            id: original_window,
        })
        .expect("select original window");
    assert_eq!(shell.product_window_id().unwrap(), original_window);
    shell
        .apply(ShellAction::CycleTab {
            direction: CycleDirection::Next,
        })
        .expect("cycle tab");
    assert_eq!(shell.snapshot().active_tab, created);
}

#[test]
fn selection_does_not_bump_containment_generation() {
    let mut shell = seed_two_workspaces();
    let generation = shell.containment_generation();
    let tab = shell.snapshot().active_tab;
    let window = shell.product_window_id().unwrap();
    shell.apply(ShellAction::SelectTab { id: tab }).unwrap();
    shell
        .apply(ShellAction::SelectWindow { id: window })
        .unwrap();
    shell
        .apply(ShellAction::CycleWindow {
            direction: CycleDirection::Next,
        })
        .unwrap();
    assert_eq!(shell.containment_generation(), generation);
}

#[test]
fn create_window_and_cycle_are_workspace_scoped() {
    let mut shell = seed_two_workspaces();
    let first_window = shell.product_window_id().unwrap();
    shell
        .apply(ShellAction::CreateWindow {
            workspace: WorkspaceId::m001_default(),
            containment_generation: shell.containment_generation(),
        })
        .unwrap();
    let second = shell.product_window_id().unwrap();
    assert_ne!(first_window, second);
    shell
        .apply(ShellAction::CycleWindow {
            direction: CycleDirection::Previous,
        })
        .unwrap();
    assert_eq!(shell.product_window_id().unwrap(), first_window);
}

#[test]
fn move_tab_before_uses_anchor_not_index() {
    let first = TabId::new();
    let second = TabId::new();
    let third = TabId::new();
    let window = WindowId::new();
    let mut shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: WorkspaceId::m001_default(),
            name: "Local".to_owned(),
            detail: None,
            attention: false,
            active_window: window,
            windows: vec![ShellWindowSeed {
                id: window,
                active_tab: first,
                tabs: vec![
                    tab_seed(first, "A"),
                    tab_seed(second, "B"),
                    tab_seed(third, "C"),
                ],
            }],
        }],
        WorkspaceId::m001_default(),
        false,
        true,
    )
    .unwrap();
    let generation = shell.containment_generation();
    shell
        .apply(ShellAction::MoveTabBefore {
            tab: third,
            before: Some(first),
            window,
            containment_generation: generation,
        })
        .unwrap();
    let titles: Vec<_> = shell
        .snapshot()
        .tabs
        .into_iter()
        .map(|tab| tab.title)
        .collect();
    assert_eq!(titles, ["C", "A", "B"]);
    assert_eq!(shell.containment_generation(), generation + 1);
}

#[test]
fn same_window_order_noop_does_not_bump_generation() {
    let first = TabId::new();
    let window = WindowId::new();
    let mut shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: WorkspaceId::m001_default(),
            name: "Local".to_owned(),
            detail: None,
            attention: false,
            active_window: window,
            windows: vec![two_tab_window(window, first, TabId::new())],
        }],
        WorkspaceId::m001_default(),
        false,
        true,
    )
    .unwrap();
    let generation = shell.containment_generation();
    shell
        .apply(ShellAction::MoveTabBefore {
            tab: first,
            before: Some(shell.snapshot().tabs[1].id),
            window,
            containment_generation: generation,
        })
        .unwrap();
    // first is already before second — no-op.
    assert_eq!(shell.containment_generation(), generation);
}

#[test]
fn last_tab_move_destroys_source_window_atomically() {
    let source = WindowId::new();
    let target = WindowId::new();
    let moving = TabId::new();
    let staying = TabId::new();
    let mut shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: WorkspaceId::m001_default(),
            name: "Local".to_owned(),
            detail: None,
            attention: false,
            active_window: source,
            windows: vec![
                ShellWindowSeed {
                    id: source,
                    active_tab: moving,
                    tabs: vec![tab_seed(moving, "Move")],
                },
                ShellWindowSeed {
                    id: target,
                    active_tab: staying,
                    tabs: vec![tab_seed(staying, "Stay")],
                },
            ],
        }],
        WorkspaceId::m001_default(),
        false,
        true,
    )
    .unwrap();
    shell
        .apply(ShellAction::MoveTabToWindow {
            tab: moving,
            window: target,
            containment_generation: 0,
        })
        .unwrap();
    assert_eq!(shell.product_window_id().unwrap(), target);
    assert_eq!(shell.snapshot().active_tab, moving);
    assert_eq!(shell.snapshot().tabs.len(), 2);
    assert_eq!(
        shell.apply(ShellAction::SelectWindow { id: source }),
        Err(ShellError::UnknownWindow)
    );
}

#[test]
fn move_only_tab_to_new_window_is_rejected() {
    let mut shell = seed_two_workspaces();
    let tab = shell.snapshot().active_tab;
    let before = containment_key(&shell);
    assert_eq!(
        shell.apply(ShellAction::MoveTabToNewWindow {
            tab,
            containment_generation: 0,
        }),
        Err(ShellError::MoveWouldNotChangeContainment)
    );
    assert_eq!(containment_key(&shell), before);
}

#[test]
fn move_tab_to_new_window_when_sibling_exists() {
    let mut shell = seed_two_workspaces();
    let original_window = shell.product_window_id().unwrap();
    shell.apply_product_create_tab().unwrap();
    let created = shell.snapshot().active_tab;
    shell
        .apply(ShellAction::MoveTabToNewWindow {
            tab: created,
            containment_generation: shell.containment_generation(),
        })
        .unwrap();
    let new_window = shell.product_window_id().unwrap();
    assert_ne!(new_window, original_window);
    assert_eq!(shell.snapshot().active_tab, created);
    assert_eq!(
        shell.snapshot().tabs.len(),
        2,
        "Workspace.tabs remains the derived union of both Windows"
    );
    shell
        .apply(ShellAction::SelectWindow {
            id: original_window,
        })
        .unwrap();
    assert_eq!(shell.snapshot().tabs.len(), 2);
    assert_ne!(shell.snapshot().active_tab, created);
}

#[test]
fn unknown_identities_fail_closed() {
    let mut shell = seed_two_workspaces();
    let before = containment_key(&shell);
    let generation = shell.containment_generation();
    assert_eq!(
        shell.apply(ShellAction::SelectWindow {
            id: WindowId::new()
        }),
        Err(ShellError::UnknownWindow)
    );
    assert_eq!(
        shell.apply(ShellAction::MoveTabToWindow {
            tab: TabId::new(),
            window: shell.product_window_id().unwrap(),
            containment_generation: generation,
        }),
        Err(ShellError::UnknownTab)
    );
    assert_eq!(containment_key(&shell), before);
}

#[test]
fn bind_rejects_second_leaf_for_same_execution() {
    let mut shell = seed_two_workspaces();
    shell.apply_product_create_tab().unwrap();
    let first = shell.snapshot().panes[0].id;
    // Select the original tab so we can bind its pane too.
    let tabs = shell.snapshot().tabs;
    let original = tabs
        .iter()
        .find(|tab| tab.id != shell.snapshot().active_tab);
    // Bind on the created tab's pane, then on the other tab's pane.
    let execution = ExecutionId::new();
    shell
        .apply(ShellAction::BindExecution {
            pane: first,
            execution,
        })
        .unwrap();
    shell
        .apply(ShellAction::SelectTab {
            id: original.unwrap().id,
        })
        .unwrap();
    let other_pane = shell.snapshot().focused_pane;
    assert_eq!(
        shell.apply(ShellAction::BindExecution {
            pane: other_pane,
            execution,
        }),
        Err(ShellError::ExecutionAlreadyBound)
    );
    assert!(shell.pane_execution(other_pane).unwrap().is_none());
}

#[test]
fn activate_workspace_raise_uses_derived_pane_recency() {
    let mut shell = seed_two_workspaces();
    let first_window = shell.product_window_id().unwrap();
    shell
        .apply(ShellAction::CreateWindow {
            workspace: WorkspaceId::m001_default(),
            containment_generation: 0,
        })
        .unwrap();
    let second_window = shell.product_window_id().unwrap();
    shell
        .apply(ShellAction::SelectWindow { id: first_window })
        .unwrap();
    shell.apply_activate_workspace(other_workspace()).unwrap();
    shell
        .apply_activate_workspace(WorkspaceId::m001_default())
        .unwrap();
    assert_eq!(
        shell.product_window_id().unwrap(),
        first_window,
        "raise must follow derived pane recency, not creation order"
    );
    assert_ne!(shell.product_window_id().unwrap(), second_window);
}

#[test]
fn reorder_and_move_never_yield_zero_tab_window() {
    let ids: Vec<TabId> = (0..3).map(|_| TabId::new()).collect();
    let window_a = WindowId::new();
    let window_b = WindowId::new();
    let mut shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: WorkspaceId::m001_default(),
            name: "Local".to_owned(),
            detail: None,
            attention: false,
            active_window: window_a,
            windows: vec![
                ShellWindowSeed {
                    id: window_a,
                    active_tab: ids[0],
                    tabs: vec![tab_seed(ids[0], "A"), tab_seed(ids[1], "B")],
                },
                ShellWindowSeed {
                    id: window_b,
                    active_tab: ids[2],
                    tabs: vec![tab_seed(ids[2], "C")],
                },
            ],
        }],
        WorkspaceId::m001_default(),
        false,
        true,
    )
    .unwrap();
    let windows = [window_a, window_b];
    for tab in &ids {
        for window in windows {
            let generation = shell.containment_generation();
            let _ = shell.apply(ShellAction::MoveTabToWindow {
                tab: *tab,
                window,
                containment_generation: generation,
            });
            assert!(
                shell.every_window_has_a_tab(),
                "no sequence may produce a zero-Tab Window"
            );
        }
    }
}

#[test]
fn rejection_is_atomic_including_unknown_tab_move() {
    let mut shell = seed_two_workspaces();
    let before_generation = shell.containment_generation();
    let before_tab = shell.snapshot().active_tab;
    let before_tabs = shell.snapshot().tabs.len();
    let _ = shell.apply(ShellAction::MoveTabBefore {
        tab: TabId::new(),
        before: None,
        window: shell.product_window_id().unwrap(),
        containment_generation: before_generation,
    });
    assert_eq!(shell.containment_generation(), before_generation);
    assert_eq!(shell.snapshot().active_tab, before_tab);
    assert_eq!(shell.snapshot().tabs.len(), before_tabs);
}

#[test]
fn cross_window_select_tab_emits_order_front() {
    let mut shell = seed_two_workspaces();
    let original_window = shell.product_window_id().unwrap();
    shell.apply_product_create_tab().unwrap();
    let created = shell.snapshot().active_tab;
    shell
        .apply(ShellAction::MoveTabToNewWindow {
            tab: created,
            containment_generation: shell.containment_generation(),
        })
        .unwrap();
    let new_window = shell.product_window_id().unwrap();
    assert_ne!(new_window, original_window);
    shell
        .apply(ShellAction::SelectWindow {
            id: original_window,
        })
        .unwrap();
    let _ = shell.take_effects();
    shell.apply(ShellAction::SelectTab { id: created }).unwrap();
    assert_eq!(shell.product_window_id().unwrap(), new_window);
    assert_eq!(
        shell.take_effects(),
        vec![ShellNativeEffect::OrderFrontMakeKey { window: new_window }]
    );
}
