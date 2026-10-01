//! SPEC-024 §14 items 1, 2–3, 8, 14, 15, and 22 (K1 + K2).

use std::path::Path;

use crate::input_policy::load_input_policy;
use crate::theme::ui_config_path_from;

use super::builtins::builtin_rows;
use super::keys::parse_keys;
use super::load::load_keybinding_table;
use super::reserved::RESERVED_KEY_NOTATIONS;
use super::types::{
    BindingContext, BindingSource, CompiledBinding, DiagnosticCategory, KeySym, KeybindingTable,
    Modifiers, NamedKey, Ordinal1To9, WorkspaceCommandId,
};

fn binding_for<'a>(
    table: &'a KeybindingTable,
    keys: &str,
    id: WorkspaceCommandId,
) -> Option<&'a CompiledBinding> {
    let sequence = parse_keys(keys).expect("test keys");
    table
        .bindings
        .iter()
        .find(|b| b.sequence == sequence && b.action.id == id)
}

fn contexts_for(table: &KeybindingTable, keys: &str) -> Vec<(WorkspaceCommandId, BindingContext)> {
    let sequence = parse_keys(keys).expect("test keys");
    table
        .bindings
        .iter()
        .filter(|b| b.sequence == sequence)
        .map(|b| (b.action.id, b.context))
        .collect()
}

fn diag_categories(table: &KeybindingTable) -> Vec<DiagnosticCategory> {
    table.diagnostics.iter().map(|d| d.category).collect()
}

#[test]
fn schema_valid_strokes_chords_and_punctuation() {
    let punct = [
        ']', '[', ',', '`', '%', '\\', '"', '!', '#', '$', '&', '\'', '(', ')', '*', '-', '.', '/',
        ':', ';', '<', '=', '?', '@', '^', '_', '{', '|', '}', '~',
    ];
    for ch in punct {
        let keys = if ch == '\\' || ch == '"' {
            continue;
        } else {
            format!("cmd+{ch}")
        };
        assert!(
            parse_keys(&keys).is_ok(),
            "expected punct key {ch:?} to parse"
        );
    }
    assert!(parse_keys("cmd+plus").is_ok());
    assert!(parse_keys("cmd+greater").is_ok());
    assert!(parse_keys("ctrl+b>n").is_ok());
    assert!(parse_keys("ctrl+b>shift+%").is_ok());
    assert!(parse_keys("cmd+shift+]").is_ok());

    let toml = r#"
[[keybindings]]
keys = "cmd+k"
action = "command_palette.open"

[[keybindings]]
keys = "ctrl+b>n"
action = "tab.create"
context = ["flow", "raw", "tui"]

[[keybindings]]
keys = "cmd+opt+1"
action = "tab.select_ordinal"
ordinal = 1

[[keybindings]]
keys = "cmd+,"
action = "tab.create"

[[keybindings]]
keys = "cmd+]"
action = "tab.select_next"

[[keybindings]]
keys = "cmd+["
action = "tab.select_previous"

[[keybindings]]
keys = "cmd+ctrl+f"
action = "command_palette.open"

[[keybindings]]
keys = "shift+%"
action = "tab.create"
context = ["flow"]

[[keybindings]]
keys = "ctrl+\\"
action = "tab.create"
context = ["composer"]

[[keybindings]]
keys = "ctrl+\""
action = "tab.create"
context = ["palette"]

[[keybindings]]
keys = "cmd+plus"
action = "pane.split_right"

[[keybindings]]
keys = "cmd+greater"
action = "pane.split_down"
"#;
    let table = load_keybinding_table(Some(toml));
    // User cmd+k replaces builtin → DuplicateSequence; cmd+ctrl+f is reserved.
    assert!(
        diag_categories(&table).contains(&DiagnosticCategory::DuplicateSequence),
        "expected DuplicateSequence for cmd+k override: {:?}",
        table.diagnostics
    );
    assert!(
        diag_categories(&table).contains(&DiagnosticCategory::ReservedCommandCollision),
        "expected ReservedCommandCollision for cmd+ctrl+f: {:?}",
        table.diagnostics
    );
    assert!(binding_for(&table, "ctrl+b>n", WorkspaceCommandId::TabCreate).is_some());
    assert_eq!(
        binding_for(&table, "cmd+opt+1", WorkspaceCommandId::TabSelectOrdinal)
            .unwrap()
            .action
            .ordinal,
        Some(Ordinal1To9::new(1).unwrap())
    );
    let chord = binding_for(&table, "ctrl+b>n", WorkspaceCommandId::TabCreate).unwrap();
    assert!(chord.context.contains(BindingContext::FLOW));
    assert!(chord.context.contains(BindingContext::RAW));
    assert!(chord.context.contains(BindingContext::TUI));
    assert!(!table.chord_prefix_index.is_empty());
}

#[test]
fn schema_rejects_invalid_keys_actions_ordinals_and_payloads() {
    assert!(parse_keys("+").is_err());
    assert!(parse_keys(">").is_err());
    assert!(parse_keys("cmd++").is_err());
    assert!(parse_keys("unknownkey").is_err());
    assert!(parse_keys("cmd+cmd+k").is_err());
    assert!(parse_keys("a>b>c>d>e").is_err());
    assert!(parse_keys("").is_err());
    assert!(parse_keys("super+k").is_err());

    let toml = r#"
[[keybindings]]
keys = "cmd++"
action = "tab.create"

[[keybindings]]
keys = "a>b>c>d>e"
action = "tab.create"

[[keybindings]]
keys = "cmd+k"
action = "settings.open"

[[keybindings]]
keys = "cmd+k"
action = "tab.select_ordinal.3"

[[keybindings]]
keys = "cmd+k"
action = "shell:rm -rf /"

[[keybindings]]
keys = "cmd+k"
action = "https://evil.example/x"

[[keybindings]]
keys = "cmd+1"
action = "tab.select_ordinal"

[[keybindings]]
keys = "cmd+1"
action = "tab.select_ordinal"
ordinal = 0

[[keybindings]]
keys = "cmd+1"
action = "tab.select_ordinal"
ordinal = 10

[[keybindings]]
keys = "cmd+1"
action = "tab.select_ordinal"
ordinal = "3"

[[keybindings]]
keys = "cmd+t"
action = "tab.create"
ordinal = 1

[[keybindings]]
keys = "cmd+d"
action = "none"
ordinal = 1

[[keybindings]]
keys = "cmd+w"
action = "window.new"

[[keybindings]]
keys = "cmd+opt+left"
action = "pane.focus_left"

[[keybindings]]
keys = "cmd+["
action = "focus_history.back"
"#;
    let table = load_keybinding_table(Some(toml));
    assert!(
        table
            .bindings
            .iter()
            .all(|b| b.source == BindingSource::Builtin),
        "invalid user rows must not yield user bindings: {:?}",
        table.bindings
    );
    let categories = diag_categories(&table);
    assert!(categories.contains(&DiagnosticCategory::InvalidKeys));
    assert!(categories.contains(&DiagnosticCategory::ChordTooLong));
    assert!(categories.contains(&DiagnosticCategory::UnknownAction));
    assert!(categories.contains(&DiagnosticCategory::DisallowedActionPayload));
    assert!(categories.contains(&DiagnosticCategory::InvalidActionArgument));
}

#[test]
fn invalid_keybindings_shape_fails_closed_to_builtins() {
    let table = load_keybinding_table(Some("keybindings = \"nope\""));
    assert!(table
        .bindings
        .iter()
        .all(|b| b.source == BindingSource::Builtin));
    assert_eq!(table.bindings.len(), builtin_rows().len());
    assert!(diag_categories(&table).contains(&DiagnosticCategory::TableIgnored));

    let table = load_keybinding_table(Some("this is not = toml ["));
    assert_eq!(table.bindings.len(), builtin_rows().len());
    assert!(table.diagnostics.is_empty());
    assert_ne!(table, KeybindingTable::empty());
}

#[test]
fn option_as_alt_unchanged_by_keybinding_load() {
    let toml = r#"
[input]
option_as_alt = true

[[keybindings]]
keys = "cmd+shift+k"
action = "command_palette.open"
"#;
    let (policy_before, _) = load_input_policy(Some(toml));
    assert!(policy_before.option_as_alt);

    let table = load_keybinding_table(Some(toml));
    assert!(binding_for(
        &table,
        "cmd+shift+k",
        WorkspaceCommandId::CommandPaletteOpen
    )
    .is_some());

    let (policy_after, warnings) = load_input_policy(Some(toml));
    assert!(policy_after.option_as_alt);
    assert_eq!(policy_before, policy_after);
    assert!(warnings.is_empty());

    let toml_false = r#"
[input]
option_as_alt = false

[[keybindings]]
keys = "cmd+shift+t"
action = "tab.create"
"#;
    let (policy, _) = load_input_policy(Some(toml_false));
    let _table = load_keybinding_table(Some(toml_false));
    let (policy_again, _) = load_input_policy(Some(toml_false));
    assert!(!policy.option_as_alt);
    assert_eq!(policy, policy_again);
}

#[test]
fn diagnostics_contain_no_secret_or_terminal_fixtures() {
    let marked_text = "SECRET_MARKED_IME_COMPOSITION_xyz";
    let terminal_fixture = "PASSWORD=hunter2 root@host prompt";
    let toml = format!(
        r#"
# fixture noise: {marked_text}
# terminal: {terminal_fixture}
[input]
option_as_alt = true

[[keybindings]]
keys = "cmd++"
action = "tab.create"
extra = 1
"#
    );
    let table = load_keybinding_table(Some(&toml));
    assert!(!table.diagnostics.is_empty());
    for diagnostic in &table.diagnostics {
        let blob = format!(
            "{} {} {} {}",
            diagnostic.message, diagnostic.keys_notation, diagnostic.action, diagnostic.category
        );
        assert!(
            !blob.contains(marked_text),
            "diagnostic leaked marked text: {blob}"
        );
        assert!(
            !blob.contains(terminal_fixture),
            "diagnostic leaked terminal fixture: {blob}"
        );
        assert!(!blob.contains("hunter2"), "diagnostic leaked password");
        assert!(
            !blob.contains("PASSWORD="),
            "diagnostic leaked env secret fixture"
        );
    }
}

#[test]
fn shared_config_path_matches_input_policy() {
    assert_eq!(
        super::keybinding_config_path(),
        crate::input_policy::input_policy_config_path()
    );
}

#[test]
fn compiled_stroke_modifiers_and_named_keys() {
    let sequence = parse_keys("cmd+shift+enter").unwrap();
    assert_eq!(sequence.strokes().len(), 1);
    let stroke = sequence.strokes()[0];
    assert!(stroke.modifiers.contains(Modifiers::CMD));
    assert!(stroke.modifiers.contains(Modifiers::SHIFT));
    assert_eq!(stroke.key, KeySym::Named(NamedKey::Enter));
}

// --- SPEC-024 §14 item 2: defaults ---

#[test]
fn defaults_every_k2_builtin_row_validates_cleanly() {
    let table = load_keybinding_table(None);
    assert!(
        table.diagnostics.is_empty(),
        "builtins must load with zero diagnostics: {:?}",
        table.diagnostics
    );
    assert_eq!(table.bindings.len(), builtin_rows().len());
    for row in builtin_rows() {
        let binding = binding_for(&table, row.keys_notation, row.id)
            .unwrap_or_else(|| panic!("missing builtin {}", row.keys_notation));
        assert_eq!(binding.source, BindingSource::Builtin);
        assert_eq!(binding.context, row.context);
        if let Some(n) = row.ordinal {
            assert_eq!(binding.action.ordinal, Ordinal1To9::new(n));
        } else {
            assert_eq!(binding.action.ordinal, None);
        }
        assert!(
            !diag_categories(&table).contains(&DiagnosticCategory::TerminalPassthroughProtected)
        );
        assert!(!diag_categories(&table).contains(&DiagnosticCategory::InvalidKeys));
        assert!(!diag_categories(&table).contains(&DiagnosticCategory::ReservedCommandCollision));
    }

    // §5.4 composer history-search row
    let history = binding_for(
        &table,
        "ctrl+r",
        WorkspaceCommandId::ComposerHistorySearchOpen,
    )
    .expect("ctrl+r composer history-search");
    assert_eq!(history.context, BindingContext::COMPOSER);

    // Exclusions: ADR-021 pane focus/zoom, SPEC-022 navigation, ADR-018 window
    assert!(contexts_for(&table, "cmd+opt+left").is_empty());
    assert!(contexts_for(&table, "cmd+shift+enter").is_empty());
    assert!(contexts_for(&table, "cmd+[").is_empty());
    assert!(contexts_for(&table, "cmd+]").is_empty());
    assert!(contexts_for(&table, "cmd+shift+o").is_empty());
    assert!(contexts_for(&table, "cmd+n").is_empty());
    assert!(contexts_for(&table, "cmd+,").is_empty());
    // Until K9, cmd+w has no builtin (accepted §4.1 / §5.0; not tab.close_focused).
    assert!(contexts_for(&table, "cmd+w").is_empty());
    assert!(binding_for(&table, "cmd+w", WorkspaceCommandId::TabCloseFocused).is_none());
}

#[test]
fn defaults_user_override_cmd_k_wins_with_duplicate_sequence() {
    let toml = r#"
[[keybindings]]
keys = "cmd+k"
action = "tab.create"
"#;
    let table = load_keybinding_table(Some(toml));
    assert!(binding_for(&table, "cmd+k", WorkspaceCommandId::CommandPaletteOpen).is_none());
    let winner = binding_for(&table, "cmd+k", WorkspaceCommandId::TabCreate).expect("user wins");
    assert_eq!(winner.source, BindingSource::User { index: 0 });
    assert_eq!(winner.context, BindingContext::APP);
    assert_eq!(
        table
            .diagnostics
            .iter()
            .filter(|d| d.category == DiagnosticCategory::DuplicateSequence)
            .count(),
        1
    );
}

#[test]
fn defaults_settings_open_unknown_and_cmd_comma_unbound() {
    let toml = r#"
[[keybindings]]
keys = "cmd+,"
action = "settings.open"
"#;
    let table = load_keybinding_table(Some(toml));
    assert!(contexts_for(&table, "cmd+,").is_empty());
    assert!(diag_categories(&table).contains(&DiagnosticCategory::UnknownAction));
}

// --- SPEC-024 §14 item 3: reserved ---

#[test]
fn reserved_every_section_4_2_stroke_is_rejected() {
    let mut rows = String::new();
    for keys in RESERVED_KEY_NOTATIONS {
        rows.push_str(&format!(
            "[[keybindings]]\nkeys = \"{keys}\"\naction = \"tab.create\"\n\n"
        ));
    }
    let table = load_keybinding_table(Some(&rows));
    let reserved_diags: Vec<_> = table
        .diagnostics
        .iter()
        .filter(|d| d.category == DiagnosticCategory::ReservedCommandCollision)
        .collect();
    assert_eq!(
        reserved_diags.len(),
        RESERVED_KEY_NOTATIONS.len(),
        "expected one ReservedCommandCollision per reserved stroke; got {:?}",
        table.diagnostics
    );
    for keys in RESERVED_KEY_NOTATIONS {
        assert!(
            reserved_diags.iter().any(|d| d.keys_notation == *keys),
            "missing ReservedCommandCollision for {keys}"
        );
        let sequence = parse_keys(keys).unwrap();
        assert!(
            table
                .bindings
                .iter()
                .filter(|b| b.sequence == sequence)
                .all(|b| matches!(b.source, BindingSource::Builtin)),
            "user must not own reserved {keys}"
        );
    }
    // Edit-menu equivalents stay reserved (not a rebindable catalog).
    for keys in ["cmd+x", "cmd+c", "cmd+v", "cmd+a", "cmd+z", "cmd+shift+z"] {
        assert!(
            RESERVED_KEY_NOTATIONS.contains(&keys),
            "edit-menu {keys} must remain in the reserved set"
        );
    }
}

#[test]
fn reserved_unbind_also_rejected() {
    let toml = r#"
[[keybindings]]
keys = "cmd+q"
action = "none"
"#;
    let table = load_keybinding_table(Some(toml));
    assert!(diag_categories(&table).contains(&DiagnosticCategory::ReservedCommandCollision));
    assert!(!diag_categories(&table).contains(&DiagnosticCategory::UnbindNoEffect));
}

#[test]
fn reserved_any_stroke_in_chord_is_rejected() {
    // §14 item 3 / accepted §7.1: reserved Command stroke anywhere in a chord.
    let toml = r#"
[[keybindings]]
keys = "cmd+q>x"
action = "tab.create"

[[keybindings]]
keys = "cmd+h>x"
action = "tab.create"

[[keybindings]]
keys = "ctrl+b>cmd+q"
action = "tab.create"

[[keybindings]]
keys = "cmd+`"
action = "tab.select_next"
"#;
    let table = load_keybinding_table(Some(toml));
    for keys in ["cmd+q>x", "cmd+h>x", "ctrl+b>cmd+q"] {
        assert!(
            table.diagnostics.iter().any(|d| d.category
                == DiagnosticCategory::ReservedCommandCollision
                && d.keys_notation == keys),
            "expected ReservedCommandCollision for chord {keys}: {:?}",
            table.diagnostics
        );
        assert!(
            contexts_for(&table, keys).is_empty(),
            "reserved-stroke chord {keys} must not bind"
        );
    }
    // Accepted §4.2 removed cmd+`; a user bind is allowed (not reserved).
    assert!(
        !table.diagnostics.iter().any(|d| {
            d.category == DiagnosticCategory::ReservedCommandCollision && d.keys_notation == "cmd+`"
        }),
        "cmd+` must not be reserved: {:?}",
        table.diagnostics
    );
    assert!(binding_for(&table, "cmd+`", WorkspaceCommandId::TabSelectNext).is_some());
}

// --- SPEC-024 §14 item 15: partial overlap and unbind ---

#[test]
fn conflict_worked_example_cmd_k_partial_overlap() {
    // builtin cmd+k → command_palette.open [app]
    // user cmd+k → tab.create [app, raw]
    // → user owns app+raw; builtin dropped; one DuplicateSequence (app)
    let toml = r#"
[[keybindings]]
keys = "cmd+k"
action = "tab.create"
context = ["app", "raw"]
"#;
    let table = load_keybinding_table(Some(toml));
    assert!(binding_for(&table, "cmd+k", WorkspaceCommandId::CommandPaletteOpen).is_none());
    let winner = binding_for(&table, "cmd+k", WorkspaceCommandId::TabCreate).unwrap();
    assert!(winner.context.contains(BindingContext::APP));
    assert!(winner.context.contains(BindingContext::RAW));
    assert_eq!(
        table
            .diagnostics
            .iter()
            .filter(|d| d.category == DiagnosticCategory::DuplicateSequence)
            .count(),
        1
    );
    assert!(table.diagnostics.iter().any(|d| {
        d.category == DiagnosticCategory::DuplicateSequence && d.message.contains("app")
    }));
}

#[test]
fn conflict_worked_example_partial_context_narrowing() {
    // user ctrl+b>n → tab.create [flow, raw]
    // user ctrl+b>n → pane.split_down [raw]
    // → first keeps [flow]; second owns [raw]; one DuplicateSequence (raw)
    let toml = r#"
[[keybindings]]
keys = "ctrl+b>n"
action = "tab.create"
context = ["flow", "raw"]

[[keybindings]]
keys = "ctrl+b>n"
action = "pane.split_down"
context = ["raw"]
"#;
    let table = load_keybinding_table(Some(toml));
    let rows = contexts_for(&table, "ctrl+b>n");
    assert_eq!(rows.len(), 2);
    let create = rows
        .iter()
        .find(|(id, _)| *id == WorkspaceCommandId::TabCreate)
        .unwrap();
    assert_eq!(create.1, BindingContext::FLOW);
    let split = rows
        .iter()
        .find(|(id, _)| *id == WorkspaceCommandId::PaneSplitDown)
        .unwrap();
    assert_eq!(split.1, BindingContext::RAW);
    assert_eq!(
        table
            .diagnostics
            .iter()
            .filter(|d| d.category == DiagnosticCategory::DuplicateSequence)
            .count(),
        1
    );
}

#[test]
fn conflict_worked_example_disjoint_contexts_both_remain() {
    // builtin cmd+t → tab.create [app]
    // user cmd+t → pane.split_down [raw]
    // → both remain
    let toml = r#"
[[keybindings]]
keys = "cmd+t"
action = "pane.split_down"
context = ["raw"]
"#;
    let table = load_keybinding_table(Some(toml));
    let rows = contexts_for(&table, "cmd+t");
    assert_eq!(rows.len(), 2);
    assert!(rows
        .iter()
        .any(|(id, ctx)| *id == WorkspaceCommandId::TabCreate && *ctx == BindingContext::APP));
    assert!(rows.iter().any(|(id, ctx)| {
        *id == WorkspaceCommandId::PaneSplitDown && *ctx == BindingContext::RAW
    }));
    assert!(!diag_categories(&table).contains(&DiagnosticCategory::DuplicateSequence));
}

#[test]
fn unbind_removes_builtin_later_rebind_and_noop() {
    let toml = r#"
[[keybindings]]
keys = "cmd+d"
action = "none"

[[keybindings]]
keys = "cmd+d"
action = "tab.create"

[[keybindings]]
keys = "cmd+shift+k"
action = "none"
"#;
    let table = load_keybinding_table(Some(toml));
    assert!(binding_for(&table, "cmd+d", WorkspaceCommandId::PaneSplitRight).is_none());
    let rebound = binding_for(&table, "cmd+d", WorkspaceCommandId::TabCreate).expect("rebind");
    assert_eq!(rebound.source, BindingSource::User { index: 1 });
    assert!(diag_categories(&table).contains(&DiagnosticCategory::UnbindNoEffect));
    assert!(table.diagnostics.iter().any(|d| {
        d.category == DiagnosticCategory::UnbindNoEffect && d.keys_notation == "cmd+shift+k"
    }));
}

// --- SPEC-024 §14 item 22: config selection ---

#[test]
fn config_selection_matches_input_policy_seyal_config_rules() {
    let custom = ui_config_path_from(
        Some(std::ffi::OsStr::new("/tmp/custom-seyal-1106.toml")),
        Some(std::ffi::OsStr::new("/Users/demo")),
    );
    assert_eq!(
        custom.as_deref(),
        Some(Path::new("/tmp/custom-seyal-1106.toml"))
    );

    let empty_env = ui_config_path_from(
        Some(std::ffi::OsStr::new("")),
        Some(std::ffi::OsStr::new("/Users/demo")),
    );
    assert_eq!(
        empty_env.as_deref(),
        Some(Path::new("/Users/demo/.config/seyal/config.toml"))
    );

    let unset = ui_config_path_from(None, Some(std::ffi::OsStr::new("/Users/demo")));
    assert_eq!(
        unset.as_deref(),
        Some(Path::new("/Users/demo/.config/seyal/config.toml"))
    );

    // Keybinding load uses the same path function as input policy.
    assert_eq!(
        super::keybinding_config_path(),
        crate::input_policy::input_policy_config_path()
    );
}
