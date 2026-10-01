//! Cold load of `[[keybindings]]` into an immutable [`KeybindingTable`].

use std::collections::BTreeMap;
use std::path::Path;

use crate::theme::{parse_toml, ui_config_path, TomlError, TomlValue};

use super::builtins::{compile_builtin_entries, PendingEntry};
use super::keys::{parse_keys, KeysError};
use super::reserved::is_reserved_sequence;
use super::types::{
    BindingContext, BindingSequence, BindingSource, CompiledBinding, DiagnosticCategory,
    KeybindingDiagnostic, KeybindingTable, Ordinal1To9, WorkspaceCommand, WorkspaceCommandId,
};

const KNOWN_FIELDS: &[&str] = &["keys", "action", "context", "ordinal"];

/// Same file selection as SPEC-006 §21.3 / `input_policy_config_path`.
pub fn keybinding_config_path() -> Option<std::path::PathBuf> {
    ui_config_path()
}

pub fn load_keybinding_table_from_path(path: Option<&Path>) -> KeybindingTable {
    let text = path.and_then(|path| std::fs::read_to_string(path).ok());
    load_keybinding_table(text.as_deref())
}

/// Parse and validate `[[keybindings]]` from TOML text, merge SPEC-024 §4.1
/// builtins, apply §4.2 reserved rejection and §7.1 / §7.3 resolution.
/// Missing/unreadable / whole-file parse failure → builtins only.
pub fn load_keybinding_table(toml_text: Option<&str>) -> KeybindingTable {
    let Some(text) = toml_text else {
        return finalize(compile_builtin_entries(), Vec::new());
    };
    let root = match parse_toml(text) {
        Ok(root) => root,
        Err(TomlError(_)) => return finalize(compile_builtin_entries(), Vec::new()),
    };
    compile_keybindings_from_root(&root)
}

fn compile_keybindings_from_root(root: &BTreeMap<String, TomlValue>) -> KeybindingTable {
    let mut diagnostics = Vec::new();
    let mut pending = compile_builtin_entries();

    let Some(value) = root.get("keybindings") else {
        return finalize(pending, diagnostics);
    };
    let Some(entries) = value.as_array_of_tables() else {
        diagnostics.push(KeybindingDiagnostic {
            category: DiagnosticCategory::TableIgnored,
            keys_notation: String::new(),
            action: String::new(),
            source: BindingSource::User { index: 0 },
            message: "keybindings ignored; expected array of tables".into(),
        });
        return finalize(pending, diagnostics);
    };

    for (index, entry) in entries.iter().enumerate() {
        let source = BindingSource::User {
            index: index as u32,
        };
        if let Some(row) = compile_entry(entry, source, &mut diagnostics) {
            pending.push(row);
        }
    }

    finalize(pending, diagnostics)
}

fn finalize(
    pending: Vec<PendingEntry>,
    mut diagnostics: Vec<KeybindingDiagnostic>,
) -> KeybindingTable {
    let bindings = resolve_conflicts(pending, &mut diagnostics);
    let chord_prefix_index = build_chord_prefix_index(&bindings);
    KeybindingTable {
        bindings,
        chord_prefix_index,
        diagnostics,
    }
}

/// SPEC-024 §7.1 steps 3–6 and §7.3 unbind tombstones.
fn resolve_conflicts(
    pending: Vec<PendingEntry>,
    diagnostics: &mut Vec<KeybindingDiagnostic>,
) -> Vec<CompiledBinding> {
    let mut live: Vec<Option<PendingEntry>> = Vec::new();

    for entry in pending {
        let mut taken = BindingContext::EMPTY;

        for slot in &mut live {
            let Some(prev) = slot.as_mut() else {
                continue;
            };
            if prev.sequence != entry.sequence {
                continue;
            }
            let overlap = prev.context.intersection(entry.context);
            if overlap.is_empty() {
                continue;
            }
            diagnostics.push(duplicate_sequence_diag(prev, &entry, overlap));
            prev.context.remove(overlap);
            taken = taken.union(overlap);
            if prev.context.is_empty() {
                *slot = None;
            }
        }

        if entry.action.is_none() {
            if taken.is_empty() {
                diagnostics.push(diag(
                    DiagnosticCategory::UnbindNoEffect,
                    &entry.keys_notation,
                    &entry.action_label,
                    entry.source,
                    "unbind took no context bits from earlier bindings",
                ));
            }
            continue;
        }

        live.push(Some(entry));
    }

    live.into_iter()
        .flatten()
        .map(|entry| CompiledBinding {
            sequence: entry.sequence,
            action: entry.action.expect("bind entries retain WorkspaceCommand"),
            context: entry.context,
            source: entry.source,
        })
        .collect()
}

fn duplicate_sequence_diag(
    earlier: &PendingEntry,
    later: &PendingEntry,
    transferred: BindingContext,
) -> KeybindingDiagnostic {
    let bits: Vec<&str> = transferred
        .iter_bits()
        .filter_map(BindingContext::bit_name)
        .collect();
    let message = format!(
        "duplicate sequence: later {} ({:?}) replaces earlier {} ({:?}) for [{}]",
        later.action_label,
        later.source,
        earlier.action_label,
        earlier.source,
        bits.join(", ")
    );
    KeybindingDiagnostic {
        category: DiagnosticCategory::DuplicateSequence,
        keys_notation: later.keys_notation.clone(),
        action: later.action_label.clone(),
        source: later.source,
        message,
    }
}

fn compile_entry(
    entry: &BTreeMap<String, TomlValue>,
    source: BindingSource,
    diagnostics: &mut Vec<KeybindingDiagnostic>,
) -> Option<PendingEntry> {
    for key in entry.keys() {
        if !KNOWN_FIELDS.contains(&key.as_str()) {
            diagnostics.push(KeybindingDiagnostic {
                category: DiagnosticCategory::UnknownFieldIgnored,
                keys_notation: string_field(entry, "keys").unwrap_or_default(),
                action: string_field(entry, "action").unwrap_or_default(),
                source,
                message: format!("unknown field {key} ignored"),
            });
        }
    }

    let keys_notation = match string_field(entry, "keys") {
        Some(keys) => keys,
        None => {
            diagnostics.push(diag(
                DiagnosticCategory::InvalidKeys,
                "",
                string_field(entry, "action").as_deref().unwrap_or_default(),
                source,
                "keys missing or not a string",
            ));
            return None;
        }
    };
    let action_raw = match string_field(entry, "action") {
        Some(action) => action,
        None => {
            diagnostics.push(diag(
                DiagnosticCategory::UnknownAction,
                &keys_notation,
                "",
                source,
                "action missing or not a string",
            ));
            return None;
        }
    };

    let sequence = match parse_keys(&keys_notation) {
        Ok(sequence) => sequence,
        Err(KeysError::ChordTooLong) => {
            diagnostics.push(diag(
                DiagnosticCategory::ChordTooLong,
                &keys_notation,
                &action_raw,
                source,
                "chord longer than 4 strokes",
            ));
            return None;
        }
        Err(_) => {
            diagnostics.push(diag(
                DiagnosticCategory::InvalidKeys,
                &keys_notation,
                &action_raw,
                source,
                "invalid keys notation",
            ));
            return None;
        }
    };

    let context = parse_context(entry, &keys_notation, &action_raw, source, diagnostics)?;

    if is_reserved_sequence(&sequence) {
        diagnostics.push(diag(
            DiagnosticCategory::ReservedCommandCollision,
            &keys_notation,
            &action_raw,
            source,
            "sequence is reserved and cannot be rebound",
        ));
        return None;
    }

    if action_raw == "none" {
        if entry.contains_key("ordinal") {
            diagnostics.push(diag(
                DiagnosticCategory::InvalidActionArgument,
                &keys_notation,
                &action_raw,
                source,
                "ordinal forbidden on action none",
            ));
            return None;
        }
        return Some(PendingEntry {
            sequence,
            keys_notation,
            action: None,
            action_label: "none".into(),
            context,
            source,
        });
    }

    if looks_like_disallowed_payload(&action_raw) {
        diagnostics.push(diag(
            DiagnosticCategory::DisallowedActionPayload,
            &keys_notation,
            &action_raw,
            source,
            "action embeds a disallowed payload",
        ));
        return None;
    }

    let Some(id) = WorkspaceCommandId::parse(&action_raw) else {
        diagnostics.push(diag(
            DiagnosticCategory::UnknownAction,
            &keys_notation,
            &action_raw,
            source,
            "unknown action id",
        ));
        return None;
    };

    let ordinal = match resolve_ordinal(entry, id, &keys_notation, &action_raw, source, diagnostics)
    {
        Ok(ordinal) => ordinal,
        Err(()) => return None,
    };

    Some(PendingEntry {
        sequence,
        keys_notation,
        action: Some(WorkspaceCommand { id, ordinal }),
        action_label: action_raw,
        context,
        source,
    })
}

fn resolve_ordinal(
    entry: &BTreeMap<String, TomlValue>,
    id: WorkspaceCommandId,
    keys_notation: &str,
    action_raw: &str,
    source: BindingSource,
    diagnostics: &mut Vec<KeybindingDiagnostic>,
) -> Result<Option<Ordinal1To9>, ()> {
    let has_ordinal = entry.contains_key("ordinal");
    if id == WorkspaceCommandId::TabSelectOrdinal {
        let Some(value) = entry.get("ordinal") else {
            diagnostics.push(diag(
                DiagnosticCategory::InvalidActionArgument,
                keys_notation,
                action_raw,
                source,
                "ordinal required for tab.select_ordinal",
            ));
            return Err(());
        };
        let Some(number) = value.as_number() else {
            diagnostics.push(diag(
                DiagnosticCategory::InvalidActionArgument,
                keys_notation,
                action_raw,
                source,
                "ordinal must be an integer 1..=9",
            ));
            return Err(());
        };
        if number.fract() != 0.0 || number < 0.0 || number > 255.0 {
            diagnostics.push(diag(
                DiagnosticCategory::InvalidActionArgument,
                keys_notation,
                action_raw,
                source,
                "ordinal must be an integer 1..=9",
            ));
            return Err(());
        }
        let Some(ordinal) = Ordinal1To9::new(number as u8) else {
            diagnostics.push(diag(
                DiagnosticCategory::InvalidActionArgument,
                keys_notation,
                action_raw,
                source,
                "ordinal must be an integer 1..=9",
            ));
            return Err(());
        };
        return Ok(Some(ordinal));
    }
    if has_ordinal {
        diagnostics.push(diag(
            DiagnosticCategory::InvalidActionArgument,
            keys_notation,
            action_raw,
            source,
            "ordinal forbidden for this action",
        ));
        return Err(());
    }
    Ok(None)
}

fn parse_context(
    entry: &BTreeMap<String, TomlValue>,
    keys_notation: &str,
    action_raw: &str,
    source: BindingSource,
    diagnostics: &mut Vec<KeybindingDiagnostic>,
) -> Option<BindingContext> {
    let Some(value) = entry.get("context") else {
        return Some(BindingContext::APP);
    };
    let Some(items) = value.as_string_array() else {
        diagnostics.push(diag(
            DiagnosticCategory::InvalidKeys,
            keys_notation,
            action_raw,
            source,
            "context must be an array of strings",
        ));
        return None;
    };
    if items.is_empty() {
        diagnostics.push(diag(
            DiagnosticCategory::InvalidKeys,
            keys_notation,
            action_raw,
            source,
            "context must be non-empty",
        ));
        return None;
    }
    let mut context = BindingContext::EMPTY;
    for item in items {
        let bit = match item.as_str() {
            "app" => BindingContext::APP,
            "flow" => BindingContext::FLOW,
            "raw" => BindingContext::RAW,
            "tui" => BindingContext::TUI,
            "composer" => BindingContext::COMPOSER,
            "palette" => BindingContext::PALETTE,
            _ => {
                diagnostics.push(diag(
                    DiagnosticCategory::InvalidKeys,
                    keys_notation,
                    action_raw,
                    source,
                    "unknown context token",
                ));
                return None;
            }
        };
        context.insert(bit);
    }
    Some(context)
}

fn looks_like_disallowed_payload(action: &str) -> bool {
    action.contains("://")
        || action.contains('/')
        || action.contains(' ')
        || action.contains(';')
        || action.starts_with("shell:")
        || action.starts_with("exec:")
        || action.chars().filter(|c| *c == '.').count() > 2
}

fn string_field(entry: &BTreeMap<String, TomlValue>, key: &str) -> Option<String> {
    entry.get(key)?.as_str().map(str::to_owned)
}

fn diag(
    category: DiagnosticCategory,
    keys_notation: &str,
    action: &str,
    source: BindingSource,
    message: &str,
) -> KeybindingDiagnostic {
    KeybindingDiagnostic {
        category,
        keys_notation: keys_notation.to_owned(),
        action: action.to_owned(),
        source,
        message: message.to_owned(),
    }
}

fn build_chord_prefix_index(bindings: &[CompiledBinding]) -> Vec<(BindingSequence, Vec<usize>)> {
    let mut map: BTreeMap<BindingSequence, Vec<usize>> = BTreeMap::new();
    for (index, binding) in bindings.iter().enumerate() {
        let strokes = binding.sequence.strokes();
        if strokes.len() < 2 {
            continue;
        }
        for len in 1..strokes.len() {
            if let Ok(prefix) = BindingSequence::try_from_strokes(strokes[..len].to_vec()) {
                map.entry(prefix).or_default().push(index);
            }
        }
    }
    map.into_iter().collect()
}
