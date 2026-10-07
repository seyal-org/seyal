//! W2a reducer tests: non-close window/tab actions and containment fencing.

use std::collections::{BTreeMap, BTreeSet};

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
    let active_tab = shell.snapshot().active_tab;
    let stale = shell.containment_generation();
    shell.apply_product_create_tab().unwrap();
    let moved_tab = shell.snapshot().active_tab;
    let destination_window = shell
        .workspaces
        .iter()
        .find(|workspace| workspace.id == other_workspace())
        .and_then(|workspace| workspace.active_window)
        .expect("second workspace window");
    let actions = [
        ShellAction::CreateWindow {
            workspace: WorkspaceId::m001_default(),
            containment_generation: stale,
        },
        ShellAction::CreateTab {
            window,
            containment_generation: stale,
        },
        ShellAction::MoveTabBefore {
            tab: moved_tab,
            before: Some(active_tab),
            window,
            containment_generation: stale,
        },
        ShellAction::MoveTabToWindow {
            tab: moved_tab,
            window: destination_window,
            containment_generation: stale,
        },
        ShellAction::MoveTabToNewWindow {
            tab: moved_tab,
            containment_generation: stale,
        },
    ];
    for action in actions {
        let mut probe = shell.clone();
        let err = probe.apply(action).unwrap_err();
        assert_eq!(err, ShellError::StaleContainment, "{action:?}");
        let mut expected = shell.clone();
        expected.last_error = Some(ShellError::StaleContainment);
        assert_eq!(
            probe, expected,
            "rejection changed reducer state: {action:?}"
        );
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
    let mut expected = shell.clone();
    expected.last_error = Some(ShellError::StaleContainment);
    assert_eq!(
        shell.apply(ShellAction::ActivateWorkspace {
            workspace: empty_id,
            containment_generation: stale,
        }),
        Err(ShellError::StaleContainment)
    );
    assert_eq!(shell, expected, "stale activation changed reducer state");
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
        .apply(ShellAction::SelectTab {
            id: original_tab,
            containment_generation: shell.containment_generation(),
        })
        .expect("select original tab after generation bump");
    assert_eq!(shell.snapshot().active_tab, original_tab);
    shell
        .apply(ShellAction::SelectWindow {
            id: original_window,
            containment_generation: shell.containment_generation(),
        })
        .expect("select original window");
    assert_eq!(shell.product_window_id().unwrap(), original_window);
    shell
        .apply(ShellAction::CycleTab {
            direction: CycleDirection::Next,
            containment_generation: shell.containment_generation(),
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
    shell
        .apply(ShellAction::SelectTab {
            id: tab,
            containment_generation: shell.containment_generation(),
        })
        .unwrap();
    shell
        .apply(ShellAction::SelectWindow {
            id: window,
            containment_generation: shell.containment_generation(),
        })
        .unwrap();
    shell
        .apply(ShellAction::CycleWindow {
            direction: CycleDirection::Next,
            containment_generation: shell.containment_generation(),
        })
        .unwrap();
    assert_eq!(shell.containment_generation(), generation);
}

#[test]
fn active_workspace_projects_from_the_most_recent_live_window() {
    let mut shell = seed_two_workspaces();
    let other_pane = shell
        .workspaces
        .iter()
        .find(|workspace| workspace.id == other_workspace())
        .and_then(|workspace| workspace.tabs().next())
        .map(|tab| tab.focused)
        .expect("other workspace pane");

    // Focus history is the accepted application-wide source for derived
    // Window recency. The product Workspace must follow its newest live Pane.
    shell.focus_history.record(other_pane);
    assert_eq!(shell.snapshot().active_workspace, other_workspace());
    assert_eq!(
        shell.snapshot().active_tab,
        shell.workspaces[1].active_tab_id().unwrap()
    );
}

#[test]
fn zero_window_active_workspace_uses_last_active_workspace_for_reentry() {
    let mut shell = seed_two_workspaces();
    shell.apply_activate_workspace(other_workspace()).unwrap();
    for workspace in &mut shell.workspaces {
        workspace.windows.clear();
        workspace.active_window = None;
    }
    shell.focus_history.purge_if(|_| true);

    assert_eq!(shell.active_workspace_id(), other_workspace());
    assert_eq!(shell.last_active_workspace(), other_workspace());
    assert_eq!(shell.product_window_id(), Err(ShellError::UnknownWindow));
    shell
        .apply_activate_workspace(other_workspace())
        .expect("re-entry creates in the last active workspace");
    assert_eq!(shell.active_workspace_id(), other_workspace());
    assert!(shell.product_window_id().is_ok());
}

#[test]
fn initial_zero_window_reentry_workspace_is_first_workspace() {
    let first = WorkspaceId::m001_default();
    let second = other_workspace();
    let shell = ShellState::from_workspaces(
        vec![
            ShellWorkspaceSeed {
                id: first,
                name: "First".to_owned(),
                detail: None,
                attention: false,
                active_window: WindowId::new(),
                windows: Vec::new(),
            },
            ShellWorkspaceSeed {
                id: second,
                name: "Second".to_owned(),
                detail: None,
                attention: false,
                active_window: WindowId::new(),
                windows: Vec::new(),
            },
        ],
        second,
        false,
        true,
    )
    .expect("zero-window shell");

    assert_eq!(shell.active_workspace_id(), first);
    assert_eq!(shell.last_active_workspace(), first);
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
            containment_generation: shell.containment_generation(),
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
        shell.apply(ShellAction::SelectWindow {
            id: source,
            containment_generation: shell.containment_generation(),
        }),
        Err(ShellError::UnknownWindow)
    );
}

#[test]
fn moving_last_tab_preserves_recency_for_its_live_panes() {
    let workspace = WorkspaceId::m001_default();
    let source = WindowId::new();
    let target = WindowId::new();
    let moving = TabId::new();
    let staying = TabId::new();
    let mut shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: workspace,
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
        workspace,
        true,
        true,
    )
    .expect("fixture");
    let first_pane = shell.snapshot().focused_pane;
    shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Right,
            containment_generation: shell.containment_generation(),
        })
        .expect("split the moved Tab");
    let most_recent_pane = shell.snapshot().focused_pane;

    // Seed both live Pane identities in recency order; moving their Tab must
    // not purge either identity merely because its source Window disappears.
    shell.focus_history.record(first_pane);
    shell.focus_history.record(most_recent_pane);
    assert_eq!(shell.focus_history.panes(), &[most_recent_pane, first_pane]);

    shell
        .apply(ShellAction::MoveTabToWindow {
            tab: moving,
            window: target,
            containment_generation: shell.containment_generation(),
        })
        .expect("move the Tab and destroy its empty source Window");

    assert_eq!(
        shell.focus_history.panes(),
        &[most_recent_pane, first_pane],
        "live Pane identities retain their recency when only containment moves"
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
            containment_generation: shell.containment_generation(),
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
            id: WindowId::new(),
            containment_generation: shell.containment_generation(),
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
            containment_generation: shell.containment_generation(),
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
        .apply(ShellAction::SelectWindow {
            id: first_window,
            containment_generation: shell.containment_generation(),
        })
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
fn generated_reorder_and_window_move_sequences_preserve_structure_and_bindings() {
    let ids: Vec<TabId> = (0..4).map(|_| TabId::new()).collect();
    let executions: Vec<ExecutionId> = (0..4).map(|_| ExecutionId::new()).collect();
    let windows = [WindowId::new(), WindowId::new()];
    let workspace = WorkspaceId::m001_default();
    let make_shell = || {
        ShellState::from_workspaces(
            vec![ShellWorkspaceSeed {
                id: workspace,
                name: "Local".to_owned(),
                detail: None,
                attention: false,
                active_window: windows[0],
                windows: vec![
                    ShellWindowSeed {
                        id: windows[0],
                        active_tab: ids[0],
                        tabs: vec![
                            tab_seed(ids[0], "A"),
                            tab_seed(ids[1], "B"),
                            tab_seed(ids[2], "C"),
                        ],
                    },
                    ShellWindowSeed {
                        id: windows[1],
                        active_tab: ids[3],
                        tabs: vec![tab_seed(ids[3], "D")],
                    },
                ],
            }],
            workspace,
            false,
            true,
        )
        .expect("fixture")
    };
    let mut initial = make_shell();
    let panes: Vec<_> = initial
        .snapshot()
        .panes
        .iter()
        .map(|pane| pane.id)
        .collect();
    for (pane, execution) in panes.iter().zip(&executions) {
        initial
            .apply(ShellAction::BindExecution {
                pane: *pane,
                execution: *execution,
            })
            .unwrap();
    }
    let assert_invariants = |shell: &ShellState| {
        assert!(shell.every_window_has_a_tab());
        let all_panes: Vec<_> = shell
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.tabs())
            .flat_map(|tab| tab.root.pane_ids())
            .collect();
        let unique_panes: std::collections::HashSet<_> = all_panes.iter().copied().collect();
        assert_eq!(
            all_panes.len(),
            unique_panes.len(),
            "Pane identities stay unique"
        );
        for execution in &executions {
            let bound = shell.panes_bound_to(*execution);
            assert!(
                bound.len() <= 1,
                "Execution binding stays unique: {execution:?}"
            );
            if !bound.is_empty() {
                assert!(unique_panes.contains(&bound[0].2));
            }
        }
    };
    // Enumerate every ordered pair of distinct Tabs and both structural
    // operations, applying each operation to a fresh identical state.
    for tab in &ids[..3] {
        for before in &ids[..3] {
            if tab != before {
                let mut shell = initial.clone();
                let generation = shell.containment_generation();
                shell
                    .apply(ShellAction::MoveTabBefore {
                        tab: *tab,
                        before: Some(*before),
                        window: windows[0],
                        containment_generation: generation,
                    })
                    .unwrap();
                assert_invariants(&shell);
            }
        }
    }
    for tab in &ids {
        for window in windows {
            let mut shell = initial.clone();
            let generation = shell.containment_generation();
            let result = shell.apply(ShellAction::MoveTabToWindow {
                tab: *tab,
                window,
                containment_generation: generation,
            });
            if result.is_err() {
                let mut expected = initial.clone();
                expected.last_error = shell.last_error();
                assert_eq!(
                    shell, expected,
                    "rejection preserves full state for tab {tab:?}, window {window:?}"
                );
            }
            assert_invariants(&shell);
        }
    }
}

#[test]
fn composed_reorder_and_move_sequence_fences_stale_action_atomically() {
    let workspace = WorkspaceId::m001_default();
    let windows = [WindowId::new(), WindowId::new()];
    let tabs: Vec<TabId> = (0..4).map(|_| TabId::new()).collect();
    let mut shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: workspace,
            name: "Local".to_owned(),
            detail: None,
            attention: false,
            active_window: windows[0],
            windows: vec![
                ShellWindowSeed {
                    id: windows[0],
                    active_tab: tabs[0],
                    tabs: vec![
                        tab_seed(tabs[0], "A"),
                        tab_seed(tabs[1], "B"),
                        tab_seed(tabs[2], "C"),
                    ],
                },
                ShellWindowSeed {
                    id: windows[1],
                    active_tab: tabs[3],
                    tabs: vec![tab_seed(tabs[3], "D")],
                },
            ],
        }],
        workspace,
        false,
        true,
    )
    .expect("two-window fixture");

    let mut bindings = BTreeMap::new();
    let all_panes: Vec<PaneId> = shell
        .workspaces
        .iter()
        .flat_map(|item| item.tabs())
        .flat_map(|tab| tab.panes.keys().copied())
        .collect();
    for (index, pane) in all_panes.iter().copied().enumerate() {
        let execution = ExecutionId::from_bytes([index as u8 + 1; 16]);
        shell
            .apply(ShellAction::BindExecution { pane, execution })
            .expect("bind each pane exactly once");
        bindings.insert(pane, execution);
    }

    let assert_invariants = |shell: &ShellState, expected_tabs: &[TabId]| {
        assert!(shell.every_window_has_a_tab());
        let actual_tabs: BTreeSet<_> = shell
            .workspaces
            .iter()
            .flat_map(|item| item.tabs())
            .map(|tab| tab.id)
            .collect();
        assert_eq!(actual_tabs, expected_tabs.iter().copied().collect());

        let mut actual_bindings = BTreeMap::new();
        for item in &shell.workspaces {
            for window in &item.windows {
                assert_eq!(window.workspace_id, item.id);
                assert!(!window.tabs.is_empty());
                for tab in &window.tabs {
                    for pane in tab.panes.values() {
                        if let Some(execution) = pane.execution {
                            assert!(actual_bindings.insert(pane.id, execution).is_none());
                            let bound = shell.panes_bound_to(execution);
                            assert_eq!(bound.len(), 1, "execution must have one leaf");
                            assert_eq!(bound[0].2, pane.id);
                        }
                    }
                }
            }
            if let Some(active) = item.active_window {
                assert!(item.window(active).is_some());
            } else {
                assert!(item.windows.is_empty());
            }
        }
        assert_eq!(actual_bindings, bindings);
    };

    let original_generation = shell.containment_generation();
    assert_invariants(&shell, &tabs);

    // Reorder C before A, then prove a structural action from the old snapshot
    // is rejected without changing any containment, focus, or execution binding.
    shell
        .apply(ShellAction::MoveTabBefore {
            tab: tabs[2],
            before: Some(tabs[0]),
            window: windows[0],
            containment_generation: shell.containment_generation(),
        })
        .expect("reorder within the source Window");
    assert_eq!(shell.containment_generation(), original_generation + 1);
    assert_invariants(&shell, &tabs);

    let mut expected = shell.clone();
    expected.last_error = Some(ShellError::StaleContainment);
    assert_eq!(
        shell.apply(ShellAction::MoveTabToWindow {
            tab: tabs[1],
            window: windows[1],
            containment_generation: original_generation,
        }),
        Err(ShellError::StaleContainment)
    );
    assert_eq!(shell, expected, "stale rejection must be atomic");
    assert_invariants(&shell, &tabs);

    // Continue with fresh generations through cross-Window moves and reorder.
    for action in [
        ShellAction::MoveTabToWindow {
            tab: tabs[1],
            window: windows[1],
            containment_generation: shell.containment_generation(),
        },
        ShellAction::MoveTabBefore {
            tab: tabs[1],
            before: Some(tabs[3]),
            window: windows[1],
            containment_generation: shell.containment_generation() + 1,
        },
        ShellAction::MoveTabToWindow {
            tab: tabs[0],
            window: windows[1],
            containment_generation: shell.containment_generation() + 2,
        },
        ShellAction::MoveTabToWindow {
            tab: tabs[2],
            window: windows[1],
            containment_generation: shell.containment_generation() + 3,
        },
    ] {
        let before = shell.containment_generation();
        shell
            .apply(action)
            .expect("freshly fenced structural action");
        assert_eq!(shell.containment_generation(), before + 1);
        assert_invariants(&shell, &tabs);
    }
    assert!(shell
        .workspaces
        .iter()
        .flat_map(|item| &item.windows)
        .all(|window| window.id != windows[0]));

    let moved_to_new = shell.containment_generation();
    shell
        .apply(ShellAction::MoveTabToNewWindow {
            tab: tabs[0],
            containment_generation: moved_to_new,
        })
        .expect("move Tab from a multi-Tab Window into a new Window");
    assert_eq!(shell.containment_generation(), moved_to_new + 1);
    assert_invariants(&shell, &tabs);
}

#[test]
fn rejection_is_atomic_including_unknown_tab_move() {
    let mut shell = seed_two_workspaces();
    let mut expected = shell.clone();
    expected.last_error = Some(ShellError::UnknownTab);
    let _ = shell.apply(ShellAction::MoveTabBefore {
        tab: TabId::new(),
        before: None,
        window: shell.product_window_id().unwrap(),
        containment_generation: shell.containment_generation(),
    });
    assert_eq!(
        shell, expected,
        "rejection preserves the complete product state"
    );
}

#[test]
fn close_focused_pane_records_successor_before_other_workspace_recency() {
    let mut shell = seed_two_workspaces();
    let workspace = WorkspaceId::m001_default();
    let other = other_workspace();
    let closing_pane = shell.snapshot().focused_pane;
    let other_pane = shell
        .workspaces
        .iter()
        .find(|item| item.id == other)
        .and_then(|item| item.tabs().next())
        .map(|tab| tab.focused)
        .expect("other workspace pane");

    shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Right,
            containment_generation: shell.containment_generation(),
        })
        .unwrap();
    let successor = shell.snapshot().focused_pane;

    // Arrange a valid history where the removed Pane is newest and the
    // other Workspace is older than the close successor.
    shell.focus_history.purge_if(|_| true);
    shell.focus_history.record(successor);
    shell.focus_history.record(other_pane);
    shell.focus_history.record(closing_pane);
    assert_eq!(shell.active_workspace_id(), workspace);

    shell
        .apply(ShellAction::ClosePane {
            id: closing_pane,
            containment_generation: shell.containment_generation(),
        })
        .unwrap();

    assert_eq!(shell.focus_history.panes().first(), Some(&successor));
    assert_eq!(shell.active_workspace_id(), workspace);
    assert_eq!(shell.snapshot().focused_pane, successor);
}

#[test]
fn close_active_tab_records_successor_before_other_workspace_recency() {
    let mut shell = seed_two_workspaces();
    let workspace = WorkspaceId::m001_default();
    let other = other_workspace();
    let successor_tab = shell.snapshot().active_tab;
    let successor_pane = shell.snapshot().focused_pane;
    let other_pane = shell
        .workspaces
        .iter()
        .find(|item| item.id == other)
        .and_then(|item| item.tabs().next())
        .map(|tab| tab.focused)
        .expect("other workspace pane");

    shell.apply_product_create_tab().unwrap();
    let closing_tab = shell.snapshot().active_tab;
    let closing_pane = shell.snapshot().focused_pane;

    shell.focus_history.purge_if(|_| true);
    shell.focus_history.record(successor_pane);
    shell.focus_history.record(other_pane);
    shell.focus_history.record(closing_pane);
    assert_eq!(shell.active_workspace_id(), workspace);

    shell
        .apply(ShellAction::CloseTab {
            id: closing_tab,
            containment_generation: shell.containment_generation(),
        })
        .unwrap();

    assert_eq!(shell.focus_history.panes().first(), Some(&successor_pane));
    assert_eq!(shell.active_workspace_id(), workspace);
    assert_eq!(shell.snapshot().active_tab, successor_tab);
    assert_eq!(shell.snapshot().focused_pane, successor_pane);
}

#[test]
fn workspace_activation_falls_back_to_retained_active_window_after_history_eviction() {
    let mut shell = seed_two_workspaces();
    let workspace = WorkspaceId::m001_default();
    shell
        .apply(ShellAction::CreateWindow {
            workspace,
            containment_generation: shell.containment_generation(),
        })
        .unwrap();
    let recently_active = shell.product_window_id().unwrap();

    // Evict every live Pane identity from the bounded history.
    for _ in 0..80 {
        shell.focus_history.record(PaneId::new());
    }

    assert_eq!(
        shell.derived_mru_window(workspace),
        Some(recently_active),
        "re-entry should use the Workspace's retained active Window"
    );
}

#[test]
fn rejected_structural_actions_preserve_full_state() {
    let shell = seed_two_workspaces();
    let workspace = WorkspaceId::m001_default();
    let other = other_workspace();
    let tab = shell.snapshot().active_tab;
    let window = shell.product_window_id().unwrap();
    let other_window = shell
        .workspaces
        .iter()
        .find(|item| item.id == other)
        .and_then(|item| item.active_window)
        .expect("other workspace window");
    let generation = shell.containment_generation();
    let cases = [
        (
            ShellAction::MoveTabBefore {
                tab,
                before: Some(TabId::new()),
                window,
                containment_generation: generation,
            },
            ShellError::UnknownTab,
        ),
        (
            ShellAction::MoveTabToWindow {
                tab,
                window: WindowId::new(),
                containment_generation: generation,
            },
            ShellError::UnknownWindow,
        ),
        (
            ShellAction::MoveTabToWindow {
                tab,
                window: other_window,
                containment_generation: generation,
            },
            ShellError::CrossWorkspaceMove,
        ),
        (
            ShellAction::CreateWindow {
                workspace: WorkspaceId::from_bytes([0x55; 16]),
                containment_generation: generation,
            },
            ShellError::UnknownWorkspace,
        ),
    ];

    for (action, error) in cases {
        let mut probe = shell.clone();
        let mut expected = shell.clone();
        expected.last_error = Some(error);
        assert_eq!(probe.apply(action), Err(error), "{action:?}");
        assert_eq!(probe, expected, "rejection changed state: {action:?}");
    }

    let mut disabled = seed_two_workspaces();
    disabled.allows_tab_creation = false;
    let window = disabled.product_window_id().unwrap();
    let mut expected = disabled.clone();
    expected.last_error = Some(ShellError::TabCreationUnavailable);
    assert_eq!(
        disabled.apply(ShellAction::CreateTab {
            window,
            containment_generation: disabled.containment_generation(),
        }),
        Err(ShellError::TabCreationUnavailable)
    );
    assert_eq!(
        disabled, expected,
        "disabled CreateTab rejection changed state"
    );
}
