//! SPEC-032 window sidebar reducer tests.

use seyal_core::{ExecutionId, PaneId, TabId, WindowId, WorkspaceId};

use super::{
    ShellAction, ShellError, ShellNativeEffect, ShellPaneSeed, ShellState, ShellTabSeed,
    ShellWindowSeed, ShellWorkspaceSeed, SplitAxis,
};

fn other_workspace() -> WorkspaceId {
    WorkspaceId::from_bytes([0x11; 16])
}

/// Unrealized pane-tree storage opens in the host; realized homes raise (AC6).
fn mark_home_unrealized(shell: &mut ShellState, workspace: WorkspaceId) {
    let item = shell
        .workspaces
        .iter_mut()
        .find(|item| item.id == workspace)
        .expect("workspace");
    let window = item.windows.first_mut().expect("home window");
    window.realized = false;
}

fn seed_two_workspaces() -> ShellState {
    let first = WorkspaceId::m001_default();
    let second = other_workspace();
    let first_window = WindowId::new();
    let second_window = WindowId::new();
    let first_tab = TabId::new();
    let second_tab = TabId::new();
    let first_pane = PaneId::new();
    let second_pane = PaneId::new();
    ShellState::from_workspaces(
        vec![
            ShellWorkspaceSeed {
                id: first,
                name: "Core Terminal".to_owned(),
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
                            allows_implicit_execution_bootstrap: false,
                        },
                    }],
                }],
            },
            ShellWorkspaceSeed {
                id: second,
                name: "API".to_owned(),
                detail: None,
                attention: false,
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
        true,
    )
    .expect("fixture")
}

#[test]
fn create_workspace_opens_in_the_current_window_without_realize() {
    let mut shell = ShellState::m001_local("local");
    let host = shell.product_window_id().unwrap();
    let before_windows = shell.snapshot().windows.len();
    let generation = shell.containment_generation();
    let _ = shell.take_effects();
    let id = WorkspaceId::from_bytes([0x41; 16]);
    let root = std::env::temp_dir().join("seyal-sidebar-create");
    let _ = std::fs::create_dir_all(&root);
    shell
        .create_workspace(id, root.to_string_lossy().into_owned(), generation)
        .expect("create");
    let snap = shell.snapshot();
    assert_eq!(snap.active_window, Some(host));
    assert_eq!(snap.selected_workspace, id);
    assert_eq!(snap.open_workspaces, vec![WorkspaceId::m001_default(), id]);
    assert_eq!(snap.windows.len(), before_windows);
    assert_eq!(snap.panes.len(), 1, "new workspace projects one pane tree");
    assert_ne!(
        snap.focused_pane,
        PaneId::from_bytes([0; 16]),
        "selected workspace must project a real focused pane"
    );
    assert!(shell.take_effects().iter().all(|effect| {
        !matches!(
            effect,
            ShellNativeEffect::RealizeWindow { .. } | ShellNativeEffect::OrderFrontMakeKey { .. }
        )
    }));
}

#[test]
fn close_last_pane_removes_workspace_from_sidebar() {
    let mut shell = seed_two_workspaces();
    mark_home_unrealized(&mut shell, other_workspace());
    let host = shell.product_window_id().unwrap();
    let first = WorkspaceId::m001_default();
    let second = other_workspace();
    let second_pane = shell
        .workspaces
        .iter()
        .find(|workspace| workspace.id == second)
        .and_then(|workspace| workspace.tabs().next())
        .map(|tab| tab.focused)
        .expect("second pane");
    shell
        .apply(ShellAction::ActivateWorkspace {
            workspace: second,
            containment_generation: shell.containment_generation(),
        })
        .expect("open unrealized second in host sidebar");
    assert_eq!(shell.snapshot().open_workspaces, vec![first, second]);
    assert_eq!(shell.snapshot().selected_workspace, second);
    assert_eq!(shell.snapshot().panes.len(), 1);
    assert_eq!(shell.snapshot().focused_pane, second_pane);

    shell
        .apply(ShellAction::ClosePane {
            id: second_pane,
            containment_generation: shell.containment_generation(),
        })
        .expect("closing last pane closes the workspace");

    let after = shell.snapshot();
    assert_eq!(after.active_window, Some(host), "host window stays");
    assert_eq!(after.open_workspaces, vec![first]);
    assert_eq!(after.selected_workspace, first);
    assert_eq!(
        after.panes.len(),
        1,
        "successor workspace pane tree is shown"
    );
    assert_ne!(after.focused_pane, second_pane);
    assert!(shell
        .workspaces
        .iter()
        .any(|workspace| workspace.id == second));
}

#[test]
fn remove_workspace_detaches_without_terminating_bindings() {
    let mut shell = seed_two_workspaces();
    mark_home_unrealized(&mut shell, other_workspace());
    let host = shell.product_window_id().unwrap();
    let other = other_workspace();
    let pane = shell
        .workspaces
        .iter()
        .find(|workspace| workspace.id == other)
        .and_then(|workspace| workspace.tabs().next())
        .map(|tab| tab.focused)
        .expect("other pane");
    let execution = ExecutionId::from_bytes([0x99; 16]);
    shell
        .apply(ShellAction::BindExecution { pane, execution })
        .unwrap();
    shell
        .apply(ShellAction::ActivateWorkspace {
            workspace: other,
            containment_generation: shell.containment_generation(),
        })
        .unwrap();
    let _ = shell.take_effects();
    shell
        .apply(ShellAction::RemoveWorkspace {
            window: host,
            workspace: other,
            containment_generation: shell.containment_generation(),
        })
        .expect("remove");
    let snap = shell.snapshot();
    assert_eq!(snap.open_workspaces, vec![WorkspaceId::m001_default()]);
    assert_eq!(snap.selected_workspace, WorkspaceId::m001_default());
    assert_eq!(snap.active_window, Some(host));
    assert!(shell
        .workspaces
        .iter()
        .any(|workspace| workspace.id == other));
    assert_eq!(shell.pane_execution(pane).unwrap(), Some(execution));
}

#[test]
fn open_rejects_copying_into_a_second_window_and_raises_presenter() {
    // AC6: solitary realized home raises; absorbing into another host is forbidden.
    let mut shell = seed_two_workspaces();
    let first = shell.product_window_id().unwrap();
    let other = other_workspace();
    let other_home = shell
        .workspaces
        .iter()
        .find(|workspace| workspace.id == other)
        .and_then(|workspace| workspace.active_window)
        .expect("other home");
    let _ = shell.take_effects();
    shell
        .apply(ShellAction::ActivateWorkspace {
            workspace: other,
            containment_generation: shell.containment_generation(),
        })
        .expect("raise solitary realized home");
    assert_eq!(shell.snapshot().active_window, Some(other_home));
    assert!(!shell
        .snapshot()
        .windows
        .iter()
        .any(|window| { window.id == first && window.open_workspaces.contains(&other) }));
    shell.set_allows_window_creation_for_test(true);
    shell
        .apply(ShellAction::CreateWindow {
            workspace: WorkspaceId::m001_default(),
            containment_generation: shell.containment_generation(),
        })
        .unwrap();
    let second = shell.product_window_id().unwrap();
    assert_ne!(second, other_home);
    let _ = shell.take_effects();
    shell
        .apply(ShellAction::ActivateWorkspace {
            workspace: other,
            containment_generation: shell.containment_generation(),
        })
        .expect("raise the presenting window again");
    assert_eq!(shell.snapshot().active_window, Some(other_home));
    assert_eq!(shell.snapshot().selected_workspace, other);
    assert!(!shell
        .snapshot()
        .windows
        .iter()
        .any(|window| { window.id == second && window.open_workspaces.contains(&other) }));
    assert!(shell.take_effects().iter().any(|effect| {
        matches!(effect, ShellNativeEffect::OrderFrontMakeKey { window } if *window == other_home)
    }));
}

#[test]
fn focus_split_and_tab_in_foreign_workspace_keep_presentation_host() {
    let mut shell = seed_two_workspaces();
    mark_home_unrealized(&mut shell, other_workspace());
    shell.allows_pane_splitting = true;
    let host = shell.product_window_id().unwrap();
    let other = other_workspace();
    let storage = shell
        .workspaces
        .iter()
        .find(|workspace| workspace.id == other)
        .and_then(|workspace| workspace.active_window)
        .expect("unrealized storage");
    shell
        .apply(ShellAction::ActivateWorkspace {
            workspace: other,
            containment_generation: shell.containment_generation(),
        })
        .expect("open foreign workspace on host");
    assert_eq!(shell.product_window_id().unwrap(), host);
    assert_eq!(shell.sidebar_host, Some(host));

    let tab = shell.snapshot().active_tab;
    let pane = shell.snapshot().focused_pane;
    shell
        .commit_focus(other, tab, pane)
        .expect("focus selected foreign pane");
    assert_eq!(
        shell.sidebar_host,
        Some(host),
        "focus must not retarget sidebar_host to unrealized storage"
    );
    assert_ne!(shell.sidebar_host, Some(storage));
    assert_eq!(shell.product_window_id().unwrap(), host);

    shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Right,
            containment_generation: shell.containment_generation(),
        })
        .expect("split in foreign workspace");
    assert_eq!(shell.sidebar_host, Some(host));
    assert_eq!(shell.product_window_id().unwrap(), host);
    let split_snap = shell.snapshot();
    assert_eq!(split_snap.panes.len(), 2);
    shell
        .commit_focus(other, split_snap.active_tab, split_snap.focused_pane)
        .expect("focus after split");
    assert_eq!(shell.sidebar_host, Some(host));

    // Create a second terminal tab inside the foreign storage tree, then focus.
    let foreign_window = storage;
    shell
        .apply(ShellAction::CreateTab {
            window: foreign_window,
            containment_generation: shell.containment_generation(),
        })
        .expect("new tab on foreign storage window");
    let after_tab = shell.snapshot();
    assert_eq!(after_tab.tabs.len(), 2);
    shell
        .commit_focus(other, after_tab.active_tab, after_tab.focused_pane)
        .expect("focus after tab switch");
    assert_eq!(
        shell.sidebar_host,
        Some(host),
        "tab switch in foreign workspace must keep presentation host"
    );
    assert_eq!(shell.product_window_id().unwrap(), host);
}

#[test]
fn sidebar_title_text_does_not_authorize_selection() {
    let mut shell = seed_two_workspaces();
    let before = shell.snapshot();
    assert_eq!(
        shell.apply(ShellAction::ActivateWorkspace {
            workspace: WorkspaceId::from_bytes([0xee; 16]),
            containment_generation: shell.containment_generation(),
        }),
        Err(ShellError::UnknownWorkspace)
    );
    assert_eq!(shell.snapshot().open_workspaces, before.open_workspaces);
}

#[test]
fn new_window_record_starts_with_one_workspace_and_no_copied_binding() {
    let mut shell = ShellState::m001_local("local");
    let root = std::env::temp_dir().join("seyal-sidebar-new-window");
    let _ = std::fs::create_dir_all(&root);
    let path = root.to_string_lossy().into_owned();
    shell
        .set_workspace_root(WorkspaceId::m001_default(), path.clone())
        .unwrap();
    let first_pane = shell.snapshot().focused_pane;
    let execution = ExecutionId::from_bytes([0x77; 16]);
    shell
        .apply(ShellAction::BindExecution {
            pane: first_pane,
            execution,
        })
        .unwrap();
    shell.set_allows_window_creation_for_test(true);
    let id = WorkspaceId::from_bytes([0x5b; 16]);
    shell.record_workspace(id, path).unwrap();
    shell
        .apply(ShellAction::CreateWindow {
            workspace: id,
            containment_generation: shell.containment_generation(),
        })
        .expect("new window");
    let snap = shell.snapshot();
    assert_eq!(snap.active_workspace, id);
    assert_eq!(snap.open_workspaces, vec![id]);
    assert_eq!(snap.windows.len(), 2);
    assert!(snap.panes.iter().all(|pane| pane.execution.is_none()));
    assert_eq!(shell.pane_execution(first_pane).unwrap(), Some(execution));
}

#[test]
fn migration_keeps_active_tab_and_unpresents_other_tab_executions() {
    let window = WindowId::new();
    let first = TabId::new();
    let second = TabId::new();
    let first_pane = PaneId::new();
    let second_pane = PaneId::new();
    let workspace = WorkspaceId::m001_default();
    let mut shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: workspace,
            name: "Local".to_owned(),
            detail: None,
            attention: false,
            active_window: window,
            windows: vec![ShellWindowSeed {
                id: window,
                active_tab: first,
                tabs: vec![
                    ShellTabSeed {
                        id: first,
                        title: "Keep".to_owned(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: first_pane,
                            title: "Pane 1".to_owned(),
                            allows_implicit_execution_bootstrap: false,
                        },
                    },
                    ShellTabSeed {
                        id: second,
                        title: "Drop".to_owned(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: second_pane,
                            title: "Pane 1".to_owned(),
                            allows_implicit_execution_bootstrap: false,
                        },
                    },
                ],
            }],
        }],
        workspace,
        false,
        true,
        true,
    )
    .expect("fixture");
    let keep_exec = ExecutionId::from_bytes([0x1a; 16]);
    let drop_exec = ExecutionId::from_bytes([0x1b; 16]);
    shell
        .apply(ShellAction::BindExecution {
            pane: first_pane,
            execution: keep_exec,
        })
        .unwrap();
    shell
        .apply(ShellAction::SelectTab {
            id: second,
            containment_generation: shell.containment_generation(),
        })
        .unwrap();
    shell
        .apply(ShellAction::BindExecution {
            pane: second_pane,
            execution: drop_exec,
        })
        .unwrap();
    shell
        .apply(ShellAction::SelectTab {
            id: first,
            containment_generation: shell.containment_generation(),
        })
        .unwrap();
    shell.migrate_seyal_tabs_to_sidebar_containment();
    let snap = shell.snapshot();
    assert_eq!(snap.tabs.len(), 1);
    assert_eq!(snap.active_tab, first);
    assert_eq!(snap.tabs[0].title, "Keep");
    assert_eq!(shell.pane_execution(first_pane).unwrap(), Some(keep_exec));
    assert!(shell.pane_execution(second_pane).is_err());
    let unpresented = shell.unpresented_executions_for_test();
    assert!(unpresented.contains(&(drop_exec, workspace)));
}
