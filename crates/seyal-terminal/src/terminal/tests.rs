//! Unit tests for TerminalState / TerminalCore.

use super::*;
use crate::line::LineIdAllocator;
use crate::protocol_reply::MAX_PROTOCOL_REPLIES;
use crate::{HistoryAnchor, HistoryAnchorResolution, LineId, TerminalError};

#[test]
fn line_identity_exhaustion_is_explicit_and_does_not_duplicate_scroll_id() {
    let mut terminal = TerminalState::new(2, 1).expect("valid terminal");
    terminal.core.line_ids = LineIdAllocator::with_next(Some(u64::MAX));

    terminal
        .feed(b"A\r\n")
        .expect("last available line id may be allocated once");
    let last = terminal.line_id(0).expect("visible line has id");
    assert_eq!(last, LineId(u64::MAX));

    assert_eq!(
        terminal.feed(b"\r\n"),
        Err(TerminalError::LineIdentityExhausted)
    );
    assert_eq!(terminal.line_id(0), Some(last));
    assert_eq!(
        terminal.feed(b"ignored after fault"),
        Err(TerminalError::LineIdentityExhausted)
    );
    assert_eq!(terminal.line_id(0), Some(last));
}

#[test]
fn resize_preflights_line_identity_for_primary_and_alternate_atomically() {
    let mut terminal = TerminalState::new(2, 1).expect("valid terminal");
    terminal.core.line_ids = LineIdAllocator::with_next(Some(u64::MAX));
    terminal
        .feed(b"\x1b[?1049h")
        .expect("alternate consumes final available id");
    assert!(terminal.modes().alternate_screen);

    assert_eq!(
        terminal.resize(2, 2),
        Err(TerminalError::LineIdentityExhausted)
    );
    assert_eq!((terminal.cols(), terminal.rows()), (2, 1));
    terminal
        .feed(b"\x1b[?1049l")
        .expect("leaving alternate needs no new id");
    assert_eq!((terminal.cols(), terminal.rows()), (2, 1));
}

#[test]
fn exposes_bounded_trusted_shell_events_without_exposing_osc_payload() {
    let mut terminal = TerminalState::new(80, 24).unwrap();
    terminal
            .feed(b"\x1b]133;C;00112233445566778899aabbccddeeff\x07\x1b]133;D;00112233445566778899aabbccddeeff;17\x1b\\")
            .unwrap();
    let token = ShellIntegrationToken::from_bytes([
        0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
    ]);

    assert_eq!(
        terminal.take_shell_integration_event(),
        Some(ShellIntegrationEvent::CommandStarted {
            token,
            line: LineId(1),
        })
    );
    assert_eq!(
        terminal.take_shell_integration_event(),
        Some(ShellIntegrationEvent::CommandFinished {
            token,
            exit_status: 17,
            line: LineId(1),
        })
    );
    assert_eq!(terminal.take_shell_integration_event(), None);
}

#[test]
fn command_finished_line_is_the_last_output_row_not_the_next_empty_one() {
    // Regression: output ending with a trailing newline leaves the
    // cursor on a fresh, still-empty row when `D` fires. That row is
    // about to be overwritten by the shell's own next prompt, not part
    // of the command's output, so `line` must back up to the row that
    // actually holds the output.
    let mut terminal = TerminalState::new(80, 24).unwrap();
    let token_bytes = [
        0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
    ];
    let token = ShellIntegrationToken::from_bytes(token_bytes);
    terminal
        .feed(b"\x1b]133;C;00112233445566778899aabbccddeeff\x07")
        .unwrap();
    assert_eq!(
        terminal.take_shell_integration_event(),
        Some(ShellIntegrationEvent::CommandStarted {
            token,
            line: LineId(1)
        })
    );
    // Real output on row 1, ending with a newline: cursor moves to a
    // fresh, empty row 2 before the `D` marker is even parsed.
    terminal.feed(b"output-line\r\n").unwrap();
    terminal
        .feed(b"\x1b]133;D;00112233445566778899aabbccddeeff;0\x07")
        .unwrap();
    assert_eq!(
        terminal.take_shell_integration_event(),
        Some(ShellIntegrationEvent::CommandFinished {
            token,
            exit_status: 0,
            line: LineId(1),
        }),
        "line must be the row holding the real output, not the empty row after it"
    );
}

#[test]
fn command_finished_line_ignores_prompt_sp_spaces_on_the_next_row() {
    // zsh PROMPT_SP (default on) writes its end-of-line mark and a row of
    // spaces, then CR, before precmd emits `D`. The next row is then not
    // empty, but it is still the next prompt's row (#1015 live finding).
    let mut terminal = TerminalState::new(80, 24).unwrap();
    let token = ShellIntegrationToken::from_bytes([
        0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
    ]);
    terminal
        .feed(b"\x1b]133;C;00112233445566778899aabbccddeeff\x07")
        .unwrap();
    let _ = terminal.take_shell_integration_event();
    terminal.feed(b"output-line\r\n").unwrap();
    terminal.feed(&[b' '; 79]).unwrap();
    terminal.feed(b"\r").unwrap();
    terminal
        .feed(b"\x1b]133;D;00112233445566778899aabbccddeeff;0\x07")
        .unwrap();
    assert_eq!(
        terminal.take_shell_integration_event(),
        Some(ShellIntegrationEvent::CommandFinished {
            token,
            exit_status: 0,
            line: LineId(1),
        }),
        "PROMPT_SP spaces must not turn the next prompt's row into output"
    );
}

#[test]
fn command_finished_line_stays_on_the_output_row_without_a_trailing_newline() {
    let mut terminal = TerminalState::new(80, 24).unwrap();
    let token = ShellIntegrationToken::from_bytes([
        0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
    ]);
    terminal
        .feed(b"\x1b]133;C;00112233445566778899aabbccddeeff\x07")
        .unwrap();
    assert_eq!(
        terminal.take_shell_integration_event(),
        Some(ShellIntegrationEvent::CommandStarted {
            token,
            line: LineId(1)
        })
    );
    // No trailing newline: cursor stays on the same row as the output.
    terminal.feed(b"no-newline-output").unwrap();
    terminal
        .feed(b"\x1b]133;D;00112233445566778899aabbccddeeff;0\x07")
        .unwrap();
    assert_eq!(
        terminal.take_shell_integration_event(),
        Some(ShellIntegrationEvent::CommandFinished {
            token,
            exit_status: 0,
            line: LineId(1),
        })
    );
}

#[test]
fn unbound_or_malformed_markers_are_not_lifecycle_events() {
    let mut terminal = TerminalState::new(80, 24).unwrap();
    terminal
            .feed(b"\x1b]133;C\x07\x1b]133;C;short\x07\x1b]133;D;00112233445566778899aabbccddeeff;bad\x07")
            .unwrap();
    assert_eq!(terminal.take_shell_integration_event(), None);
}

#[test]
fn prompt_start_marker_is_a_trusted_shell_event_with_exact_shape() {
    let mut terminal = TerminalState::new(80, 24).unwrap();
    let token = ShellIntegrationToken::from_bytes([0xab; 16]);
    terminal
        .feed(b"\x1b]133;A;abababababababababababababababab\x07")
        .unwrap();
    assert_eq!(
        terminal.take_shell_integration_event(),
        Some(ShellIntegrationEvent::PromptStarted { token })
    );
    // Shapes outside A;<32hex> / C;<32hex> / D;<32hex>;<i32> stay deferred.
    terminal
            .feed(b"\x1b]133;A\x07\x1b]133;A;abababababababababababababababab;extra\x07\x1b]133;C;abababababababababababababababab;pwd\x07\x1b]133;B;abababababababababababababababab\x07")
            .unwrap();
    assert_eq!(terminal.take_shell_integration_event(), None);
}

#[test]
fn primary_history_range_returns_scrolled_rows_by_line_id() {
    let mut terminal = TerminalState::new(4, 2).unwrap();
    terminal.feed(b"one\r\ntwo\r\nthree").unwrap();
    let first = terminal.line_id(0).unwrap();
    let last = terminal.line_id(1).unwrap();
    let rows = terminal.primary_history_range(LineId(1), last, 8).unwrap();
    assert_eq!(rows.first().map(|(id, _)| *id), Some(LineId(1)));
    assert!(rows.iter().any(|(_, cells)| {
        cells
            .iter()
            .map(|cell| cell.character)
            .collect::<String>()
            .starts_with("one")
    }));
    assert!(terminal
        .primary_history_range(first, last, 0)
        .unwrap()
        .is_empty());
}

#[test]
fn primary_history_range_remains_available_during_alternate_screen() {
    let mut terminal = TerminalState::new(4, 2).unwrap();
    terminal.feed(b"one\r\ntwo\r\nthree").unwrap();
    let last = terminal.line_id(1).unwrap();
    let before = terminal.primary_history_range(LineId(1), last, 8).unwrap();
    assert!(!before.is_empty());
    terminal.feed(b"\x1b[?1049h").unwrap();
    assert!(terminal.modes().alternate_screen);
    let during = terminal.primary_history_range(LineId(1), last, 8).unwrap();
    assert_eq!(
        during.len(),
        before.len(),
        "primary history must remain readable while alternate screen is active"
    );
    assert_eq!(
        during
            .iter()
            .map(|(id, cells)| (*id, cells.iter().map(|c| c.character).collect::<String>()))
            .collect::<Vec<_>>(),
        before
            .iter()
            .map(|(id, cells)| (*id, cells.iter().map(|c| c.character).collect::<String>()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn retained_history_records_softwrap_and_hardbreak_lineage() {
    let mut terminal = TerminalState::new(2, 1).unwrap();
    terminal.feed(b"ab").unwrap();
    terminal.feed(b"c\n").unwrap();
    let history: Vec<_> = terminal
        .core
        .primary
        .history_entries()
        .map(|entry| {
            (
                entry.line_id(),
                entry.break_after(),
                entry
                    .presentation_cells()
                    .iter()
                    .map(|cell| cell.character)
                    .collect::<String>(),
            )
        })
        .collect();

    assert!(
        history.iter().any(|(_, break_after, text)| {
            matches!(break_after, crate::HistoryBreakAfter::SoftWrap) && text == "ab"
        }),
        "soft-wrapped overflow row should be retained with SoftWrap lineage"
    );
    assert!(
        history.iter().any(|(_, break_after, text)| {
            matches!(break_after, crate::HistoryBreakAfter::HardBreak) && text == "c"
        }),
        "explicit line feed should be retained with HardBreak lineage and no viewport padding"
    );
}

#[test]
fn history_projection_omits_viewport_padding_but_keeps_explicit_spaces() {
    let mut terminal = TerminalState::new(4, 1).unwrap();
    terminal.feed(b"a b\r\n").unwrap();

    let units = terminal.primary_history_units_range(LineId(1), LineId(1), 8);
    assert_eq!(
        units
            .iter()
            .map(|unit| unit.text.as_str())
            .collect::<String>(),
        "a b"
    );
    assert_eq!(
        terminal
            .primary_history_range(LineId(1), LineId(1), 8)
            .unwrap()[0]
            .1
            .len(),
        3
    );
}

#[test]
fn primary_resize_reflows_active_soft_wrapped_source() {
    let mut terminal = TerminalState::new(4, 2).unwrap();
    terminal.feed(b"abcdef").unwrap();
    terminal.resize(8, 2).unwrap();
    assert_eq!(terminal.row_text(0).as_deref(), Some("abcdef  "));
    assert_eq!(
        terminal.primary_row_break_after(0),
        Some(crate::HistoryBreakAfter::HardBreak),
        "resize must carry the source boundary onto the materialized row"
    );
    terminal.resize(3, 2).unwrap();
    let text = format!(
        "{}{}",
        terminal.row_text(0).unwrap(),
        terminal.row_text(1).unwrap()
    );
    assert!(
        text.starts_with("abc"),
        "rows={:?} reflow text={text:?}",
        (terminal.row_text(0), terminal.row_text(1))
    );
    assert!(text.contains("def"), "reflow text={text:?}");
}

#[test]
fn alternate_screen_never_adds_primary_history() {
    let mut terminal = TerminalState::new(4, 1).unwrap();
    terminal.feed(b"primary\r\n").unwrap();
    let before = terminal.primary_history_resident_bytes();
    terminal
        .feed(b"\x1b[?1049halternate\r\nalternate\r\n\x1b[?1049l")
        .unwrap();
    assert_eq!(terminal.primary_history_resident_bytes(), before);
    assert!(terminal.primary_history_eviction_generation() == 0);
}

#[test]
fn source_breaks_stay_bounded_to_active_lines_after_long_output() {
    let mut terminal = TerminalState::new(8, 2).unwrap();
    for i in 0..200 {
        terminal.feed(format!("line-{i}\r\n").as_bytes()).unwrap();
    }
    assert!(
        terminal.primary_source_break_count() <= 4,
        "source_breaks leaked retained lineage metadata: {}",
        terminal.primary_source_break_count()
    );
}

#[test]
fn retained_unit_preserves_canonical_multiscalar_payload_and_anchor() {
    let mut terminal = TerminalState::new(2, 1).unwrap();
    terminal.feed("界\u{301}\r\n".as_bytes()).unwrap();
    let line_id = terminal
        .primary_history_units_range(LineId(1), LineId(u64::MAX), 1)
        .into_iter()
        .next()
        .expect("wide source row is retained")
        .anchor
        .line_id;
    let unit = terminal.primary_history_unit(HistoryAnchor {
        line_id,
        unit_offset: 0,
    });
    assert!(matches!(
        unit,
        HistoryAnchorResolution::Resolved { ref text, width: 2, .. }
            if text == "界\u{301}"
    ));

    let projected = terminal.primary_history_units_range(line_id, line_id, 8);
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].anchor.unit_offset, 0);
    assert_eq!(projected[0].text, "界\u{301}");
    assert_eq!(projected[0].width, 2);
}

#[test]
fn history_reflow_is_invariant_under_input_chunking() {
    let input = "alpha界\u{301}xyz\r\nsecond-line\r\nthird";
    let mut one_shot = TerminalState::new(6, 1).unwrap();
    one_shot.feed(input.as_bytes()).unwrap();
    let mut bytewise = TerminalState::new(6, 1).unwrap();
    for byte in input.as_bytes() {
        bytewise.feed(std::slice::from_ref(byte)).unwrap();
    }
    assert_eq!(
        one_shot.primary_history_reflow(5, 64),
        bytewise.primary_history_reflow(5, 64)
    );
}

#[test]
fn prepare_resize_leaves_geometry_and_damage_unchanged_until_commit() {
    let mut terminal = TerminalState::new(4, 2).unwrap();
    let _ = terminal.take_damage();
    let generation = terminal.damage_generation();
    let prepared = terminal.prepare_resize(8, 4).expect("prepare");
    assert_eq!((terminal.cols(), terminal.rows()), (4, 2));
    assert_eq!(terminal.damage_generation(), generation);
    assert!(terminal.take_damage().is_none());

    terminal.commit_resize(prepared);
    assert_eq!((terminal.cols(), terminal.rows()), (8, 4));
    let damage = terminal.take_damage().expect("damage after commit");
    assert!(damage.full);
    assert!(terminal.damage_generation() > generation);
}

#[test]
fn dsr_cpr_emits_ordered_replies_with_chunk_equivalence() {
    let mut one_shot = TerminalState::new(80, 24).unwrap();
    one_shot.feed(b"\x1b[10;20H\x1b[6n").unwrap();
    let expected = one_shot.take_protocol_reply().expect("cpr reply");
    assert_eq!(expected.as_bytes(), b"\x1b[10;20R");
    assert!(one_shot.take_protocol_reply().is_none());

    let mut chunked = TerminalState::new(80, 24).unwrap();
    for byte in b"\x1b[10;20H\x1b[6n" {
        chunked.feed(std::slice::from_ref(byte)).unwrap();
    }
    assert_eq!(
        chunked.take_protocol_reply().map(|r| r.as_bytes().to_vec()),
        Some(expected.as_bytes().to_vec())
    );
}

#[test]
fn multiple_queries_preserve_reply_order_and_bound() {
    let mut terminal = TerminalState::new(80, 24).unwrap();
    terminal.feed(b"\x1b[1;1H\x1b[6n\x1b[2;3H\x1b[6n").unwrap();
    assert_eq!(
        terminal.take_protocol_reply().unwrap().as_bytes(),
        b"\x1b[1;1R"
    );
    assert_eq!(
        terminal.take_protocol_reply().unwrap().as_bytes(),
        b"\x1b[2;3R"
    );

    let mut flood = TerminalState::new(80, 24).unwrap();
    let before = flood.diagnostics().deferred_sequences;
    for _ in 0..(MAX_PROTOCOL_REPLIES + 4) {
        flood.feed(b"\x1b[6n").unwrap();
    }
    let mut drained = 0usize;
    while flood.take_protocol_reply().is_some() {
        drained += 1;
    }
    assert_eq!(drained, MAX_PROTOCOL_REPLIES);
    assert!(flood.diagnostics().deferred_sequences > before);
}

#[test]
fn decrqm_mode_25_and_unknown_queries_are_safe() {
    let mut terminal = TerminalState::new(80, 24).unwrap();
    terminal.feed(b"\x1b[?25l\x1b[?25$p").unwrap();
    assert_eq!(
        terminal.take_protocol_reply().unwrap().as_bytes(),
        b"\x1b[?25;2$y"
    );

    // Mode 2027 defaults to set and is queryable (SPEC-011 §3).
    terminal.feed(b"\x1b[?2027$p").unwrap();
    assert_eq!(
        terminal.take_protocol_reply().unwrap().as_bytes(),
        b"\x1b[?2027;1$y"
    );

    let deferred_before = terminal.diagnostics().deferred_sequences;
    terminal.feed(b"\x1b[0n\x1b[?999$p").unwrap();
    assert!(terminal.take_protocol_reply().is_none());
    assert!(terminal.diagnostics().deferred_sequences > deferred_before);
}

#[test]
fn replies_enqueued_before_feed_fault_remain_takeable() {
    let mut terminal = TerminalState::new(2, 1).expect("valid terminal");
    terminal.core.line_ids = LineIdAllocator::with_next(Some(u64::MAX));
    terminal
        .feed(b"A\r\n")
        .expect("consume final available line id");
    assert_eq!(
        terminal.feed(b"\x1b[6n\r\n"),
        Err(TerminalError::LineIdentityExhausted)
    );
    assert_eq!(
        terminal.take_protocol_reply().unwrap().as_bytes(),
        b"\x1b[1;1R"
    );
    assert!(terminal.take_protocol_reply().is_none());
}

#[test]
fn huge_sparse_history_span_is_bounded_by_retained_storage() {
    use std::time::{Duration, Instant};

    let mut terminal = TerminalState::new(4, 2).unwrap();
    terminal.feed(b"one\r\ntwo\r\nthree\r\nfour").unwrap();
    // Alternate screen burns LineIds, creating gaps in primary identity space.
    terminal.feed(b"\x1b[?1049h\x1b[?1049l").unwrap();
    terminal.feed(b"five\r\nsix").unwrap();

    let started = Instant::now();
    let rows = terminal
        .primary_history_range(LineId(1), LineId(u64::MAX), 512)
        .unwrap();
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "history lookup must not scale with numeric LineId distance"
    );
    assert!(rows.len() <= 512);
    assert!(!rows.is_empty());

    let absent = terminal
        .primary_history_range(LineId(u64::MAX - 10), LineId(u64::MAX), 8)
        .unwrap();
    assert!(absent.is_empty());
}

#[test]
fn oversized_geometry_is_rejected_without_mutating_state() {
    assert!(matches!(
        TerminalState::new(MAX_TERMINAL_COLUMNS, MAX_TERMINAL_ROWS + 1),
        Err(TerminalError::InvalidSize)
    ));
    assert!(matches!(
        TerminalState::new(MAX_TERMINAL_COLUMNS + 1, MAX_TERMINAL_ROWS),
        Err(TerminalError::InvalidSize)
    ));
    let mut terminal = TerminalState::new(80, 24).unwrap();
    let generation = terminal.damage_generation();
    assert!(matches!(
        terminal.prepare_resize(u16::MAX, u16::MAX),
        Err(TerminalError::InvalidSize)
    ));
    assert_eq!((terminal.cols(), terminal.rows()), (80, 24));
    assert_eq!(terminal.damage_generation(), generation);
    terminal
        .prepare_resize(MAX_TERMINAL_COLUMNS, MAX_TERMINAL_ROWS)
        .expect("max accepted geometry prepares");
}
