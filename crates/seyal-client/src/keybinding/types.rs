//! Typed SPEC-024 cold keybinding values. Immutable after load.

use std::fmt;

/// Modifier bits for one stroke (SPEC-024 §3.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Modifiers(u8);

impl Modifiers {
    pub const EMPTY: Self = Self(0);
    pub const CMD: Self = Self(1 << 0);
    pub const CTRL: Self = Self(1 << 1);
    pub const SHIFT: Self = Self(1 << 2);
    pub const OPT: Self = Self(1 << 3);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub(crate) const fn from_bits_truncated(bits: u8) -> Self {
        Self(bits)
    }
}

/// Named non-character keys from SPEC-024 §3.2.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NamedKey {
    Enter,
    Tab,
    Space,
    Escape,
    Backspace,
    Up,
    Down,
    Left,
    Right,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
}

/// Layout-independent key symbol after cold parse.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeySym {
    Named(NamedKey),
    /// ASCII letter (lowercase), digit, or punct scalar.
    Char(char),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyStroke {
    pub modifiers: Modifiers,
    pub key: KeySym,
}

/// One stroke or chord of 1..=4 strokes.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BindingSequence {
    strokes: Vec<KeyStroke>,
}

impl BindingSequence {
    pub fn strokes(&self) -> &[KeyStroke] {
        &self.strokes
    }

    pub(crate) fn try_from_strokes(strokes: Vec<KeyStroke>) -> Result<Self, ()> {
        if (1..=4).contains(&strokes.len()) {
            Ok(Self { strokes })
        } else {
            Err(())
        }
    }
}

/// Context bits (SPEC-024 §6.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BindingContext(u8);

impl BindingContext {
    pub const EMPTY: Self = Self(0);
    pub const APP: Self = Self(1 << 0);
    pub const FLOW: Self = Self(1 << 1);
    pub const RAW: Self = Self(1 << 2);
    pub const TUI: Self = Self(1 << 3);
    pub const COMPOSER: Self = Self(1 << 4);
    pub const PALETTE: Self = Self(1 << 5);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    pub const fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }

    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Individual context bits set on this value, in SPEC-024 token order.
    pub fn iter_bits(self) -> impl Iterator<Item = Self> {
        [
            Self::APP,
            Self::FLOW,
            Self::RAW,
            Self::TUI,
            Self::COMPOSER,
            Self::PALETTE,
        ]
        .into_iter()
        .filter(move |bit| self.contains(*bit))
    }

    pub fn bit_name(self) -> Option<&'static str> {
        match self {
            Self::APP => Some("app"),
            Self::FLOW => Some("flow"),
            Self::RAW => Some("raw"),
            Self::TUI => Some("tui"),
            Self::COMPOSER => Some("composer"),
            Self::PALETTE => Some("palette"),
            _ => None,
        }
    }
}

/// Closed WorkspaceCommandId catalog currently admitted at load (SPEC-024 §5).
/// Gated ids (window.*, ADR-021 pane verbs, SPEC-022 Back/Forward) stay out until
/// their typed actions land (R5.0.1 / R5.1.3 / R5.5.3). `goto.open` is admitted
/// with the N4 surface (R5.5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WorkspaceCommandId {
    CommandPaletteOpen,
    CommandPaletteClose,
    TabCreate,
    TabCloseFocused,
    TabSelectPrevious,
    TabSelectNext,
    TabSelectOrdinal,
    PaneSplitRight,
    PaneSplitDown,
    PaneCloseFocused,
    PaneFocusNext,
    PaneFocusPrevious,
    PresentationSetFlow,
    PresentationSetRaw,
    PresentationSetTui,
    PresentationToggleRaw,
    PresentationToggleTui,
    ComposerHistorySearchOpen,
    GotoOpen,
    AppQuit,
}

impl WorkspaceCommandId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CommandPaletteOpen => "command_palette.open",
            Self::CommandPaletteClose => "command_palette.close",
            Self::TabCreate => "tab.create",
            Self::TabCloseFocused => "tab.close_focused",
            Self::TabSelectPrevious => "tab.select_previous",
            Self::TabSelectNext => "tab.select_next",
            Self::TabSelectOrdinal => "tab.select_ordinal",
            Self::PaneSplitRight => "pane.split_right",
            Self::PaneSplitDown => "pane.split_down",
            Self::PaneCloseFocused => "pane.close_focused",
            Self::PaneFocusNext => "pane.focus_next",
            Self::PaneFocusPrevious => "pane.focus_previous",
            Self::PresentationSetFlow => "presentation.set_flow",
            Self::PresentationSetRaw => "presentation.set_raw",
            Self::PresentationSetTui => "presentation.set_tui",
            Self::PresentationToggleRaw => "presentation.toggle_raw",
            Self::PresentationToggleTui => "presentation.toggle_tui",
            Self::ComposerHistorySearchOpen => "composer.history_search.open",
            Self::GotoOpen => "goto.open",
            Self::AppQuit => "app.quit",
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        Some(match id {
            "command_palette.open" => Self::CommandPaletteOpen,
            "command_palette.close" => Self::CommandPaletteClose,
            "tab.create" => Self::TabCreate,
            "tab.close_focused" => Self::TabCloseFocused,
            "tab.select_previous" => Self::TabSelectPrevious,
            "tab.select_next" => Self::TabSelectNext,
            "tab.select_ordinal" => Self::TabSelectOrdinal,
            "pane.split_right" => Self::PaneSplitRight,
            "pane.split_down" => Self::PaneSplitDown,
            "pane.close_focused" => Self::PaneCloseFocused,
            "pane.focus_next" => Self::PaneFocusNext,
            "pane.focus_previous" => Self::PaneFocusPrevious,
            "presentation.set_flow" => Self::PresentationSetFlow,
            "presentation.set_raw" => Self::PresentationSetRaw,
            "presentation.set_tui" => Self::PresentationSetTui,
            "presentation.toggle_raw" => Self::PresentationToggleRaw,
            "presentation.toggle_tui" => Self::PresentationToggleTui,
            "composer.history_search.open" => Self::ComposerHistorySearchOpen,
            "goto.open" => Self::GotoOpen,
            "app.quit" => Self::AppQuit,
            _ => return None,
        })
    }
}

/// Tab ordinal 1..=9 for `tab.select_ordinal`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ordinal1To9(u8);

impl Ordinal1To9 {
    pub fn new(value: u8) -> Option<Self> {
        if (1..=9).contains(&value) {
            Some(Self(value))
        } else {
            None
        }
    }

    pub fn get(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WorkspaceCommand {
    pub id: WorkspaceCommandId,
    pub ordinal: Option<Ordinal1To9>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BindingSource {
    Builtin,
    User { index: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledBinding {
    pub sequence: BindingSequence,
    pub action: WorkspaceCommand,
    pub context: BindingContext,
    pub source: BindingSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiagnosticCategory {
    InvalidKeys,
    UnknownAction,
    InvalidActionArgument,
    DisallowedActionPayload,
    ChordTooLong,
    ReservedCommandCollision,
    TerminalPassthroughProtected,
    DuplicateSequence,
    ChordPrefixShadowed,
    UnbindNoEffect,
    UnknownFieldIgnored,
    TableIgnored,
}

impl DiagnosticCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidKeys => "InvalidKeys",
            Self::UnknownAction => "UnknownAction",
            Self::InvalidActionArgument => "InvalidActionArgument",
            Self::DisallowedActionPayload => "DisallowedActionPayload",
            Self::ChordTooLong => "ChordTooLong",
            Self::ReservedCommandCollision => "ReservedCommandCollision",
            Self::TerminalPassthroughProtected => "TerminalPassthroughProtected",
            Self::DuplicateSequence => "DuplicateSequence",
            Self::ChordPrefixShadowed => "ChordPrefixShadowed",
            Self::UnbindNoEffect => "UnbindNoEffect",
            Self::UnknownFieldIgnored => "UnknownFieldIgnored",
            Self::TableIgnored => "TableIgnored",
        }
    }
}

impl fmt::Display for DiagnosticCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Non-secret load diagnostic (SPEC-024 §7.2 / §12.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeybindingDiagnostic {
    pub category: DiagnosticCategory,
    pub keys_notation: String,
    pub action: String,
    pub source: BindingSource,
    pub message: String,
}

/// Immutable cold keybinding table. Distinct from `InputPolicy` and `UserUiSettings`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeybindingTable {
    pub bindings: Vec<CompiledBinding>,
    /// Proper prefixes of multi-stroke sequences → binding indices (K4 consumes).
    pub chord_prefix_index: Vec<(BindingSequence, Vec<usize>)>,
    pub diagnostics: Vec<KeybindingDiagnostic>,
}

impl KeybindingTable {
    pub fn empty() -> Self {
        Self {
            bindings: Vec::new(),
            chord_prefix_index: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}
