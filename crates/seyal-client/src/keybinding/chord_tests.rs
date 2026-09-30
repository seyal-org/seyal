//! SPEC-024 §14 item 9 + K4 timeout / cancel / presentation-clear tests.

use std::time::{Duration, Instant};

use crate::presentation::PresentationMode;

use super::chord::{ChordPrefixState, CHORD_PREFIX_TIMEOUT};
use super::load::load_keybinding_table;
use super::route::{route_context_set, route_keystroke, RouteOutcome};
use super::stroke::normalized_from_notation;
use super::types::{
    BindingContext, DiagnosticCategory, KeyStroke, KeySym, KeybindingTable, Modifiers,
    WorkspaceCommand, WorkspaceCommandId,
};

fn raw() -> BindingContext {
    route_context_set(false, PresentationMode::Raw, false)
}

fn flow() -> BindingContext {
    route_context_set(false, PresentationMode::Flow, false)
}

fn route_at(
    table: &KeybindingTable,
    keys: &str,
    ctx: BindingContext,
    chord: &mut ChordPrefixState,
    now: Instant,
) -> RouteOutcome {
    let stroke = normalized_from_notation(keys).expect(keys);
    route_keystroke(table, &stroke, ctx, false, chord, now)
}

fn chord_table() -> KeybindingTable {
    let toml = r#"
[[keybindings]]
keys = "ctrl+b>n"
action = "tab.create"
context = ["raw"]
"#;
    load_keybinding_table(Some(toml))
}

// --- §14 item 9: runtime chord + load-time shadowing ---

#[test]
fn item9_ctrl_b_n_dispatches_once_without_pty_echo_of_prefix() {
    let table = chord_table();
    let mut chord = ChordPrefixState::new();
    let t0 = Instant::now();

    let prefix = route_at(&table, "ctrl+b", raw(), &mut chord, t0);
    assert_eq!(prefix, RouteOutcome::PrefixWait);
    assert!(
        !prefix.writes_pty_bytes(),
        "consumed prefix must not echo to PTY"
    );
    assert!(chord.is_active());

    let complete = route_at(
        &table,
        "n",
        raw(),
        &mut chord,
        t0 + Duration::from_millis(10),
    );
    assert!(matches!(
        complete,
        RouteOutcome::Matched {
            command: WorkspaceCommand {
                id: WorkspaceCommandId::TabCreate,
                ..
            }
        }
    ));
    assert!(!complete.writes_pty_bytes());
    assert!(!chord.is_active());
}

#[test]
fn item9_builtin_cmd_k_shadows_user_chord_until_unbound() {
    let shadowed = r#"
[[keybindings]]
keys = "cmd+k>t"
action = "tab.create"
context = ["app"]
"#;
    let table = load_keybinding_table(Some(shadowed));
    assert!(table
        .diagnostics
        .iter()
        .any(|d| d.category == DiagnosticCategory::ChordPrefixShadowed));
    assert!(
        table
            .bindings
            .iter()
            .all(|b| b.sequence.strokes().len() < 2),
        "cmd+k>t must be dropped while builtin cmd+k survives"
    );

    let unbound = r#"
[[keybindings]]
keys = "cmd+k"
action = "none"

[[keybindings]]
keys = "cmd+k>t"
action = "tab.create"
context = ["app"]
"#;
    let table = load_keybinding_table(Some(unbound));
    assert!(
        !table
            .diagnostics
            .iter()
            .any(|d| d.category == DiagnosticCategory::ChordPrefixShadowed),
        "after unbind, chord must survive: {:?}",
        table.diagnostics
    );
    assert!(table.bindings.iter().any(|b| {
        b.sequence.strokes().len() == 2
            && b.action.id == WorkspaceCommandId::TabCreate
            && b.context.contains(BindingContext::APP)
    }));
}

#[test]
fn item9_disjoint_ctrl_b_flow_and_chord_raw_both_survive() {
    let toml = r#"
[[keybindings]]
keys = "ctrl+b"
action = "pane.split_down"
context = ["flow"]

[[keybindings]]
keys = "ctrl+b>n"
action = "tab.create"
context = ["raw"]
"#;
    let table = load_keybinding_table(Some(toml));
    assert!(!table
        .diagnostics
        .iter()
        .any(|d| d.category == DiagnosticCategory::ChordPrefixShadowed));
    assert!(table.bindings.iter().any(|b| {
        b.sequence.strokes().len() == 1
            && b.action.id == WorkspaceCommandId::PaneSplitDown
            && b.context == BindingContext::FLOW
    }));
    assert!(table.bindings.iter().any(|b| {
        b.sequence.strokes().len() == 2 && b.action.id == WorkspaceCommandId::TabCreate
    }));

    let mut chord = ChordPrefixState::new();
    let t0 = Instant::now();
    // Flow: single-stroke wins; does not open a prefix.
    let flow_hit = route_at(&table, "ctrl+b", flow(), &mut chord, t0);
    assert!(matches!(
        flow_hit,
        RouteOutcome::Matched {
            command: WorkspaceCommand {
                id: WorkspaceCommandId::PaneSplitDown,
                ..
            }
        }
    ));
    assert!(!chord.is_active());

    // Raw: opens chord prefix.
    let raw_prefix = route_at(&table, "ctrl+b", raw(), &mut chord, t0);
    assert_eq!(raw_prefix, RouteOutcome::PrefixWait);
    assert!(chord.is_active());
}

#[test]
fn item9_three_sequence_case_leaves_s_and_u_drops_t() {
    // S=ctrl+b [flow], T=ctrl+b>n [flow, raw], U=ctrl+b>n>x [raw]
    let toml = r#"
[[keybindings]]
keys = "ctrl+b"
action = "pane.split_down"
context = ["flow"]

[[keybindings]]
keys = "ctrl+b>n"
action = "tab.create"
context = ["flow", "raw"]

[[keybindings]]
keys = "ctrl+b>n>x"
action = "pane.split_right"
context = ["raw"]
"#;
    let table = load_keybinding_table(Some(toml));
    assert!(table
        .diagnostics
        .iter()
        .any(|d| d.category == DiagnosticCategory::ChordPrefixShadowed));

    let has_s = table.bindings.iter().any(|b| {
        b.sequence.strokes().len() == 1
            && b.action.id == WorkspaceCommandId::PaneSplitDown
            && b.context.contains(BindingContext::FLOW)
    });
    let has_t = table
        .bindings
        .iter()
        .any(|b| b.sequence.strokes().len() == 2 && b.action.id == WorkspaceCommandId::TabCreate);
    let has_u = table.bindings.iter().any(|b| {
        b.sequence.strokes().len() == 3 && b.action.id == WorkspaceCommandId::PaneSplitRight
    });
    assert!(has_s, "S must survive");
    assert!(!has_t, "T must be dropped");
    assert!(has_u, "U must survive");
}

// --- timeout / cancel / presentation clear ---

#[test]
fn timeout_clears_prefix_without_dispatch_or_pty() {
    let table = chord_table();
    let mut chord = ChordPrefixState::new();
    let t0 = Instant::now();

    let prefix = route_at(&table, "ctrl+b", raw(), &mut chord, t0);
    assert_eq!(prefix, RouteOutcome::PrefixWait);
    assert!(!prefix.writes_pty_bytes());

    let after = route_at(
        &table,
        "n",
        raw(),
        &mut chord,
        t0 + CHORD_PREFIX_TIMEOUT + Duration::from_millis(1),
    );
    // Timeout cleared the prefix; bare `n` is not a binding → Fallthrough (may PTY).
    assert_eq!(after, RouteOutcome::Fallthrough);
    assert!(!chord.is_active());
    // The timed-out prefix itself was never dispatched and never echoed.
    assert!(after.matched_command().is_none());
}

#[test]
fn cancel_unmatched_continuation_clears_prefix_without_echoing_it() {
    let table = chord_table();
    let mut chord = ChordPrefixState::new();
    let t0 = Instant::now();

    assert_eq!(
        route_at(&table, "ctrl+b", raw(), &mut chord, t0),
        RouteOutcome::PrefixWait
    );
    // Unmatched continuation: cancel prefix; reclassify `x` from clean state.
    let cancel = route_at(
        &table,
        "x",
        raw(),
        &mut chord,
        t0 + Duration::from_millis(5),
    );
    assert_eq!(cancel, RouteOutcome::Fallthrough);
    assert!(
        cancel.writes_pty_bytes(),
        "continuation may fall through; prefix must not be synthesized"
    );
    assert!(!chord.is_active());
}

#[test]
fn presentation_switch_clear_drops_prefix_without_dispatch() {
    use crate::app::{AppAction, ApplicationRoot, BindingEvidence};
    use seyal_core::{AttachmentId, ExecutionId};

    let mut root = ApplicationRoot::new();
    let fence = root.fence();
    root.apply(AppAction::Bind {
        fence,
        evidence: BindingEvidence {
            execution: ExecutionId::from_bytes([7; 16]),
            attachment: AttachmentId::from_bytes([8; 16]),
            controller: true,
            pty_generation: 1,
            alternate_screen: false,
        },
    })
    .expect("bind");

    let t0 = Instant::now();
    root.chord_prefix.force_active_for_test(
        vec![KeyStroke {
            modifiers: Modifiers::CTRL,
            key: KeySym::Char('b'),
        }],
        t0,
    );
    assert!(root.chord_prefix.is_active());

    // Presentation-route change (Flow → Tui via alternate-screen refresh) clears prefix.
    root.apply(AppAction::Refresh {
        fence: root.fence(),
        alternate_screen: true,
    })
    .expect("refresh");
    assert!(
        !root.chord_prefix.is_active(),
        "presentation switch must clear chord prefix"
    );
    assert_eq!(
        root.snapshot().eligibility,
        crate::app::PresentationEligibility::Tui
    );
}
