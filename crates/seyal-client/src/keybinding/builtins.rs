//! SPEC-024 §4.1 builtin default rows for K2 / K7.
//!
//! Includes directional `pane.focus_*` (`cmd+opt+arrow`) and K7
//! `pane.zoom_toggle` (`cmd+shift+enter`). Excludes `pane.equalize_*`,
//! SPEC-022 navigation rows (K8), and ADR-018 `window.new` (R5.0.1).
//! `pane.swap_*` / `pane.move_*` are catalog ids without M003 builtins.
//! Includes §5.4 `ctrl+r` composer history-search.

use super::keys::parse_keys;
use super::types::{
    BindingContext, BindingSequence, BindingSource, Ordinal1To9, WorkspaceCommand,
    WorkspaceCommandId,
};

/// One builtin row before conflict resolution.
pub(crate) struct BuiltinRow {
    pub keys_notation: &'static str,
    pub id: WorkspaceCommandId,
    pub ordinal: Option<u8>,
    pub context: BindingContext,
}

/// M003 builtins admitted by K2 (see module docs for exclusions).
pub(crate) fn builtin_rows() -> &'static [BuiltinRow] {
    // Context constants for the static table.
    const APP: BindingContext = BindingContext::APP;
    const PALETTE: BindingContext = BindingContext::PALETTE;
    const COMPOSER: BindingContext = BindingContext::COMPOSER;

    &[
        BuiltinRow {
            keys_notation: "cmd+k",
            id: WorkspaceCommandId::CommandPaletteOpen,
            ordinal: None,
            context: APP,
        },
        BuiltinRow {
            keys_notation: "cmd+t",
            id: WorkspaceCommandId::TabCreate,
            ordinal: None,
            context: APP,
        },
        BuiltinRow {
            keys_notation: "cmd+w",
            id: WorkspaceCommandId::TabCloseFocused,
            ordinal: None,
            context: APP,
        },
        BuiltinRow {
            keys_notation: "cmd+shift+[",
            id: WorkspaceCommandId::TabSelectPrevious,
            ordinal: None,
            context: APP,
        },
        BuiltinRow {
            keys_notation: "cmd+shift+]",
            id: WorkspaceCommandId::TabSelectNext,
            ordinal: None,
            context: APP,
        },
        BuiltinRow {
            keys_notation: "cmd+d",
            id: WorkspaceCommandId::PaneSplitRight,
            ordinal: None,
            context: APP,
        },
        BuiltinRow {
            keys_notation: "cmd+shift+d",
            id: WorkspaceCommandId::PaneSplitDown,
            ordinal: None,
            context: APP,
        },
        BuiltinRow {
            keys_notation: "cmd+1",
            id: WorkspaceCommandId::TabSelectOrdinal,
            ordinal: Some(1),
            context: APP,
        },
        BuiltinRow {
            keys_notation: "cmd+2",
            id: WorkspaceCommandId::TabSelectOrdinal,
            ordinal: Some(2),
            context: APP,
        },
        BuiltinRow {
            keys_notation: "cmd+3",
            id: WorkspaceCommandId::TabSelectOrdinal,
            ordinal: Some(3),
            context: APP,
        },
        BuiltinRow {
            keys_notation: "cmd+4",
            id: WorkspaceCommandId::TabSelectOrdinal,
            ordinal: Some(4),
            context: APP,
        },
        BuiltinRow {
            keys_notation: "cmd+5",
            id: WorkspaceCommandId::TabSelectOrdinal,
            ordinal: Some(5),
            context: APP,
        },
        BuiltinRow {
            keys_notation: "cmd+6",
            id: WorkspaceCommandId::TabSelectOrdinal,
            ordinal: Some(6),
            context: APP,
        },
        BuiltinRow {
            keys_notation: "cmd+7",
            id: WorkspaceCommandId::TabSelectOrdinal,
            ordinal: Some(7),
            context: APP,
        },
        BuiltinRow {
            keys_notation: "cmd+8",
            id: WorkspaceCommandId::TabSelectOrdinal,
            ordinal: Some(8),
            context: APP,
        },
        BuiltinRow {
            keys_notation: "cmd+9",
            id: WorkspaceCommandId::TabSelectOrdinal,
            ordinal: Some(9),
            context: APP,
        },
        BuiltinRow {
            keys_notation: "cmd+enter",
            id: WorkspaceCommandId::PresentationToggleRaw,
            ordinal: None,
            context: APP,
        },
        BuiltinRow {
            keys_notation: "cmd+opt+enter",
            id: WorkspaceCommandId::PresentationToggleTui,
            ordinal: None,
            context: APP,
        },
        BuiltinRow {
            keys_notation: "cmd+opt+left",
            id: WorkspaceCommandId::PaneFocusLeft,
            ordinal: None,
            context: APP,
        },
        BuiltinRow {
            keys_notation: "cmd+opt+right",
            id: WorkspaceCommandId::PaneFocusRight,
            ordinal: None,
            context: APP,
        },
        BuiltinRow {
            keys_notation: "cmd+opt+up",
            id: WorkspaceCommandId::PaneFocusUp,
            ordinal: None,
            context: APP,
        },
        BuiltinRow {
            keys_notation: "cmd+opt+down",
            id: WorkspaceCommandId::PaneFocusDown,
            ordinal: None,
            context: APP,
        },
        BuiltinRow {
            // SPEC-024 §4.1 zoom toggle chord (not a secret).
            keys_notation: "cmd+shift+enter", // gitleaks:allow
            id: WorkspaceCommandId::PaneZoomToggle,
            ordinal: None,
            context: APP,
        },
        BuiltinRow {
            keys_notation: "escape",
            id: WorkspaceCommandId::CommandPaletteClose,
            ordinal: None,
            context: PALETTE,
        },
        BuiltinRow {
            keys_notation: "ctrl+r",
            id: WorkspaceCommandId::ComposerHistorySearchOpen,
            ordinal: None,
            context: COMPOSER,
        },
    ]
}

pub(crate) struct PendingEntry {
    pub sequence: BindingSequence,
    pub keys_notation: String,
    pub action: Option<WorkspaceCommand>,
    pub action_label: String,
    pub context: BindingContext,
    pub source: BindingSource,
}

pub(crate) fn compile_builtin_entries() -> Vec<PendingEntry> {
    builtin_rows()
        .iter()
        .map(|row| {
            let sequence = parse_keys(row.keys_notation)
                .unwrap_or_else(|_| panic!("builtin keys must parse: {}", row.keys_notation));
            assert!(
                !super::load::violates_terminal_passthrough(
                    sequence.strokes().first(),
                    row.context
                ),
                "builtin {} violates TerminalPassthroughProtected",
                row.keys_notation
            );
            let ordinal = row
                .ordinal
                .map(|n| Ordinal1To9::new(n).expect("builtin ordinal 1..=9"));
            PendingEntry {
                sequence,
                keys_notation: row.keys_notation.to_owned(),
                action: Some(WorkspaceCommand {
                    id: row.id,
                    ordinal,
                }),
                action_label: row.id.as_str().to_owned(),
                context: row.context,
                source: BindingSource::Builtin,
            }
        })
        .collect()
}
