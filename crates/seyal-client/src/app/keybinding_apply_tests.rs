//! K8: `goto.open` dispatch opens the existing N4 surface.

use crate::goto::GotoScope;
use crate::keybinding::{WorkspaceCommand, WorkspaceCommandId};

use super::ApplicationRoot;

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

fn invoke(root: &mut ApplicationRoot, id: WorkspaceCommandId) -> Result<(), super::AppError> {
    let route = root.keybinding_route_context(false);
    root.invoke_workspace_command(WorkspaceCommand { id, ordinal: None }, route)
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
        Err(super::AppError::ActionUnavailable)
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
