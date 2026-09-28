//! SPEC-024 §14 item 18 for directional pane focus (K7).

use seyal_core::{PaneId, TabId, WindowId, WorkspaceId};

use super::*;
use crate::keybinding::{
    load_keybinding_table, normalized_from_notation, route_keystroke, BindingContext, RouteOutcome,
    WorkspaceCommand, WorkspaceCommandId,
};
use crate::shell::{ShellPaneSeed, ShellTabSeed, ShellWindowSeed, ShellWorkspaceSeed, SplitAxis};

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

fn invoke_builtin(root: &mut ApplicationRoot, id: WorkspaceCommandId) -> Result<(), AppError> {
    root.invoke_workspace_command(WorkspaceCommand { id, ordinal: None }, BindingContext::APP)
}

fn route_builtin(keys: &str) -> RouteOutcome {
    let table = load_keybinding_table(None);
    let stroke = normalized_from_notation(keys).expect(keys);
    route_keystroke(&table, &stroke, BindingContext::APP, false)
}

fn shell_identity(snap: &ShellSnapshot) -> (u64, crate::shell::PaneTree, PaneId, Option<PaneId>) {
    (
        snap.containment_generation,
        snap.tree.clone(),
        snap.focused_pane,
        snap.zoomed,
    )
}

#[test]
fn item18_focus_ids_match_builtins_and_write_zero_pty_bytes() {
    for (keys, id) in [
        ("cmd+opt+left", WorkspaceCommandId::PaneFocusLeft),
        ("cmd+opt+right", WorkspaceCommandId::PaneFocusRight),
        ("cmd+opt+up", WorkspaceCommandId::PaneFocusUp),
        ("cmd+opt+down", WorkspaceCommandId::PaneFocusDown),
    ] {
        let matched = route_builtin(keys);
        assert!(
            matches!(
                matched,
                RouteOutcome::Matched {
                    command: WorkspaceCommand { id: matched_id, .. }
                } if matched_id == id
            ),
            "{keys} → {id:?}: {matched:?}"
        );
        assert!(
            !matched.writes_pty_bytes(),
            "{keys} must write zero PTY bytes"
        );
    }
}

#[test]
fn item18_focus_right_moves_focus_with_zero_pty_bytes() {
    let mut root = split_enabled_root();
    root.apply(AppAction::SplitFocused {
        axis: SplitAxis::Right,
    })
    .expect("split");
    let right = root.snapshot().shell.focused_pane;
    let left = root
        .snapshot()
        .shell
        .panes
        .iter()
        .map(|pane| pane.id)
        .find(|id| *id != right)
        .expect("left");
    root.apply(AppAction::FocusPane { id: left })
        .expect("focus left");
    let before_output = root.snapshot().output_utf8.clone();
    let before_tree = root.snapshot().shell.tree.clone();

    invoke_builtin(&mut root, WorkspaceCommandId::PaneFocusRight).expect("focus right");
    let snap = root.snapshot();
    assert_eq!(snap.shell.focused_pane, right);
    assert_eq!(snap.shell.tree, before_tree);
    assert_eq!(snap.output_utf8, before_output);
}

#[test]
fn item18_no_neighbor_rejects_atomically() {
    let mut root = split_enabled_root();
    let before = shell_identity(&root.snapshot().shell);
    let focused = root.snapshot().shell.focused_pane;
    let before_output = root.snapshot().output_utf8.clone();

    assert_eq!(
        invoke_builtin(&mut root, WorkspaceCommandId::PaneFocusLeft),
        Err(AppError::NoDirectionalNeighbor)
    );
    assert_eq!(
        invoke_builtin(&mut root, WorkspaceCommandId::PaneFocusUp),
        Err(AppError::NoDirectionalNeighbor)
    );
    assert_eq!(shell_identity(&root.snapshot().shell), before);
    assert_eq!(root.snapshot().shell.focused_pane, focused);
    assert_eq!(root.snapshot().output_utf8, before_output);
}

#[test]
fn item18_unmatched_stroke_still_falls_through_as_k3() {
    let table = load_keybinding_table(None);
    let stroke = normalized_from_notation("ctrl+b").expect("ctrl+b");
    let outcome = route_keystroke(&table, &stroke, BindingContext::APP, false);
    assert_eq!(outcome, RouteOutcome::Fallthrough);

    let cmd = normalized_from_notation("cmd+u").expect("cmd+u");
    let unmatched = route_keystroke(&table, &cmd, BindingContext::APP, false);
    assert_eq!(unmatched, RouteOutcome::UnmatchedCommand);
    assert!(!unmatched.writes_pty_bytes());
}

#[test]
fn item18_zoom_swap_move_equalize_ids_remain_absent() {
    assert!(WorkspaceCommandId::parse("pane.zoom_toggle").is_none());
    assert!(WorkspaceCommandId::parse("pane.swap_left").is_none());
    assert!(WorkspaceCommandId::parse("pane.swap_right").is_none());
    assert!(WorkspaceCommandId::parse("pane.move_left").is_none());
    assert!(WorkspaceCommandId::parse("pane.move_right").is_none());
    assert!(WorkspaceCommandId::parse("pane.equalize_focused").is_none());
    assert!(WorkspaceCommandId::parse("pane.equalize_tab").is_none());
    assert_eq!(
        route_builtin("cmd+shift+enter"),
        RouteOutcome::UnmatchedCommand
    );
}
