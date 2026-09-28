//! SPEC-024 K1–K6: cold `[[keybindings]]` schema → immutable [`KeybindingTable`],
//! §6 routing gate, §8 chord prefix state, §11 shortcut projection, and K6
//! headed-evidence regressions for the implemented catalog.
//!
//! Distinct from [`crate::input_policy::InputPolicy`] and theme
//! [`crate::theme::UserUiSettings`]. Prefix state is product UI state, not VT.

mod builtins;
mod chord;
mod keys;
mod load;
mod projection;
mod reserved;
mod route;
mod stroke;
mod types;

#[cfg(test)]
mod chord_tests;
#[cfg(test)]
mod k6_tests;
#[cfg(test)]
mod projection_tests;
#[cfg(test)]
mod route_tests;
#[cfg(test)]
mod tests;

pub use chord::{ChordPrefixActive, ChordPrefixState, CHORD_PREFIX_TIMEOUT};
pub use load::{
    keybinding_config_path, load_keybinding_table, load_keybinding_table_from_path,
    process_keybinding_table,
};
pub use projection::{
    encode_menu_key_equivalent, project_shortcuts, projected_item_for, workspace_command_ffi_id,
    workspace_command_from_ffi_id, KeybindingShortcutProjection, ProjectedShortcut, ShortcutHint,
};
pub use route::{
    fallthrough_is_flow, fallthrough_is_terminal, resolve_tab_ordinal, route_context_set,
    route_keystroke, validate_workspace_command, workspace_command_permitted, InvokeError,
    RouteOutcome,
};
pub use stroke::NormalizedStroke;
pub use types::{
    BindingContext, BindingSequence, BindingSource, CompiledBinding, DiagnosticCategory, KeyStroke,
    KeySym, KeybindingDiagnostic, KeybindingTable, Modifiers, NamedKey, Ordinal1To9,
    WorkspaceCommand, WorkspaceCommandId,
};
