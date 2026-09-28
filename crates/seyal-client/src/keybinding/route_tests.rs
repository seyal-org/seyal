//! SPEC-024 K3 §14 routing / Raw-TUI non-interception tests.

use crate::presentation::PresentationMode;

use super::builtins::builtin_rows;
use super::load::load_keybinding_table;
use super::route::{
    fallthrough_is_flow, fallthrough_is_terminal, resolve_tab_ordinal, route_context_set,
    route_keystroke, validate_workspace_command, InvokeError, RouteOutcome,
};
use super::stroke::{normalized_from_notation, NormalizedStroke};
use super::types::{
    BindingContext, BindingSequence, BindingSource, CompiledBinding, DiagnosticCategory, KeyStroke,
    KeySym, KeybindingTable, Modifiers, Ordinal1To9, WorkspaceCommand, WorkspaceCommandId,
};

fn route(
    table: &KeybindingTable,
    keys: &str,
    ctx: BindingContext,
    composition: bool,
) -> RouteOutcome {
    let stroke = normalized_from_notation(keys).expect(keys);
    route_keystroke(table, &stroke, ctx, composition)
}

fn raw_ctx() -> BindingContext {
    route_context_set(false, PresentationMode::Raw, false)
}

fn tui_ctx() -> BindingContext {
    route_context_set(false, PresentationMode::Tui, false)
}

fn flow_composer_ctx() -> BindingContext {
    route_context_set(false, PresentationMode::Flow, true)
}

fn palette_ctx() -> BindingContext {
    route_context_set(true, PresentationMode::Flow, false)
}

// --- §14 item 4: Command non-leak ---

#[test]
fn item4_matched_and_unmatched_command_write_zero_pty_bytes() {
    let table = load_keybinding_table(None);
    let matched = route(&table, "cmd+k", raw_ctx(), false);
    assert!(matches!(
        matched,
        RouteOutcome::Matched {
            command: WorkspaceCommand {
                id: WorkspaceCommandId::CommandPaletteOpen,
                ..
            }
        }
    ));
    assert!(!matched.writes_pty_bytes());

    let unmatched = route(&table, "cmd+u", raw_ctx(), false);
    assert_eq!(unmatched, RouteOutcome::UnmatchedCommand);
    assert!(!unmatched.writes_pty_bytes());

    let reserved = route(&table, "cmd+q", tui_ctx(), false);
    assert_eq!(reserved, RouteOutcome::ReservedCommand);
    assert!(!reserved.writes_pty_bytes());
}

// --- §14 item 5: passthrough protection (must-fail-before-fix regression) ---

#[test]
fn item5_ctrl_c_with_app_is_terminal_passthrough_protected() {
    let default_app = r#"
[[keybindings]]
keys = "ctrl+c"
action = "tab.create"
"#;
    let table = load_keybinding_table(Some(default_app));
    assert!(table
        .diagnostics
        .iter()
        .any(|d| d.category == DiagnosticCategory::TerminalPassthroughProtected));
    assert!(table
        .bindings
        .iter()
        .all(|b| b.action.id != WorkspaceCommandId::TabCreate
            || !b.sequence.strokes()[0].modifiers.contains(Modifiers::CTRL)
            || b.sequence.strokes()[0].key != KeySym::Char('c')));

    let with_raw = r#"
[[keybindings]]
keys = "ctrl+c"
action = "tab.create"
context = ["app", "raw"]
"#;
    let table = load_keybinding_table(Some(with_raw));
    assert!(table
        .diagnostics
        .iter()
        .any(|d| d.category == DiagnosticCategory::TerminalPassthroughProtected));
}

#[test]
fn item5_regression_rogue_app_ctrl_c_would_intercept_without_load_gate() {
    // Documents the old interception: if an app-context ctrl+c binding entered
    // the table, Raw would consume Control-C as ApplicationCommand (zero PTY).
    let rogue = KeybindingTable {
        bindings: vec![CompiledBinding {
            sequence: BindingSequence::try_from_strokes(vec![KeyStroke {
                modifiers: Modifiers::CTRL,
                key: KeySym::Char('c'),
            }])
            .unwrap(),
            keys_notation: "ctrl+c".to_owned(),
            action: WorkspaceCommand {
                id: WorkspaceCommandId::TabCreate,
                ordinal: None,
            },
            context: BindingContext::APP,
            source: BindingSource::User { index: 0 },
        }],
        chord_prefix_index: Vec::new(),
        diagnostics: Vec::new(),
    };
    let intercepted = route(&rogue, "ctrl+c", raw_ctx(), false);
    assert!(
        matches!(intercepted, RouteOutcome::Matched { .. }),
        "pre-fix interception: app ctrl+c would match in Raw"
    );
    assert!(!intercepted.writes_pty_bytes());

    // After the load gate, defaults leave Control-C on the terminal fallthrough.
    let defaults = load_keybinding_table(None);
    let outcome = route(&defaults, "ctrl+c", raw_ctx(), false);
    assert_eq!(outcome, RouteOutcome::Fallthrough);
    assert!(outcome.writes_pty_bytes());
    assert!(fallthrough_is_terminal(raw_ctx()));
}

// --- §14 item 6: opt-in intercept ---

#[test]
fn item6_opt_in_raw_consumes_without_pty_flow_and_palette_load_clean() {
    let toml = r#"
[[keybindings]]
keys = "ctrl+c"
action = "tab.create"
context = ["raw"]
"#;
    let table = load_keybinding_table(Some(toml));
    assert!(!table
        .diagnostics
        .iter()
        .any(|d| d.category == DiagnosticCategory::TerminalPassthroughProtected));
    let matched = route(&table, "ctrl+c", raw_ctx(), false);
    assert!(matches!(matched, RouteOutcome::Matched { .. }));
    assert!(!matched.writes_pty_bytes());

    // Same stroke in Flow does not match raw-only binding.
    assert_eq!(
        route(&table, "ctrl+c", flow_composer_ctx(), false),
        RouteOutcome::Fallthrough
    );

    let flow_only = r#"
[[keybindings]]
keys = "ctrl+x"
action = "tab.create"
context = ["flow"]
"#;
    let flow_table = load_keybinding_table(Some(flow_only));
    assert!(
        flow_table.diagnostics.is_empty()
            || flow_table
                .diagnostics
                .iter()
                .all(|d| d.category != DiagnosticCategory::TerminalPassthroughProtected)
    );

    let palette_only = r#"
[[keybindings]]
keys = "ctrl+y"
action = "command_palette.close"
context = ["palette"]
"#;
    let palette_table = load_keybinding_table(Some(palette_only));
    assert!(palette_table
        .diagnostics
        .iter()
        .all(|d| d.category != DiagnosticCategory::TerminalPassthroughProtected));
}

// --- §14 item 7: IME ---

#[test]
fn item7_composition_skips_non_command_but_command_still_matches() {
    let table = load_keybinding_table(None);
    assert_eq!(
        route(&table, "escape", palette_ctx(), true),
        RouteOutcome::CompositionConsumes
    );
    assert_eq!(
        route(&table, "ctrl+r", flow_composer_ctx(), true),
        RouteOutcome::CompositionConsumes
    );
    let cmd = route(&table, "cmd+k", flow_composer_ctx(), true);
    assert!(matches!(
        cmd,
        RouteOutcome::Matched {
            command: WorkspaceCommand {
                id: WorkspaceCommandId::CommandPaletteOpen,
                ..
            }
        }
    ));
}

// --- §14 item 11: stale/unavailable ---

#[test]
fn item11_stale_or_unavailable_invoke_is_action_unavailable() {
    let table = load_keybinding_table(None);
    let create = WorkspaceCommand {
        id: WorkspaceCommandId::TabCreate,
        ordinal: None,
    };
    assert_eq!(
        validate_workspace_command(&table, create, palette_ctx()),
        Err(InvokeError::ActionUnavailable)
    );
    let ordinal = WorkspaceCommand {
        id: WorkspaceCommandId::TabSelectOrdinal,
        ordinal: Ordinal1To9::new(3),
    };
    assert_eq!(
        resolve_tab_ordinal(ordinal, 2),
        Err(InvokeError::ActionUnavailable)
    );
    assert_eq!(resolve_tab_ordinal(ordinal, 3), Ok(3));
}

// --- §14 item 13: Flow unmatched does not hit terminal ---

#[test]
fn item13_flow_unmatched_is_not_terminal_fallthrough() {
    let table = load_keybinding_table(None);
    let flow = route_context_set(false, PresentationMode::Flow, false);
    let outcome = route(&table, "a", flow, false);
    assert_eq!(outcome, RouteOutcome::Fallthrough);
    assert!(fallthrough_is_flow(flow));
    assert!(!fallthrough_is_terminal(flow));
}

// --- §14 item 16: punctuation matching ---

#[test]
fn item16_cmd_shift_bracket_matches_both_notations_via_layout_scalars() {
    let table = load_keybinding_table(None);
    // US layout: unshifted base ']', Shift-applied '}'.
    let event = NormalizedStroke {
        modifiers: Modifiers::CMD.union(Modifiers::SHIFT),
        key: KeySym::Char(']'),
        shift_applied: Some('}'),
    };
    let route = raw_ctx();
    let from_bracket = route_keystroke(&table, &event, route, false);
    assert!(matches!(
        from_bracket,
        RouteOutcome::Matched {
            command: WorkspaceCommand {
                id: WorkspaceCommandId::TabSelectNext,
                ..
            }
        }
    ));

    // Binding written as cmd+shift+} must match the same physical event.
    let user = r#"
[[keybindings]]
keys = "cmd+shift+}"
action = "tab.create"
"#;
    let table = load_keybinding_table(Some(user));
    let matched = route_keystroke(&table, &event, route, false);
    assert!(matches!(
        matched,
        RouteOutcome::Matched {
            command: WorkspaceCommand {
                id: WorkspaceCommandId::TabCreate,
                ..
            }
        }
    ));

    // Synthetic non-US layout: base is 'ü' — US keycode tables must not invent a match.
    let non_us = NormalizedStroke {
        modifiers: Modifiers::CMD.union(Modifiers::SHIFT),
        key: KeySym::Char('ü'),
        shift_applied: Some('Ü'),
    };
    assert_eq!(
        route_keystroke(&table, &non_us, route, false),
        RouteOutcome::UnmatchedCommand
    );
}

// --- §14 item 17: ordinal ---

#[test]
fn item17_builtin_and_user_ordinal_three_select_tab_three() {
    let table = load_keybinding_table(None);
    let builtin = route(&table, "cmd+3", raw_ctx(), false);
    let command = builtin.matched_command().expect("cmd+3");
    assert_eq!(command.id, WorkspaceCommandId::TabSelectOrdinal);
    assert_eq!(command.ordinal, Ordinal1To9::new(3));
    assert_eq!(resolve_tab_ordinal(command, 5), Ok(3));
    assert_eq!(
        resolve_tab_ordinal(command, 2),
        Err(InvokeError::ActionUnavailable)
    );

    let user = r#"
[[keybindings]]
keys = "cmd+opt+3"
action = "tab.select_ordinal"
ordinal = 3
"#;
    let table = load_keybinding_table(Some(user));
    let user_match = route(&table, "cmd+opt+3", raw_ctx(), false);
    let command = user_match.matched_command().expect("user ordinal");
    assert_eq!(command.ordinal, Ordinal1To9::new(3));
    assert_eq!(resolve_tab_ordinal(command, 3), Ok(3));
}

// --- §14 item 19 keybinding half: palette modal ---

#[test]
fn item19_palette_open_rejects_cmd_t_match_and_menu_invoke() {
    let table = load_keybinding_table(None);
    let palette = palette_ctx();
    assert_eq!(palette, BindingContext::PALETTE);

    // Keybinding match: app-context cmd+t does not intersect {palette}.
    assert_eq!(
        route(&table, "cmd+t", palette, false),
        RouteOutcome::UnmatchedCommand
    );

    let create = WorkspaceCommand {
        id: WorkspaceCommandId::TabCreate,
        ordinal: None,
    };
    assert_eq!(
        validate_workspace_command(&table, create, palette),
        Err(InvokeError::ActionUnavailable)
    );

    // Escape still closes the palette.
    assert!(matches!(
        route(&table, "escape", palette, false),
        RouteOutcome::Matched {
            command: WorkspaceCommand {
                id: WorkspaceCommandId::CommandPaletteClose,
                ..
            }
        }
    ));
}

// --- §14 item 20: composer history-search ---

#[test]
fn item20_ctrl_r_composer_opens_history_raw_falls_through_unbind_removes() {
    let table = load_keybinding_table(None);
    let open = route(&table, "ctrl+r", flow_composer_ctx(), false);
    assert!(matches!(
        open,
        RouteOutcome::Matched {
            command: WorkspaceCommand {
                id: WorkspaceCommandId::ComposerHistorySearchOpen,
                ..
            }
        }
    ));
    assert!(!open.writes_pty_bytes());

    assert_eq!(
        route(&table, "ctrl+r", raw_ctx(), false),
        RouteOutcome::Fallthrough
    );
    assert_eq!(
        route(&table, "ctrl+r", tui_ctx(), false),
        RouteOutcome::Fallthrough
    );
    assert!(fallthrough_is_terminal(raw_ctx()));

    let unbind = r#"
[[keybindings]]
keys = "ctrl+r"
action = "none"
context = ["composer"]
"#;
    let table = load_keybinding_table(Some(unbind));
    assert_eq!(
        route(&table, "ctrl+r", flow_composer_ctx(), false),
        RouteOutcome::Fallthrough
    );
}

#[test]
fn builtins_still_load_clean_under_passthrough_gate() {
    let table = load_keybinding_table(None);
    assert!(table.diagnostics.is_empty());
    assert_eq!(table.bindings.len(), builtin_rows().len());
}

#[test]
fn arrow_and_control_c_raw_fallthrough_unchanged() {
    let table = load_keybinding_table(None);
    for keys in ["ctrl+c", "up", "down", "left", "right"] {
        let outcome = route(&table, keys, raw_ctx(), false);
        assert_eq!(outcome, RouteOutcome::Fallthrough, "{keys}");
        assert!(outcome.writes_pty_bytes(), "{keys}");
    }
}
