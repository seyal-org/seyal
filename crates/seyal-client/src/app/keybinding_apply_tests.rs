//! SPEC-024 §14 item 18 / K7: zoom, swap, move, focus, and equalize dispatch.

use crate::goto::GotoScope;
use crate::keybinding::{WorkspaceCommand, WorkspaceCommandId};
use crate::pane_layout::SplitRatio;
use crate::shell::{
    PaneTree, ShellAction, ShellPaneSeed, ShellState, ShellTabSeed, ShellWindowSeed,
    ShellWorkspaceSeed, SplitAxis,
};
use seyal_core::{PaneId, TabId, WindowId, WorkspaceId};

use super::{AppError, ApplicationRoot};

#[test]
fn goto_open_command_opens_existing_goto_surface() {
    let mut root = ApplicationRoot::new();
    assert!(!root.snapshot().goto.open);

    let route = root.keybinding_route_context(false);
    root.invoke_workspace_command(
        WorkspaceCommand {
            id: WorkspaceCommandId::GotoOpen,
            ordinal: None,
        },
        route,
    )
    .expect("goto.open");

    let goto = root.snapshot().goto;
    assert!(goto.open);
    assert_eq!(goto.scope, GotoScope::Panes);
}

fn split_enabled_root() -> ApplicationRoot {
    let tab = TabId::new();
    let window = WindowId::new();
    let shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: WorkspaceId::m001_default(),
            name: "Local".to_owned(),
            detail: None,
            attention: false,
            active_window: window,
            windows: vec![ShellWindowSeed {
                id: window,
                active_tab: tab,
                tabs: vec![ShellTabSeed {
                    id: tab,
                    title: "Terminal".to_owned(),
                    attention: false,
                    pane: ShellPaneSeed {
                        id: PaneId::new(),
                        title: "Pane 1".to_owned(),
                        allows_implicit_execution_bootstrap: true,
                    },
                }],
            }],
        }],
        WorkspaceId::m001_default(),
        true,
        false,
    )
    .expect("fixture");
    ApplicationRoot::with_shell(shell)
}

fn invoke(root: &mut ApplicationRoot, id: WorkspaceCommandId) -> Result<(), AppError> {
    let route = root.keybinding_route_context(false);
    root.invoke_workspace_command(WorkspaceCommand { id, ordinal: None }, route)
}

fn seed_ab(root: &mut ApplicationRoot) -> (PaneId, PaneId) {
    let a = root.snapshot().shell.focused_pane;
    root.split_focused(SplitAxis::Right).expect("A|B");
    let b = root.snapshot().shell.focused_pane;
    root.focus_pane(a).expect("focus A");
    (a, b)
}

#[test]
fn item18_zoom_toggle_zooms_and_unzooms() {
    let mut root = split_enabled_root();
    let (a, _b) = seed_ab(&mut root);
    assert_eq!(root.snapshot().shell.zoomed, None);

    invoke(&mut root, WorkspaceCommandId::PaneZoomToggle).expect("zoom");
    assert_eq!(root.snapshot().shell.zoomed, Some(a));

    invoke(&mut root, WorkspaceCommandId::PaneZoomToggle).expect("unzoom");
    assert_eq!(root.snapshot().shell.zoomed, None);
}

#[test]
fn item18_swap_right_preserves_pane_ids_and_miss_rejects() {
    let mut root = split_enabled_root();
    let (a, _b) = seed_ab(&mut root);
    let before_ids: std::collections::BTreeSet<_> =
        root.snapshot().shell.panes.iter().map(|p| p.id).collect();

    invoke(&mut root, WorkspaceCommandId::PaneSwapRight).expect("swap right");
    let after = root.snapshot().shell;
    assert_eq!(after.focused_pane, a);
    let after_ids: std::collections::BTreeSet<_> = after.panes.iter().map(|p| p.id).collect();
    assert_eq!(before_ids, after_ids);

    // Leftmost leaf has no left neighbor.
    let mut root = split_enabled_root();
    let (a, _b) = seed_ab(&mut root);
    assert_eq!(
        invoke(&mut root, WorkspaceCommandId::PaneSwapLeft),
        Err(AppError::NoDirectionalNeighbor)
    );
    assert_eq!(root.snapshot().shell.focused_pane, a);
    assert_eq!(root.snapshot().shell.zoomed, None);
}

#[test]
fn item18_move_right_preserves_ids_and_miss_rejects() {
    let mut root = split_enabled_root();
    let (a, b) = seed_ab(&mut root);
    let before_gen = root.snapshot().shell.containment_generation;
    let before_ids: std::collections::BTreeSet<_> =
        root.snapshot().shell.panes.iter().map(|p| p.id).collect();

    invoke(&mut root, WorkspaceCommandId::PaneMoveRight).expect("move right");
    let after = root.snapshot().shell;
    assert!(after.containment_generation > before_gen);
    let after_ids: std::collections::BTreeSet<_> = after.panes.iter().map(|p| p.id).collect();
    assert_eq!(before_ids, after_ids);
    assert!(after.panes.iter().any(|p| p.id == a));
    assert!(after.panes.iter().any(|p| p.id == b));

    let mut single = split_enabled_root();
    let focused = single.snapshot().shell.focused_pane;
    assert_eq!(
        invoke(&mut single, WorkspaceCommandId::PaneMoveDown),
        Err(AppError::NoDirectionalNeighbor)
    );
    assert_eq!(single.snapshot().shell.focused_pane, focused);
}

#[test]
fn item18_swap_left_from_right_leaf_uses_neighbor() {
    let mut root = split_enabled_root();
    let (_a, b) = seed_ab(&mut root);
    root.focus_pane(b).expect("focus B");
    let before_gen = root.snapshot().shell.containment_generation;
    invoke(&mut root, WorkspaceCommandId::PaneSwapLeft).expect("swap left from B");
    assert!(root.snapshot().shell.containment_generation > before_gen);
    assert_eq!(root.snapshot().shell.focused_pane, b);
}

#[test]
fn item18_focus_right_calls_focus_direction_and_miss_rejects() {
    let mut root = split_enabled_root();
    let (a, b) = seed_ab(&mut root);
    assert_eq!(root.snapshot().shell.focused_pane, a);

    invoke(&mut root, WorkspaceCommandId::PaneFocusRight).expect("focus right");
    assert_eq!(root.snapshot().shell.focused_pane, b);
    assert_eq!(root.shell.last_error(), None);

    // No right neighbor from B.
    assert_eq!(
        invoke(&mut root, WorkspaceCommandId::PaneFocusRight),
        Err(AppError::NoDirectionalNeighbor)
    );
    assert_eq!(root.snapshot().shell.focused_pane, b);
    assert_eq!(
        root.shell.last_error(),
        Some(crate::shell::ShellError::NoDirectionalNeighbor)
    );

    // Zoom + miss: focus and zoom unchanged except last_error (PT3 contract).
    root.focus_pane(a).expect("focus A");
    root.shell
        .apply(ShellAction::ZoomPane { id: a })
        .expect("zoom A");
    assert_eq!(
        invoke(&mut root, WorkspaceCommandId::PaneFocusLeft),
        Err(AppError::NoDirectionalNeighbor)
    );
    assert_eq!(root.snapshot().shell.focused_pane, a);
    assert_eq!(root.snapshot().shell.zoomed, Some(a));
}

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

fn seed_nested_uneven(root: &mut ApplicationRoot) {
    root.split_focused(SplitAxis::Right).expect("root right");
    root.split_focused(SplitAxis::Down).expect("mid down");
    root.split_focused(SplitAxis::Right).expect("inner right");
    let leaves = topology_fingerprint(&root.snapshot().shell.tree).0;
    assert_eq!(leaves.len(), 4);
    let uneven = |value| SplitRatio::from_fraction(value).expect("fixture ratio");
    root.shell
        .apply(ShellAction::SetSplitRatio {
            pane: leaves[0],
            ratio: uneven(0.3),
        })
        .expect("root ratio");
    root.shell
        .apply(ShellAction::SetSplitRatio {
            pane: leaves[1],
            ratio: uneven(0.7),
        })
        .expect("mid ratio");
    root.shell
        .apply(ShellAction::SetSplitRatio {
            pane: leaves[2],
            ratio: uneven(0.25),
        })
        .expect("inner ratio");
    root.focus_pane(leaves[3]).expect("focus deepest");
}

#[test]
fn item18_equalize_focused_calls_pt4_reducer() {
    let mut root = split_enabled_root();
    seed_nested_uneven(&mut root);
    let before = root.snapshot().shell;
    let topo_before = topology_fingerprint(&before.tree);
    let before_ratios = ratios(&before.tree);
    assert_eq!(before_ratios.len(), 3);

    invoke(&mut root, WorkspaceCommandId::PaneEqualizeFocused).expect("equalize focused");

    let after = root.snapshot().shell;
    let after_ratios = ratios(&after.tree);
    assert_eq!(after_ratios[0], before_ratios[0]);
    assert_eq!(after_ratios[1], before_ratios[1]);
    assert_eq!(after_ratios[2], SplitRatio::HALF);
    assert_eq!(topology_fingerprint(&after.tree), topo_before);
    assert_eq!(after.focused_pane, before.focused_pane);
    assert_eq!(after.last_error, None);
}

#[test]
fn item18_equalize_tab_calls_pt4_reducer_and_single_leaf_is_noop() {
    let mut nested = split_enabled_root();
    seed_nested_uneven(&mut nested);
    let before = nested.snapshot().shell;
    let topo_before = topology_fingerprint(&before.tree);
    assert_ne!(ratios(&before.tree), vec![SplitRatio::HALF; 3]);

    invoke(&mut nested, WorkspaceCommandId::PaneEqualizeTab).expect("equalize tab");

    let after = nested.snapshot().shell;
    assert_eq!(ratios(&after.tree), vec![SplitRatio::HALF; 3]);
    assert_eq!(topology_fingerprint(&after.tree), topo_before);
    assert_eq!(after.focused_pane, before.focused_pane);

    let mut single = split_enabled_root();
    let leaf_before = single.snapshot().shell;
    invoke(&mut single, WorkspaceCommandId::PaneEqualizeFocused).expect("single-leaf focused");
    invoke(&mut single, WorkspaceCommandId::PaneEqualizeTab).expect("single-leaf tab");
    let leaf_after = single.snapshot().shell;
    assert_eq!(leaf_after.tree, leaf_before.tree);
    assert_eq!(leaf_after.focused_pane, leaf_before.focused_pane);
    assert!(ratios(&leaf_after.tree).is_empty());
}
