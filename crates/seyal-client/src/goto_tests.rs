//! SPEC-022 §12 items 28–30 and N4 scope-filter coverage for the goto surface.

use seyal_core::{ExecutionId, PaneId, TabId, WorkspaceId};

use crate::app::{AppAction, ApplicationRoot};
use crate::goto::{GotoAction, GotoScope, GotoState, GOTO_ENUMERATION_BOUND};
use crate::navigation::ResourceAddress;
use crate::shell::{ShellAction, ShellPaneSeed, ShellState, ShellTabSeed, ShellWorkspaceSeed};

fn twin_shell() -> (
    ShellState,
    WorkspaceId,
    WorkspaceId,
    TabId,
    TabId,
    PaneId,
    PaneId,
) {
    let w1 = WorkspaceId::from_bytes([0xa1; 16]);
    let w2 = WorkspaceId::from_bytes([0xa2; 16]);
    let t1 = TabId::from_bytes([0xb1; 16]);
    let t2 = TabId::from_bytes([0xb2; 16]);
    let p1 = PaneId::from_bytes([0xc1; 16]);
    let p2 = PaneId::from_bytes([0xc2; 16]);
    let shell = ShellState::from_workspaces(
        vec![
            ShellWorkspaceSeed {
                id: w1,
                name: "Twin".into(),
                detail: None,
                attention: false,
                active_tab: t1,
                tabs: vec![ShellTabSeed {
                    id: t1,
                    title: "Shared".into(),
                    attention: false,
                    pane: ShellPaneSeed {
                        id: p1,
                        title: "Shared".into(),
                        allows_implicit_execution_bootstrap: true,
                    },
                }],
            },
            ShellWorkspaceSeed {
                id: w2,
                name: "Twin".into(),
                detail: None,
                attention: true,
                active_tab: t2,
                tabs: vec![ShellTabSeed {
                    id: t2,
                    title: "Shared".into(),
                    attention: true,
                    pane: ShellPaneSeed {
                        id: p2,
                        title: "Shared".into(),
                        allows_implicit_execution_bootstrap: false,
                    },
                }],
            },
        ],
        w1,
        true,
        true,
    )
    .expect("fixture");
    (shell, w1, w2, t1, t2, p1, p2)
}

fn open_scope(state: &mut GotoState, shell: &ShellState, scope: GotoScope) {
    state.apply(GotoAction::Open { scope }, 0).expect("open");
    state.rebuild(&shell.navigation_inventory());
}

/// SPEC-022 §12 item 28: identical labels remain independently addressable.
#[test]
fn identical_labels_remain_independently_addressable() {
    let (shell, w1, w2, t1, t2, p1, p2) = twin_shell();
    let inventory = shell.navigation_inventory();
    let mut goto = GotoState::new();

    open_scope(&mut goto, &shell, GotoScope::Workspaces);
    let rows = goto.snapshot().rows;
    assert_eq!(rows.len(), 2);
    // Primary display names are identical; state badges may differ. Addresses
    // must still be distinct so run cannot confuse the two resources.
    assert!(rows[0].label.starts_with("Twin"));
    assert!(rows[1].label.starts_with("Twin"));
    assert_ne!(rows[0].address, rows[1].address);
    assert_eq!(
        rows[0].address,
        ResourceAddress::Workspace { workspace: w1 }
    );
    assert_eq!(
        rows[1].address,
        ResourceAddress::Workspace { workspace: w2 }
    );

    open_scope(&mut goto, &shell, GotoScope::Tabs);
    let tabs = goto.snapshot().rows;
    assert_eq!(tabs.len(), 2);
    assert_ne!(tabs[0].address, tabs[1].address);
    assert_eq!(
        tabs[0].address,
        ResourceAddress::Tab {
            workspace: w1,
            tab: t1
        }
    );
    assert_eq!(
        tabs[1].address,
        ResourceAddress::Tab {
            workspace: w2,
            tab: t2
        }
    );

    open_scope(&mut goto, &shell, GotoScope::Panes);
    let panes = goto.snapshot().rows;
    assert_eq!(panes.len(), 2);
    assert_ne!(panes[0].address, panes[1].address);
    assert_eq!(
        panes[0].address,
        ResourceAddress::Pane {
            workspace: w1,
            tab: t1,
            pane: p1
        }
    );
    assert_eq!(
        panes[1].address,
        ResourceAddress::Pane {
            workspace: w2,
            tab: t2,
            pane: p2
        }
    );

    // Same primary title text appears under Workspaces and Tabs scopes with
    // different address kinds — proof labels are not identity.
    assert!(inventory.workspaces.iter().all(|w| w.name == "Twin"));
    assert!(inventory.tabs.iter().all(|t| t.title == "Shared"));
}

/// SPEC-022 §12 item 29: rename between projection and run does not retarget.
#[test]
fn rename_between_projection_and_run_keeps_stored_address() {
    let (mut shell, w1, w2, _t1, t2, _p1, p2) = twin_shell();
    let mut goto = GotoState::new();
    open_scope(&mut goto, &shell, GotoScope::Workspaces);
    goto.apply(GotoAction::SetQuery("Twin · attention".into()), 0)
        .unwrap();
    goto.rebuild(&shell.navigation_inventory());
    let snap = goto.snapshot();
    assert_eq!(snap.rows.len(), 1);
    let address = snap.rows[0].address;
    assert_eq!(address, ResourceAddress::Workspace { workspace: w2 });

    shell
        .rename_workspace_for_test(w2, "Renamed")
        .expect("rename workspace");
    shell
        .rename_tab_for_test(t2, "Renamed Tab")
        .expect("rename tab");
    shell
        .rename_pane_for_test(p2, "Renamed Pane")
        .expect("rename pane");

    // Frozen selection still carries w2; live labels changed.
    assert_eq!(goto.selected_address(), Some(address));
    let live = shell.navigation_inventory();
    assert_eq!(
        live.workspaces.iter().find(|w| w.id == w2).unwrap().name,
        "Renamed"
    );

    let mut root = ApplicationRoot::with_shell(shell);
    root.apply(AppAction::Navigate {
        fence: root.fence(),
        address,
    })
    .expect("navigate stored address");
    assert_eq!(root.snapshot().shell.active_workspace, w2);
    assert_eq!(root.snapshot().shell.active_workspace, w2);
    // Original active workspace was w1; we navigated to renamed w2 by address.
    assert_ne!(w1, w2);
    assert_eq!(root.snapshot().shell.focused_pane, p2);
}

/// SPEC-022 §12 item 30: truncation is reported and stable.
#[test]
fn truncated_enumeration_is_reported_and_stable() {
    let mut tabs = Vec::new();
    let workspace = WorkspaceId::from_bytes([0xd1; 16]);
    let active_tab = TabId::from_bytes([0; 16]);
    for i in 0..(GOTO_ENUMERATION_BOUND + 5) {
        let mut bytes = [0u8; 16];
        bytes[0] = (i + 1) as u8;
        let tab = if i == 0 {
            active_tab
        } else {
            TabId::from_bytes(bytes)
        };
        let mut pane_bytes = [0u8; 16];
        pane_bytes[0] = (i + 1) as u8;
        pane_bytes[1] = 0xff;
        tabs.push(ShellTabSeed {
            id: tab,
            title: format!("Tab-{i:03}"),
            attention: false,
            pane: ShellPaneSeed {
                id: PaneId::from_bytes(pane_bytes),
                title: format!("Pane-{i:03}"),
                allows_implicit_execution_bootstrap: true,
            },
        });
    }
    let shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: workspace,
            name: "Wide".into(),
            detail: None,
            attention: false,
            active_tab,
            tabs,
        }],
        workspace,
        true,
        true,
    )
    .expect("fixture");

    let mut goto = GotoState::new();
    open_scope(&mut goto, &shell, GotoScope::Tabs);
    let first = goto.snapshot();
    assert!(first.truncated, "overflow must be reported, not silent");
    assert_eq!(first.rows.len(), GOTO_ENUMERATION_BOUND);
    let addresses: Vec<_> = first.rows.iter().map(|r| r.address).collect();

    // Repeated projection of identical state is byte-stable.
    goto.rebuild(&shell.navigation_inventory());
    let second = goto.snapshot();
    assert!(second.truncated);
    assert_eq!(second.rows.len(), GOTO_ENUMERATION_BOUND);
    assert_eq!(
        second.rows.iter().map(|r| r.address).collect::<Vec<_>>(),
        addresses
    );
    assert_eq!(
        second
            .rows
            .iter()
            .map(|r| r.label.clone())
            .collect::<Vec<_>>(),
        first
            .rows
            .iter()
            .map(|r| r.label.clone())
            .collect::<Vec<_>>()
    );
}

fn assert_scope_only_kind(scope: GotoScope, rows: &[crate::goto::GotoRow]) {
    for row in rows {
        match (scope, row.address) {
            (GotoScope::Workspaces, ResourceAddress::Workspace { .. }) => {}
            (GotoScope::Tabs, ResourceAddress::Tab { .. }) => {}
            (GotoScope::Panes, ResourceAddress::Pane { .. }) => {}
            (GotoScope::Sessions, ResourceAddress::Execution { .. }) => {}
            _ => panic!("scope {scope:?} produced foreign address {:?}", row.address),
        }
    }
}

#[test]
fn filter_workspaces_scope_lists_only_workspaces() {
    let (shell, _w1, w2, ..) = twin_shell();
    let mut goto = GotoState::new();
    open_scope(&mut goto, &shell, GotoScope::Workspaces);
    goto.apply(GotoAction::SetQuery("attention".into()), 0)
        .unwrap();
    goto.rebuild(&shell.navigation_inventory());
    let snap = goto.snapshot();
    assert!(!snap.truncated);
    assert_eq!(snap.rows.len(), 1);
    assert_scope_only_kind(GotoScope::Workspaces, &snap.rows);
    assert_eq!(
        snap.rows[0].address,
        ResourceAddress::Workspace { workspace: w2 }
    );
}

#[test]
fn filter_tabs_scope_lists_only_tabs() {
    let (shell, ..) = twin_shell();
    let mut goto = GotoState::new();
    open_scope(&mut goto, &shell, GotoScope::Tabs);
    goto.apply(GotoAction::SetQuery("Shared · Twin".into()), 0)
        .unwrap();
    goto.rebuild(&shell.navigation_inventory());
    let snap = goto.snapshot();
    assert_eq!(snap.rows.len(), 2);
    assert_scope_only_kind(GotoScope::Tabs, &snap.rows);
}

#[test]
fn filter_panes_scope_lists_only_panes() {
    let (shell, ..) = twin_shell();
    let mut goto = GotoState::new();
    open_scope(&mut goto, &shell, GotoScope::Panes);
    goto.apply(GotoAction::SetQuery("focused".into()), 0)
        .unwrap();
    goto.rebuild(&shell.navigation_inventory());
    let snap = goto.snapshot();
    assert_eq!(snap.rows.len(), 1);
    assert_scope_only_kind(GotoScope::Panes, &snap.rows);
    assert!(snap.rows[0].label.contains("focused"));
}

#[test]
fn filter_sessions_scope_lists_only_executions() {
    let (mut shell, w1, _w2, t1, _t2, p1, _p2) = twin_shell();
    let execution = ExecutionId::from_bytes([0xe1; 16]);
    shell
        .apply(ShellAction::BindExecution {
            pane: p1,
            execution,
        })
        .expect("bind");
    let mut goto = GotoState::new();
    open_scope(&mut goto, &shell, GotoScope::Sessions);
    goto.apply(GotoAction::SetQuery("live".into()), 0).unwrap();
    goto.rebuild(&shell.navigation_inventory());
    let snap = goto.snapshot();
    assert_eq!(snap.rows.len(), 1);
    assert_scope_only_kind(GotoScope::Sessions, &snap.rows);
    assert_eq!(
        snap.rows[0].address,
        ResourceAddress::Execution { execution }
    );
    // Session row still navigates by the stored execution address through
    // ApplicationRoot's Navigate commit (no second resolver).
    let mut root = ApplicationRoot::with_shell(shell);
    // Focus the other workspace first so Navigate must move.
    root.apply(AppAction::Navigate {
        fence: root.fence(),
        address: ResourceAddress::Workspace {
            workspace: WorkspaceId::from_bytes([0xa2; 16]),
        },
    })
    .unwrap();
    assert_ne!(root.snapshot().shell.active_workspace, w1);
    root.apply(AppAction::OpenGoto {
        fence: root.fence(),
        scope: GotoScope::Sessions,
    })
    .unwrap();
    root.apply(AppAction::RunGoto {
        fence: root.fence(),
        address: Some(ResourceAddress::Execution { execution }),
    })
    .unwrap();
    assert_eq!(root.snapshot().shell.active_workspace, w1);
    assert_eq!(root.snapshot().shell.active_tab, t1);
    assert_eq!(root.snapshot().shell.focused_pane, p1);
    assert!(!root.snapshot().goto.open);
}
