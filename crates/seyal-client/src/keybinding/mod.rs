//! SPEC-024 K1/K2/K3: cold `[[keybindings]]` schema → immutable [`KeybindingTable`],
//! plus the §6 routing gate (match order, Raw/TUI non-interception, invoke re-validation).
//!
//! Distinct from [`crate::input_policy::InputPolicy`] and theme
//! [`crate::theme::UserUiSettings`]. Chord prefix runtime state is K4.

mod builtins;
mod keys;
mod load;
mod reserved;
mod route;
mod stroke;
mod types;

#[cfg(test)]
mod route_tests;
#[cfg(test)]
mod tests;

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
