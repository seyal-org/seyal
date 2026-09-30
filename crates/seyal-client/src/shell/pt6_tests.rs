//! SPEC-025 §11 P1–P9 property / adversarial suite (PT6 / #1167).
//!
//! Uses the same deterministic LCG already used by `tests/app_invariants.rs`
//! (no new test framework). P3 (equalize) is named as blocked on #928.

use std::collections::{BTreeMap, BTreeSet};

use super::focus_direction::is_geometric_neighbor;
use super::*;
use seyal_core::{ExecutionId, WindowId};

/// SPEC-025 P3 (equalize topology/ratio invariant) stays blocked on #928.
/// Named so the suite does not silently omit equalize; no equalize reducer
/// is added here (PT4 owns that slice).
const P3_BLOCKED_ON: &str = "#928";

fn other_workspace() -> WorkspaceId {
    WorkspaceId::from_bytes([0x11; 16])
}

fn seed_splittable() -> ShellState {
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

/// Deterministic LCG matching `tests/app_invariants.rs` (crate-local generator).
struct Lcg(u64);

impl Lcg {
    fn from_seed(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15))
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.0
    }

    fn index(&mut self, len: usize) -> usize {
        debug_assert!(len > 0);
        (self.next_u64() as usize) % len
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.index(items.len())]
    }
}

fn pane_ids(snap: &ShellSnapshot) -> BTreeSet<PaneId> {
    snap.panes.iter().map(|pane| pane.id).collect()
}

fn pane_bindings(snap: &ShellSnapshot) -> BTreeMap<PaneId, Option<ExecutionId>> {
    snap.panes
        .iter()
        .map(|pane| (pane.id, pane.execution))
        .collect()
}

fn leaf_id_set(tree: &PaneTree) -> BTreeSet<PaneId> {
    tree.pane_ids().into_iter().collect()
}

/// SPEC-025 I2.1–I2.3 and I2.6.
fn assert_tab_invariants(snap: &ShellSnapshot) {
    let leaves = leaf_id_set(&snap.tree);
    let panes = pane_ids(snap);
    assert_eq!(leaves, panes, "I2.1: panes.keys() equals leaf multiset");
    assert!(panes.contains(&snap.focused_pane), "I2.2: focused ∈ panes");
    match snap.zoomed {
        None => {}
        Some(z) => {
            assert!(panes.contains(&z), "I2.3: zoomed ∈ panes");
            assert_eq!(snap.focused_pane, z, "I2.6: zoomed implies focused");
        }
    }
}

fn assert_rejection_byte_identical(before: &ShellState, after: &ShellState, expected: ShellError) {
    assert_eq!(after.last_error(), Some(expected));
    let mut comparable = after.clone();
    comparable.last_error = before.last_error;
    assert_eq!(
        comparable, *before,
        "P4: rejection must leave ShellState byte-identical except last_error"
    );
}

/// P8: pane move/swap/zoom/focus must not emit terminate or provision effects.
/// `ShellNativeEffect` has no Terminate/Provision variants (type surface);
/// these reducers must also emit an empty effect list (no window lifecycle).
fn assert_no_terminate_or_provision_effects(effects: &[ShellNativeEffect]) {
    assert!(
        effects.is_empty(),
        "P8: pane ops must emit no effects (no terminate/provision/window lifecycle); got {effects:?}"
    );
}

fn grow_small_tree(shell: &mut ShellState, rng: &mut Lcg, target_leaves: usize) {
    while shell.snapshot().panes.len() < target_leaves {
        let axis = if rng.next_u64().is_multiple_of(2) {
            SplitAxis::Right
        } else {
            SplitAxis::Down
        };
        let ids: Vec<_> = shell.snapshot().panes.iter().map(|p| p.id).collect();
        let id = *rng.pick(&ids);
        shell
            .apply(ShellAction::SplitPane {
                id,
                axis,
                containment_generation: shell.containment_generation(),
            })
            .expect("split while growing small tree");
    }
}

fn bind_some_panes(shell: &mut ShellState, rng: &mut Lcg, count: usize) {
    let ids: Vec<_> = shell.snapshot().panes.iter().map(|p| p.id).collect();
    let mut bound = 0usize;
    let mut salt = 0u8;
    while bound < count && bound < ids.len() {
        let id = ids[bound];
        salt = salt.wrapping_add(0x11);
        let execution = ExecutionId::from_bytes([salt; 16]);
        match shell.apply(ShellAction::BindExecution {
            pane: id,
            execution,
        }) {
            Ok(()) => bound += 1,
            Err(_) => {
                let _ = rng.next_u64();
                salt = salt.wrapping_add(1);
            }
        }
        if salt == 0 {
            break;
        }
    }
}

fn pick_two_distinct(rng: &mut Lcg, ids: &[PaneId]) -> Option<(PaneId, PaneId)> {
    if ids.len() < 2 {
        return None;
    }
    let a = *rng.pick(ids);
    let mut b = *rng.pick(ids);
    let mut guard = 0;
    while b == a && guard < 8 {
        b = *rng.pick(ids);
        guard += 1;
    }
    if a == b {
        None
    } else {
        Some((a, b))
    }
}

fn sides() -> [MoveSide; 4] {
    [
        MoveSide::Left,
        MoveSide::Right,
        MoveSide::Above,
        MoveSide::Below,
    ]
}

fn directions() -> [FocusDirection; 4] {
    [
        FocusDirection::Left,
        FocusDirection::Right,
        FocusDirection::Up,
        FocusDirection::Down,
    ]
}

/// Adversarial / property action kinds for P9 streams.
#[derive(Clone, Copy)]
enum StreamKind {
    Swap,
    Move,
    Zoom,
    Unzoom,
    Focus,
    FocusDir,
    Close,
    StaleId,
    SelfMove,
    ZoomThenClose,
}

const STREAM_KINDS: &[StreamKind] = &[
    StreamKind::Swap,
    StreamKind::Move,
    StreamKind::Zoom,
    StreamKind::Unzoom,
    StreamKind::Focus,
    StreamKind::FocusDir,
    StreamKind::Close,
    StreamKind::StaleId,
    StreamKind::SelfMove,
    StreamKind::ZoomThenClose,
];

fn apply_stream_step(shell: &mut ShellState, rng: &mut Lcg, kind: StreamKind) {
    let snap = shell.snapshot();
    let ids: Vec<_> = snap.panes.iter().map(|p| p.id).collect();
    let stale = PaneId::new();
    match kind {
        StreamKind::Swap => {
            let Some((a, b)) = pick_two_distinct(rng, &ids) else {
                return;
            };
            let before_state = shell.clone();
            let before = before_state.snapshot();
            let _ = shell.take_effects();
            match shell.apply(ShellAction::SwapPanes {
                a,
                b,
                containment_generation: shell.containment_generation(),
            }) {
                Ok(()) => {
                    assert_eq!(pane_ids(&shell.snapshot()), pane_ids(&before));
                    assert_eq!(pane_bindings(&shell.snapshot()), pane_bindings(&before));
                    assert_no_terminate_or_provision_effects(&shell.take_effects());
                    assert_tab_invariants(&shell.snapshot());
                }
                Err(err) => assert_rejection_byte_identical(&before_state, shell, err),
            }
        }
        StreamKind::Move => {
            let Some((pane, neighbor)) = pick_two_distinct(rng, &ids) else {
                return;
            };
            let side = *rng.pick(&sides());
            let before_state = shell.clone();
            let before = before_state.snapshot();
            let _ = shell.take_effects();
            match shell.apply(ShellAction::MovePaneBeside {
                pane,
                neighbor,
                side,
                containment_generation: shell.containment_generation(),
            }) {
                Ok(()) => {
                    assert_eq!(pane_ids(&shell.snapshot()), pane_ids(&before));
                    assert_eq!(pane_bindings(&shell.snapshot()), pane_bindings(&before));
                    assert_no_terminate_or_provision_effects(&shell.take_effects());
                    assert_tab_invariants(&shell.snapshot());
                }
                Err(err) => assert_rejection_byte_identical(&before_state, shell, err),
            }
        }
        StreamKind::Zoom => {
            let id = *rng.pick(&ids);
            let before_tree = snap.tree.clone();
            let before_state = shell.clone();
            let _ = shell.take_effects();
            match shell.apply(ShellAction::ZoomPane { id }) {
                Ok(()) => {
                    assert_eq!(
                        shell.snapshot().tree,
                        before_tree,
                        "P2: zoom preserves tree"
                    );
                    assert_no_terminate_or_provision_effects(&shell.take_effects());
                    assert_tab_invariants(&shell.snapshot());
                }
                Err(err) => assert_rejection_byte_identical(&before_state, shell, err),
            }
        }
        StreamKind::Unzoom => {
            let before_tree = snap.tree.clone();
            let before_state = shell.clone();
            let _ = shell.take_effects();
            match shell.apply(ShellAction::Unzoom) {
                Ok(()) => {
                    assert_eq!(
                        shell.snapshot().tree,
                        before_tree,
                        "P2: unzoom preserves tree"
                    );
                    assert_no_terminate_or_provision_effects(&shell.take_effects());
                    assert_tab_invariants(&shell.snapshot());
                }
                Err(ShellError::NotZoomed) => {
                    assert_rejection_byte_identical(&before_state, shell, ShellError::NotZoomed);
                }
                Err(err) => panic!("unexpected unzoom error: {err:?}"),
            }
        }
        StreamKind::Focus => {
            let id = *rng.pick(&ids);
            let before_state = shell.clone();
            let _ = shell.take_effects();
            match shell.apply(ShellAction::FocusPane { id }) {
                Ok(()) => {
                    assert_no_terminate_or_provision_effects(&shell.take_effects());
                    assert_tab_invariants(&shell.snapshot());
                }
                Err(err) => assert_rejection_byte_identical(&before_state, shell, err),
            }
        }
        StreamKind::FocusDir => {
            let direction = *rng.pick(&directions());
            let focused = snap.focused_pane;
            let before_state = shell.clone();
            let before_tree = snap.tree.clone();
            let _ = shell.take_effects();
            match shell.apply(ShellAction::FocusDirection { direction }) {
                Ok(()) => {
                    let chosen = shell.snapshot().focused_pane;
                    assert!(
                        is_geometric_neighbor(&before_tree, focused, chosen, direction),
                        "P7: success must focus a geometric neighbor"
                    );
                    assert_no_terminate_or_provision_effects(&shell.take_effects());
                    assert_tab_invariants(&shell.snapshot());
                }
                Err(ShellError::NoDirectionalNeighbor) => {
                    assert_rejection_byte_identical(
                        &before_state,
                        shell,
                        ShellError::NoDirectionalNeighbor,
                    );
                }
                Err(err) => panic!("unexpected focus-dir error: {err:?}"),
            }
        }
        StreamKind::Close => {
            let id = *rng.pick(&ids);
            let before_state = shell.clone();
            let leaf_count = ids.len();
            match shell.apply(ShellAction::ClosePane {
                id,
                containment_generation: shell.containment_generation(),
            }) {
                Ok(()) => {
                    assert!(!shell.snapshot().panes.is_empty(), "P6: never zero leaves");
                    assert!(shell.snapshot().panes.len() < leaf_count);
                    assert_tab_invariants(&shell.snapshot());
                }
                Err(ShellError::CannotCloseLastPane) => {
                    assert_eq!(leaf_count, 1);
                    assert_rejection_byte_identical(
                        &before_state,
                        shell,
                        ShellError::CannotCloseLastPane,
                    );
                }
                Err(err) => assert_rejection_byte_identical(&before_state, shell, err),
            }
        }
        StreamKind::StaleId => {
            let before_state = shell.clone();
            let (action, expect_last_pane_gate) = match rng.next_u64() % 5 {
                0 => (
                    ShellAction::SwapPanes {
                        a: stale,
                        b: ids[0],
                        containment_generation: shell.containment_generation(),
                    },
                    false,
                ),
                1 => (
                    ShellAction::MovePaneBeside {
                        pane: stale,
                        neighbor: ids[0],
                        side: MoveSide::Right,
                        containment_generation: shell.containment_generation(),
                    },
                    false,
                ),
                2 => (ShellAction::ZoomPane { id: stale }, false),
                3 => (ShellAction::FocusPane { id: stale }, false),
                _ => (
                    ShellAction::ClosePane {
                        id: stale,
                        containment_generation: shell.containment_generation(),
                    },
                    true,
                ),
            };
            let err = shell.apply(action).expect_err("stale id must reject");
            // Close checks last-pane before unknown-id, so a stale close on a
            // single-leaf Tab yields CannotCloseLastPane (still fail-closed).
            let expected = if expect_last_pane_gate && ids.len() == 1 {
                ShellError::CannotCloseLastPane
            } else {
                ShellError::UnknownPane
            };
            assert_eq!(err, expected);
            assert_rejection_byte_identical(&before_state, shell, expected);
        }
        StreamKind::SelfMove => {
            let id = *rng.pick(&ids);
            let before_state = shell.clone();
            let err = shell
                .apply(ShellAction::MovePaneBeside {
                    pane: id,
                    neighbor: id,
                    side: MoveSide::Left,
                    containment_generation: shell.containment_generation(),
                })
                .expect_err("self-move must reject");
            assert_eq!(err, ShellError::InvalidMoveTarget);
            assert_rejection_byte_identical(&before_state, shell, ShellError::InvalidMoveTarget);
        }
        StreamKind::ZoomThenClose => {
            if ids.len() < 2 {
                return;
            }
            let close_id = ids
                .iter()
                .copied()
                .find(|id| {
                    snap.panes
                        .iter()
                        .find(|p| p.id == *id)
                        .is_some_and(|p| p.execution.is_none())
                })
                .unwrap_or(ids[0]);
            let zoom_id = *rng.pick(&ids);
            let _ = shell.apply(ShellAction::ZoomPane { id: zoom_id });
            let before_tree = shell.snapshot().tree.clone();
            let before_state = shell.clone();
            match shell.apply(ShellAction::ClosePane {
                id: close_id,
                containment_generation: shell.containment_generation(),
            }) {
                Ok(()) => {
                    assert!(!shell.snapshot().panes.is_empty());
                    assert_ne!(shell.snapshot().tree, before_tree);
                    assert_tab_invariants(&shell.snapshot());
                }
                Err(err) => assert_rejection_byte_identical(&before_state, shell, err),
            }
        }
    }
}

#[test]
fn spec025_p3_equalize_blocked_on_issue_928() {
    assert_eq!(P3_BLOCKED_ON, "#928");
    // EqualizeFocused / EqualizeTab are not on ShellAction on this head.
    // PT4 / #928 owns equalize; this suite covers P1, P2, P4–P9-without-equalize.
}

#[test]
fn spec025_p1_generated_swap_and_move_preserve_ids_and_bindings() {
    for seed in 1u64..=32 {
        let mut rng = Lcg::from_seed(seed);
        let mut shell = seed_splittable();
        grow_small_tree(&mut shell, &mut rng, 2 + (seed as usize % 3));
        bind_some_panes(&mut shell, &mut rng, 1 + (seed as usize % 2));

        for _ in 0..8 {
            let before = shell.snapshot();
            let ids: Vec<_> = before.panes.iter().map(|p| p.id).collect();
            let Some((a, b)) = pick_two_distinct(&mut rng, &ids) else {
                break;
            };
            let _ = shell.take_effects();
            if rng.next_u64().is_multiple_of(2) {
                shell
                    .apply(ShellAction::SwapPanes {
                        a,
                        b,
                        containment_generation: shell.containment_generation(),
                    })
                    .expect("swap on distinct leaves");
            } else {
                let side = *rng.pick(&sides());
                shell
                    .apply(ShellAction::MovePaneBeside {
                        pane: a,
                        neighbor: b,
                        side,
                        containment_generation: shell.containment_generation(),
                    })
                    .expect("move beside on distinct leaves");
            }
            let after = shell.snapshot();
            assert_eq!(pane_ids(&after), pane_ids(&before), "P1 PaneId set");
            assert_eq!(
                pane_bindings(&after),
                pane_bindings(&before),
                "P1 execution bindings"
            );
            assert_no_terminate_or_provision_effects(&shell.take_effects());
        }
    }
}

#[test]
fn spec025_p2_generated_zoom_unzoom_preserve_topology() {
    for seed in 1u64..=24 {
        let mut rng = Lcg::from_seed(seed.wrapping_add(100));
        let mut shell = seed_splittable();
        grow_small_tree(&mut shell, &mut rng, 2 + (seed as usize % 3));
        let ids: Vec<_> = shell.snapshot().panes.iter().map(|p| p.id).collect();

        for _ in 0..6 {
            let id = *rng.pick(&ids);
            let before_tree = shell.snapshot().tree.clone();
            let _ = shell.take_effects();
            shell
                .apply(ShellAction::ZoomPane { id })
                .expect("zoom existing leaf");
            assert_eq!(shell.snapshot().tree, before_tree);
            assert_no_terminate_or_provision_effects(&shell.take_effects());

            let before_unzoom = shell.snapshot().tree.clone();
            shell.apply(ShellAction::Unzoom).expect("unzoom");
            assert_eq!(shell.snapshot().tree, before_unzoom);
            assert_no_terminate_or_provision_effects(&shell.take_effects());
        }
    }
}

#[test]
fn spec025_p4_generated_rejections_are_byte_identical_except_last_error() {
    for seed in 1u64..=24 {
        let mut rng = Lcg::from_seed(seed.wrapping_add(200));
        let mut shell = seed_splittable();
        grow_small_tree(&mut shell, &mut rng, 3);
        let ids: Vec<_> = shell.snapshot().panes.iter().map(|p| p.id).collect();
        let stale = PaneId::new();
        let id = ids[0];

        // Focus a leaf and try Left; may miss (reject) or succeed depending on layout.
        shell
            .apply(ShellAction::FocusPane { id })
            .expect("focus for directional sample");

        let rejects: &[(ShellAction, ShellError)] = &[
            (
                ShellAction::SwapPanes {
                    a: id,
                    b: id,
                    containment_generation: shell.containment_generation(),
                },
                ShellError::InvalidMoveTarget,
            ),
            (
                ShellAction::MovePaneBeside {
                    pane: id,
                    neighbor: id,
                    side: MoveSide::Right,
                    containment_generation: shell.containment_generation(),
                },
                ShellError::InvalidMoveTarget,
            ),
            (
                ShellAction::SwapPanes {
                    a: stale,
                    b: id,
                    containment_generation: shell.containment_generation(),
                },
                ShellError::UnknownPane,
            ),
            (ShellAction::ZoomPane { id: stale }, ShellError::UnknownPane),
            (
                ShellAction::FocusPane { id: stale },
                ShellError::UnknownPane,
            ),
            (
                ShellAction::ClosePane {
                    id: stale,
                    containment_generation: shell.containment_generation(),
                },
                ShellError::UnknownPane,
            ),
            (ShellAction::Unzoom, ShellError::NotZoomed),
        ];

        for &(action, expected) in rejects {
            let before = shell.clone();
            let err = shell.apply(action).expect_err("must reject");
            assert_eq!(err, expected);
            assert_rejection_byte_identical(&before, &shell, expected);
        }

        // Directional miss: try all directions from each leaf; at least one miss.
        let mut saw_miss = false;
        for &focused in &ids {
            shell
                .apply(ShellAction::FocusPane { id: focused })
                .expect("focus");
            for &direction in &directions() {
                let before = shell.clone();
                match shell.apply(ShellAction::FocusDirection { direction }) {
                    Ok(()) => {
                        shell
                            .apply(ShellAction::FocusPane { id: focused })
                            .expect("restore");
                    }
                    Err(ShellError::NoDirectionalNeighbor) => {
                        assert_rejection_byte_identical(
                            &before,
                            &shell,
                            ShellError::NoDirectionalNeighbor,
                        );
                        saw_miss = true;
                    }
                    Err(err) => panic!("unexpected: {err:?}"),
                }
            }
        }
        assert!(
            saw_miss || ids.len() == 1,
            "P4 directional-miss sample for seed {seed}"
        );
        let _ = rng.next_u64();
    }
}

#[test]
fn spec025_p5_successful_transitions_preserve_tab_invariants() {
    for seed in 1u64..=16 {
        let mut rng = Lcg::from_seed(seed.wrapping_add(300));
        let mut shell = seed_splittable();
        grow_small_tree(&mut shell, &mut rng, 4);
        bind_some_panes(&mut shell, &mut rng, 1);
        assert_tab_invariants(&shell.snapshot());

        for kind in STREAM_KINDS {
            apply_stream_step(&mut shell, &mut rng, *kind);
            assert_tab_invariants(&shell.snapshot());
        }
    }
}

#[test]
fn spec025_p6_close_never_yields_zero_leaves() {
    for seed in 1u64..=16 {
        let mut rng = Lcg::from_seed(seed.wrapping_add(400));
        let mut shell = seed_splittable();
        grow_small_tree(&mut shell, &mut rng, 2 + (seed as usize % 3));

        loop {
            let ids: Vec<_> = shell
                .snapshot()
                .panes
                .iter()
                .filter(|p| p.execution.is_none())
                .map(|p| p.id)
                .collect();
            if ids.is_empty() {
                break;
            }
            let id = *rng.pick(&ids);
            let before = shell.clone();
            let count = shell.snapshot().panes.len();
            match shell.apply(ShellAction::ClosePane {
                id,
                containment_generation: shell.containment_generation(),
            }) {
                Ok(()) => {
                    assert!(!shell.snapshot().panes.is_empty());
                    assert_eq!(shell.snapshot().panes.len(), count - 1);
                    assert_tab_invariants(&shell.snapshot());
                }
                Err(ShellError::CannotCloseLastPane) => {
                    assert_eq!(count, 1);
                    assert_rejection_byte_identical(
                        &before,
                        &shell,
                        ShellError::CannotCloseLastPane,
                    );
                    break;
                }
                Err(err) => panic!("unexpected close error: {err:?}"),
            }
            if shell.snapshot().panes.len() == 1 {
                let only = shell.snapshot().focused_pane;
                let before = shell.clone();
                assert_eq!(
                    shell.apply(ShellAction::ClosePane {
                        id: only,
                        containment_generation: shell.containment_generation()
                    }),
                    Err(ShellError::CannotCloseLastPane)
                );
                assert_rejection_byte_identical(&before, &shell, ShellError::CannotCloseLastPane);
                break;
            }
        }
    }
}

#[test]
fn spec025_p7_generated_directional_focus_neighbor_or_reject() {
    for seed in 1u64..=16 {
        let mut rng = Lcg::from_seed(seed.wrapping_add(500));
        let mut shell = seed_splittable();
        grow_small_tree(&mut shell, &mut rng, 2 + (seed as usize % 3));
        let ids: Vec<_> = shell.snapshot().panes.iter().map(|p| p.id).collect();
        for &focused in &ids {
            shell
                .apply(ShellAction::FocusPane { id: focused })
                .expect("focus");
            for &direction in &directions() {
                let before = shell.clone();
                let before_tree = before.snapshot().tree.clone();
                let _ = shell.take_effects();
                match shell.apply(ShellAction::FocusDirection { direction }) {
                    Ok(()) => {
                        let chosen = shell.snapshot().focused_pane;
                        assert!(is_geometric_neighbor(
                            &before_tree,
                            focused,
                            chosen,
                            direction
                        ));
                        assert_no_terminate_or_provision_effects(&shell.take_effects());
                        shell
                            .apply(ShellAction::FocusPane { id: focused })
                            .expect("restore");
                    }
                    Err(ShellError::NoDirectionalNeighbor) => {
                        assert_rejection_byte_identical(
                            &before,
                            &shell,
                            ShellError::NoDirectionalNeighbor,
                        );
                    }
                    Err(err) => panic!("unexpected: {err:?}"),
                }
            }
        }
    }
}

#[test]
fn spec025_p8_move_swap_zoom_focus_emit_no_terminate_or_provision() {
    let mut rng = Lcg::from_seed(42);
    let mut shell = seed_splittable();
    grow_small_tree(&mut shell, &mut rng, 3);
    let ids: Vec<_> = shell.snapshot().panes.iter().map(|p| p.id).collect();
    let (a, b) = (ids[0], ids[1]);

    let actions = [
        ShellAction::SwapPanes {
            a,
            b,
            containment_generation: shell.containment_generation(),
        },
        ShellAction::MovePaneBeside {
            pane: a,
            neighbor: b,
            side: MoveSide::Below,
            containment_generation: shell.containment_generation(),
        },
        ShellAction::ZoomPane { id: a },
        ShellAction::Unzoom,
        ShellAction::FocusPane { id: b },
        ShellAction::FocusDirection {
            direction: FocusDirection::Right,
        },
        ShellAction::FocusDirection {
            direction: FocusDirection::Left,
        },
    ];

    for action in actions {
        let _ = shell.take_effects();
        let _ = shell.apply(action);
        assert_no_terminate_or_provision_effects(&shell.take_effects());
    }
}

#[test]
fn spec025_p9_random_small_tree_sequences_preserve_p1_through_p8() {
    for seed in 1u64..=48 {
        let mut rng = Lcg::from_seed(seed.wrapping_add(900));
        let mut shell = seed_splittable();
        grow_small_tree(&mut shell, &mut rng, 2 + (seed as usize % 3));
        bind_some_panes(&mut shell, &mut rng, seed as usize % 2);

        for _ in 0..20 {
            let kind = *rng.pick(STREAM_KINDS);
            apply_stream_step(&mut shell, &mut rng, kind);
            assert_tab_invariants(&shell.snapshot());
            assert!(!shell.snapshot().panes.is_empty(), "P6 via P9");
        }
    }
}
