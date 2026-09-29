//! SPEC-024 §6 routing gate: context sets, match order, invoke re-validation.

use crate::presentation::PresentationMode;

use super::reserved::is_reserved_sequence;
use super::stroke::{stroke_matches, NormalizedStroke};
use super::types::{
    BindingContext, BindingSequence, KeyStroke, KeybindingTable, WorkspaceCommand,
    WorkspaceCommandId,
};

/// Outcome of one key event through SPEC-024 §6.2 (K3: no chord prefixes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteOutcome {
    /// §4.2 reserved Command — native AppKit/menu only; never PTY.
    ReservedCommand,
    /// Table match → typed WorkspaceCommand. Writes zero PTY bytes.
    Matched { command: WorkspaceCommand },
    /// Command miss → ordinary native app/menu; never composition, never PTY.
    UnmatchedCommand,
    /// Non-Command skipped while composition owns the event (§9 / §6.2 step 3).
    CompositionConsumes,
    /// No binding; caller continues SPEC-006 terminal / Flow / native fallthrough.
    Fallthrough,
}

impl RouteOutcome {
    /// ApplicationCommand paths never write PTY bytes (SPEC-024 §6.2 / item 4).
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

/// Route one already-normalized stroke through §6.2 (single-stroke only; chords are K4).
pub fn route_keystroke(
    table: &KeybindingTable,
    stroke: &NormalizedStroke,
    route: BindingContext,
    composition_active: bool,
) -> RouteOutcome {
    if stroke.has_command() {
        if is_reserved_event(stroke) {
            return RouteOutcome::ReservedCommand;
        }
        if let Some(command) = match_single_stroke(table, stroke, route) {
            return RouteOutcome::Matched { command };
        }
        return RouteOutcome::UnmatchedCommand;
    }

    if composition_active {
        return RouteOutcome::CompositionConsumes;
    }

    if let Some(command) = match_single_stroke(table, stroke, route) {
        return RouteOutcome::Matched { command };
    }
    RouteOutcome::Fallthrough
}

fn is_reserved_event(stroke: &NormalizedStroke) -> bool {
    // Reserved set is exact Command sequences; compare as a one-stroke BindingSequence.
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
    // Shift-applied form of a reserved stroke (e.g. cmd+shift+}` vs cmd+shift+]).
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
    let mut best: Option<(u8, usize, WorkspaceCommand)> = None;
    for (index, binding) in table.bindings.iter().enumerate() {
        if binding.sequence.strokes().len() != 1 {
            continue;
        }
        let binding_stroke = &binding.sequence.strokes()[0];
        if !stroke_matches(binding_stroke, stroke) {
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
