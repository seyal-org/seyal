//! SPEC-024 §3.2 normalized stroke matching (layout scalars, not US keycodes).

use super::types::{KeyStroke, KeySym, Modifiers, NamedKey};

/// Already-normalized native stroke (ADR-015). Rust owns matching.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NormalizedStroke {
    pub modifiers: Modifiers,
    /// Unshifted layout base after native normalization.
    pub key: KeySym,
    /// Shift-applied scalar (`characters` / Shift-preserved), when present.
    pub shift_applied: Option<char>,
}

impl NormalizedStroke {
    pub fn has_command(self) -> bool {
        self.modifiers.contains(Modifiers::CMD)
    }

    /// SPEC-024 R6.3.1: every stroke without `cmd` is terminal-capable.
    pub fn is_terminal_capable(self) -> bool {
        !self.has_command()
    }

    /// Decode a host-normalized FFI stroke (Swift forwards scalars only).
    pub fn from_ffi(modifier_bits: u8, named: bool, base: u32, shift_applied: u32) -> Option<Self> {
        let modifiers = Modifiers::from_bits_truncated(modifier_bits);
        let key = if named {
            KeySym::Named(named_key_from_u32(base)?)
        } else {
            let ch = char::from_u32(base)?;
            KeySym::Char(if ch.is_ascii_alphabetic() {
                ch.to_ascii_lowercase()
            } else {
                ch
            })
        };
        let shift_applied = if shift_applied == 0 {
            None
        } else {
            Some(char::from_u32(shift_applied)?)
        };
        Some(Self {
            modifiers,
            key,
            shift_applied,
        })
    }
}

/// True when a compiled binding stroke matches this normalized event (§3.2).
pub fn stroke_matches(binding: &KeyStroke, event: &NormalizedStroke) -> bool {
    let want_shift = binding.modifiers.contains(Modifiers::SHIFT);
    let has_shift = event.modifiers.contains(Modifiers::SHIFT);
    if want_shift != has_shift {
        return false;
    }

    let non_shift = Modifiers::CMD
        .union(Modifiers::CTRL)
        .union(Modifiers::OPT)
        .bits();
    if binding.modifiers.bits() & non_shift != event.modifiers.bits() & non_shift {
        return false;
    }

    if !want_shift {
        return key_eq(binding.key, event.key);
    }

    key_eq(binding.key, event.key) || shift_applied_matches(binding.key, event.shift_applied)
}

fn key_eq(binding: KeySym, event: KeySym) -> bool {
    match (binding, event) {
        (KeySym::Named(a), KeySym::Named(b)) => a == b,
        (KeySym::Char(a), KeySym::Char(b)) => chars_eq(a, b),
        _ => false,
    }
}

fn shift_applied_matches(binding: KeySym, shift_applied: Option<char>) -> bool {
    let KeySym::Char(expected) = binding else {
        return false;
    };
    let Some(actual) = shift_applied else {
        return false;
    };
    chars_eq(expected, actual)
}

fn chars_eq(a: char, b: char) -> bool {
    if a.is_ascii_alphabetic() && b.is_ascii_alphabetic() {
        a.eq_ignore_ascii_case(&b)
    } else {
        a == b
    }
}

fn named_key_from_u32(value: u32) -> Option<NamedKey> {
    Some(match value {
        0 => NamedKey::Enter,
        1 => NamedKey::Tab,
        2 => NamedKey::Space,
        3 => NamedKey::Escape,
        4 => NamedKey::Backspace,
        5 => NamedKey::Up,
        6 => NamedKey::Down,
        7 => NamedKey::Left,
        8 => NamedKey::Right,
        9 => NamedKey::F1,
        10 => NamedKey::F2,
        11 => NamedKey::F3,
        12 => NamedKey::F4,
        13 => NamedKey::F5,
        14 => NamedKey::F6,
        15 => NamedKey::F7,
        16 => NamedKey::F8,
        17 => NamedKey::F9,
        18 => NamedKey::F10,
        19 => NamedKey::F11,
        20 => NamedKey::F12,
        21 => NamedKey::Home,
        22 => NamedKey::End,
        23 => NamedKey::PageUp,
        24 => NamedKey::PageDown,
        25 => NamedKey::Delete,
        _ => return None,
    })
}

/// Build a [`NormalizedStroke`] from a single-stroke keys notation (tests).
#[cfg(test)]
pub fn normalized_from_notation(notation: &str) -> Option<NormalizedStroke> {
    let sequence = super::keys::parse_keys(notation).ok()?;
    let stroke = *sequence.strokes().first()?;
    let shift_applied = if stroke.modifiers.contains(Modifiers::SHIFT) {
        match stroke.key {
            KeySym::Char(ch) => Some(ch),
            KeySym::Named(_) => None,
        }
    } else {
        None
    };
    Some(NormalizedStroke {
        modifiers: stroke.modifiers,
        key: stroke.key,
        shift_applied,
    })
}
