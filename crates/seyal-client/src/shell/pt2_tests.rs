//! SPEC-025 §12 PT2 fixtures (items 6–8) and property P1.
//!
//! Kept as a `_tests` sibling so the existing shell suite stays under hard_loc.

use std::collections::{BTreeMap, BTreeSet};

use super::*;
use seyal_core::{ExecutionId, WindowId};

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

fn assert_rejection_atomic(shell: &ShellState, before: &ShellState) {
    assert_eq!(
        shell.containment_fingerprint(),
        before.containment_fingerprint()
    );
}

/// Nested fixture: `Split(Leaf(A), Split(Leaf(B), Leaf(C)))`.
fn seed_nested_abc() -> (ShellState, PaneId, PaneId, PaneId) {
    let mut shell = seed_two_workspaces();
    let a = shell.snapshot().focused_pane;
    shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Right,
            containment_generation: shell.containment_generation(),
        })
        .expect("split A|B");
    let b = shell.snapshot().focused_pane;
    shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Right,
            containment_generation: shell.containment_generation(),
        })
        .expect("split B|C under right");
    let c = shell.snapshot().focused_pane;
    (shell, a, b, c)
}

fn pane_id_set(snap: &ShellSnapshot) -> BTreeSet<PaneId> {
    snap.panes.iter().map(|pane| pane.id).collect()
}

fn pane_bindings(snap: &ShellSnapshot) -> BTreeMap<PaneId, Option<ExecutionId>> {
    snap.panes
        .iter()
        .map(|pane| (pane.id, pane.execution))
        .collect()
}

fn assert_identity_preserved(before: &ShellSnapshot, after: &ShellSnapshot) {
    assert_eq!(pane_id_set(after), pane_id_set(before));
    assert_eq!(pane_bindings(after), pane_bindings(before));
}

fn leaf_pair(axis: SplitAxis, first: PaneId, second: PaneId) -> PaneTree {
    PaneTree::Split {
        axis,
        first: Box::new(PaneTree::Leaf(first)),
        second: Box::new(PaneTree::Leaf(second)),
        ratio: SplitRatio::HALF,
    }
}

// --- SPEC-025 §12 items 6–8 ---

#[test]
fn spec025_6_swap_preserves_ids_bindings_and_exchanges_slots() {
    let (mut shell, a, b, c) = seed_nested_abc();
    let exec_a = ExecutionId::from_bytes([0xAA; 16]);
    let exec_c = ExecutionId::from_bytes([0xCC; 16]);
    shell
        .apply(ShellAction::BindExecution {
            pane: a,
            execution: exec_a,
        })
        .expect("bind A");
    shell
        .apply(ShellAction::BindExecution {
            pane: c,
            execution: exec_c,
        })
        .expect("bind C");
    shell
        .apply(ShellAction::FocusPane { id: b })
        .expect("focus B");
    shell
        .apply(ShellAction::ZoomPane { id: b })
        .expect("zoom B");
    let before = shell.snapshot();
    assert_eq!(
        before.tree,
        PaneTree::Split {
            axis: SplitAxis::Right,
            first: Box::new(PaneTree::Leaf(a)),
            second: Box::new(leaf_pair(SplitAxis::Right, b, c)),
            ratio: SplitRatio::HALF,
        }
    );
    shell
        .apply(ShellAction::SwapPanes {
            a,
            b: c,
            containment_generation: shell.containment_generation(),
        })
        .expect("swap A↔C");
    let after = shell.snapshot();
    assert_identity_preserved(&before, &after);
    assert_eq!(after.focused_pane, b, "focus stays on B");
    assert_eq!(after.zoomed, None, "structural swap clears zoom");
    assert_eq!(
        after.tree,
        PaneTree::Split {
            axis: SplitAxis::Right,
            first: Box::new(PaneTree::Leaf(c)),
            second: Box::new(leaf_pair(SplitAxis::Right, b, a)),
            ratio: SplitRatio::HALF,
        },
        "only leaf slots exchange"
    );
    assert_eq!(
        after.panes.iter().find(|p| p.id == a).unwrap().execution,
        Some(exec_a)
    );
    assert_eq!(
        after.panes.iter().find(|p| p.id == c).unwrap().execution,
        Some(exec_c)
    );
}

#[test]
fn spec025_7_move_beside_each_side_collapses_old_slot_and_preserves_ids() {
    // Start: Split(A, Split(B, C)). Move C beside A for each side.
    // Collapse removes C → Split(A, B); insert beside A → Split(Split(…), B).
    let cases = [
        (MoveSide::Left, SplitAxis::Right, true),
        (MoveSide::Right, SplitAxis::Right, false),
        (MoveSide::Above, SplitAxis::Down, true),
        (MoveSide::Below, SplitAxis::Down, false),
    ];
    for (side, axis, pane_first) in cases {
        let (mut shell, a, b, c) = seed_nested_abc();
        let exec_a = ExecutionId::from_bytes([0xA1; 16]);
        let exec_b = ExecutionId::from_bytes([0xB1; 16]);
        shell
            .apply(ShellAction::BindExecution {
                pane: a,
                execution: exec_a,
            })
            .expect("bind A");
        shell
            .apply(ShellAction::BindExecution {
                pane: b,
                execution: exec_b,
            })
            .expect("bind B");
        shell
            .apply(ShellAction::FocusPane { id: a })
            .expect("focus A");
        shell
            .apply(ShellAction::ZoomPane { id: a })
            .expect("zoom A");
        let before = shell.snapshot();
        shell
            .apply(ShellAction::MovePaneBeside {
                pane: c,
                neighbor: a,
                side,
                containment_generation: shell.containment_generation(),
            })
            .expect("move C beside A");
        let after = shell.snapshot();
        assert_identity_preserved(&before, &after);
        assert_eq!(after.focused_pane, a);
        assert_eq!(after.zoomed, None, "structural move clears zoom");
        assert_eq!(
            after.panes.iter().find(|p| p.id == a).unwrap().execution,
            Some(exec_a)
        );
        assert_eq!(
            after.panes.iter().find(|p| p.id == b).unwrap().execution,
            Some(exec_b)
        );
        let (first_id, second_id) = if pane_first { (c, a) } else { (a, c) };
        assert_eq!(
            after.tree,
            PaneTree::Split {
                axis: SplitAxis::Right,
                first: Box::new(leaf_pair(axis, first_id, second_id)),
                second: Box::new(PaneTree::Leaf(b)),
                ratio: SplitRatio::HALF,
            },
            "side {side:?}: old C slot collapsed; new split beside A; B remains"
        );
    }
}

#[test]
fn spec025_8_move_pane_equals_neighbor_rejects_atomically() {
    let (mut shell, a, _, _) = seed_nested_abc();
    let before = shell.clone();
    assert_eq!(
        shell.apply(ShellAction::MovePaneBeside {
            pane: a,
            neighbor: a,
            side: MoveSide::Right,
            containment_generation: shell.containment_generation(),
        }),
        Err(ShellError::InvalidMoveTarget)
    );
    assert_rejection_atomic(&shell, &before);
    assert_eq!(shell.last_error(), Some(ShellError::InvalidMoveTarget));
}

#[test]
fn spec025_8_move_neighbor_in_another_tab_is_invalid_target() {
    let pane = PaneId::new();
    let foreign_tab = TabId::new();
    let window = WindowId::new();
    let workspace = WorkspaceId::m001_default();
    let second_tab_id = TabId::new();
    let second_pane = PaneId::new();
    let mut shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: workspace,
            name: "Seyal OSS".to_owned(),
            detail: None,
            attention: false,
            active_window: window,
            windows: vec![ShellWindowSeed {
                id: window,
                active_tab: foreign_tab,
                tabs: vec![
                    ShellTabSeed {
                        id: foreign_tab,
                        title: "Current".to_owned(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: pane,
                            title: "Source".to_owned(),
                            allows_implicit_execution_bootstrap: true,
                        },
                    },
                    ShellTabSeed {
                        id: second_tab_id,
                        title: "Other".to_owned(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: second_pane,
                            title: "Foreign neighbor".to_owned(),
                            allows_implicit_execution_bootstrap: true,
                        },
                    },
                ],
            }],
        }],
        workspace,
        true,
        true,
    )
    .expect("two-tab fixture");
    let before = shell.clone();

    assert_eq!(
        shell.apply(ShellAction::MovePaneBeside {
            pane,
            neighbor: second_pane,
            side: MoveSide::Right,
            containment_generation: shell.containment_generation(),
        }),
        Err(ShellError::InvalidMoveTarget)
    );
    assert_rejection_atomic(&shell, &before);
    assert_eq!(shell.last_error(), Some(ShellError::InvalidMoveTarget));
    assert!(shell.location_of_pane(second_pane).is_some());
}

#[test]
fn swap_a_equals_b_and_unknown_ids_reject_atomically() {
    let (mut shell, a, _, _) = seed_nested_abc();
    let before = shell.clone();
    assert_eq!(
        shell.apply(ShellAction::SwapPanes {
            a,
            b: a,
            containment_generation: shell.containment_generation(),
        }),
        Err(ShellError::InvalidMoveTarget)
    );
    assert_rejection_atomic(&shell, &before);
    assert_eq!(shell.last_error(), Some(ShellError::InvalidMoveTarget));

    let stale = PaneId::new();
    for action in [
        ShellAction::SwapPanes {
            a,
            b: stale,
            containment_generation: shell.containment_generation(),
        },
        ShellAction::SwapPanes {
            a: stale,
            b: a,
            containment_generation: shell.containment_generation(),
        },
        ShellAction::MovePaneBeside {
            pane: stale,
            neighbor: a,
            side: MoveSide::Left,
            containment_generation: shell.containment_generation(),
        },
        ShellAction::MovePaneBeside {
            pane: a,
            neighbor: stale,
            side: MoveSide::Left,
            containment_generation: shell.containment_generation(),
        },
    ] {
        let before = shell.clone();
        assert_eq!(shell.apply(action), Err(ShellError::UnknownPane));
        assert_rejection_atomic(&shell, &before);
        assert_eq!(shell.last_error(), Some(ShellError::UnknownPane));
    }
}

/// SPEC-025 property P1: successful swap/move leave PaneId set and bindings unchanged.
#[test]
fn spec025_p1_swap_and_move_preserve_pane_ids_and_execution_bindings() {
    let (mut shell, a, b, c) = seed_nested_abc();
    let exec_a = ExecutionId::from_bytes([0x11; 16]);
    let exec_b = ExecutionId::from_bytes([0x22; 16]);
    shell
        .apply(ShellAction::BindExecution {
            pane: a,
            execution: exec_a,
        })
        .expect("bind A");
    shell
        .apply(ShellAction::BindExecution {
            pane: b,
            execution: exec_b,
        })
        .expect("bind B");

    let before_swap = shell.snapshot();
    shell
        .apply(ShellAction::SwapPanes {
            a: b,
            b: c,
            containment_generation: shell.containment_generation(),
        })
        .expect("swap");
    assert_identity_preserved(&before_swap, &shell.snapshot());

    let before_move = shell.snapshot();
    shell
        .apply(ShellAction::MovePaneBeside {
            pane: a,
            neighbor: c,
            side: MoveSide::Below,
            containment_generation: shell.containment_generation(),
        })
        .expect("move");
    assert_identity_preserved(&before_move, &shell.snapshot());
    assert_eq!(shell.snapshot().focused_pane, before_move.focused_pane);
}

#[test]
fn spec025_stale_containment_generation_rejects_swap_and_move_beside() {
    let (mut shell, a, b, c) = seed_nested_abc();
    let generation = shell.containment_generation();
    shell
        .apply(ShellAction::SwapPanes {
            a,
            b,
            containment_generation: generation,
        })
        .expect("swap");
    assert!(shell.containment_generation() > generation);
    let stale = generation;
    let fingerprint = shell.containment_fingerprint();
    for action in [
        ShellAction::SwapPanes {
            a: b,
            b: c,
            containment_generation: stale,
        },
        ShellAction::MovePaneBeside {
            pane: c,
            neighbor: a,
            side: MoveSide::Right,
            containment_generation: stale,
        },
    ] {
        let prior = shell.clone();
        assert_eq!(shell.apply(action), Err(ShellError::StaleContainment));
        assert_rejection_atomic(&shell, &prior);
        assert_eq!(shell.last_error(), Some(ShellError::StaleContainment));
        assert_eq!(shell.containment_fingerprint(), fingerprint);
    }
    let now = shell.containment_generation();
    shell
        .apply(ShellAction::SwapPanes {
            a: b,
            b: c,
            containment_generation: now,
        })
        .expect("fresh swap");
    assert!(shell.containment_generation() > now);
}
