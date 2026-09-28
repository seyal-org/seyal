//! SPEC-024 §4.2 enumerated reserved / non-rebindable Command set (R4.2.0).

use std::sync::OnceLock;

use super::keys::parse_keys;
use super::types::BindingSequence;

/// Exact reserved key notations from SPEC-024 §4.2 (both tables).
pub(crate) const RESERVED_KEY_NOTATIONS: &[&str] = &[
    // Application / AppKit menu equivalents
    "cmd+q",
    "cmd+h",
    "cmd+opt+h",
    "cmd+m",
    "cmd+opt+m",
    "cmd+`",
    "cmd+shift+`",
    "cmd+ctrl+f",
    "cmd+x",
    "cmd+c",
    "cmd+v",
    "cmd+a",
    "cmd+z",
    "cmd+shift+z",
    "cmd+ctrl+space",
    // System-intercepted strokes
    "cmd+tab",
    "cmd+shift+tab",
    "cmd+space",
    "cmd+opt+space",
    "cmd+opt+escape",
    "cmd+shift+3",
    "cmd+shift+4",
    "cmd+shift+5",
    "cmd+ctrl+q",
    "cmd+opt+d",
];

fn reserved_sequences() -> &'static [BindingSequence] {
    static SEQUENCES: OnceLock<Vec<BindingSequence>> = OnceLock::new();
    SEQUENCES.get_or_init(|| {
        RESERVED_KEY_NOTATIONS
            .iter()
            .map(|notation| {
                parse_keys(notation)
                    .unwrap_or_else(|_| panic!("reserved keys must parse: {notation}"))
            })
            .collect()
    })
}

pub(crate) fn is_reserved_sequence(sequence: &BindingSequence) -> bool {
    reserved_sequences()
        .iter()
        .any(|reserved| reserved == sequence)
}
