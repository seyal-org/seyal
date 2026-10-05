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
fn production_shell_enables_tab_creation_and_keeps_splits_fail_closed() {
    let mut shell = ShellState::m001_local("/tmp/seyal");
    let snap = shell.snapshot();
    assert_eq!(snap.workspaces.len(), 1);
    assert_eq!(snap.workspaces[0].id, WorkspaceId::m001_default());
    assert_eq!(snap.tabs.len(), 1);
    assert_eq!(snap.layout, LayoutDescription::Single);
    assert_eq!(snap.panes.len(), 1);
    assert_eq!(snap.panes[0].title, "Pane 1");
    assert!(snap.panes[0].allows_implicit_bootstrap);
    // C2b / #1175: production CreateTab is enabled; splits stay fail-closed until C3.
    assert!(shell.allows_tab_creation());
    assert!(!shell.allows_pane_splitting());
    // Hierarchical close is always admitted while a Window exists (W2b).
    assert!(snap.allows_tab_close);
    assert!(snap.allows_pane_close);
    shell
        .apply_product_create_tab()
        .expect("production shell admits CreateTab");
    assert_eq!(shell.snapshot().tabs.len(), 2);
    let focused = shell.snapshot().focused_pane;
    assert_eq!(
        shell.apply(ShellAction::SplitPane {
            id: focused,
            axis: SplitAxis::Right
        }),
        Err(ShellError::PaneSplitUnavailable)
    );
    assert_eq!(shell.snapshot().layout, LayoutDescription::Single);
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
        shell.apply(ShellAction::SelectTab { id: TabId::new() }),
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
