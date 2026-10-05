//! SPEC-024 §14 items 12 and 19 (menu half); AX label privacy.

use super::load::load_keybinding_table;
use super::projection::{
    project_shortcuts, projected_item_for, workspace_command_ffi_id, workspace_command_from_ffi_id,
    ProjectedShortcut,
};
use super::route::route_context_set;
use super::types::{BindingContext, KeySym, Modifiers, WorkspaceCommand, WorkspaceCommandId};
use crate::presentation::PresentationMode;

fn item_for(
    id: WorkspaceCommandId,
    projection: &super::projection::KeybindingShortcutProjection,
) -> &ProjectedShortcut {
    projected_item_for(projection, WorkspaceCommand { id, ordinal: None })
        .unwrap_or_else(|| panic!("missing projected {}", id.as_str()))
}

/// SPEC-024 §14 item 12 (R11.1).
#[test]
fn item12_menu_ax_projection_key_equivalent_and_chord_hints() {
    let toml = r#"
[[keybindings]]
keys = "cmd+shift+p"
action = "command_palette.open"
"#;
    let table = load_keybinding_table(Some(toml));
    let route = route_context_set(false, PresentationMode::Flow, false);
    let projection = project_shortcuts(&table, route);

    let palette = item_for(WorkspaceCommandId::CommandPaletteOpen, &projection);
    assert_eq!(
        palette.key_equivalent_notation.as_deref(),
        Some("cmd+shift+p"),
        "highest-declaration-index single-stroke app binding wins"
    );
    let equiv = palette.key_equivalent.expect("key equivalent");
    assert_eq!(equiv.modifiers, Modifiers::CMD.union(Modifiers::SHIFT));
    assert_eq!(equiv.key, KeySym::Char('p'));
    let notations: Vec<&str> = palette
        .hints
        .iter()
        .map(|h| h.keys_notation.as_str())
        .collect();
    assert!(
        notations.contains(&"cmd+k") && notations.contains(&"cmd+shift+p"),
        "both bindings appear as hints: {notations:?}"
    );
    assert!(!palette.hints.iter().any(|h| h.is_chord));

    // Chord-only command: unbind builtin cmd+t, bind a Command chord (app-safe).
    let chord_only = r#"
[[keybindings]]
keys = "cmd+t"
action = "none"

[[keybindings]]
keys = "cmd+shift+t>n"
action = "tab.create"
"#;
    let table = load_keybinding_table(Some(chord_only));
    let projection = project_shortcuts(&table, route);
    let new_tab = item_for(WorkspaceCommandId::TabCreate, &projection);
    assert!(
        new_tab.key_equivalent.is_none(),
        "chords never become key equivalents"
    );
    assert_eq!(new_tab.hints.len(), 1);
    assert!(new_tab.hints[0].is_chord);
    assert_eq!(new_tab.hints[0].keys_notation, "cmd+shift+t>n");
}

/// SPEC-024 §14 item 19 (menu half): projection enabled bits for palette modal.
#[test]
fn item19_menu_half_projection_disables_non_palette_while_open() {
    let table = load_keybinding_table(None);
    let closed = route_context_set(false, PresentationMode::Flow, false);
    let open = route_context_set(true, PresentationMode::Flow, false);

    let when_closed = project_shortcuts(&table, closed);
    let when_open = project_shortcuts(&table, open);

    let new_tab_closed = item_for(WorkspaceCommandId::TabCreate, &when_closed);
    let new_tab_open = item_for(WorkspaceCommandId::TabCreate, &when_open);
    assert!(
        new_tab_closed.enabled,
        "New Tab enabled when palette closed"
    );
    assert!(
        !new_tab_open.enabled,
        "New Tab disabled while palette open (R6.4.2)"
    );

    let palette_closed = item_for(WorkspaceCommandId::CommandPaletteOpen, &when_closed);
    let palette_open = item_for(WorkspaceCommandId::CommandPaletteOpen, &when_open);
    assert!(palette_closed.enabled);
    // command_palette.open is app-context only; not permitted under {palette}.
    assert!(
        !palette_open.enabled,
        "non-palette-context open command disabled while palette owns the route"
    );

    // No palette-context menu-visible item in M003 K5; Close remains Escape-only.
    assert!(
        when_open.items.iter().all(|item| !item.enabled),
        "every projected menu item is non-palette-context and disabled while open"
    );
}

#[test]
fn accessibility_label_carries_no_terminal_text() {
    let terminal_fixture = "echo seyal-ax-terminal-fixture-9f3a";
    let toml = r#"
[[keybindings]]
keys = "cmd+shift+p"
action = "command_palette.open"
"#
    .to_string();
    let table = load_keybinding_table(Some(&toml));
    // Inject fixture into a notation only if mis-wired; projection must never
    // pull ApplicationRoot output / terminal buffers into labels.
    let route = BindingContext::APP.union(BindingContext::FLOW);
    let projection = project_shortcuts(&table, route);
    for item in &projection.items {
        assert!(
            !item.accessibility_label.contains(terminal_fixture),
            "AX label leaked terminal fixture: {}",
            item.accessibility_label
        );
        assert!(
            !item.accessibility_label.contains('\n'),
            "AX label must stay a short product string"
        );
        for hint in &item.hints {
            assert!(!hint.keys_notation.contains(terminal_fixture));
        }
    }
    let palette = item_for(WorkspaceCommandId::CommandPaletteOpen, &projection);
    assert!(
        palette.accessibility_label.starts_with("Command Palette"),
        "{}",
        palette.accessibility_label
    );
}

/// `goto.open` is a normal menu-visible workspace command (K8 binding, K5 projection).
#[test]
fn goto_open_projects_with_stable_menu_id() {
    let table = load_keybinding_table(None);
    let route = route_context_set(false, PresentationMode::Flow, false);
    let projection = project_shortcuts(&table, route);
    let goto = item_for(WorkspaceCommandId::GotoOpen, &projection);
    assert_eq!(goto.key_equivalent_notation.as_deref(), Some("cmd+shift+o"));
    assert!(goto.enabled, "goto.open permitted on the app route");
    assert!(goto.accessibility_label.starts_with("Go to…"));
    assert_eq!(workspace_command_ffi_id(WorkspaceCommandId::GotoOpen), 21);
    let round_trip = workspace_command_from_ffi_id(21, 0).expect("menu id 21");
    assert_eq!(round_trip.id, WorkspaceCommandId::GotoOpen);
    assert!(round_trip.ordinal.is_none());
}

/// Focus-history remains key-only in M003 (not a menu-visible K5 row).
#[test]
fn focus_history_omitted_from_menu_projection() {
    let table = load_keybinding_table(None);
    let route = route_context_set(false, PresentationMode::Flow, false);
    let projection = project_shortcuts(&table, route);
    assert!(projected_item_for(
        &projection,
        WorkspaceCommand {
            id: WorkspaceCommandId::FocusHistoryBack,
            ordinal: None,
        }
    )
    .is_none());
    assert!(projected_item_for(
        &projection,
        WorkspaceCommand {
            id: WorkspaceCommandId::FocusHistoryForward,
            ordinal: None,
        }
    )
    .is_none());
}

/// §7.3 / R11.1 / R11.3: unbinding a projected command keeps title + route enablement.
#[test]
fn unbind_projected_command_keeps_title_and_route_enablement() {
    let toml = r#"
[[keybindings]]
keys = "cmd+t"
action = "none"
"#;
    let table = load_keybinding_table(Some(toml));
    let route = route_context_set(false, PresentationMode::Flow, false);
    let projection = project_shortcuts(&table, route);
    let new_tab = item_for(WorkspaceCommandId::TabCreate, &projection);
    assert!(
        new_tab.key_equivalent.is_none(),
        "unbind removes the menu key equivalent"
    );
    assert!(
        new_tab.hints.is_empty(),
        "no surviving bindings means no hints"
    );
    assert!(
        new_tab.enabled,
        "enabled follows the app route, not binding presence"
    );
    assert_eq!(
        new_tab.accessibility_label, "New Tab",
        "title remains present after unbind"
    );

    let palette_open = route_context_set(true, PresentationMode::Flow, false);
    let when_open = project_shortcuts(&table, palette_open);
    let new_tab_open = item_for(WorkspaceCommandId::TabCreate, &when_open);
    assert!(
        !new_tab_open.enabled,
        "palette modal still disables non-palette menu commands"
    );
}

/// R6.2.1 specificity: Raw-only rebind of cmd+t wins over builtin app tab.create.
#[test]
fn item_specificity_raw_rebind_of_projected_cmd_t() {
    use super::chord::ChordPrefixState;
    use super::route::{route_keystroke, RouteOutcome};
    use super::stroke::NormalizedStroke;
    use super::types::{KeySym, Modifiers};
    use std::time::Instant;

    let toml = r#"
[[keybindings]]
keys = "cmd+t"
action = "pane.split_down"
context = ["raw"]
"#;
    let table = load_keybinding_table(Some(toml));
    let event = NormalizedStroke {
        modifiers: Modifiers::CMD,
        key: KeySym::Char('t'),
        shift_applied: None,
    };
    let mut chord = ChordPrefixState::new();
    let raw = route_context_set(false, PresentationMode::Raw, false);
    let matched = route_keystroke(&table, &event, raw, false, &mut chord, Instant::now());
    assert!(
        matches!(
            matched,
            RouteOutcome::Matched {
                command: WorkspaceCommand {
                    id: WorkspaceCommandId::PaneSplitDown,
                    ..
                }
            }
        ),
        "Raw specificity must beat app tab.create before any menu path: {matched:?}"
    );

    // Flow still sees the app New Tab binding (Matched), so the menu path never
    // runs — but the projected ⌘T equivalent remains the steal surface when the
    // route drops `app` (palette). That case is covered below.
    let flow = route_context_set(false, PresentationMode::Flow, false);
    let mut chord = ChordPrefixState::new();
    let flow_match = route_keystroke(&table, &event, flow, false, &mut chord, Instant::now());
    assert!(
        matches!(
            flow_match,
            RouteOutcome::Matched {
                command: WorkspaceCommand {
                    id: WorkspaceCommandId::TabCreate,
                    ..
                }
            }
        ),
        "Flow keeps app tab.create; menu must not be the authority: {flow_match:?}"
    );
}

/// R6.2.1 / §6.2 step 2c: consume unmatched Command only when a projected
/// menu equivalent would steal across an inactive context.
#[test]
fn unmatched_command_menu_steal_only_for_inactive_projected_equivalent() {
    use super::projection::projected_menu_steals_unmatched_command;
    use super::stroke::normalized_from_notation;

    let table = load_keybinding_table(None);
    let cmd_t = normalized_from_notation("cmd+t").expect("cmd+t");
    let cmd_left = normalized_from_notation("cmd+left").expect("cmd+left");
    let cmd_u = normalized_from_notation("cmd+u").expect("cmd+u");

    let palette = route_context_set(true, PresentationMode::Flow, false);
    assert!(
        projected_menu_steals_unmatched_command(&table, &cmd_t, palette),
        "palette-open cmd+t equals inactive New Tab projected equivalent"
    );
    assert!(
        !projected_menu_steals_unmatched_command(&table, &cmd_left, palette),
        "Cmd+Left is not a projected menu equivalent"
    );
    assert!(
        !projected_menu_steals_unmatched_command(&table, &cmd_u, palette),
        "ordinary unbound cmd+u must stay native"
    );

    let flow = route_context_set(false, PresentationMode::Flow, true);
    assert!(
        !projected_menu_steals_unmatched_command(&table, &cmd_t, flow),
        "Flow permits New Tab — route would Match, not steal-via-unmatched"
    );
    assert!(
        !projected_menu_steals_unmatched_command(&table, &cmd_left, flow),
        "composer Cmd+Left must remain native text editing"
    );
}
