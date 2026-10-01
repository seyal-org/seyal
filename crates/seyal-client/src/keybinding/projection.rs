//! SPEC-024 §11: read-only `KeybindingShortcutProjection` for menus and AX.

use super::route::workspace_command_permitted;
use super::types::{
    BindingContext, BindingSequence, KeyStroke, KeySym, KeybindingTable, NamedKey,
    WorkspaceCommand, WorkspaceCommandId,
};

/// One discoverable binding hint (single-stroke or chord). Never live input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutHint {
    pub keys_notation: String,
    pub is_chord: bool,
}

/// Projected menu/AX row for one menu-visible WorkspaceCommand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectedShortcut {
    pub command: WorkspaceCommand,
    /// Highest-declaration-index single-stroke `app` binding, if any.
    pub key_equivalent: Option<KeyStroke>,
    /// Notation of the winning key equivalent (for hosts that prefer strings).
    pub key_equivalent_notation: Option<String>,
    /// Every surviving binding in declaration order (chords included).
    pub hints: Vec<ShortcutHint>,
    /// R6.4.2: permitted against the current route context set.
    pub enabled: bool,
    /// AX/menu label: command id + hints only; never terminal contents.
    pub accessibility_label: String,
}

/// Immutable §11 projection. Key equivalents are cold; `enabled` is route-derived.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeybindingShortcutProjection {
    pub items: Vec<ProjectedShortcut>,
}

/// Menu-visible commands realized by the native host in M003 K5.
/// Excludes pane/navigation catalog rows (deferred) and composer-only triggers.
fn is_menu_visible(command: WorkspaceCommand) -> bool {
    match command.id {
        WorkspaceCommandId::CommandPaletteOpen
        | WorkspaceCommandId::TabCreate
        | WorkspaceCommandId::TabCloseFocused
        | WorkspaceCommandId::TabSelectPrevious
        | WorkspaceCommandId::TabSelectNext
        | WorkspaceCommandId::PaneSplitRight
        | WorkspaceCommandId::PaneSplitDown
        | WorkspaceCommandId::PresentationToggleRaw
        | WorkspaceCommandId::PresentationToggleTui
        | WorkspaceCommandId::GotoOpen
        | WorkspaceCommandId::FocusHistoryBack
        | WorkspaceCommandId::FocusHistoryForward => command.ordinal.is_none(),
        // Ordinal tabs are key-only; not separate menu rows in M003.
        WorkspaceCommandId::TabSelectOrdinal
        | WorkspaceCommandId::CommandPaletteClose
        | WorkspaceCommandId::PaneCloseFocused
        | WorkspaceCommandId::PaneFocusNext
        | WorkspaceCommandId::PaneFocusPrevious
        | WorkspaceCommandId::PresentationSetFlow
        | WorkspaceCommandId::PresentationSetRaw
        | WorkspaceCommandId::PresentationSetTui
        | WorkspaceCommandId::ComposerHistorySearchOpen
        | WorkspaceCommandId::AppQuit => false,
    }
}

/// Human title for menu/AX (no terminal text).
fn command_title(command: WorkspaceCommand) -> &'static str {
    match command.id {
        WorkspaceCommandId::CommandPaletteOpen => "Command Palette",
        WorkspaceCommandId::TabCreate => "New Tab",
        WorkspaceCommandId::TabCloseFocused => "Close Tab",
        WorkspaceCommandId::TabSelectPrevious => "Previous Tab",
        WorkspaceCommandId::TabSelectNext => "Next Tab",
        WorkspaceCommandId::PaneSplitRight => "Split Right",
        WorkspaceCommandId::PaneSplitDown => "Split Down",
        WorkspaceCommandId::PresentationToggleRaw => "Toggle Raw",
        WorkspaceCommandId::PresentationToggleTui => "Toggle TUI",
        WorkspaceCommandId::GotoOpen => "Go to…",
        WorkspaceCommandId::FocusHistoryBack => "Back",
        WorkspaceCommandId::FocusHistoryForward => "Forward",
        other => other.as_str(),
    }
}

/// Build the §11 projection for `route` (enabled bits follow R6.4.2).
pub fn project_shortcuts(
    table: &KeybindingTable,
    route: BindingContext,
) -> KeybindingShortcutProjection {
    let mut order: Vec<WorkspaceCommand> = Vec::new();
    for binding in &table.bindings {
        if !is_menu_visible(binding.action) {
            continue;
        }
        if !order.contains(&binding.action) {
            order.push(binding.action);
        }
    }

    let items = order
        .into_iter()
        .map(|command| project_one(table, command, route))
        .collect();
    KeybindingShortcutProjection { items }
}

fn project_one(
    table: &KeybindingTable,
    command: WorkspaceCommand,
    route: BindingContext,
) -> ProjectedShortcut {
    let mut hints = Vec::new();
    let mut key_equivalent: Option<(usize, KeyStroke, String)> = None;

    for (index, binding) in table.bindings.iter().enumerate() {
        if binding.action != command {
            continue;
        }
        let is_chord = binding.sequence.strokes().len() > 1;
        hints.push(ShortcutHint {
            keys_notation: binding.keys_notation.clone(),
            is_chord,
        });
        if is_chord {
            continue;
        }
        if !binding.context.contains(BindingContext::APP) {
            continue;
        }
        let Some(stroke) = single_stroke(&binding.sequence) else {
            continue;
        };
        match key_equivalent {
            None => {
                key_equivalent = Some((index, stroke, binding.keys_notation.clone()));
            }
            Some((best_index, _, _)) if index > best_index => {
                key_equivalent = Some((index, stroke, binding.keys_notation.clone()));
            }
            _ => {}
        }
    }

    let accessibility_label = accessibility_label_for(command, &hints);
    ProjectedShortcut {
        command,
        key_equivalent: key_equivalent.as_ref().map(|(_, stroke, _)| *stroke),
        key_equivalent_notation: key_equivalent.map(|(_, _, notation)| notation),
        hints,
        enabled: workspace_command_permitted(table, command, route),
        accessibility_label,
    }
}

fn single_stroke(sequence: &BindingSequence) -> Option<KeyStroke> {
    let strokes = sequence.strokes();
    if strokes.len() == 1 {
        Some(strokes[0])
    } else {
        None
    }
}

fn accessibility_label_for(command: WorkspaceCommand, hints: &[ShortcutHint]) -> String {
    let title = command_title(command);
    if hints.is_empty() {
        return title.to_owned();
    }
    let joined = hints
        .iter()
        .map(|hint| hint.keys_notation.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!("{title} ({joined})")
}

/// Look up one projected item by command (for native menu wiring).
pub fn projected_item_for(
    projection: &KeybindingShortcutProjection,
    command: WorkspaceCommand,
) -> Option<&ProjectedShortcut> {
    projection.items.iter().find(|item| item.command == command)
}

/// Stable FFI discriminant for [`WorkspaceCommandId`] (K5 menu surface).
pub fn workspace_command_ffi_id(id: WorkspaceCommandId) -> u16 {
    match id {
        WorkspaceCommandId::CommandPaletteOpen => 0,
        WorkspaceCommandId::CommandPaletteClose => 1,
        WorkspaceCommandId::TabCreate => 2,
        WorkspaceCommandId::TabCloseFocused => 3,
        WorkspaceCommandId::TabSelectPrevious => 4,
        WorkspaceCommandId::TabSelectNext => 5,
        WorkspaceCommandId::TabSelectOrdinal => 6,
        WorkspaceCommandId::PaneSplitRight => 7,
        WorkspaceCommandId::PaneSplitDown => 8,
        WorkspaceCommandId::PaneCloseFocused => 9,
        WorkspaceCommandId::PaneFocusNext => 10,
        WorkspaceCommandId::PaneFocusPrevious => 11,
        WorkspaceCommandId::PresentationSetFlow => 12,
        WorkspaceCommandId::PresentationSetRaw => 13,
        WorkspaceCommandId::PresentationSetTui => 14,
        WorkspaceCommandId::PresentationToggleRaw => 15,
        WorkspaceCommandId::PresentationToggleTui => 16,
        WorkspaceCommandId::ComposerHistorySearchOpen => 17,
        WorkspaceCommandId::AppQuit => 18,
        WorkspaceCommandId::FocusHistoryBack => 19,
        WorkspaceCommandId::FocusHistoryForward => 20,
        WorkspaceCommandId::GotoOpen => 21,
    }
}

pub fn workspace_command_from_ffi_id(id: u16, ordinal: u8) -> Option<WorkspaceCommand> {
    let id = match id {
        0 => WorkspaceCommandId::CommandPaletteOpen,
        1 => WorkspaceCommandId::CommandPaletteClose,
        2 => WorkspaceCommandId::TabCreate,
        3 => WorkspaceCommandId::TabCloseFocused,
        4 => WorkspaceCommandId::TabSelectPrevious,
        5 => WorkspaceCommandId::TabSelectNext,
        6 => WorkspaceCommandId::TabSelectOrdinal,
        7 => WorkspaceCommandId::PaneSplitRight,
        8 => WorkspaceCommandId::PaneSplitDown,
        9 => WorkspaceCommandId::PaneCloseFocused,
        10 => WorkspaceCommandId::PaneFocusNext,
        11 => WorkspaceCommandId::PaneFocusPrevious,
        12 => WorkspaceCommandId::PresentationSetFlow,
        13 => WorkspaceCommandId::PresentationSetRaw,
        14 => WorkspaceCommandId::PresentationSetTui,
        15 => WorkspaceCommandId::PresentationToggleRaw,
        16 => WorkspaceCommandId::PresentationToggleTui,
        17 => WorkspaceCommandId::ComposerHistorySearchOpen,
        18 => WorkspaceCommandId::AppQuit,
        19 => WorkspaceCommandId::FocusHistoryBack,
        20 => WorkspaceCommandId::FocusHistoryForward,
        21 => WorkspaceCommandId::GotoOpen,
        _ => return None,
    };
    let ordinal = if ordinal == 0 {
        None
    } else {
        super::types::Ordinal1To9::new(ordinal)
    };
    if id == WorkspaceCommandId::TabSelectOrdinal && ordinal.is_none() {
        return None;
    }
    Some(WorkspaceCommand { id, ordinal })
}

/// Encode a key stroke for `NSMenuItem` (scalar + modifier bits + named flag).
pub fn encode_menu_key_equivalent(stroke: &KeyStroke) -> (u8, u8, u32) {
    let modifiers = stroke.modifiers.bits();
    match stroke.key {
        KeySym::Char(ch) => (modifiers, 0, ch as u32),
        KeySym::Named(named) => (modifiers, 1, named_key_discriminant(named)),
    }
}

fn named_key_discriminant(named: NamedKey) -> u32 {
    match named {
        NamedKey::Enter => 0,
        NamedKey::Tab => 1,
        NamedKey::Space => 2,
        NamedKey::Escape => 3,
        NamedKey::Backspace => 4,
        NamedKey::Up => 5,
        NamedKey::Down => 6,
        NamedKey::Left => 7,
        NamedKey::Right => 8,
        NamedKey::F1 => 9,
        NamedKey::F2 => 10,
        NamedKey::F3 => 11,
        NamedKey::F4 => 12,
        NamedKey::F5 => 13,
        NamedKey::F6 => 14,
        NamedKey::F7 => 15,
        NamedKey::F8 => 16,
        NamedKey::F9 => 17,
        NamedKey::F10 => 18,
        NamedKey::F11 => 19,
        NamedKey::F12 => 20,
        NamedKey::Home => 21,
        NamedKey::End => 22,
        NamedKey::PageUp => 23,
        NamedKey::PageDown => 24,
        NamedKey::Delete => 25,
    }
}
