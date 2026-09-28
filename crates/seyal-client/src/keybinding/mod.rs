//! SPEC-024 K1–K4: cold `[[keybindings]]` schema → immutable [`KeybindingTable`],
//! §6 routing gate, and §8 chord prefix state machine.
//!
//! Distinct from [`crate::input_policy::InputPolicy`] and theme
//! [`crate::theme::UserUiSettings`]. Prefix state is product UI state, not VT.

mod builtins;
mod chord;
mod keys;
mod load;
mod reserved;
mod route;
mod stroke;
mod types;

#[cfg(test)]
mod chord_tests;
#[cfg(test)]
mod route_tests;
#[cfg(test)]
mod tests;

pub use chord::{ChordPrefixActive, ChordPrefixState, CHORD_PREFIX_TIMEOUT};
pub use load::{keybinding_config_path, load_keybinding_table, load_keybinding_table_from_path};
pub use route::{
    fallthrough_is_flow, fallthrough_is_terminal, resolve_tab_ordinal, route_context_set,
    route_keystroke, validate_workspace_command, InvokeError, RouteOutcome,
};
pub use stroke::NormalizedStroke;
pub use types::{
    BindingContext, BindingSequence, BindingSource, CompiledBinding, DiagnosticCategory, KeyStroke,
    KeySym, KeybindingDiagnostic, KeybindingTable, Modifiers, NamedKey, Ordinal1To9,
    WorkspaceCommand, WorkspaceCommandId,
};
