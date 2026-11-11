//! SPEC-024 §14 item 18 / K7: zoom, swap, move, focus, and equalize dispatch.

#[cfg(target_os = "macos")]
#[path = "pane_tree_apply_tests.rs"]
mod pane_tree_apply_tests;

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

/// Non-default chrome modes and a composer draft, so §5.6 "unchanged" is observable.
fn root_with_non_default_chrome_and_draft() -> ApplicationRoot {
    use crate::chrome::{InspectorMode, LeftPanelMode};
    use crate::composer::ComposerAction;

    let mut root = ApplicationRoot::new();
    root.set_left_panel(LeftPanelMode::Tabs).unwrap();
    root.set_inspector_mode(InspectorMode::Pane).unwrap();
    let pane = root.snapshot().shell.focused_pane;
    root.composer
        .apply(ComposerAction::EnsurePane { pane })
        .unwrap();
    let epoch = root.composer.snapshot(pane).unwrap().epoch;
    root.composer
        .apply(ComposerAction::SetDraft {
            pane,
            text: "draft kept".into(),
            epoch,
        })
        .unwrap();
    root
}

/// SPEC-024 §5.6 / §14 item 23: each toggle inverts exactly one visibility
/// field, leaves every other product field unchanged, and a second toggle
/// restores the prior state.
#[test]
fn chrome_toggles_invert_one_field_and_restore_on_second_press() {
    for id in [
        WorkspaceCommandId::ChromeToggleLeftPanel,
        WorkspaceCommandId::ChromeToggleInspector,
    ] {
        let mut root = root_with_non_default_chrome_and_draft();
        let pane = root.snapshot().shell.focused_pane;
        let before = root.snapshot();

        invoke(&mut root, id).expect("toggle");
        let mut after = root.snapshot();
        match id {
            WorkspaceCommandId::ChromeToggleLeftPanel => {
                assert!(!after.chrome.left_visible);
                after.chrome.left_visible = before.chrome.left_visible;
            }
            _ => {
                assert!(!after.chrome.inspector_visible);
                after.chrome.inspector_visible = before.chrome.inspector_visible;
            }
        }
        assert_eq!(
            after,
            before,
            "{} changed more than visibility",
            id.as_str()
        );
        assert_eq!(root.composer.snapshot(pane).unwrap().draft, "draft kept");

        invoke(&mut root, id).expect("second toggle");
        assert_eq!(root.snapshot(), before, "{} second press", id.as_str());
    }
}

/// The toggle reads the authoritative snapshot at dispatch: it inverts the
/// state another path left behind, not a cached host value.
#[test]
fn chrome_toggle_inverts_visibility_set_by_another_path() {
    let mut root = ApplicationRoot::new();
    root.set_shell_visibility(false, false, false).unwrap();

    invoke(&mut root, WorkspaceCommandId::ChromeToggleInspector).unwrap();
    let chrome = root.snapshot().chrome;
    assert!(!chrome.left_visible);
    assert!(chrome.inspector_visible);
    assert!(!chrome.tab_strip_visible, "tab strip is kept, not reset");
}

/// `cmd+0` / `cmd+opt+0` route through the Rust table to the toggles.
#[test]
fn builtin_strokes_route_to_chrome_toggles() {
    use crate::keybinding::{NormalizedStroke, RouteOutcome};

    let mut root = ApplicationRoot::new();
    // Host FFI modifier bits: CMD=1, OPT=8.
    for (keys, modifier_bits, id) in [
        ("cmd+0", 1, WorkspaceCommandId::ChromeToggleLeftPanel),
        (
            "cmd+opt+0",
            1 | 8,
            WorkspaceCommandId::ChromeToggleInspector,
        ),
    ] {
        let stroke =
            NormalizedStroke::from_ffi(modifier_bits, false, u32::from('0'), 0).expect(keys);
        let outcome = root
            .route_normalized_keystroke(&stroke, false, false)
            .unwrap();
        assert_eq!(
            outcome,
            RouteOutcome::Matched {
                command: WorkspaceCommand { id, ordinal: None }
            },
            "{keys}"
        );
    }
    let chrome = root.snapshot().chrome;
    assert!(!chrome.left_visible);
    assert!(!chrome.inspector_visible);
}

/// R6.4.1: with the palette owning the route the toggles are unavailable and
/// visibility is unchanged.
#[test]
fn chrome_toggles_are_unavailable_while_the_palette_owns_the_route() {
    let mut root = ApplicationRoot::new();
    invoke(&mut root, WorkspaceCommandId::CommandPaletteOpen).unwrap();
    assert_eq!(
        invoke(&mut root, WorkspaceCommandId::ChromeToggleLeftPanel),
        Err(AppError::ActionUnavailable)
    );
    assert!(root.snapshot().chrome.left_visible);
}

/// R5.6.2: menu titles derive from the current visibility.
#[test]
fn live_menu_titles_follow_visibility() {
    let left = WorkspaceCommand {
        id: WorkspaceCommandId::ChromeToggleLeftPanel,
        ordinal: None,
    };
    let inspector = WorkspaceCommand {
        id: WorkspaceCommandId::ChromeToggleInspector,
        ordinal: None,
    };
    let mut root = ApplicationRoot::new();
    assert_eq!(root.live_menu_title(left), "Hide Sidebar");
    assert_eq!(root.live_menu_title(inspector), "Hide Inspector");

    invoke(&mut root, WorkspaceCommandId::ChromeToggleLeftPanel).unwrap();
    assert_eq!(root.live_menu_title(left), "Show Sidebar");
    assert_eq!(root.live_menu_title(inspector), "Hide Inspector");

    invoke(&mut root, WorkspaceCommandId::ChromeToggleInspector).unwrap();
    assert_eq!(root.live_menu_title(inspector), "Show Inspector");

    let goto = WorkspaceCommand {
        id: WorkspaceCommandId::GotoOpen,
        ordinal: None,
    };
    assert_eq!(
        root.live_menu_title(goto),
        "Go to…",
        "stateless titles pass through"
    );
}
