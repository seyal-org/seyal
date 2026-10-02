//! SPEC-024 K1: cold `[[keybindings]]` schema → immutable [`KeybindingTable`].
//!
//! Distinct from [`crate::input_policy::InputPolicy`] and theme
//! [`crate::theme::UserUiSettings`]. No event routing, menu wiring, or chord
//! runtime state.

mod keys;
mod load;
mod types;

#[cfg(test)]
mod tests;

pub use load::{keybinding_config_path, load_keybinding_table, load_keybinding_table_from_path};
pub use types::{
    BindingContext, BindingSequence, BindingSource, CompiledBinding, DiagnosticCategory, KeyStroke,
    KeySym, KeybindingDiagnostic, KeybindingTable, Modifiers, NamedKey, Ordinal1To9,
    WorkspaceCommand, WorkspaceCommandId,
};
