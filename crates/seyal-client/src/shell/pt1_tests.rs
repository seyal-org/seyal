//! SPEC-025 §12 PT1 fixtures (items 1–5, 9–12, 17).
//!
//! Kept as a `_tests` sibling so the W2/W3 shell suite stays under hard_loc.

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

// --- SPEC-025 §12 PT1 fixtures (items 1–5, 9–12, 17) ---

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

#[test]
fn spec025_1_split_focuses_new_leaf_and_clears_zoom() {
    let mut shell = seed_two_workspaces();
    let original = shell.snapshot().focused_pane;
    shell
        .apply(ShellAction::ZoomPane { id: original })
        .expect("zoom");
    assert_eq!(shell.snapshot().zoomed, Some(original));
    shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Right,
            containment_generation: shell.containment_generation(),
        })
        .expect("split right");
    let right = shell.snapshot();
    assert_eq!(right.layout, LayoutDescription::SplitRight);
    assert_ne!(right.focused_pane, original);
    assert_eq!(right.zoomed, None);
    shell
        .apply(ShellAction::ZoomPane {
            id: right.focused_pane,
        })
        .expect("re-zoom");
    shell
        .apply(ShellAction::SplitPane {
            id: right.focused_pane,
            axis: SplitAxis::Down,
            containment_generation: shell.containment_generation(),
        })
        .expect("split down");
    let down = shell.snapshot();
    assert_eq!(down.layout, LayoutDescription::SplitRight);
    assert_eq!(down.tabs[0].pane_count, 3);
    assert_ne!(down.focused_pane, right.focused_pane);
    assert_eq!(down.zoomed, None);
    assert_eq!(
        down.windows[0].tabs[0].zoomed, None,
        "product WindowTabSnapshot exposes cleared zoom"
    );
}

#[test]
fn spec025_2_close_non_focused_leaf_preserves_focus() {
    let (mut shell, a, b, c) = seed_nested_abc();
    shell
        .apply(ShellAction::FocusPane { id: a })
        .expect("focus A");
    shell
        .apply(ShellAction::ClosePane {
            id: b,
            containment_generation: shell.containment_generation(),
        })
        .expect("close B");
    let snap = shell.snapshot();
    assert_eq!(snap.focused_pane, a);
    assert_eq!(snap.tabs[0].pane_count, 2);
    assert!(snap.panes.iter().any(|pane| pane.id == a));
    assert!(snap.panes.iter().any(|pane| pane.id == c));
    assert!(!snap.panes.iter().any(|pane| pane.id == b));
}

#[test]
fn spec025_3_close_focused_uses_sibling_first_successor() {
    let (mut shell, a, b, c) = seed_nested_abc();
    shell
        .apply(ShellAction::FocusPane { id: b })
        .expect("focus B");
    shell
        .apply(ShellAction::ClosePane {
            id: b,
            containment_generation: shell.containment_generation(),
        })
        .expect("close B");
    let snap = shell.snapshot();
    assert_eq!(
        snap.focused_pane, c,
        "sibling-first successor is C, not whole-tree first leaf A"
    );
    assert_ne!(snap.focused_pane, a);
    assert_eq!(snap.tabs[0].pane_count, 2);
}

#[test]
fn spec025_4_close_last_pane_peels_under_w2b() {
    // W2b/W4b hierarchical peel: last Pane closes the Tab/Window rather than
    // rejecting with CannotCloseLastPane (PT1-alone policy on the W4a tip).
    let mut shell = seed_two_workspaces();
    let only = shell.snapshot().focused_pane;
    assert!(shell
        .apply(ShellAction::ClosePane {
            id: only,
            containment_generation: shell.containment_generation(),
        })
        .is_ok());
}

#[test]
fn spec025_5_stale_pane_id_fails_closed_on_pt1_actions() {
    let mut shell = seed_two_workspaces();
    shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Right,
            containment_generation: shell.containment_generation(),
        })
        .expect("split");
    let stale = PaneId::new();
    let actions = [
        ShellAction::ClosePane {
            id: stale,
            containment_generation: shell.containment_generation(),
        },
        ShellAction::FocusPane { id: stale },
        ShellAction::SplitPane {
            id: stale,
            axis: SplitAxis::Right,
            containment_generation: shell.containment_generation(),
        },
        ShellAction::ZoomPane { id: stale },
    ];
    for action in actions {
        let before = shell.clone();
        assert_eq!(shell.apply(action), Err(ShellError::UnknownPane));
        assert_rejection_atomic(&shell, &before);
        assert_eq!(shell.last_error(), Some(ShellError::UnknownPane));
    }
}

#[test]
fn spec025_5_stale_containment_generation_rejects_structural_pt1_actions() {
    let mut shell = seed_two_workspaces();
    let generation = shell.containment_generation();
    shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Right,
            containment_generation: generation,
        })
        .expect("split");
    assert!(shell.containment_generation() > generation);
    let focused = shell.snapshot().focused_pane;
    let stale = generation; // pre-split generation
    let before = shell.clone();
    let fingerprint = before.containment_fingerprint();
    for action in [
        ShellAction::SplitFocused {
            axis: SplitAxis::Down,
            containment_generation: stale,
        },
        ShellAction::SplitPane {
            id: focused,
            axis: SplitAxis::Down,
            containment_generation: stale,
        },
        ShellAction::ClosePane {
            id: focused,
            containment_generation: stale,
        },
    ] {
        let prior = shell.clone();
        assert_eq!(shell.apply(action), Err(ShellError::StaleContainment));
        assert_rejection_atomic(&shell, &prior);
        assert_eq!(shell.last_error(), Some(ShellError::StaleContainment));
        assert_eq!(shell.containment_fingerprint(), fingerprint);
    }
    // Focus/Zoom/Unzoom are not generation-fenced and must not bump.
    let gen_now = shell.containment_generation();
    shell
        .apply(ShellAction::FocusPane { id: focused })
        .expect("focus");
    shell
        .apply(ShellAction::ZoomPane { id: focused })
        .expect("zoom");
    shell.apply(ShellAction::Unzoom).expect("unzoom");
    assert_eq!(shell.containment_generation(), gen_now);
}

#[test]
fn spec025_9_zoom_overlay_preserves_topology_and_snapshot_field() {
    let mut shell = seed_two_workspaces();
    shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Right,
            containment_generation: shell.containment_generation(),
        })
        .expect("split");
    let before = shell.snapshot();
    let target = before.focused_pane;
    let tree_before = before.tree.clone();
    shell
        .apply(ShellAction::ZoomPane { id: target })
        .expect("zoom");
    let after = shell.snapshot();
    assert_eq!(after.tree, tree_before);
    assert_eq!(after.layout, before.layout);
    assert_eq!(after.zoomed, Some(target));
    assert_eq!(after.focused_pane, target);
    assert_eq!(after.windows[0].tabs[0].zoomed, Some(target));
    shell
        .apply(ShellAction::ZoomPane { id: target })
        .expect("idempotent zoom");
    assert_eq!(shell.snapshot().zoomed, Some(target));
    assert_eq!(shell.snapshot().tree, tree_before);
}

#[test]
fn spec025_10_unzoom_preserves_focus_and_not_zoomed_fails_closed() {
    let mut shell = seed_two_workspaces();
    let pane = shell.snapshot().focused_pane;
    let before = shell.clone();
    assert_eq!(shell.apply(ShellAction::Unzoom), Err(ShellError::NotZoomed));
    assert_rejection_atomic(&shell, &before);
    assert_eq!(shell.last_error(), Some(ShellError::NotZoomed));
    shell
        .apply(ShellAction::ZoomPane { id: pane })
        .expect("zoom");
    shell.apply(ShellAction::Unzoom).expect("unzoom");
    let snap = shell.snapshot();
    assert_eq!(snap.zoomed, None);
    assert_eq!(snap.focused_pane, pane);
}

#[test]
fn spec025_11_focus_away_from_zoomed_clears_zoom() {
    let (mut shell, a, b, _) = seed_nested_abc();
    shell
        .apply(ShellAction::FocusPane { id: a })
        .expect("focus A");
    shell
        .apply(ShellAction::ZoomPane { id: a })
        .expect("zoom A");
    shell
        .apply(ShellAction::FocusPane { id: b })
        .expect("focus B");
    let snap = shell.snapshot();
    assert_eq!(snap.focused_pane, b);
    assert_eq!(snap.zoomed, None);
}

#[test]
fn spec025_12_close_zoomed_leaf_clears_zoom_and_applies_successor() {
    let (mut shell, a, b, c) = seed_nested_abc();
    shell
        .apply(ShellAction::FocusPane { id: b })
        .expect("focus B");
    shell
        .apply(ShellAction::ZoomPane { id: b })
        .expect("zoom B");
    shell
        .apply(ShellAction::ClosePane {
            id: b,
            containment_generation: shell.containment_generation(),
        })
        .expect("close B");
    let snap = shell.snapshot();
    assert_eq!(snap.zoomed, None);
    assert_eq!(snap.focused_pane, c);
    assert_ne!(snap.focused_pane, a);
}

#[test]
fn spec025_17_close_successor_differs_from_whole_tree_first_pane() {
    // Fixture: Split(A, Split(B, C)); close focused B.
    // Sibling-first → C. Pre-contract whole-tree first_pane of the remaining
    // root Split(A, C) → A. This locks the intentional SPEC-025 §5.2 change.
    let (mut shell, a, b, c) = seed_nested_abc();
    shell
        .apply(ShellAction::FocusPane { id: b })
        .expect("focus B");
    shell
        .apply(ShellAction::ClosePane {
            id: b,
            containment_generation: shell.containment_generation(),
        })
        .expect("close B");
    let snap = shell.snapshot();
    assert_eq!(snap.focused_pane, c);
    assert_eq!(snap.tree.first_pane(), Some(a));
    assert_ne!(
        snap.focused_pane,
        snap.tree.first_pane().expect("remaining tree has a leaf"),
        "regression: successor must not be whole-tree first_pane()"
    );
}
