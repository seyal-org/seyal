//! SPEC-024 §14 items 1, 8, and 14 for K1.

use crate::input_policy::load_input_policy;

use super::keys::parse_keys;
use super::load::load_keybinding_table;
use super::types::{
    BindingContext, DiagnosticCategory, KeySym, KeybindingTable, Modifiers, NamedKey, Ordinal1To9,
    WorkspaceCommandId,
};

#[test]
fn schema_valid_strokes_chords_and_punctuation() {
    let punct = [
        ']', '[', ',', '`', '%', '\\', '"', '!', '#', '$', '&', '\'', '(', ')', '*', '-', '.', '/',
        ':', ';', '<', '=', '?', '@', '^', '_', '{', '|', '}', '~',
    ];
    for ch in punct {
        let keys = if ch == '\\' || ch == '"' {
            // Exercised via escaped TOML below.
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
keys = "cmd+`"
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
    assert!(
        table.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        table.diagnostics
    );
    assert_eq!(table.bindings.len(), 12);
    assert_eq!(
        table.bindings[0].action.id,
        WorkspaceCommandId::CommandPaletteOpen
    );
    assert_eq!(
        table.bindings[2].action.ordinal,
        Some(Ordinal1To9::new(1).unwrap())
    );
    assert!(table.bindings[1].context.contains(BindingContext::FLOW));
    assert!(table.bindings[1].context.contains(BindingContext::RAW));
    assert!(table.bindings[1].context.contains(BindingContext::TUI));
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
        table.bindings.is_empty(),
        "invalid tables must not yield partial bindings: {:?}",
        table.bindings
    );
    let categories: Vec<_> = table.diagnostics.iter().map(|d| d.category).collect();
    assert!(categories.contains(&DiagnosticCategory::InvalidKeys));
    assert!(categories.contains(&DiagnosticCategory::ChordTooLong));
    assert!(categories.contains(&DiagnosticCategory::UnknownAction));
    assert!(categories.contains(&DiagnosticCategory::DisallowedActionPayload));
    assert!(categories.contains(&DiagnosticCategory::InvalidActionArgument));
}

#[test]
fn invalid_keybindings_shape_fails_closed() {
    let table = load_keybinding_table(Some("keybindings = \"nope\""));
    assert!(table.bindings.is_empty());
    assert_eq!(table.diagnostics.len(), 1);
    assert_eq!(
        table.diagnostics[0].category,
        DiagnosticCategory::TableIgnored
    );

    let table = load_keybinding_table(Some("this is not = toml ["));
    assert_eq!(table, KeybindingTable::empty());
}

#[test]
fn option_as_alt_unchanged_by_keybinding_load() {
    let toml = r#"
[input]
option_as_alt = true

[[keybindings]]
keys = "cmd+k"
action = "command_palette.open"
"#;
    let (policy_before, _) = load_input_policy(Some(toml));
    assert!(policy_before.option_as_alt);

    let table = load_keybinding_table(Some(toml));
    assert_eq!(table.bindings.len(), 1);

    let (policy_after, warnings) = load_input_policy(Some(toml));
    assert!(policy_after.option_as_alt);
    assert_eq!(policy_before, policy_after);
    assert!(warnings.is_empty());

    let toml_false = r#"
[input]
option_as_alt = false

[[keybindings]]
keys = "cmd+t"
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
    assert!(table.bindings.is_empty());
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
