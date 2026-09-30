//! SPEC-024 §3.2 stroke and chord notation parsing.

use super::types::{BindingSequence, KeyStroke, KeySym, Modifiers, NamedKey};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeysError {
    Empty,
    InvalidStroke,
    ChordTooLong,
    DuplicateModifier,
    UnknownToken,
}

pub(crate) fn parse_keys(notation: &str) -> Result<BindingSequence, KeysError> {
    let trimmed = notation.trim();
    if trimmed.is_empty() {
        return Err(KeysError::Empty);
    }
    let mut strokes = Vec::new();
    for part in trimmed.split('>') {
        let stroke = parse_stroke(part.trim())?;
        strokes.push(stroke);
    }
    if strokes.len() > 4 {
        return Err(KeysError::ChordTooLong);
    }
    BindingSequence::try_from_strokes(strokes).map_err(|_| KeysError::Empty)
}

fn parse_stroke(raw: &str) -> Result<KeyStroke, KeysError> {
    if raw.is_empty() {
        return Err(KeysError::InvalidStroke);
    }
    let mut modifiers = Modifiers::EMPTY;
    let mut key: Option<KeySym> = None;
    for token in raw.split('+') {
        let token = token.trim();
        if token.is_empty() {
            return Err(KeysError::InvalidStroke);
        }
        let lower = token.to_ascii_lowercase();
        match lower.as_str() {
            "cmd" => insert_mod(&mut modifiers, Modifiers::CMD)?,
            "ctrl" => insert_mod(&mut modifiers, Modifiers::CTRL)?,
            "shift" => insert_mod(&mut modifiers, Modifiers::SHIFT)?,
            "opt" => insert_mod(&mut modifiers, Modifiers::OPT)?,
            "plus" => set_key(&mut key, KeySym::Char('+'))?,
            "greater" => set_key(&mut key, KeySym::Char('>'))?,
            named => {
                if let Some(named_key) = parse_named(named) {
                    set_key(&mut key, KeySym::Named(named_key))?;
                } else if token.chars().count() == 1 {
                    let ch = token.chars().next().expect("one char");
                    if is_letter(ch) {
                        set_key(&mut key, KeySym::Char(ch.to_ascii_lowercase()))?;
                    } else if is_digit(ch) || is_punct(ch) {
                        set_key(&mut key, KeySym::Char(ch))?;
                    } else {
                        return Err(KeysError::UnknownToken);
                    }
                } else {
                    return Err(KeysError::UnknownToken);
                }
            }
        }
    }
    let Some(key) = key else {
        return Err(KeysError::InvalidStroke);
    };
    // Bare "+" or ">" would have been split away; reject modifier-only strokes.
    Ok(KeyStroke { modifiers, key })
}

fn insert_mod(mods: &mut Modifiers, bit: Modifiers) -> Result<(), KeysError> {
    if mods.contains(bit) {
        return Err(KeysError::DuplicateModifier);
    }
    mods.insert(bit);
    Ok(())
}

fn set_key(slot: &mut Option<KeySym>, key: KeySym) -> Result<(), KeysError> {
    if slot.is_some() {
        return Err(KeysError::InvalidStroke);
    }
    *slot = Some(key);
    Ok(())
}

fn parse_named(token: &str) -> Option<NamedKey> {
    Some(match token {
        "enter" => NamedKey::Enter,
        "tab" => NamedKey::Tab,
        "space" => NamedKey::Space,
        "escape" => NamedKey::Escape,
        "backspace" => NamedKey::Backspace,
        "up" => NamedKey::Up,
        "down" => NamedKey::Down,
        "left" => NamedKey::Left,
        "right" => NamedKey::Right,
        "f1" => NamedKey::F1,
        "f2" => NamedKey::F2,
        "f3" => NamedKey::F3,
        "f4" => NamedKey::F4,
        "f5" => NamedKey::F5,
        "f6" => NamedKey::F6,
        "f7" => NamedKey::F7,
        "f8" => NamedKey::F8,
        "f9" => NamedKey::F9,
        "f10" => NamedKey::F10,
        "f11" => NamedKey::F11,
        "f12" => NamedKey::F12,
        "home" => NamedKey::Home,
        "end" => NamedKey::End,
        "pageup" => NamedKey::PageUp,
        "pagedown" => NamedKey::PageDown,
        "delete" => NamedKey::Delete,
        _ => return None,
    })
}

fn is_letter(ch: char) -> bool {
    ch.is_ascii_alphabetic()
}

fn is_digit(ch: char) -> bool {
    ch.is_ascii_digit()
}

/// Printable ASCII punct from SPEC-024 §3.2 (excluding `+` and `>` separators).
fn is_punct(ch: char) -> bool {
    matches!(
        ch,
        '!' | '"'
            | '#'
            | '$'
            | '%'
            | '&'
            | '\''
            | '('
            | ')'
            | '*'
            | ','
            | '-'
            | '.'
            | '/'
            | ':'
            | ';'
            | '<'
            | '='
            | '?'
            | '@'
            | '['
            | '\\'
            | ']'
            | '^'
            | '_'
            | '`'
            | '{'
            | '|'
            | '}'
            | '~'
    )
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn rejects_bare_plus_and_greater_as_key() {
        assert_eq!(parse_keys("+"), Err(KeysError::InvalidStroke));
        assert_eq!(parse_keys("ctrl++"), Err(KeysError::InvalidStroke));
        // ">" alone is one empty stroke on each side after split… actually
        // "a>b" works; bare ">" splits to ["", ""] → InvalidStroke.
        assert_eq!(parse_keys(">"), Err(KeysError::InvalidStroke));
        assert!(parse_keys("cmd+plus").is_ok());
        assert!(parse_keys("cmd+greater").is_ok());
    }
}
