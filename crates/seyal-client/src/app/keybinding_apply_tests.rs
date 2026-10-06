//! SPEC-024 §14 item 18 / K7 (#1145): zoom, swap, and move dispatch.
//!
//! Directional focus (#1150) and equalize (PT4/#928) are out of scope.

use crate::goto::GotoScope;
use crate::keybinding::{WorkspaceCommand, WorkspaceCommandId};
use crate::shell::{
    ShellAction, ShellPaneSeed, ShellState, ShellTabSeed, ShellWindowSeed, ShellWorkspaceSeed,
    SplitAxis,
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

#[test]
fn item18_focus_does_not_add_equalize_catalog() {
    // This Issue owns only the four focus ids; equalize waits for PT4/#928.
    assert_eq!(
        WorkspaceCommandId::parse("pane.focus_left"),
        Some(WorkspaceCommandId::PaneFocusLeft)
    );
    assert!(WorkspaceCommandId::parse("pane.equalize_focused").is_none());
}
