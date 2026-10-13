//! SPEC-022 §12 items 24–27: cross-window WindowActivation (N5).

use std::collections::HashMap;

use seyal_core::{PaneId, TabId, WindowId, WorkspaceId};

use crate::shell::{
    ShellNativeEffect, ShellPaneSeed, ShellState, ShellTabSeed, ShellWindowSeed, ShellWorkspaceSeed,
};

use super::{
    navigate, ExecutionPresence, NavigateHistory, NavigationPrincipal, ResolvedTarget,
    ResourceAddress,
};

struct MapInventory {
    records: HashMap<seyal_core::ExecutionId, ExecutionPresence>,
}

impl MapInventory {
    fn new() -> Self {
        Self {
            records: HashMap::new(),
        }
    }
}

impl super::ExecutionInventory for MapInventory {
    fn presence(&self, execution: seyal_core::ExecutionId) -> Option<ExecutionPresence> {
        self.records.get(&execution).copied()
    }
}

fn seed_two_windows() -> (
    ShellState,
    WorkspaceId,
    TabId,
    TabId,
    PaneId,
    PaneId,
    WindowId,
    WindowId,
) {
    let workspace = WorkspaceId::m001_default();
    let t1 = TabId::new();
    let t2 = TabId::new();
    let p1 = PaneId::new();
    let p2 = PaneId::new();
    let win1 = WindowId::new();
    let win2 = WindowId::new();
    let shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: workspace,
            name: "Local".to_owned(),
            detail: None,
            attention: false,
            active_window: win1,
            windows: vec![
                ShellWindowSeed {
                    id: win1,
                    active_tab: t1,
                    tabs: vec![ShellTabSeed {
                        id: t1,
                        title: "One".to_owned(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: p1,
                            title: "A".to_owned(),
                            allows_implicit_execution_bootstrap: true,
                        },
                    }],
                },
                ShellWindowSeed {
                    id: win2,
                    active_tab: t2,
                    tabs: vec![ShellTabSeed {
                        id: t2,
                        title: "Two".to_owned(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: p2,
                            title: "B".to_owned(),
                            allows_implicit_execution_bootstrap: false,
                        },
                    }],
                },
            ],
        }],
        workspace,
        false,
        true,
        false,
    )
    .expect("two windows");
    (shell, workspace, t1, t2, p1, p2, win1, win2)
}

fn placement_map(shell: &ShellState) -> Vec<(WindowId, Vec<TabId>)> {
    shell
        .snapshot()
        .windows
        .iter()
        .map(|window| {
            (
                window.id,
                window.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>(),
            )
        })
        .collect()
}

#[test]
fn navigate_other_window_emits_one_window_activation_without_reparenting() {
    let (mut shell, workspace, _, t2, _, p2, win1, win2) = seed_two_windows();
    let before_map = placement_map(&shell);
    assert_eq!(shell.snapshot().active_window, win1);
    let _ = shell.take_effects();

    assert_eq!(
        navigate(
            ResourceAddress::Pane {
                workspace,
                tab: t2,
                pane: p2,
            },
            &mut shell,
            &MapInventory::new(),
            NavigationPrincipal::local_user(),
            NavigateHistory::ApplyOnly,
        ),
        Ok(ResolvedTarget::Pane {
            workspace,
            tab: t2,
            pane: p2
        })
    );
    let after = shell.snapshot();
    assert_eq!(after.active_window, win2);
    assert_eq!(after.active_tab, t2);
    assert_eq!(after.focused_pane, p2);
    let effects = shell.take_effects();
    let activations: Vec<_> = effects
        .iter()
        .filter_map(|effect| match effect {
            ShellNativeEffect::WindowActivation { window } => Some(*window),
            _ => None,
        })
        .collect();
    assert_eq!(activations, [win2]);
    assert_eq!(placement_map(&shell), before_map);
    assert_eq!(shell.window_of_tab(t2), Some(win2));
}

#[test]
fn navigate_same_window_emits_no_window_activation() {
    let workspace = WorkspaceId::m001_default();
    let t1 = TabId::new();
    let t2 = TabId::new();
    let p1 = PaneId::new();
    let p2 = PaneId::new();
    let win1 = WindowId::new();
    let win2 = WindowId::new();
    let t_other = TabId::new();
    let p_other = PaneId::new();
    let mut shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: workspace,
            name: "Local".to_owned(),
            detail: None,
            attention: false,
            active_window: win1,
            windows: vec![
                ShellWindowSeed {
                    id: win1,
                    active_tab: t1,
                    tabs: vec![
                        ShellTabSeed {
                            id: t1,
                            title: "One".to_owned(),
                            attention: false,
                            pane: ShellPaneSeed {
                                id: p1,
                                title: "A".to_owned(),
                                allows_implicit_execution_bootstrap: true,
                            },
                        },
                        ShellTabSeed {
                            id: t2,
                            title: "Two".to_owned(),
                            attention: false,
                            pane: ShellPaneSeed {
                                id: p2,
                                title: "B".to_owned(),
                                allows_implicit_execution_bootstrap: false,
                            },
                        },
                    ],
                },
                ShellWindowSeed {
                    id: win2,
                    active_tab: t_other,
                    tabs: vec![ShellTabSeed {
                        id: t_other,
                        title: "Other".to_owned(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: p_other,
                            title: "C".to_owned(),
                            allows_implicit_execution_bootstrap: false,
                        },
                    }],
                },
            ],
        }],
        workspace,
        false,
        true,
        false,
    )
    .expect("fixture");
    let _ = shell.take_effects();

    navigate(
        ResourceAddress::Pane {
            workspace,
            tab: t2,
            pane: p2,
        },
        &mut shell,
        &MapInventory::new(),
        NavigationPrincipal::local_user(),
        NavigateHistory::ApplyOnly,
    )
    .expect("same-window tab");
    assert!(
        shell.take_effects().is_empty(),
        "R5.2 emits WindowActivation only when the hosting window is not active"
    );
    assert_eq!(shell.snapshot().active_window, win1);
    assert_eq!(shell.snapshot().focused_pane, p2);
}

#[test]
fn reported_activation_failure_keeps_committed_focus() {
    use crate::app::{AppAction, AppError, ApplicationRoot, NativeEffect, WindowNativeEvent};

    let (shell, workspace, _, t2, _, p2, _, win2) = seed_two_windows();
    let mut root = ApplicationRoot::with_shell(shell);
    while !root.snapshot().pending_effects.is_empty() {
        root.apply(AppAction::AckEffect).unwrap();
    }
    root.apply(AppAction::Navigate {
        fence: root.fence(),
        address: ResourceAddress::Pane {
            workspace,
            tab: t2,
            pane: p2,
        },
    })
    .unwrap();
    let snap = root.snapshot();
    assert_eq!(snap.shell.active_window, win2);
    assert_eq!(snap.shell.focused_pane, p2);
    let activations: Vec<_> = snap
        .pending_effects
        .iter()
        .filter(|effect| matches!(effect, NativeEffect::WindowActivation { .. }))
        .copied()
        .collect();
    assert_eq!(
        activations,
        [NativeEffect::WindowActivation { window: win2 }]
    );

    assert_eq!(
        root.apply(AppAction::ReportWindowEvent {
            window: win2,
            event: WindowNativeEvent::ActivationFailed,
        }),
        Err(AppError::WindowActivationFailed)
    );
    let after = root.snapshot();
    assert_eq!(after.last_error, Some(AppError::WindowActivationFailed));
    assert_eq!(after.shell.active_window, win2);
    assert_eq!(after.shell.focused_pane, p2);
    assert_eq!(after.shell.active_tab, t2);
}

#[test]
fn resource_address_and_recency_history_omit_window_id() {
    // SPEC-022 R6.2: addresses and recency entries never store WindowId.
    // N3 FocusHistory is a separate PR; this branch's recency store is PaneId.
    let address = ResourceAddress::Pane {
        workspace: WorkspaceId::m001_default(),
        tab: TabId::new(),
        pane: PaneId::new(),
    };
    match address {
        ResourceAddress::Workspace { workspace: _ }
        | ResourceAddress::Tab {
            workspace: _,
            tab: _,
        }
        | ResourceAddress::Pane {
            workspace: _,
            tab: _,
            pane: _,
        }
        | ResourceAddress::Execution { execution: _ } => {}
    }
}
