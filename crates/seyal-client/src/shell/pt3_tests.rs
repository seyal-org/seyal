//! SPEC-025 §12 PT3 fixtures (items 14–15) and property P7.
//!
//! Kept as a `_tests` sibling so the shell suite stays under hard_loc.

use super::focus_direction::is_geometric_neighbor;
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

fn assert_rejection_atomic(shell: &ShellState, before: &ShellState) {
    assert_eq!(
        shell.containment_fingerprint(),
        before.containment_fingerprint()
    );
}

/// 2×2 grid via default-half splits (columns then rows):
/// ```text
/// A | B
/// -----
/// C | D
/// ```
fn seed_grid_2x2() -> (ShellState, PaneId, PaneId, PaneId, PaneId) {
    let mut shell = seed_two_workspaces();
    let a = shell.snapshot().focused_pane;
    shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Right,
            containment_generation: shell.containment_generation(),
        })
        .expect("A|B");
    let b = shell.snapshot().focused_pane;
    shell
        .apply(ShellAction::FocusPane { id: a })
        .expect("focus A");
    shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Down,
            containment_generation: shell.containment_generation(),
        })
        .expect("split A column");
    let c = shell.snapshot().focused_pane;
    shell
        .apply(ShellAction::FocusPane { id: b })
        .expect("focus B");
    shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Down,
            containment_generation: shell.containment_generation(),
        })
        .expect("split B column");
    let d = shell.snapshot().focused_pane;
    shell
        .apply(ShellAction::FocusPane { id: a })
        .expect("focus A");
    (shell, a, b, c, d)
}

/// Uneven default-half tree: `Split(Right, Leaf(A), Split(Down, Leaf(B), Leaf(C)))`.
/// A is a full-height left half; B and C are stacked right halves.
fn seed_uneven_abc() -> (ShellState, PaneId, PaneId, PaneId) {
    let mut shell = seed_two_workspaces();
    let a = shell.snapshot().focused_pane;
    shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Right,
            containment_generation: shell.containment_generation(),
        })
        .expect("A|B");
    let b = shell.snapshot().focused_pane;
    shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Down,
            containment_generation: shell.containment_generation(),
        })
        .expect("B/C");
    let c = shell.snapshot().focused_pane;
    shell
        .apply(ShellAction::FocusPane { id: a })
        .expect("focus A");
    (shell, a, b, c)
}

// --- SPEC-025 §12 items 14–15 ---

#[test]
fn spec025_14_directional_focus_2x2_and_uneven_tie_break() {
    let (mut shell, a, b, c, d) = seed_grid_2x2();
    let before_tree = shell.snapshot().tree.clone();

    shell
        .apply(ShellAction::FocusDirection {
            direction: FocusDirection::Right,
        })
        .expect("A → Right → B");
    assert_eq!(shell.snapshot().focused_pane, b);
    assert_eq!(shell.snapshot().tree, before_tree);

    shell
        .apply(ShellAction::FocusDirection {
            direction: FocusDirection::Down,
        })
        .expect("B → Down → D");
    assert_eq!(shell.snapshot().focused_pane, d);

    shell
        .apply(ShellAction::FocusDirection {
            direction: FocusDirection::Left,
        })
        .expect("D → Left → C");
    assert_eq!(shell.snapshot().focused_pane, c);

    shell
        .apply(ShellAction::FocusDirection {
            direction: FocusDirection::Up,
        })
        .expect("C → Up → A");
    assert_eq!(shell.snapshot().focused_pane, a);

    // Uneven: A Right has B and C as edge-sharing candidates with equal
    // orthogonal center distance; pre-order picks B over C.
    let (mut uneven, _a, b, c) = seed_uneven_abc();
    uneven
        .apply(ShellAction::FocusDirection {
            direction: FocusDirection::Right,
        })
        .expect("A → Right → B (tie-break)");
    assert_eq!(uneven.snapshot().focused_pane, b);
    assert_ne!(uneven.snapshot().focused_pane, c);
}

#[test]
fn spec025_14_directional_focus_while_zoomed_clears_zoom() {
    let (mut shell, a, b, _, _) = seed_grid_2x2();
    shell
        .apply(ShellAction::ZoomPane { id: a })
        .expect("zoom A");
    let before_tree = shell.snapshot().tree.clone();
    let before_ids: Vec<_> = shell.snapshot().panes.iter().map(|p| p.id).collect();

    shell
        .apply(ShellAction::FocusDirection {
            direction: FocusDirection::Right,
        })
        .expect("focus away while zoomed");
    let after = shell.snapshot();
    assert_eq!(after.focused_pane, b);
    assert_eq!(after.zoomed, None);
    assert_eq!(after.tree, before_tree);
    let after_ids: Vec<_> = after.panes.iter().map(|p| p.id).collect();
    assert_eq!(after_ids, before_ids);
}

#[test]
fn spec025_15_no_directional_neighbor_rejects_atomically() {
    let (mut shell, a, _, _, _) = seed_grid_2x2();
    shell
        .apply(ShellAction::FocusPane { id: a })
        .expect("focus A");
    shell
        .apply(ShellAction::ZoomPane { id: a })
        .expect("zoom A");

    for direction in [FocusDirection::Left, FocusDirection::Up] {
        let before = shell.clone();
        assert_eq!(
            shell.apply(ShellAction::FocusDirection { direction }),
            Err(ShellError::NoDirectionalNeighbor)
        );
        assert_rejection_atomic(&shell, &before);
        assert_eq!(shell.last_error(), Some(ShellError::NoDirectionalNeighbor));
        assert_eq!(shell.snapshot().focused_pane, a);
        assert_eq!(shell.snapshot().zoomed, Some(a));
    }

    // Single-leaf Tab has no neighbor in any direction.
    let mut single = seed_two_workspaces();
    let only = single.snapshot().focused_pane;
    let before_single = single.clone();
    assert_eq!(
        single.apply(ShellAction::FocusDirection {
            direction: FocusDirection::Right,
        }),
        Err(ShellError::NoDirectionalNeighbor)
    );
    assert_rejection_atomic(&single, &before_single);
    assert_eq!(single.snapshot().focused_pane, only);
}

/// SPEC-025 property P7: success focuses a geometric neighbor; miss rejects.
#[test]
fn spec025_p7_directional_focus_neighbor_or_reject() {
    let fixtures: Vec<(ShellState, &[FocusDirection])> = {
        let (grid, _, _, _, _) = seed_grid_2x2();
        let (uneven, _, _, _) = seed_uneven_abc();
        let single = seed_two_workspaces();
        vec![
            (
                grid,
                &[
                    FocusDirection::Left,
                    FocusDirection::Right,
                    FocusDirection::Up,
                    FocusDirection::Down,
                ],
            ),
            (
                uneven,
                &[
                    FocusDirection::Left,
                    FocusDirection::Right,
                    FocusDirection::Up,
                    FocusDirection::Down,
                ],
            ),
            (
                single,
                &[
                    FocusDirection::Left,
                    FocusDirection::Right,
                    FocusDirection::Up,
                    FocusDirection::Down,
                ],
            ),
        ]
    };

    for (mut shell, directions) in fixtures {
        let pane_ids: Vec<_> = shell.snapshot().panes.iter().map(|p| p.id).collect();
        for &focused in &pane_ids {
            shell
                .apply(ShellAction::FocusPane { id: focused })
                .expect("focus leaf");
            for &direction in directions {
                let before = shell.clone();
                let before_tree = before.snapshot().tree.clone();
                match shell.apply(ShellAction::FocusDirection { direction }) {
                    Ok(()) => {
                        let chosen = shell.snapshot().focused_pane;
                        assert!(
                            is_geometric_neighbor(&before_tree, focused, chosen, direction),
                            "chosen leaf must be a §5.7 neighbor"
                        );
                        assert_eq!(shell.snapshot().tree, before_tree);
                        // Restore focused for the next direction sample.
                        shell
                            .apply(ShellAction::FocusPane { id: focused })
                            .expect("restore focus");
                    }
                    Err(ShellError::NoDirectionalNeighbor) => {
                        assert_rejection_atomic(&shell, &before);
                        assert_eq!(shell.snapshot().focused_pane, focused);
                    }
                    Err(other) => panic!("unexpected rejection: {other:?}"),
                }
            }
        }
    }
}
