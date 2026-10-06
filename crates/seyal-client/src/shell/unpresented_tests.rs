//! ADR-018 §3.3 / W6 reducer tests. Construct Unpresented states directly.

use seyal_core::{ExecutionId, PaneId, TabId, WindowId, WorkspaceId};

use super::{
    ShellAction, ShellError, ShellPaneSeed, ShellState, ShellTabSeed, ShellWindowSeed,
    ShellWorkspaceSeed, SplitAxis,
};

fn workspace_a() -> WorkspaceId {
    WorkspaceId::m001_default()
}

fn workspace_b() -> WorkspaceId {
    WorkspaceId::from_bytes([0x22; 16])
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
                name: "Local".to_owned(),
                detail: None,
                attention: false,
                active_window: first_window,
                windows: vec![ShellWindowSeed {
                    id: first_window,
                    active_tab: first_tab,
                    tabs: vec![ShellTabSeed {
                        id: first_tab,
                        title: "Terminal".to_owned(),
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
        workspace_a(),
        true,
        true,
        true,
    )
    .expect("seed")
}

#[test]
fn enumerate_zero_one_and_n_is_deterministic() {
    let mut shell = seed_two_workspaces();
    assert!(shell.live_unpresented(workspace_a()).is_empty());

    let low = ExecutionId::from_bytes([0x01; 16]);
    let high = ExecutionId::from_bytes([0xfe; 16]);
    let mid = ExecutionId::from_bytes([0x80; 16]);
    // Insert out of order; enumeration must sort by ExecutionId.
    shell
        .apply(ShellAction::RecordUnpresented {
            execution: high,
            workspace: workspace_a(),
        })
        .unwrap();
    shell
        .apply(ShellAction::RecordUnpresented {
            execution: low,
            workspace: workspace_a(),
        })
        .unwrap();
    assert_eq!(shell.live_unpresented(workspace_a()), vec![low, high]);

    shell
        .apply(ShellAction::RecordUnpresented {
            execution: mid,
            workspace: workspace_a(),
        })
        .unwrap();
    assert_eq!(shell.live_unpresented(workspace_a()), vec![low, mid, high]);
    // Other workspace stays empty — no unstable cross-workspace auto-pick.
    assert!(shell.live_unpresented(workspace_b()).is_empty());
}

#[test]
fn adopt_preserves_execution_id_and_rejects_already_bound() {
    let mut shell = seed_two_workspaces();
    let pane = shell.snapshot().focused_pane;
    let execution = ExecutionId::from_bytes([0x44; 16]);
    shell
        .apply(ShellAction::RecordUnpresented {
            execution,
            workspace: workspace_a(),
        })
        .unwrap();
    shell
        .apply(ShellAction::AdoptExecution { pane, execution })
        .expect("adopt");
    assert_eq!(shell.pane_execution(pane).unwrap(), Some(execution));
    assert!(shell.live_unpresented(workspace_a()).is_empty());

    let before = shell.containment_generation();
    // Already bound → invariant 4 reason, even though the catalog entry is gone.
    assert_eq!(
        shell.apply(ShellAction::AdoptExecution { pane, execution }),
        Err(ShellError::ExecutionAlreadyBound)
    );
    assert_eq!(
        shell.apply(ShellAction::BindExecution {
            pane,
            execution: ExecutionId::from_bytes([0x55; 16]),
        }),
        Err(ShellError::ExecutionAlreadyBound)
    );
    assert_eq!(shell.containment_generation(), before);
    assert_eq!(shell.pane_execution(pane).unwrap(), Some(execution));
}

#[test]
fn adopt_rejects_execution_already_bound_elsewhere() {
    let mut shell = seed_two_workspaces();
    shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Right,
        })
        .unwrap();
    let second = shell.snapshot().focused_pane;
    let first = shell
        .snapshot()
        .panes
        .iter()
        .find(|pane| pane.id != second)
        .unwrap()
        .id;
    let execution = ExecutionId::new();
    shell
        .apply(ShellAction::BindExecution {
            pane: first,
            execution,
        })
        .unwrap();
    assert_eq!(
        shell.apply(ShellAction::RecordUnpresented {
            execution,
            workspace: workspace_a(),
        }),
        Err(ShellError::ExecutionAlreadyBound)
    );
    assert_eq!(
        shell.apply(ShellAction::AdoptExecution {
            pane: second,
            execution,
        }),
        Err(ShellError::ExecutionAlreadyBound)
    );
}

#[test]
fn adopt_rejects_cross_workspace() {
    let mut shell = seed_two_workspaces();
    let pane = shell.snapshot().focused_pane;
    let execution = ExecutionId::new();
    shell
        .apply(ShellAction::RecordUnpresented {
            execution,
            workspace: workspace_b(),
        })
        .unwrap();
    let before = shell.containment_generation();
    assert_eq!(
        shell.apply(ShellAction::AdoptExecution { pane, execution }),
        Err(ShellError::CrossWorkspaceAdopt)
    );
    assert_eq!(shell.containment_generation(), before);
    assert_eq!(shell.live_unpresented(workspace_b()), vec![execution]);
}

#[test]
fn adopt_rejects_retired_or_finalized() {
    let mut shell = seed_two_workspaces();
    let pane = shell.snapshot().focused_pane;
    let execution = ExecutionId::new();
    assert_eq!(
        shell.apply(ShellAction::AdoptExecution { pane, execution }),
        Err(ShellError::ExecutionNotUnpresented)
    );
    shell
        .apply(ShellAction::RecordUnpresented {
            execution,
            workspace: workspace_a(),
        })
        .unwrap();
    shell
        .apply(ShellAction::ForgetUnpresented { execution })
        .unwrap();
    assert_eq!(
        shell.apply(ShellAction::AdoptExecution { pane, execution }),
        Err(ShellError::ExecutionNotUnpresented)
    );
}

#[test]
fn terminate_is_distinct_and_not_emitted_by_close() {
    let mut shell = seed_two_workspaces();
    shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Right,
        })
        .unwrap();
    let created = shell.snapshot().focused_pane;
    shell
        .apply(ShellAction::ClosePane {
            id: created,
            containment_generation: shell.containment_generation(),
        })
        .unwrap();
    assert!(shell.take_effects().is_empty());

    let execution = ExecutionId::new();
    shell
        .apply(ShellAction::RecordUnpresented {
            execution,
            workspace: workspace_a(),
        })
        .unwrap();
    shell
        .apply(ShellAction::TerminateExecution { execution })
        .unwrap();
    assert!(
        shell.take_effects().is_empty(),
        "dispose is a ProvisioningEffect AttachController path, not a host NativeEffect"
    );
    assert!(shell.live_unpresented(workspace_a()).is_empty());
    assert_eq!(
        shell.apply(ShellAction::TerminateExecution { execution }),
        Err(ShellError::ExecutionNotUnpresented)
    );
}

#[test]
fn replace_live_unpresented_drops_bound_and_retired() {
    let mut shell = seed_two_workspaces();
    let pane = shell.snapshot().focused_pane;
    let kept = ExecutionId::from_bytes([0x10; 16]);
    let bound = ExecutionId::from_bytes([0x20; 16]);
    let retired = ExecutionId::from_bytes([0x30; 16]);
    shell
        .apply(ShellAction::RecordUnpresented {
            execution: retired,
            workspace: workspace_a(),
        })
        .unwrap();
    shell
        .apply(ShellAction::BindExecution {
            pane,
            execution: bound,
        })
        .unwrap();
    shell.replace_live_unpresented([(kept, workspace_a()), (bound, workspace_a())]);
    assert_eq!(shell.live_unpresented(workspace_a()), vec![kept]);
}
