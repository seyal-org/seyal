//! Regressions for the default terminal/window shortcuts and TOML overrides.

use super::load::load_keybinding_table;
use super::types::{KeybindingTable, WorkspaceCommandId};

fn has_binding(table: &KeybindingTable, keys: &str, action: WorkspaceCommandId) -> bool {
    let sequence = super::keys::parse_keys(keys).expect("test keys");
    table
        .bindings
        .iter()
        .any(|binding| binding.sequence == sequence && binding.action.id == action)
}

#[test]
fn defaults_assign_cmd_t_to_terminal_and_cmd_n_to_window() {
    let table = load_keybinding_table(None);
    assert!(has_binding(
        &table,
        "cmd+t",
        WorkspaceCommandId::TerminalCreate
    ));
    assert!(!has_binding(&table, "cmd+t", WorkspaceCommandId::TabCreate));
    assert!(has_binding(&table, "cmd+n", WorkspaceCommandId::WindowNew));
    assert!(has_binding(
        &table,
        "cmd+shift+n",
        WorkspaceCommandId::WindowNew
    ));
}

#[test]
fn toml_can_rebind_terminal_and_window_shortcuts() {
    let toml = r#"
[[keybindings]]
keys = "cmd+t"
action = "none"

[[keybindings]]
keys = "cmd+shift+t"
action = "terminal.create"

[[keybindings]]
keys = "cmd+n"
action = "tab.create"

[[keybindings]]
keys = "cmd+shift+n"
action = "none"
"#;
    let table = load_keybinding_table(Some(toml));

    assert!(has_binding(
        &table,
        "cmd+shift+t",
        WorkspaceCommandId::TerminalCreate
    ));
    assert!(has_binding(
        &table,
        "cmd+n",
        WorkspaceCommandId::TabCreate
    ));
    assert!(!has_binding(
        &table,
        "cmd+t",
        WorkspaceCommandId::TerminalCreate
    ));
    assert!(!has_binding(
        &table,
        "cmd+shift+n",
        WorkspaceCommandId::WindowNew
    ));
}
