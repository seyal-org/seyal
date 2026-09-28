//! SPEC-024 K6 adversarial / cold-only regressions for the implemented catalog.
//!
//! Headed/XCUI coverage for harness-drivable cases lives under `macos/Seyal/Tests/`.
//! Items the headed harness cannot inject (cold OnceLock identity, reserved
//! override load diagnostics, composition×chord race) are proven here.

use std::ptr;
use std::time::{Duration, Instant};

use crate::presentation::PresentationMode;
use crate::theme::reload_process_ui_configuration_for_test;

use super::chord::ChordPrefixState;
use super::load::{load_keybinding_table, process_keybinding_table};
use super::route::{fallthrough_is_terminal, route_context_set, route_keystroke, RouteOutcome};
use super::stroke::normalized_from_notation;
use super::types::{
    BindingContext, DiagnosticCategory, KeyStroke, KeySym, Modifiers, WorkspaceCommand,
    WorkspaceCommandId,
};

fn raw() -> BindingContext {
    route_context_set(false, PresentationMode::Raw, false)
}

fn tui() -> BindingContext {
    route_context_set(false, PresentationMode::Tui, false)
}

fn route_at(
    table: &super::types::KeybindingTable,
    keys: &str,
    ctx: BindingContext,
    composition: bool,
    chord: &mut ChordPrefixState,
    now: Instant,
) -> RouteOutcome {
    let stroke = normalized_from_notation(keys).expect(keys);
    route_keystroke(table, &stroke, ctx, composition, chord, now)
}

fn chord_table() -> super::types::KeybindingTable {
    let toml = r#"
[[keybindings]]
keys = "ctrl+b>n"
action = "tab.create"
context = ["raw"]
"#;
    load_keybinding_table(Some(toml))
}

// --- §14 item 10: cold-only ---

#[test]
fn item10_theme_reload_leaves_keybinding_table_identity_unchanged() {
    let before = process_keybinding_table();
    let before_ptr = ptr::from_ref(before);
    // Simulated theme / UI cold reload must not recompile or replace the table.
    let _ = reload_process_ui_configuration_for_test(None);
    let after = process_keybinding_table();
    let after_ptr = ptr::from_ref(after);
    assert_eq!(
        before_ptr, after_ptr,
        "KeybindingTable must stay cold (OnceLock identity) across theme reload"
    );
    assert!(std::ptr::eq(before, after));
}

// --- Acceptance: Raw/TUI passthrough (Control-C + arrows) ---

#[test]
fn raw_and_tui_deliver_control_c_and_arrows_as_terminal_fallthrough() {
    let table = load_keybinding_table(None);
    let mut chord = ChordPrefixState::new();
    let now = Instant::now();
    for ctx in [raw(), tui()] {
        assert!(fallthrough_is_terminal(ctx));
        for keys in ["ctrl+c", "up", "down", "left", "right"] {
            let outcome = route_at(&table, keys, ctx, false, &mut chord, now);
            assert_eq!(outcome, RouteOutcome::Fallthrough, "{keys} in {ctx:?}");
            assert!(
                outcome.writes_pty_bytes(),
                "{keys} must remain on the terminal path"
            );
            assert!(!chord.is_active());
        }
    }
}

// --- Acceptance: ApplicationCommand match writes zero PTY bytes ---

#[test]
fn application_command_match_writes_zero_pty_bytes_in_raw_and_tui() {
    let table = load_keybinding_table(None);
    let mut chord = ChordPrefixState::new();
    let now = Instant::now();
    for ctx in [raw(), tui()] {
        let outcome = route_at(&table, "cmd+k", ctx, false, &mut chord, now);
        assert!(matches!(
            outcome,
            RouteOutcome::Matched {
                command: WorkspaceCommand {
                    id: WorkspaceCommandId::CommandPaletteOpen,
                    ..
                }
            }
        ));
        assert!(
            !outcome.writes_pty_bytes(),
            "ApplicationCommand must never write PTY bytes"
        );
    }
}

// --- Acceptance: reserved override attempt ---

#[test]
fn reserved_override_attempt_does_not_rebind_command_q() {
    let toml = r#"
[[keybindings]]
keys = "cmd+q"
action = "tab.create"
"#;
    let table = load_keybinding_table(Some(toml));
    assert!(table.diagnostics.iter().any(|d| d.category
        == DiagnosticCategory::ReservedCommandCollision
        && d.keys_notation == "cmd+q"));
    assert!(
        table
            .bindings
            .iter()
            .filter(|b| {
                b.sequence.strokes().len() == 1
                    && b.sequence.strokes()[0].modifiers.contains(Modifiers::CMD)
                    && b.sequence.strokes()[0].key == KeySym::Char('q')
            })
            .all(|b| b.action.id != WorkspaceCommandId::TabCreate),
        "user must not own reserved cmd+q"
    );
    let mut chord = ChordPrefixState::new();
    let outcome = route_at(&table, "cmd+q", raw(), false, &mut chord, Instant::now());
    assert_eq!(outcome, RouteOutcome::ReservedCommand);
    assert!(!outcome.writes_pty_bytes());
}

// --- Adversarial: composition racing a binding / active chord ---

#[test]
fn composition_race_clears_chord_prefix_and_skips_non_command() {
    use crate::app::{AppAction, ApplicationRoot, BindingEvidence};
    use seyal_core::{AttachmentId, ExecutionId};

    let mut root = ApplicationRoot::new();
    let fence = root.fence();
    root.apply(AppAction::Bind {
        fence,
        evidence: BindingEvidence {
            execution: ExecutionId::from_bytes([0x11; 16]),
            attachment: AttachmentId::from_bytes([0x22; 16]),
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

    // Composition owns the event: prefix clears; non-Command does not match.
    let stroke = normalized_from_notation("n").expect("n");
    let outcome = root
        .route_normalized_keystroke(&stroke, false, true)
        .expect("route");
    assert_eq!(outcome, RouteOutcome::CompositionConsumes);
    assert!(!outcome.writes_pty_bytes());
    assert!(
        !root.chord_prefix.is_active(),
        "composition race must clear the waiting chord prefix"
    );

    // Command still resolves while composition is active (§6.2 step 2 / §14.7).
    let cmd = normalized_from_notation("cmd+k").expect("cmd+k");
    let matched = root
        .route_normalized_keystroke(&cmd, false, true)
        .expect("cmd route");
    assert!(matches!(
        matched,
        RouteOutcome::Matched {
            command: WorkspaceCommand {
                id: WorkspaceCommandId::CommandPaletteOpen,
                ..
            }
        }
    ));
    assert!(!matched.writes_pty_bytes());
}

// --- Acceptance / adversarial: presentation switch mid-chord ---

#[test]
fn presentation_switch_mid_chord_clears_prefix_and_writes_no_consumed_prefix_to_pty() {
    use crate::app::{AppAction, ApplicationRoot, BindingEvidence};
    use seyal_core::{AttachmentId, ExecutionId};

    let table = chord_table();
    let mut chord = ChordPrefixState::new();
    let t0 = Instant::now();
    let prefix = route_at(&table, "ctrl+b", raw(), false, &mut chord, t0);
    assert_eq!(prefix, RouteOutcome::PrefixWait);
    assert!(
        !prefix.writes_pty_bytes(),
        "consumed prefix must never echo to the PTY"
    );
    assert!(chord.is_active());

    let mut root = ApplicationRoot::new();
    let fence = root.fence();
    root.apply(AppAction::Bind {
        fence,
        evidence: BindingEvidence {
            execution: ExecutionId::from_bytes([0x33; 16]),
            attachment: AttachmentId::from_bytes([0x44; 16]),
            controller: true,
            pty_generation: 1,
            alternate_screen: false,
        },
    })
    .expect("bind");
    root.chord_prefix.force_active_for_test(
        vec![KeyStroke {
            modifiers: Modifiers::CTRL,
            key: KeySym::Char('b'),
        }],
        t0,
    );
    assert!(root.chord_prefix.is_active());

    // Explicit presentation toggle (Flow → Raw) clears prefix without dispatch.
    let toggle = WorkspaceCommand {
        id: WorkspaceCommandId::PresentationToggleRaw,
        ordinal: None,
    };
    let route = root.keybinding_route_context(false);
    root.invoke_workspace_command_for_menu(toggle, route)
        .expect("toggle raw");
    assert!(
        !root.chord_prefix.is_active(),
        "presentation switch mid-chord must clear the prefix"
    );

    // After clear, the continuation is a fresh route — never a synthesized prefix echo.
    let after = route_at(
        &table,
        "n",
        raw(),
        false,
        &mut root.chord_prefix,
        t0 + Duration::from_millis(5),
    );
    assert_eq!(after, RouteOutcome::Fallthrough);
    assert!(after.matched_command().is_none());
}

// --- Adversarial: passthrough protection still gates load ---

#[test]
fn passthrough_protection_rejects_app_ctrl_c_and_keeps_raw_fallthrough() {
    let toml = r#"
[[keybindings]]
keys = "ctrl+c"
action = "tab.create"
context = ["app", "raw"]
"#;
    let table = load_keybinding_table(Some(toml));
    assert!(table
        .diagnostics
        .iter()
        .any(|d| d.category == DiagnosticCategory::TerminalPassthroughProtected));
    let mut chord = ChordPrefixState::new();
    let outcome = route_at(&table, "ctrl+c", raw(), false, &mut chord, Instant::now());
    assert_eq!(outcome, RouteOutcome::Fallthrough);
    assert!(outcome.writes_pty_bytes());
}

// --- Conflict diagnostics stay free of terminal fixtures (§14.14) ---

#[test]
fn conflict_diagnostics_never_contain_terminal_fixtures() {
    let fixture = "echo seyal-k6-terminal-fixture-c0ffee";
    let toml = r#"
[[keybindings]]
keys = "cmd+k"
action = "tab.create"
context = ["app", "raw"]

[[keybindings]]
keys = "ctrl+c"
action = "pane.split_down"
"#;
    let table = load_keybinding_table(Some(toml));
    assert!(!table.diagnostics.is_empty());
    for d in &table.diagnostics {
        assert!(!d.message.contains(fixture));
        assert!(!d.keys_notation.contains(fixture));
        assert!(!d.action.contains(fixture));
        assert!(!d.message.contains('\n'));
    }
    assert!(table
        .diagnostics
        .iter()
        .any(|d| d.category == DiagnosticCategory::DuplicateSequence));
    assert!(table
        .diagnostics
        .iter()
        .any(|d| d.category == DiagnosticCategory::TerminalPassthroughProtected));
}
