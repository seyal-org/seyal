//! SPEC-022 §12 items 1–7 for N1 address + pure resolver.

use std::collections::HashMap;
use std::mem::{size_of, size_of_val};

use seyal_core::{ExecutionId, PaneId, TabId, WindowId, WorkspaceId};

use crate::shell::{
    ShellAction, ShellPaneSeed, ShellState, ShellTabSeed, ShellWindowSeed, ShellWorkspaceSeed,
    SplitAxis,
};

use super::{
    decode_resource_address, navigate, resolve, ExecutionPresence, NavigateHistory,
    NavigationPrincipal, NavigationRejection, ResolvedTarget, ResourceAddress, WorkspaceAccess,
    RESOURCE_ADDRESS_ABI_VERSION, RESOURCE_ADDRESS_KIND_EXECUTION, RESOURCE_ADDRESS_KIND_PANE,
    RESOURCE_ADDRESS_KIND_TAB, RESOURCE_ADDRESS_KIND_WORKSPACE,
};

struct MapInventory {
    records: HashMap<ExecutionId, ExecutionPresence>,
}

impl MapInventory {
    fn new() -> Self {
        Self {
            records: HashMap::new(),
        }
    }

    fn with(execution: ExecutionId, presence: ExecutionPresence) -> Self {
        let mut inventory = Self::new();
        inventory.records.insert(execution, presence);
        inventory
    }
}

impl super::ExecutionInventory for MapInventory {
    fn presence(&self, execution: ExecutionId) -> Option<ExecutionPresence> {
        self.records.get(&execution).copied()
    }
}

fn workspace_b() -> WorkspaceId {
    WorkspaceId::from_bytes([0x22; 16])
}

fn seed_shell() -> (
    ShellState,
    WorkspaceId,
    WorkspaceId,
    TabId,
    TabId,
    PaneId,
    PaneId,
) {
    let w1 = WorkspaceId::m001_default();
    let w2 = workspace_b();
    let t1 = TabId::new();
    let t2 = TabId::new();
    let p1 = PaneId::new();
    let p2 = PaneId::new();
    let t_other = TabId::new();
    let p_other = PaneId::new();
    let win1 = WindowId::new();
    let win2 = WindowId::new();
    let shell = ShellState::from_workspaces(
        vec![
            ShellWorkspaceSeed {
                id: w1,
                name: "Alpha".to_owned(),
                detail: Some("shared-label".to_owned()),
                attention: false,
                active_window: win1,
                windows: vec![ShellWindowSeed {
                    id: win1,
                    active_tab: t1,
                    tabs: vec![
                        ShellTabSeed {
                            id: t1,
                            title: "Tab One".to_owned(),
                            attention: false,
                            pane: ShellPaneSeed {
                                id: p1,
                                title: "Pane A".to_owned(),
                                allows_implicit_execution_bootstrap: true,
                            },
                        },
                        ShellTabSeed {
                            id: t2,
                            title: "Tab Two".to_owned(),
                            attention: false,
                            pane: ShellPaneSeed {
                                id: p2,
                                title: "Pane B".to_owned(),
                                allows_implicit_execution_bootstrap: false,
                            },
                        },
                    ],
                }],
            },
            ShellWorkspaceSeed {
                id: w2,
                name: "Alpha".to_owned(),
                detail: Some("shared-label".to_owned()),
                attention: false,
                active_window: win2,
                windows: vec![ShellWindowSeed {
                    id: win2,
                    active_tab: t_other,
                    tabs: vec![ShellTabSeed {
                        id: t_other,
                        title: "Other".to_owned(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: p_other,
                            title: "Other Pane".to_owned(),
                            allows_implicit_execution_bootstrap: false,
                        },
                    }],
                }],
            },
        ],
        w1,
        true,
        true,
        false,
    )
    .expect("fixture");
    (shell, w1, w2, t1, t2, p1, p2)
}

// --- §12.1 type shape -------------------------------------------------------

#[test]
fn resource_address_is_copy_and_holds_no_string() {
    fn assert_copy<T: Copy>(value: T) -> T {
        value
    }

    let workspace = ResourceAddress::Workspace {
        workspace: WorkspaceId::m001_default(),
    };
    let tab = ResourceAddress::Tab {
        workspace: WorkspaceId::m001_default(),
        tab: TabId::new(),
    };
    let pane = ResourceAddress::Pane {
        workspace: WorkspaceId::m001_default(),
        tab: TabId::new(),
        pane: PaneId::new(),
    };
    let execution = ResourceAddress::Execution {
        execution: ExecutionId::new(),
    };

    let _ = assert_copy(workspace);
    let _ = assert_copy(tab);
    let _ = assert_copy(pane);
    let _ = assert_copy(execution);

    // Copy types cannot contain String/PathBuf; size stays a small ID pack.
    assert!(size_of::<ResourceAddress>() <= 64);
    assert_eq!(size_of_val(&workspace), size_of::<ResourceAddress>());
}

// --- §12.2 equality ---------------------------------------------------------

#[test]
fn equality_is_component_wise_and_ignores_labels() {
    let w1 = WorkspaceId::m001_default();
    let w2 = workspace_b();
    let t1 = TabId::from_bytes([0x01; 16]);
    let t2 = TabId::from_bytes([0x02; 16]);
    let p1 = PaneId::from_bytes([0x03; 16]);
    let p2 = PaneId::from_bytes([0x04; 16]);
    let e1 = ExecutionId::from_bytes([0x05; 16]);
    let e2 = ExecutionId::from_bytes([0x06; 16]);

    assert_eq!(
        ResourceAddress::Workspace { workspace: w1 },
        ResourceAddress::Workspace { workspace: w1 }
    );
    assert_ne!(
        ResourceAddress::Workspace { workspace: w1 },
        ResourceAddress::Workspace { workspace: w2 }
    );
    assert_ne!(
        ResourceAddress::Tab {
            workspace: w1,
            tab: t1
        },
        ResourceAddress::Tab {
            workspace: w1,
            tab: t2
        }
    );
    assert_ne!(
        ResourceAddress::Pane {
            workspace: w1,
            tab: t1,
            pane: p1
        },
        ResourceAddress::Pane {
            workspace: w1,
            tab: t1,
            pane: p2
        }
    );
    assert_ne!(
        ResourceAddress::Execution { execution: e1 },
        ResourceAddress::Execution { execution: e2 }
    );
    // Distinct kinds with overlapping bytes still differ.
    assert_ne!(
        ResourceAddress::Workspace { workspace: w1 },
        ResourceAddress::Execution {
            execution: ExecutionId::from_bytes(w1.to_bytes())
        }
    );

    // Display labels are not part of identity: two workspaces sharing a name
    // remain unequal by id (seed_shell uses identical names).
    let (shell, a, b, _, _, _, _) = seed_shell();
    let snap = shell.snapshot();
    assert_eq!(snap.workspaces[0].name, snap.workspaces[1].name);
    assert_ne!(
        ResourceAddress::Workspace { workspace: a },
        ResourceAddress::Workspace { workspace: b }
    );
}

// --- §12.3 unsupported kind -------------------------------------------------

#[test]
fn unknown_kind_version_or_size_is_unsupported_without_reading_state() {
    assert_eq!(
        decode_resource_address(99, RESOURCE_ADDRESS_KIND_WORKSPACE, &[0; 16]),
        Err(NavigationRejection::UnsupportedKind)
    );
    assert_eq!(
        decode_resource_address(RESOURCE_ADDRESS_ABI_VERSION, 99, &[0; 16]),
        Err(NavigationRejection::UnsupportedKind)
    );
    assert_eq!(
        decode_resource_address(
            RESOURCE_ADDRESS_ABI_VERSION,
            RESOURCE_ADDRESS_KIND_WORKSPACE,
            &[0; 15]
        ),
        Err(NavigationRejection::UnsupportedKind)
    );
    assert_eq!(
        decode_resource_address(
            RESOURCE_ADDRESS_ABI_VERSION,
            RESOURCE_ADDRESS_KIND_TAB,
            &[0; 16]
        ),
        Err(NavigationRejection::UnsupportedKind)
    );
    assert_eq!(
        decode_resource_address(
            RESOURCE_ADDRESS_ABI_VERSION,
            RESOURCE_ADDRESS_KIND_PANE,
            &[0; 32]
        ),
        Err(NavigationRejection::UnsupportedKind)
    );
    assert_eq!(
        decode_resource_address(
            RESOURCE_ADDRESS_ABI_VERSION,
            RESOURCE_ADDRESS_KIND_EXECUTION,
            &[0; 17]
        ),
        Err(NavigationRejection::UnsupportedKind)
    );
}

// --- §12.4 each rejection variant -------------------------------------------

#[test]
fn rejection_navigation_denied() {
    let (mut shell, w1, w2, _, _, p1, _) = seed_shell();
    let denied = NavigationPrincipal {
        local_navigation: true,
        workspaces: WorkspaceAccess::Only(&[]),
    };
    assert_eq!(
        resolve(
            ResourceAddress::Workspace { workspace: w1 },
            &shell,
            &MapInventory::new(),
            denied
        ),
        Err(NavigationRejection::NavigationDenied)
    );
    assert_eq!(
        resolve(
            ResourceAddress::Workspace {
                workspace: WorkspaceId::from_bytes([0xff; 16])
            },
            &shell,
            &MapInventory::new(),
            denied
        ),
        Err(NavigationRejection::NavigationDenied)
    );

    // SPEC-022 test 13a(b): one binding outside the principal's Workspace set.
    let single = ExecutionId::from_bytes([0x21; 16]);
    shell
        .apply(ShellAction::BindExecution {
            pane: p1,
            execution: single,
        })
        .expect("bind single");
    let only_w2 = NavigationPrincipal {
        local_navigation: true,
        workspaces: WorkspaceAccess::Only(std::slice::from_ref(&w2)),
    };
    assert_eq!(
        resolve(
            ResourceAddress::Execution { execution: single },
            &shell,
            &MapInventory::with(single, ExecutionPresence::Live),
            only_w2
        ),
        Err(NavigationRejection::NavigationDenied)
    );

    // Same rule for n >= 2: AmbiguousTarget must not leak unauthorized shape.
    let (mut shell_multi, _, w2b, _, _, p_multi, _) = seed_shell();
    let multi = ExecutionId::from_bytes([0x22; 16]);
    shell_multi
        .apply(ShellAction::BindExecution {
            pane: p_multi,
            execution: multi,
        })
        .expect("bind first for multi");
    shell_multi
        .apply(ShellAction::SplitPane {
            id: p_multi,
            axis: SplitAxis::Right,
        })
        .expect("split");
    let second = shell_multi
        .snapshot()
        .panes
        .iter()
        .map(|pane| pane.id)
        .find(|id| *id != p_multi)
        .expect("split pane");
    shell_multi
        .overwrite_pane_execution_for_test(second, multi)
        .expect("defensive dual bind for AmbiguousTarget");
    let only_w2_multi = NavigationPrincipal {
        local_navigation: true,
        workspaces: WorkspaceAccess::Only(std::slice::from_ref(&w2b)),
    };
    assert_eq!(
        resolve(
            ResourceAddress::Execution { execution: multi },
            &shell_multi,
            &MapInventory::with(multi, ExecutionPresence::Live),
            only_w2_multi
        ),
        Err(NavigationRejection::NavigationDenied)
    );
}

#[test]
fn rejection_unknown_workspace() {
    let (shell, _, _, _, _, _, _) = seed_shell();
    assert_eq!(
        resolve(
            ResourceAddress::Workspace {
                workspace: WorkspaceId::from_bytes([0xab; 16])
            },
            &shell,
            &MapInventory::new(),
            NavigationPrincipal::local_user()
        ),
        Err(NavigationRejection::UnknownWorkspace)
    );
}

#[test]
fn rejection_unknown_tab() {
    let (shell, w1, _, _, _, _, _) = seed_shell();
    assert_eq!(
        resolve(
            ResourceAddress::Tab {
                workspace: w1,
                tab: TabId::from_bytes([0xcd; 16])
            },
            &shell,
            &MapInventory::new(),
            NavigationPrincipal::local_user()
        ),
        Err(NavigationRejection::UnknownTab)
    );
}

#[test]
fn rejection_unknown_pane() {
    let (shell, w1, _, t1, _, _, _) = seed_shell();
    assert_eq!(
        resolve(
            ResourceAddress::Pane {
                workspace: w1,
                tab: t1,
                pane: PaneId::from_bytes([0xef; 16])
            },
            &shell,
            &MapInventory::new(),
            NavigationPrincipal::local_user()
        ),
        Err(NavigationRejection::UnknownPane)
    );
}

#[test]
fn rejection_unknown_execution() {
    let (shell, _, _, _, _, _, _) = seed_shell();
    let missing = ExecutionId::from_bytes([0x10; 16]);
    assert_eq!(
        resolve(
            ResourceAddress::Execution { execution: missing },
            &shell,
            &MapInventory::new(),
            NavigationPrincipal::local_user()
        ),
        Err(NavigationRejection::UnknownExecution)
    );
}

#[test]
fn rejection_not_composed_variant() {
    let (shell, w1, _, t1, _, _, p2) = seed_shell();
    // p2 lives under t2; requesting it under t1 is NotComposed.
    assert_eq!(
        resolve(
            ResourceAddress::Pane {
                workspace: w1,
                tab: t1,
                pane: p2
            },
            &shell,
            &MapInventory::new(),
            NavigationPrincipal::local_user()
        ),
        Err(NavigationRejection::NotComposed)
    );
}

#[test]
fn rejection_target_terminated() {
    let (shell, _, _, _, _, _, _) = seed_shell();
    let exited = ExecutionId::from_bytes([0x11; 16]);
    assert_eq!(
        resolve(
            ResourceAddress::Execution { execution: exited },
            &shell,
            &MapInventory::with(exited, ExecutionPresence::ExitedHeld),
            NavigationPrincipal::local_user()
        ),
        Err(NavigationRejection::TargetTerminated)
    );
}

#[test]
fn rejection_target_unbound() {
    let (shell, _, _, _, _, _, _) = seed_shell();
    let live = ExecutionId::from_bytes([0x12; 16]);
    assert_eq!(
        resolve(
            ResourceAddress::Execution { execution: live },
            &shell,
            &MapInventory::with(live, ExecutionPresence::Live),
            NavigationPrincipal::local_user()
        ),
        Err(NavigationRejection::TargetUnbound)
    );
}

#[test]
fn rejection_ambiguous_target() {
    let (mut shell, _, _, _, _, p1, _) = seed_shell();
    let execution = ExecutionId::from_bytes([0x13; 16]);
    shell
        .apply(ShellAction::BindExecution {
            pane: p1,
            execution,
        })
        .expect("bind first");
    shell
        .apply(ShellAction::SplitPane {
            id: p1,
            axis: SplitAxis::Right,
        })
        .expect("split");
    let second = shell
        .snapshot()
        .panes
        .iter()
        .map(|pane| pane.id)
        .find(|id| *id != p1)
        .expect("split pane");
    shell
        .overwrite_pane_execution_for_test(second, execution)
        .expect("defensive dual bind for AmbiguousTarget");
    assert_eq!(
        resolve(
            ResourceAddress::Execution { execution },
            &shell,
            &MapInventory::with(execution, ExecutionPresence::Live),
            NavigationPrincipal::local_user()
        ),
        Err(NavigationRejection::AmbiguousTarget)
    );
}

#[test]
fn rejection_unsupported_kind_variant() {
    assert_eq!(
        decode_resource_address(0, RESOURCE_ADDRESS_KIND_WORKSPACE, &[0; 16]),
        Err(NavigationRejection::UnsupportedKind)
    );
}

// --- §12.5 NotComposed leaves focus unchanged --------------------------------

#[test]
fn not_composed_pane_under_wrong_tab_leaves_focus_unchanged() {
    let (shell, w1, _, t1, t2, _, p2) = seed_shell();
    let before = shell.snapshot();
    assert_eq!(before.active_tab, t1);
    let focused = before.focused_pane;

    let result = resolve(
        ResourceAddress::Pane {
            workspace: w1,
            tab: t1,
            pane: p2,
        },
        &shell,
        &MapInventory::new(),
        NavigationPrincipal::local_user(),
    );
    assert_eq!(result, Err(NavigationRejection::NotComposed));

    let after = shell.snapshot();
    assert_eq!(after, before);
    assert_eq!(after.focused_pane, focused);
    assert_eq!(after.active_tab, t1);
    // Sanity: p2 really belongs to t2.
    assert_eq!(shell.location_of_pane(p2), Some((w1, t2)));
}

// --- §12.6 resolution purity ------------------------------------------------

#[test]
fn resolution_never_mutates_shell_state() {
    let (mut shell, w1, w2, t1, t2, p1, p2) = seed_shell();
    let execution = ExecutionId::from_bytes([0x14; 16]);
    shell
        .apply(ShellAction::BindExecution {
            pane: p1,
            execution,
        })
        .expect("bind");
    let before = shell.snapshot();
    let inventory = MapInventory::with(execution, ExecutionPresence::Live);
    let principal = NavigationPrincipal::local_user();

    let cases = [
        ResourceAddress::Workspace { workspace: w1 },
        ResourceAddress::Workspace { workspace: w2 },
        ResourceAddress::Tab {
            workspace: w1,
            tab: t1,
        },
        ResourceAddress::Tab {
            workspace: w1,
            tab: t2,
        },
        ResourceAddress::Pane {
            workspace: w1,
            tab: t1,
            pane: p1,
        },
        ResourceAddress::Pane {
            workspace: w1,
            tab: t2,
            pane: p2,
        },
        ResourceAddress::Execution { execution },
        ResourceAddress::Pane {
            workspace: w1,
            tab: t1,
            pane: p2,
        },
        ResourceAddress::Tab {
            workspace: w2,
            tab: t1,
        },
        ResourceAddress::Execution {
            execution: ExecutionId::from_bytes([0x99; 16]),
        },
    ];

    for address in cases {
        let _ = resolve(address, &shell, &inventory, principal);
        assert_eq!(
            shell.snapshot(),
            before,
            "resolve mutated shell for {address:?}"
        );
    }
}

// --- §12.7 cross-workspace tab ----------------------------------------------

#[test]
fn cross_workspace_tab_rejects_without_silent_correction() {
    let (shell, w1, w2, t1, _, _, _) = seed_shell();
    assert_eq!(shell.workspace_of_tab(t1), Some(w1));

    let result = resolve(
        ResourceAddress::Tab {
            workspace: w2,
            tab: t1,
        },
        &shell,
        &MapInventory::new(),
        NavigationPrincipal::local_user(),
    );
    assert_eq!(result, Err(NavigationRejection::NotComposed));
    // Must not resolve as if the workspace were rewritten to w1.
    assert_ne!(
        result,
        Ok(ResolvedTarget::Tab {
            workspace: w1,
            tab: t1
        })
    );
}

#[test]
fn successful_decode_and_resolve_happy_paths() {
    let (shell, w1, _, t1, _, p1, _) = seed_shell();
    let mut payload = [0_u8; 48];
    payload[0..16].copy_from_slice(&w1.to_bytes());
    payload[16..32].copy_from_slice(&t1.to_bytes());
    payload[32..48].copy_from_slice(&p1.to_bytes());
    let address = decode_resource_address(
        RESOURCE_ADDRESS_ABI_VERSION,
        RESOURCE_ADDRESS_KIND_PANE,
        &payload,
    )
    .expect("decode");
    assert_eq!(
        resolve(
            address,
            &shell,
            &MapInventory::new(),
            NavigationPrincipal::local_user()
        ),
        Ok(ResolvedTarget::Pane {
            workspace: w1,
            tab: t1,
            pane: p1
        })
    );
}

// --- §12.8 successful Navigate across inactive Workspace --------------------

#[test]
fn navigate_pane_in_inactive_workspace_activates_all_in_one_transition() {
    let (mut shell, w1, w2, _, _, p1, _) = seed_shell();
    assert_eq!(shell.snapshot().active_workspace, w1);
    assert_eq!(shell.snapshot().focused_pane, p1);

    let w2_focus = shell.workspace_focus(w2).expect("w2");
    let address = ResourceAddress::Pane {
        workspace: w2,
        tab: w2_focus.active_tab,
        pane: w2_focus.focused_pane,
    };
    assert_eq!(
        navigate(
            address,
            &mut shell,
            &MapInventory::new(),
            NavigationPrincipal::local_user(),
            NavigateHistory::ApplyOnly,
        ),
        Ok(ResolvedTarget::Pane {
            workspace: w2,
            tab: w2_focus.active_tab,
            pane: w2_focus.focused_pane
        })
    );
    let after = shell.snapshot();
    assert_eq!(after.active_workspace, w2);
    assert_eq!(after.active_tab, w2_focus.active_tab);
    assert_eq!(after.focused_pane, w2_focus.focused_pane);
}

// --- §12.9 already-active Pane is success no-op -----------------------------

#[test]
fn navigate_already_active_pane_is_success_noop() {
    let (mut shell, w1, _, t1, _, p1, _) = seed_shell();
    let before = shell.focus_checkpoint();
    assert_eq!(
        navigate(
            ResourceAddress::Pane {
                workspace: w1,
                tab: t1,
                pane: p1
            },
            &mut shell,
            &MapInventory::new(),
            NavigationPrincipal::local_user(),
            NavigateHistory::ApplyOnly,
        ),
        Ok(ResolvedTarget::Pane {
            workspace: w1,
            tab: t1,
            pane: p1
        })
    );
    assert_eq!(shell.focus_checkpoint(), before);
    // Focus unchanged; history recording is ApplyOnly in this N2 regression.
}

// --- §12.10 Navigate never changes presentation/binding/PTY facts -----------

#[test]
fn navigate_does_not_change_bindings() {
    let (mut shell, w1, w2, t1, _, p1, _) = seed_shell();
    let execution = ExecutionId::from_bytes([0x21; 16]);
    shell
        .apply(ShellAction::BindExecution {
            pane: p1,
            execution,
        })
        .expect("bind");
    let before_bound = shell.panes_bound_to(execution);
    assert_eq!(before_bound, vec![(w1, t1, p1)]);
    let w2_focus = shell.workspace_focus(w2).expect("w2");
    navigate(
        ResourceAddress::Pane {
            workspace: w2,
            tab: w2_focus.active_tab,
            pane: w2_focus.focused_pane,
        },
        &mut shell,
        &MapInventory::new(),
        NavigationPrincipal::local_user(),
        NavigateHistory::ApplyOnly,
    )
    .expect("navigate");
    assert_eq!(
        shell.panes_bound_to(execution),
        before_bound,
        "Navigate must not bind/unbind executions"
    );
}

// --- §12.11 atomicity: destroyed Tab leaves Workspace selection unchanged ---

#[test]
fn navigate_rejects_destroyed_tab_without_workspace_change() {
    let (mut shell, w1, w2, _, _, _, _) = seed_shell();
    // Point at a Tab that never existed under w2.
    let ghost = TabId::from_bytes([0xde; 16]);
    let pane = PaneId::from_bytes([0xad; 16]);
    let before = shell.focus_checkpoint();
    assert_eq!(before.active_workspace, w1);
    assert_eq!(
        navigate(
            ResourceAddress::Pane {
                workspace: w2,
                tab: ghost,
                pane
            },
            &mut shell,
            &MapInventory::new(),
            NavigationPrincipal::local_user(),
            NavigateHistory::ApplyOnly,
        ),
        Err(NavigationRejection::UnknownTab)
    );
    assert_eq!(
        shell.focus_checkpoint(),
        before,
        "rejected navigate must not change Workspace selection"
    );
}

// --- §12.12 Execution with no bound Pane → TargetUnbound, no attach ---------

#[test]
fn navigate_unbound_execution_is_target_unbound_without_attach() {
    let (mut shell, _, _, _, _, p1, _) = seed_shell();
    let live = ExecutionId::from_bytes([0x22; 16]);
    let before = shell.focus_checkpoint();
    assert!(shell.panes_bound_to(live).is_empty());
    assert_eq!(
        navigate(
            ResourceAddress::Execution { execution: live },
            &mut shell,
            &MapInventory::with(live, ExecutionPresence::Live),
            NavigationPrincipal::local_user(),
            NavigateHistory::ApplyOnly,
        ),
        Err(NavigationRejection::TargetUnbound)
    );
    assert_eq!(shell.focus_checkpoint(), before);
    assert!(shell.panes_bound_to(live).is_empty());
    assert_eq!(shell.pane_execution(p1).expect("p1"), None);
}

// --- §12.13 exited / ambiguous / destroyed pane matrix ----------------------

#[test]
fn navigate_execution_rejection_matrix_r8_3() {
    let (mut shell, w1, _, t1, _, p1, _) = seed_shell();
    let exited = ExecutionId::from_bytes([0x23; 16]);

    // Exited, record held, no Pane bound → TargetTerminated.
    assert_eq!(
        navigate(
            ResourceAddress::Execution { execution: exited },
            &mut shell,
            &MapInventory::with(exited, ExecutionPresence::ExitedHeld),
            NavigationPrincipal::local_user(),
            NavigateHistory::ApplyOnly,
        ),
        Err(NavigationRejection::TargetTerminated)
    );

    // Exited record released → UnknownExecution.
    assert_eq!(
        navigate(
            ResourceAddress::Execution { execution: exited },
            &mut shell,
            &MapInventory::new(),
            NavigationPrincipal::local_user(),
            NavigateHistory::ApplyOnly,
        ),
        Err(NavigationRejection::UnknownExecution)
    );

    // Exited still bound to exactly one Pane → resolves/navigates to that Pane.
    shell
        .apply(ShellAction::BindExecution {
            pane: p1,
            execution: exited,
        })
        .expect("bind");
    assert_eq!(
        navigate(
            ResourceAddress::Execution { execution: exited },
            &mut shell,
            &MapInventory::with(exited, ExecutionPresence::ExitedHeld),
            NavigationPrincipal::local_user(),
            NavigateHistory::ApplyOnly,
        ),
        Ok(ResolvedTarget::Pane {
            workspace: w1,
            tab: t1,
            pane: p1
        })
    );

    // Bound to two Panes → AmbiguousTarget, no navigation change after split.
    shell
        .apply(ShellAction::SplitPane {
            id: p1,
            axis: SplitAxis::Right,
        })
        .expect("split");
    let second = shell
        .snapshot()
        .panes
        .iter()
        .map(|pane| pane.id)
        .find(|id| *id != p1)
        .expect("split pane");
    shell
        .overwrite_pane_execution_for_test(second, exited)
        .expect("defensive dual bind for AmbiguousTarget");
    let before = shell.focus_checkpoint();
    assert_eq!(
        navigate(
            ResourceAddress::Execution { execution: exited },
            &mut shell,
            &MapInventory::with(exited, ExecutionPresence::ExitedHeld),
            NavigationPrincipal::local_user(),
            NavigateHistory::ApplyOnly,
        ),
        Err(NavigationRejection::AmbiguousTarget)
    );
    assert_eq!(shell.focus_checkpoint(), before);

    // Destroyed Pane address → UnknownPane, never TargetTerminated.
    let gone = PaneId::from_bytes([0x24; 16]);
    assert_eq!(
        navigate(
            ResourceAddress::Pane {
                workspace: w1,
                tab: t1,
                pane: gone
            },
            &mut shell,
            &MapInventory::new(),
            NavigationPrincipal::local_user(),
            NavigateHistory::ApplyOnly,
        ),
        Err(NavigationRejection::UnknownPane)
    );
}

#[test]
fn navigate_unauthorized_principal_is_denied_without_existence_probe() {
    let (mut shell, w1, _, _, _, p1, _) = seed_shell();
    let denied = NavigationPrincipal {
        local_navigation: true,
        workspaces: WorkspaceAccess::Only(&[]),
    };
    let before = shell.focus_checkpoint();
    assert_eq!(
        navigate(
            ResourceAddress::Workspace { workspace: w1 },
            &mut shell,
            &MapInventory::new(),
            denied,
            NavigateHistory::ApplyOnly,
        ),
        Err(NavigationRejection::NavigationDenied)
    );
    assert_eq!(
        navigate(
            ResourceAddress::Workspace {
                workspace: WorkspaceId::from_bytes([0xff; 16])
            },
            &mut shell,
            &MapInventory::new(),
            denied,
            NavigateHistory::ApplyOnly,
        ),
        Err(NavigationRejection::NavigationDenied)
    );
    assert_eq!(shell.focus_checkpoint(), before);

    // §12.13a(b): Execution bound once / twice in a denied Workspace → Denied,
    // never AmbiguousTarget or an existence probe success.
    let exec = ExecutionId::from_bytes([0x55; 16]);
    shell
        .apply(ShellAction::BindExecution {
            pane: p1,
            execution: exec,
        })
        .expect("bind once");
    let after_bind = shell.focus_checkpoint();
    assert_eq!(
        navigate(
            ResourceAddress::Execution { execution: exec },
            &mut shell,
            &MapInventory::with(exec, ExecutionPresence::Live),
            denied,
            NavigateHistory::ApplyOnly,
        ),
        Err(NavigationRejection::NavigationDenied)
    );
    assert_eq!(shell.focus_checkpoint(), after_bind);

    shell
        .apply(ShellAction::SplitPane {
            id: p1,
            axis: SplitAxis::Right,
        })
        .expect("split");
    let second = shell
        .snapshot()
        .panes
        .iter()
        .map(|pane| pane.id)
        .find(|id| *id != p1)
        .expect("split pane");
    shell
        .overwrite_pane_execution_for_test(second, exec)
        .expect("defensive dual bind for NavigationDenied");
    let after_ambiguous_bind = shell.focus_checkpoint();
    assert_eq!(
        navigate(
            ResourceAddress::Execution { execution: exec },
            &mut shell,
            &MapInventory::with(exec, ExecutionPresence::Live),
            denied,
            NavigateHistory::ApplyOnly,
        ),
        Err(NavigationRejection::NavigationDenied)
    );
    assert_eq!(shell.focus_checkpoint(), after_ambiguous_bind);

    // local_navigation: false denies Execution addresses before binding lookup.
    let no_local = NavigationPrincipal {
        local_navigation: false,
        workspaces: WorkspaceAccess::AllLocal,
    };
    assert_eq!(
        navigate(
            ResourceAddress::Execution { execution: exec },
            &mut shell,
            &MapInventory::with(exec, ExecutionPresence::Live),
            no_local,
            NavigateHistory::ApplyOnly,
        ),
        Err(NavigationRejection::NavigationDenied)
    );
    assert_eq!(shell.focus_checkpoint(), after_ambiguous_bind);
}

#[test]
fn navigate_workspace_focuses_active_tab_and_focused_pane() {
    // SPEC-022 §12.8a: Workspace navigation selects that Workspace's active Tab
    // and focused Pane. Zero-Window Workspaces are unrepresentable in the
    // single-window tree until the ADR-018 window slice; covered there (§12.8b).
    let (mut shell, w1, w2, _, _, _, _) = seed_shell();
    assert_ne!(shell.snapshot().active_workspace, w2);
    assert_eq!(
        navigate(
            ResourceAddress::Workspace { workspace: w2 },
            &mut shell,
            &MapInventory::new(),
            NavigationPrincipal::local_user(),
            NavigateHistory::ApplyOnly,
        ),
        Ok(ResolvedTarget::Workspace { workspace: w2 })
    );
    let snap = shell.snapshot();
    assert_eq!(snap.active_workspace, w2);
    let expected = shell
        .workspace_focus(w2)
        .expect("w2 focus triple after navigate");
    assert_eq!(snap.active_tab, expected.active_tab);
    assert_eq!(snap.focused_pane, expected.focused_pane);

    // Return to w1 via Workspace address.
    assert_eq!(
        navigate(
            ResourceAddress::Workspace { workspace: w1 },
            &mut shell,
            &MapInventory::new(),
            NavigationPrincipal::local_user(),
            NavigateHistory::ApplyOnly,
        ),
        Ok(ResolvedTarget::Workspace { workspace: w1 })
    );
    let back = shell.snapshot();
    assert_eq!(back.active_workspace, w1);
    let w1_focus = shell.workspace_focus(w1).expect("w1");
    assert_eq!(back.active_tab, w1_focus.active_tab);
    assert_eq!(back.focused_pane, w1_focus.focused_pane);
}

// --- §12.14 ordinal-rebinding regression ------------------------------------

#[test]
fn address_run_reaches_original_target_after_ordinal_would_shift() {
    let (mut shell, w1, _, t1, t2, p1, _) = seed_shell();
    // Store the address of t2 while t1 is focused. A later CreateTab shifts
    // the palette ordinals; running by address must still select t2.
    let stored = ResourceAddress::Tab {
        workspace: w1,
        tab: t2,
    };
    assert_eq!(shell.snapshot().active_tab, t1);
    assert_eq!(shell.snapshot().focused_pane, p1);
    shell
        .apply_product_create_tab()
        .expect("create shifts ordinals");
    let after_create = shell.snapshot();
    assert_ne!(
        after_create.active_tab, t2,
        "CreateTab must leave t2 inactive so ordinals of switch-tab rows shift"
    );
    assert_eq!(
        navigate(
            stored,
            &mut shell,
            &MapInventory::new(),
            NavigationPrincipal::local_user(),
            NavigateHistory::ApplyOnly,
        ),
        Ok(ResolvedTarget::Tab {
            workspace: w1,
            tab: t2
        })
    );
    assert_eq!(shell.snapshot().active_tab, t2);
    assert_eq!(
        shell.snapshot().focused_pane,
        shell
            .tab_focused_pane(w1, t2)
            .expect("t2 focused pane after address run")
    );
}

#[test]
fn address_run_fails_closed_when_target_gone_instead_of_other_ordinal_action() {
    let (mut shell, w1, _, t1, _, p1, _) = seed_shell();
    shell
        .apply(ShellAction::SplitPane {
            id: p1,
            axis: SplitAxis::Right,
        })
        .expect("split");
    let created = shell.snapshot().focused_pane;
    assert_ne!(created, p1);
    shell
        .apply(ShellAction::FocusPane { id: p1 })
        .expect("refocus");
    let stored = ResourceAddress::Pane {
        workspace: w1,
        tab: t1,
        pane: created,
    };
    shell
        .apply(ShellAction::ClosePane { id: created })
        .expect("destroy stored target");
    let before = shell.focus_checkpoint();
    // A fresh ordinal rebuild would now offer other rows at the old index.
    // Address path must fail closed, not silently run a neighbour action.
    assert_eq!(
        navigate(
            stored,
            &mut shell,
            &MapInventory::new(),
            NavigationPrincipal::local_user(),
            NavigateHistory::ApplyOnly,
        ),
        Err(NavigationRejection::UnknownPane)
    );
    assert_eq!(shell.focus_checkpoint(), before);
}
