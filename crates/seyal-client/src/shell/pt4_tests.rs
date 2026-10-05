//! PT4 equalize fixtures (SPEC-025 §5.6 / §12 item 13).

use super::*;
use crate::pane_layout::SplitRatio;
use seyal_core::WindowId;

fn ratios(tree: &PaneTree) -> Vec<SplitRatio> {
    match tree {
        PaneTree::Leaf(_) => Vec::new(),
        PaneTree::Split {
            first,
            second,
            ratio,
            ..
        } => {
            let mut out = vec![*ratio];
            out.extend(ratios(first));
            out.extend(ratios(second));
            out
        }
    }
}

fn topology_fingerprint(tree: &PaneTree) -> (Vec<PaneId>, Vec<SplitAxis>) {
    match tree {
        PaneTree::Leaf(id) => (vec![*id], Vec::new()),
        PaneTree::Split {
            axis,
            first,
            second,
            ..
        } => {
            let (mut panes, mut axes) = topology_fingerprint(first);
            let (right_panes, right_axes) = topology_fingerprint(second);
            panes.extend(right_panes);
            axes.push(*axis);
            axes.extend(right_axes);
            (panes, axes)
        }
    }
}

fn uneven(value: f32) -> SplitRatio {
    SplitRatio::from_fraction(value).expect("fixture ratio")
}

/// Nested tree via production split actions, then uneven ratios via #928.
///
/// ```text
/// Split Right (0.3)
///   Leaf a
///   Split Down (0.7)
///     Leaf b
///     Split Right (0.25)
///       Leaf c
///       Leaf d   ← focused
/// ```
fn shell_with_nested_splits() -> ShellState {
    let a = PaneId::new();
    let tab_id = TabId::new();
    let window_id = WindowId::new();
    let mut shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: WorkspaceId::m001_default(),
            name: "Local".to_owned(),
            detail: None,
            attention: false,
            active_window: window_id,
            windows: vec![ShellWindowSeed {
                id: window_id,
                active_tab: tab_id,
                tabs: vec![ShellTabSeed {
                    id: tab_id,
                    title: "Terminal".to_owned(),
                    attention: false,
                    pane: ShellPaneSeed {
                        id: a,
                        title: "a".to_owned(),
                        allows_implicit_execution_bootstrap: true,
                    },
                }],
            }],
        }],
        WorkspaceId::m001_default(),
        true,
        true,
    )
    .expect("fixture");

    shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Right,
            containment_generation: shell.containment_generation(),
        })
        .expect("split root right");
    shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Down,
            containment_generation: shell.containment_generation(),
        })
        .expect("split mid down");
    shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Right,
            containment_generation: shell.containment_generation(),
        })
        .expect("split inner right");

    let leaves = shell.snapshot().tree.pane_ids();
    assert_eq!(leaves.len(), 4);
    shell
        .apply(ShellAction::SetSplitRatio {
            pane: leaves[0],
            ratio: uneven(0.3),
        })
        .expect("root ratio");
    shell
        .apply(ShellAction::SetSplitRatio {
            pane: leaves[1],
            ratio: uneven(0.7),
        })
        .expect("mid ratio");
    shell
        .apply(ShellAction::SetSplitRatio {
            pane: leaves[2],
            ratio: uneven(0.25),
        })
        .expect("inner ratio");

    let focused = leaves[3];
    shell
        .apply(ShellAction::FocusPane { id: focused })
        .expect("focus deepest");
    shell
}

#[test]
fn equalize_tab_normalizes_nested_ratios_and_preserves_topology() {
    let mut shell = shell_with_nested_splits();
    let before = shell.snapshot();
    let focused = before.focused_pane;
    assert_ne!(ratios(&before.tree), vec![SplitRatio::HALF; 3]);
    let topo_before = topology_fingerprint(&before.tree);
    let generation = before.containment_generation;

    shell
        .apply(ShellAction::EqualizeTab {
            containment_generation: generation,
        })
        .expect("EqualizeTab");

    let after = shell.snapshot();
    assert_eq!(ratios(&after.tree), vec![SplitRatio::HALF; 3]);
    assert_eq!(topology_fingerprint(&after.tree), topo_before);
    assert_eq!(after.focused_pane, focused);
    assert_eq!(after.containment_generation, generation + 1);
}

#[test]
fn equalize_focused_scopes_to_nearest_ancestor_split() {
    let mut shell = shell_with_nested_splits();
    let before = shell.snapshot();
    let focused = before.focused_pane;
    let topo_before = topology_fingerprint(&before.tree);
    let generation = before.containment_generation;
    let before_ratios = ratios(&before.tree);
    assert_eq!(before_ratios.len(), 3);

    shell
        .apply(ShellAction::EqualizeFocused {
            containment_generation: generation,
        })
        .expect("EqualizeFocused");

    let after = shell.snapshot();
    let after_ratios = ratios(&after.tree);
    // Nearest ancestor of the deepest focused leaf is the inner Right split only.
    assert_eq!(after_ratios[0], before_ratios[0]); // root unchanged
    assert_eq!(after_ratios[1], before_ratios[1]); // mid unchanged
    assert_eq!(after_ratios[2], SplitRatio::HALF); // inner equalized
    assert_eq!(topology_fingerprint(&after.tree), topo_before);
    assert_eq!(after.focused_pane, focused);
    assert_eq!(after.containment_generation, generation + 1);
}

#[test]
fn equalize_focused_on_single_leaf_tab_is_success_noop() {
    let mut shell = ShellState::m001_local(".");
    let before = shell.snapshot();
    assert_eq!(before.tree, PaneTree::Leaf(before.focused_pane));
    let generation = before.containment_generation;

    shell
        .apply(ShellAction::EqualizeFocused {
            containment_generation: generation,
        })
        .expect("single-leaf EqualizeFocused");

    let after = shell.snapshot();
    assert_eq!(after.tree, before.tree);
    assert_eq!(after.focused_pane, before.focused_pane);
    assert_eq!(after.containment_generation, generation + 1);
    assert!(ratios(&after.tree).is_empty());
}

#[test]
fn equalize_tab_on_single_leaf_tab_is_success_noop() {
    let mut shell = ShellState::m001_local(".");
    let before = shell.snapshot();
    let generation = before.containment_generation;

    shell
        .apply(ShellAction::EqualizeTab {
            containment_generation: generation,
        })
        .expect("single-leaf EqualizeTab");

    let after = shell.snapshot();
    assert_eq!(after.tree, before.tree);
    assert_eq!(after.containment_generation, generation + 1);
}

#[test]
fn equalize_rejects_stale_containment_generation() {
    let mut shell = shell_with_nested_splits();
    let before = shell.snapshot();
    let stale = before.containment_generation.wrapping_add(9);
    let ratios_before = ratios(&before.tree);

    assert_eq!(
        shell.apply(ShellAction::EqualizeTab {
            containment_generation: stale,
        }),
        Err(ShellError::StaleContainment)
    );
    assert_eq!(
        shell.apply(ShellAction::EqualizeFocused {
            containment_generation: stale,
        }),
        Err(ShellError::StaleContainment)
    );

    let after = shell.snapshot();
    assert_eq!(ratios(&after.tree), ratios_before);
    assert_eq!(after.containment_generation, before.containment_generation);
    assert_eq!(after.last_error, Some(ShellError::StaleContainment));
}

#[test]
fn equalize_tab_when_ratios_already_half_is_ratio_noop_that_still_bumps() {
    let mut shell = shell_with_nested_splits();
    let generation = shell.containment_generation();
    shell
        .apply(ShellAction::EqualizeTab {
            containment_generation: generation,
        })
        .expect("first equalize");
    let mid = shell.snapshot();
    assert_eq!(ratios(&mid.tree), vec![SplitRatio::HALF; 3]);

    shell
        .apply(ShellAction::EqualizeTab {
            containment_generation: mid.containment_generation,
        })
        .expect("second equalize");
    let after = shell.snapshot();
    assert_eq!(ratios(&after.tree), vec![SplitRatio::HALF; 3]);
    assert_eq!(after.containment_generation, mid.containment_generation + 1);
}

#[test]
fn equalize_tab_clears_zoom_including_ratio_noop() {
    let mut shell = shell_with_nested_splits();
    let zoomed = shell.snapshot().focused_pane;
    shell
        .apply(ShellAction::ZoomPane { id: zoomed })
        .expect("zoom");
    assert_eq!(shell.snapshot().zoomed, Some(zoomed));
    let generation = shell.containment_generation();

    shell
        .apply(ShellAction::EqualizeTab {
            containment_generation: generation,
        })
        .expect("EqualizeTab");
    let after = shell.snapshot();
    assert_eq!(after.zoomed, None);
    assert_eq!(after.focused_pane, zoomed);
    assert_eq!(after.containment_generation, generation + 1);

    shell
        .apply(ShellAction::ZoomPane { id: zoomed })
        .expect("re-zoom");
    shell
        .apply(ShellAction::EqualizeTab {
            containment_generation: after.containment_generation,
        })
        .expect("ratio-noop EqualizeTab still clears zoom");
    assert_eq!(shell.snapshot().zoomed, None);
}

#[test]
fn equalize_focused_clears_zoom_on_single_leaf_noop() {
    let mut shell = ShellState::m001_local(".");
    let pane = shell.snapshot().focused_pane;
    shell
        .apply(ShellAction::ZoomPane { id: pane })
        .expect("zoom");
    assert_eq!(shell.snapshot().zoomed, Some(pane));
    let generation = shell.containment_generation();

    shell
        .apply(ShellAction::EqualizeFocused {
            containment_generation: generation,
        })
        .expect("single-leaf EqualizeFocused");
    let after = shell.snapshot();
    assert_eq!(after.zoomed, None);
    assert_eq!(after.focused_pane, pane);
    assert_eq!(after.tree, PaneTree::Leaf(pane));
    assert_eq!(after.containment_generation, generation + 1);
}

#[test]
fn stale_equalize_does_not_clear_zoom() {
    let mut shell = shell_with_nested_splits();
    let zoomed = shell.snapshot().focused_pane;
    shell
        .apply(ShellAction::ZoomPane { id: zoomed })
        .expect("zoom");
    let before = shell.snapshot();
    let stale = before.containment_generation.wrapping_add(9);

    assert_eq!(
        shell.apply(ShellAction::EqualizeTab {
            containment_generation: stale,
        }),
        Err(ShellError::StaleContainment)
    );
    let after = shell.snapshot();
    assert_eq!(after.zoomed, Some(zoomed));
    assert_eq!(after.containment_generation, before.containment_generation);
}
