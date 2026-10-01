//! SPEC-024 §6 routing gate: context sets, match order, chords (§8), invoke re-validation.

use std::time::Instant;

use crate::presentation::PresentationMode;

use super::chord::ChordPrefixState;
use super::reserved::is_reserved_sequence;
use super::stroke::{stroke_matches, NormalizedStroke};
use super::types::{
    BindingContext, BindingSequence, KeyStroke, KeybindingTable, WorkspaceCommand,
    WorkspaceCommandId,
};

/// Outcome of one key event through SPEC-024 §6.2 (including §8 chords).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteOutcome {
    /// §4.2 reserved Command — native AppKit/menu only; never PTY.
    ReservedCommand,
    /// Table match → typed WorkspaceCommand. Writes zero PTY bytes.
    Matched { command: WorkspaceCommand },
    /// Chord prefix consumed; waiting for the next stroke. Writes zero PTY bytes.
    PrefixWait,
    /// Command miss → ordinary native app/menu; never composition, never PTY.
    UnmatchedCommand,
    /// Non-Command skipped while composition owns the event (§9 / §6.2 step 3).
    CompositionConsumes,
    /// No binding; caller continues SPEC-006 terminal / Flow / native fallthrough.
    Fallthrough,
}

impl RouteOutcome {
    /// ApplicationCommand / prefix-wait paths never write PTY bytes (SPEC-024 §6.2 / §8).
    pub fn writes_pty_bytes(self) -> bool {
        matches!(self, Self::Fallthrough)
    }

    pub fn matched_command(self) -> Option<WorkspaceCommand> {
        match self {
            Self::Matched { command } => Some(command),
            _ => None,
        }
    }
}

/// Invoke-time rejection (SPEC-024 §10 / R6.4.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvokeError {
    ActionUnavailable,
}

/// Current route context set (SPEC-024 §6.1).
pub fn route_context_set(
    palette_open: bool,
    mode: PresentationMode,
    composer_first_responder: bool,
) -> BindingContext {
    if palette_open {
        return BindingContext::PALETTE;
    }
    match mode {
        PresentationMode::Flow => {
            let mut ctx = BindingContext::APP.union(BindingContext::FLOW);
            if composer_first_responder {
                ctx.insert(BindingContext::COMPOSER);
            }
            ctx
        }
        PresentationMode::Raw => BindingContext::APP.union(BindingContext::RAW),
        PresentationMode::Tui => BindingContext::APP.union(BindingContext::TUI),
    }
}

/// SPEC-024 §6.1 specificity: `palette` > `composer` > `flow`/`raw`/`tui` > `app`.
pub fn context_specificity(bit: BindingContext) -> u8 {
    if bit.contains(BindingContext::PALETTE) {
        4
    } else if bit.contains(BindingContext::COMPOSER) {
        3
    } else if bit.contains(BindingContext::FLOW)
        || bit.contains(BindingContext::RAW)
        || bit.contains(BindingContext::TUI)
    {
        2
    } else if bit.contains(BindingContext::APP) {
        1
    } else {
        0
    }
}

fn best_intersecting_specificity(binding_ctx: BindingContext, route: BindingContext) -> u8 {
    binding_ctx
        .intersection(route)
        .iter_bits()
        .map(context_specificity)
        .max()
        .unwrap_or(0)
}

/// Route one already-normalized stroke through §6.2 / §8.
pub fn route_keystroke(
    table: &KeybindingTable,
    stroke: &NormalizedStroke,
    route: BindingContext,
    composition_active: bool,
    chord: &mut ChordPrefixState,
    now: Instant,
) -> RouteOutcome {
    let _ = chord.expire_if_due(now);

    if chord.is_active() {
        return route_with_active_prefix(table, stroke, route, composition_active, chord, now);
    }

    route_clean(table, stroke, route, composition_active, chord, now)
}

fn route_with_active_prefix(
    table: &KeybindingTable,
    stroke: &NormalizedStroke,
    route: BindingContext,
    composition_active: bool,
    chord: &mut ChordPrefixState,
    now: Instant,
) -> RouteOutcome {
    let prefix = chord
        .active()
        .expect("active prefix")
        .prefix
        .strokes()
        .to_vec();

    if let Some(command) = match_extended_complete(table, &prefix, stroke, route) {
        chord.clear();
        return RouteOutcome::Matched { command };
    }

    if let Some(extended) = match_extended_prefix(table, &prefix, stroke, route) {
        chord.extend(extended, now);
        return RouteOutcome::PrefixWait;
    }

    // R8.3: unmatched continuation cancels prefix; reclassify from clean state.
    chord.clear();
    route_clean(table, stroke, route, composition_active, chord, now)
}

fn route_clean(
    table: &KeybindingTable,
    stroke: &NormalizedStroke,
    route: BindingContext,
    composition_active: bool,
    chord: &mut ChordPrefixState,
    now: Instant,
) -> RouteOutcome {
    if stroke.has_command() {
        if is_reserved_event(stroke) {
            return RouteOutcome::ReservedCommand;
        }
        if let Some(command) = match_single_stroke(table, stroke, route) {
            return RouteOutcome::Matched { command };
        }
        // R8.4: never activate a Command prefix while composition is active —
        // a later commit (e.g. candidate click) must not complete a stale wait.
        if composition_active {
            return RouteOutcome::UnmatchedCommand;
        }
        // A Command stroke never opens a non-Command chord prefix.
        if let Some(prefix) = open_chord_prefix(table, stroke, route) {
            chord.activate(prefix, now);
            return RouteOutcome::PrefixWait;
        }
        return RouteOutcome::UnmatchedCommand;
    }

    if composition_active {
        return RouteOutcome::CompositionConsumes;
    }

    // Completing a surviving single-stroke binding never also opens a prefix (§7.1 step 8).
    if let Some(command) = match_single_stroke(table, stroke, route) {
        return RouteOutcome::Matched { command };
    }

    if let Some(prefix) = open_chord_prefix(table, stroke, route) {
        chord.activate(prefix, now);
        return RouteOutcome::PrefixWait;
    }

    RouteOutcome::Fallthrough
}

fn is_reserved_event(stroke: &NormalizedStroke) -> bool {
    let as_binding = KeyStroke {
        modifiers: stroke.modifiers,
        key: stroke.key,
    };
    let Ok(sequence) = BindingSequence::try_from_strokes(vec![as_binding]) else {
        return false;
    };
    if is_reserved_sequence(&sequence) {
        return true;
    }
    if let Some(ch) = stroke.shift_applied {
        let shifted = KeyStroke {
            modifiers: stroke.modifiers,
            key: super::types::KeySym::Char(ch),
        };
        if let Ok(seq) = BindingSequence::try_from_strokes(vec![shifted]) {
            return is_reserved_sequence(&seq);
        }
    }
    false
}

fn match_single_stroke(
    table: &KeybindingTable,
    stroke: &NormalizedStroke,
    route: BindingContext,
) -> Option<WorkspaceCommand> {
    match_binding_length(table, stroke, route, 1, &[])
}

fn open_chord_prefix(
    table: &KeybindingTable,
    stroke: &NormalizedStroke,
    route: BindingContext,
) -> Option<BindingSequence> {
    for (prefix, indices) in &table.chord_prefix_index {
        if prefix.strokes().len() != 1 {
            continue;
        }
        if !stroke_matches(&prefix.strokes()[0], stroke) {
            continue;
        }
        let usable = indices.iter().any(|&index| {
            table
                .bindings
                .get(index)
                .is_some_and(|b| !b.context.intersection(route).is_empty())
        });
        if usable {
            return Some(prefix.clone());
        }
    }
    None
}

fn match_extended_complete(
    table: &KeybindingTable,
    prefix: &[KeyStroke],
    stroke: &NormalizedStroke,
    route: BindingContext,
) -> Option<WorkspaceCommand> {
    let want_len = prefix.len() + 1;
    match_binding_length(table, stroke, route, want_len, prefix)
}

fn match_extended_prefix(
    table: &KeybindingTable,
    prefix: &[KeyStroke],
    stroke: &NormalizedStroke,
    route: BindingContext,
) -> Option<BindingSequence> {
    let want_len = prefix.len() + 1;
    for (candidate, indices) in &table.chord_prefix_index {
        if candidate.strokes().len() != want_len {
            continue;
        }
        if !candidate.strokes().starts_with(prefix) {
            continue;
        }
        if !stroke_matches(&candidate.strokes()[prefix.len()], stroke) {
            continue;
        }
        let usable = indices.iter().any(|&index| {
            table
                .bindings
                .get(index)
                .is_some_and(|b| !b.context.intersection(route).is_empty())
        });
        if usable {
            return Some(candidate.clone());
        }
    }
    None
}

fn match_binding_length(
    table: &KeybindingTable,
    stroke: &NormalizedStroke,
    route: BindingContext,
    len: usize,
    prefix: &[KeyStroke],
) -> Option<WorkspaceCommand> {
    let mut best: Option<(u8, usize, WorkspaceCommand)> = None;
    for (index, binding) in table.bindings.iter().enumerate() {
        let strokes = binding.sequence.strokes();
        if strokes.len() != len {
            continue;
        }
        if len > 1 {
            if !strokes.starts_with(prefix) {
                continue;
            }
            if !stroke_matches(&strokes[prefix.len()], stroke) {
                continue;
            }
        } else if !stroke_matches(&strokes[0], stroke) {
            continue;
        }
        let overlap = binding.context.intersection(route);
        if overlap.is_empty() {
            continue;
        }
        let score = best_intersecting_specificity(binding.context, route);
        match best {
            None => best = Some((score, index, binding.action)),
            Some((best_score, best_index, _)) => {
                if score > best_score || (score == best_score && index > best_index) {
                    best = Some((score, index, binding.action));
                }
            }
        }
    }
    best.map(|(_, _, command)| command)
}

/// R6.4.1 / §10.2: menu or keybinding invoke is permitted only when some
/// surviving binding for the command intersects the current route context set.
pub fn workspace_command_permitted(
    table: &KeybindingTable,
    command: WorkspaceCommand,
    route: BindingContext,
) -> bool {
    table
        .bindings
        .iter()
        .any(|binding| binding.action == command && !binding.context.intersection(route).is_empty())
}

/// Re-validate a menu-invoked (or synthetic) WorkspaceCommand against the route.
pub fn validate_workspace_command(
    table: &KeybindingTable,
    command: WorkspaceCommand,
    route: BindingContext,
) -> Result<(), InvokeError> {
    if workspace_command_permitted(table, command, route) {
        Ok(())
    } else {
        Err(InvokeError::ActionUnavailable)
    }
}

/// Convenience: true when Fallthrough under Raw/TUI must reach the terminal path.
pub fn fallthrough_is_terminal(route: BindingContext) -> bool {
    route.contains(BindingContext::RAW) || route.contains(BindingContext::TUI)
}

/// Item 13: Flow-active unmatched keys must not hit a hidden terminal route.
pub fn fallthrough_is_flow(route: BindingContext) -> bool {
    route.contains(BindingContext::FLOW) && !fallthrough_is_terminal(route)
}

/// Resolve `tab.select_ordinal` against the current tab list (1-based).
pub fn resolve_tab_ordinal(command: WorkspaceCommand, tab_count: usize) -> Result<u8, InvokeError> {
    if command.id != WorkspaceCommandId::TabSelectOrdinal {
        return Err(InvokeError::ActionUnavailable);
    }
    let Some(ordinal) = command.ordinal else {
        return Err(InvokeError::ActionUnavailable);
    };
    let n = ordinal.get() as usize;
    if n == 0 || n > tab_count {
        return Err(InvokeError::ActionUnavailable);
    }
    Ok(ordinal.get())
}
