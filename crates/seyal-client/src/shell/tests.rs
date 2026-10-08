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

#[test]
fn production_shell_enables_tab_creation_and_pane_splitting() {
    let mut shell = ShellState::m001_local("/tmp/seyal");
    let snap = shell.snapshot();
    assert_eq!(snap.workspaces.len(), 1);
    assert_eq!(snap.workspaces[0].id, WorkspaceId::m001_default());
    assert_eq!(snap.tabs.len(), 1);
    assert_eq!(snap.layout, LayoutDescription::Single);
    assert_eq!(snap.panes.len(), 1);
    assert_eq!(snap.panes[0].title, "Pane 1");
    assert!(snap.panes[0].allows_implicit_bootstrap);
    // C2b / #1175 + C3 / #1217: production CreateTab and SplitFocused are enabled.
    assert!(shell.allows_tab_creation());
    assert!(shell.allows_pane_splitting());
    assert!(!snap.allows_tab_close);
    assert!(!snap.allows_pane_close);
    shell
        .apply_product_create_tab()
        .expect("production shell admits CreateTab");
    assert_eq!(shell.snapshot().tabs.len(), 2);
    let focused = shell.snapshot().focused_pane;
    shell
        .apply(ShellAction::SplitPane {
            id: focused,
            axis: SplitAxis::Right,
            containment_generation: shell.containment_generation(),
        })
        .expect("production shell admits SplitPane");
    assert_ne!(shell.snapshot().layout, LayoutDescription::Single);
    assert_eq!(shell.snapshot().panes.len(), 2);
}

#[test]
fn close_enablement_is_projected_from_the_same_rule_close_enforces() {
    let mut shell = seed_two_workspaces();
    let single = shell.snapshot();
    assert!(!single.allows_tab_close);
    assert!(!single.allows_pane_close);

    shell.apply_product_create_tab().expect("tabs allowed");
    let two_tabs = shell.snapshot();
    assert!(two_tabs.allows_tab_close);
    assert!(!two_tabs.allows_pane_close);

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
    assert!(!shell.snapshot().allows_pane_close);
    shell
        .apply(ShellAction::CloseTab {
            id: two_tabs.active_tab,
            containment_generation: shell.containment_generation(),
        })
        .expect("close created tab");
    let closed = shell.snapshot();
    assert!(!closed.allows_tab_close);
    assert_eq!(
        shell.apply(ShellAction::CloseTab {
            id: closed.active_tab,
            containment_generation: shell.containment_generation(),
        }),
        Err(ShellError::CannotCloseLastTab)
    );
    assert_eq!(
        shell.apply(ShellAction::ClosePane {
            id: closed.focused_pane,
            containment_generation: shell.containment_generation(),
        }),
        Err(ShellError::CannotCloseLastPane)
    );
}

#[test]
fn select_create_close_tabs_are_authoritative() {
    let mut shell = seed_two_workspaces();
    let before = shell.snapshot();
    shell.apply_product_create_tab().expect("tabs allowed");
    let after_create = shell.snapshot();
    assert_eq!(after_create.tabs.len(), 2);
    assert_ne!(after_create.active_tab, before.active_tab);
    let created = after_create.active_tab;
    shell
        .apply(ShellAction::SelectTab {
            id: before.active_tab,
            containment_generation: shell.containment_generation(),
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
    assert_eq!(
        shell.apply(ShellAction::CloseTab {
            id: before.active_tab,
            containment_generation: shell.containment_generation(),
        }),
        Err(ShellError::CannotCloseLastTab)
    );
}

#[test]
fn stale_close_tab_is_rejected_without_mutation() {
    let mut shell = seed_two_workspaces();
    let tab = shell.snapshot().active_tab;
    let stale_generation = shell.containment_generation();
    shell.apply_product_create_tab().expect("create tab");
    let before_rejected_close = shell.snapshot();

    assert_eq!(
        shell.apply(ShellAction::CloseTab {
            id: tab,
            containment_generation: stale_generation,
        }),
        Err(ShellError::StaleContainment)
    );
    let after_rejected_close = shell.snapshot();
    assert_eq!(after_rejected_close.tabs, before_rejected_close.tabs);
    assert_eq!(
        after_rejected_close.containment_generation,
        before_rejected_close.containment_generation
    );
    assert!(shell.take_removed_tab_panes().is_empty());
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
    assert_eq!(
        shell.apply(ShellAction::ClosePane {
            id: original,
            containment_generation: shell.containment_generation(),
        }),
        Err(ShellError::CannotCloseLastPane)
    );
}

#[test]
fn execution_bound_pane_close_releases_binding() {
    let mut shell = seed_two_workspaces();
    let bound = shell.snapshot().focused_pane;
    let execution = ExecutionId::from_bytes([7; 16]);
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
        .expect("close bound releases presentation");
    assert_eq!(shell.take_released_execution(), Some((bound, execution)));
    let snap = shell.snapshot();
    assert_eq!(snap.layout, LayoutDescription::Single);
    assert_eq!(snap.focused_pane, created);
    assert!(snap.panes[0].execution.is_none());
}

#[test]
fn workspace_selection_switches_tab_inventory() {
    let mut shell = seed_two_workspaces();
    let first = shell.snapshot();
    shell
        .apply_activate_workspace(other_workspace())
        .expect("select second workspace");
    let second = shell.snapshot();
    assert_eq!(second.active_workspace, other_workspace());
    assert_eq!(second.tabs[0].title, "API");
    assert_ne!(second.active_tab, first.active_tab);
    assert!(second.workspaces[1].attention);
}

#[test]
fn stale_identities_fail_closed_and_leave_state() {
    let mut shell = seed_two_workspaces();
    let before = shell.snapshot();
    assert_eq!(
        shell.apply_activate_workspace(WorkspaceId::from_bytes([0xff; 16])),
        Err(ShellError::UnknownWorkspace)
    );
    assert_eq!(
        shell.apply(ShellAction::SelectTab {
            id: TabId::new(),
            containment_generation: shell.containment_generation(),
        }),
        Err(ShellError::UnknownTab)
    );
    assert_eq!(
        shell.apply(ShellAction::FocusPane { id: PaneId::new() }),
        Err(ShellError::UnknownPane)
    );
    assert_eq!(shell.snapshot().active_workspace, before.active_workspace);
    assert_eq!(shell.snapshot().active_tab, before.active_tab);
    assert_eq!(shell.snapshot().focused_pane, before.focused_pane);
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
            ratio: SplitRatio::HALF,
        },
    );
    assert_eq!(tree.pane_ids(), vec![a, b]);
    assert_eq!(tree.layout_description(), LayoutDescription::SplitRight);
}
