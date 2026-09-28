//! SPEC-022 §12 items 1–7 for N1 address + pure resolver.

use std::collections::HashMap;
use std::mem::{size_of, size_of_val};

use seyal_core::{ExecutionId, PaneId, TabId, WorkspaceId};

use crate::shell::{
    ShellAction, ShellPaneSeed, ShellState, ShellTabSeed, ShellWorkspaceSeed, SplitAxis,
};

use super::{
    decode_resource_address, resolve, ExecutionPresence, NavigationPrincipal, NavigationRejection,
    ResolvedTarget, ResourceAddress, WorkspaceAccess, RESOURCE_ADDRESS_ABI_VERSION,
    RESOURCE_ADDRESS_KIND_EXECUTION, RESOURCE_ADDRESS_KIND_PANE, RESOURCE_ADDRESS_KIND_TAB,
    RESOURCE_ADDRESS_KIND_WORKSPACE,
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
    let shell = ShellState::from_workspaces(
        vec![
            ShellWorkspaceSeed {
                id: w1,
                name: "Alpha".to_owned(),
                detail: Some("shared-label".to_owned()),
                attention: false,
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
            },
            ShellWorkspaceSeed {
                id: w2,
                name: "Alpha".to_owned(),
                detail: Some("shared-label".to_owned()),
                attention: false,
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
            },
        ],
        w1,
        true,
        true,
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
    let (shell, w1, _, _, _, _, _) = seed_shell();
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
        .apply(ShellAction::BindExecution {
            pane: second,
            execution,
        })
        .expect("bind second");
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
