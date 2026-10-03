//! SPEC-024 §4.2 enumerated reserved / non-rebindable Command set (R4.2.0).

use std::sync::OnceLock;

use super::keys::parse_keys;
use super::types::{BindingSequence, KeyStroke};

/// Exact reserved key notations from SPEC-024 §4.2 (both tables).
///
/// Accepted §4.2 removed `cmd+\`` / `cmd+shift+\`` (window cycling is
/// Rust-owned `window.cycle_*`); do not re-reserve them here.
pub(crate) const RESERVED_KEY_NOTATIONS: &[&str] = &[
    // Application / AppKit menu equivalents
    "cmd+q",
    "cmd+h",
    "cmd+opt+h",
    "cmd+m",
    "cmd+opt+m",
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

fn reserved_strokes() -> &'static [KeyStroke] {
    static STROKES: OnceLock<Vec<KeyStroke>> = OnceLock::new();
    STROKES.get_or_init(|| {
        RESERVED_KEY_NOTATIONS
            .iter()
            .map(|notation| {
                let sequence = parse_keys(notation)
                    .unwrap_or_else(|_| panic!("reserved keys must parse: {notation}"));
                assert_eq!(
                    sequence.strokes().len(),
                    1,
                    "reserved notations are single strokes: {notation}"
                );
                sequence.strokes()[0]
            })
            .collect()
    })
}

/// SPEC-024 §4.2 / §7.1 step 2 / §7.3: reject when **any** stroke in the
/// sequence is a reserved Command stroke (not only whole-sequence equality).
pub(crate) fn is_reserved_sequence(sequence: &BindingSequence) -> bool {
    let reserved = reserved_strokes();
    sequence
        .strokes()
        .iter()
        .any(|stroke| reserved.iter().any(|r| r == stroke))
}
