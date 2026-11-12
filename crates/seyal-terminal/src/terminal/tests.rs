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
    // Shapes outside A/C/D and the R readiness marker stay deferred.
    terminal
            .feed(b"\x1b]133;A\x07\x1b]133;A;abababababababababababababababab;extra\x07\x1b]133;C;abababababababababababababababab;pwd\x07\x1b]133;B;abababababababababababababababab\x07")
            .unwrap();
    assert_eq!(terminal.take_shell_integration_event(), None);
}

#[test]
fn readiness_marker_is_the_fourth_shape_and_names_the_shell() {
    let mut terminal = TerminalState::new(80, 24).unwrap();
    let token = ShellIntegrationToken::from_bytes([0x11; 16]);
    let mut hex = String::new();
    token.write_hex(&mut hex);
    terminal
        .feed(format!("\x1b]133;R;{hex};z\x07").as_bytes())
        .unwrap();
    assert_eq!(
        terminal.take_shell_integration_event(),
        Some(ShellIntegrationEvent::Readiness {
            token,
            shell: SessionShellKind::Zsh,
        })
    );
    terminal
        .feed(format!("\x1b]133;R;{hex};b\x07").as_bytes())
        .unwrap();
    assert_eq!(
        terminal.take_shell_integration_event(),
        Some(ShellIntegrationEvent::Readiness {
            token,
            shell: SessionShellKind::Bash,
        })
    );
    terminal
        .feed(format!("\x1b]133;R;{hex};q\x07\x1b]133;R;{hex};z;extra\x07").as_bytes())
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
fn resize_during_output_after_wrapped_command_preserves_every_output_row() {
    let mut terminal = TerminalState::new(90, 39).unwrap();
    terminal
        .feed(
            b"user-rc-ran\r\nfor i in $(seq 1 12); do printf 'RESIZE_BLOCK_LINE %02d\\n' \"$i\"; sleep 0.3; done\r\nRESIZE_BLOCK_LINE 01\r\nRESIZE_BLOCK_LINE 02\r\nRESIZE_BLOCK_LINE 03\r\n",
        )
        .unwrap();

    terminal.resize(42, 39).unwrap();
    terminal
        .feed(
            b"RESIZE_BLOCK_LINE 04\r\nRESIZE_BLOCK_LINE 05\r\nRESIZE_BLOCK_LINE 06\r\nRESIZE_BLOCK_LINE 07\r\nRESIZE_BLOCK_LINE 08\r\nRESIZE_BLOCK_LINE 09\r\nRESIZE_BLOCK_LINE 10\r\nRESIZE_BLOCK_LINE 11\r\nRESIZE_BLOCK_LINE 12\r\n",
        )
        .unwrap();

    let text = terminal
        .primary_history_range(LineId(1), LineId(u64::MAX), 128)
        .unwrap()
        .into_iter()
        .map(|(_, cells)| {
            cells
                .into_iter()
                .map(|cell| cell.character)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    for line in 1..=12 {
        let expected = format!("RESIZE_BLOCK_LINE {line:02}");
        assert!(
            text.contains(&expected),
            "resize lost {expected} after a wrapped command echo:\n{text}"
        );
    }
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

#[test]
fn shell_boundary_journals_in_wire_order_with_cell_mutations() {
    use crate::primary_blocks::attribution::{MarkerKind, PrimaryEvent};

    let mut terminal = TerminalState::new(8, 2).unwrap();
    terminal
        .feed(b"hi\x1b]133;C;00112233445566778899aabbccddeeff\x07out")
        .unwrap();

    let events = terminal.core.primary.engine.journal.events();
    let mut saw_hi = false;
    let mut saw_boundary = false;
    let mut saw_out = false;
    for event in events {
        match event {
            PrimaryEvent::CellMutation { version, .. } if version.unit_offset == 0 && !saw_hi => {
                saw_hi = true;
                assert!(!saw_boundary, "h must journal before CommandStart");
            }
            PrimaryEvent::ShellBoundaryCandidate {
                marker: MarkerKind::CommandStart,
                token_accepted: true,
                ..
            } => {
                assert!(saw_hi, "CommandStart must follow prior print");
                assert!(!saw_out, "CommandStart must precede post-marker print");
                saw_boundary = true;
            }
            PrimaryEvent::CellMutation { .. } if saw_boundary => {
                saw_out = true;
            }
            _ => {}
        }
    }
    assert!(saw_hi && saw_boundary && saw_out);

    let (_, region_h) = terminal.core.primary.engine.live_cell(0, 0).unwrap();
    let (_, region_o) = terminal.core.primary.engine.live_cell(2, 0).unwrap();
    assert_ne!(
        region_h, region_o,
        "bytes after CommandStart must leave unstructured"
    );
    assert_eq!(
        region_o,
        terminal.core.primary.engine.active_region(),
        "post-marker print joins armed output region"
    );
}

#[test]
fn print_overwrite_mints_unit_version_through_primary_engine() {
    // Binding fixture: C A D \r B → raster B A D; B must mint a new UnitVersion.
    use crate::primary_blocks::attribution::PrimaryEvent;

    let mut terminal = TerminalState::new(8, 2).unwrap();
    terminal.feed(b"CAD\rB").unwrap();
    assert_eq!(terminal.cell(0, 0).unwrap().character, 'B');
    assert_eq!(terminal.cell(1, 0).unwrap().character, 'A');
    assert_eq!(terminal.cell(2, 0).unwrap().character, 'D');

    let (live, region) = terminal
        .core
        .primary
        .engine
        .live_cell(0, 0)
        .expect("window map resolves col0");
    assert_eq!(live.character, 'B');
    assert_eq!(region, terminal.core.primary.engine.unstructured_region());

    let mutations: Vec<_> = terminal
        .core
        .primary
        .engine
        .journal
        .events()
        .iter()
        .filter_map(|event| match event {
            PrimaryEvent::CellMutation { version, .. } => Some(*version),
            _ => None,
        })
        .collect();
    assert!(
        mutations.len() >= 4,
        "CAD + B overwrite must journal lead writes: {}",
        mutations.len()
    );
    let first_col0 = mutations
        .iter()
        .find(|v| v.unit_offset == 0)
        .expect("C write");
    let overwrite_col0 = mutations
        .iter()
        .rev()
        .find(|v| v.unit_offset == 0)
        .expect("B overwrite");
    assert_eq!(first_col0.line_id, overwrite_col0.line_id);
    assert!(overwrite_col0.content_generation > first_col0.content_generation);
}

#[test]
fn scroll_up_preserves_block_region_on_kept_window_rows() {
    use crate::history::HistoryBreakAfter;
    use crate::primary_blocks::types::BlockId;
    use crate::{Cell, LineId, Style};

    let mut terminal = TerminalState::new(4, 3).expect("terminal");
    let block = terminal
        .core
        .primary
        .engine
        .open_block(BlockId(11))
        .unwrap()
        .clone();
    let output = block.output;
    for row in 0..3u16 {
        for col in 0..4u16 {
            let ch = char::from(b'A' + (row * 4 + col) as u8);
            terminal
                .core
                .primary
                .engine
                .write_at(
                    col,
                    row,
                    Cell::lead_inline(ch, 1, Style::default()),
                    output,
                    LineId(u64::from(row) + 1),
                    u64::from(col),
                )
                .unwrap();
        }
    }
    assert_eq!(
        terminal.primary_live_region_cells(output.0).len(),
        12,
        "precondition: all window slots Block-owned"
    );
    terminal
        .core
        .primary
        .scroll_up(
            1,
            &mut terminal.core.line_ids,
            Some(&mut terminal.core.grapheme_store),
            HistoryBreakAfter::HardBreak,
        )
        .expect("scroll up");
    let live = terminal.primary_live_region_cells(output.0);
    assert!(
        !live.is_empty(),
        "kept Block cells must retain RegionId after scroll remint"
    );
    assert!(
        live.iter()
            .any(|(_, row, cell)| *row == 0 && cell.character == 'E'),
        "row 1 content must move to row 0 still owned by the Block: {live:?}"
    );
    assert!(
        live.iter().all(|(_, row, _)| *row < 2),
        "blanked bottom row must not remain Block-owned: {live:?}"
    );
    let spans = terminal.primary_on_window_region_spans();
    let span = spans
        .iter()
        .find(|span| span.block_id.0 == 11)
        .expect("block span");
    assert_eq!(span.first_row, 0);
    assert_eq!(span.row_count, 2);
}

#[test]
fn resize_preserves_block_region_on_reflowed_window_cells() {
    use crate::primary_blocks::types::BlockId;
    use crate::{Cell, LineId, Style};

    let mut terminal = TerminalState::new(8, 3).expect("terminal");
    let block = terminal
        .core
        .primary
        .engine
        .open_block(BlockId(21))
        .unwrap()
        .clone();
    let output = block.output;
    for (col, ch) in [(0u16, 'R'), (1, 'S'), (2, 'T')] {
        terminal
            .core
            .primary
            .engine
            .write_at(
                col,
                0,
                Cell::lead_inline(ch, 1, Style::default()),
                output,
                LineId(1),
                u64::from(col),
            )
            .unwrap();
    }
    assert_eq!(
        terminal.primary_live_region_cells(output.0).len(),
        3,
        "precondition: Block owns three live cells"
    );
    // Column shrink takes the reflow path used when a Pane split changes
    // geometry — RegionId must survive replace_window_grid (#1484).
    terminal.resize(4, 3).expect("resize");
    let live = terminal.primary_live_region_cells(output.0);
    assert!(
        !live.is_empty(),
        "reflowed window cells must keep Block RegionId after resize"
    );
    assert!(
        live.iter().any(|(_, _, cell)| cell.character == 'R'),
        "Block glyphs must remain Region-owned after resize: {live:?}"
    );
}

#[test]
fn sealed_native_cells_resolve_from_history_store_without_spill() {
    use crate::history::{HistoryBreakAfter, HistoryLine};
    use crate::primary_blocks::types::BlockId;
    use crate::{Cell, LineId, Style};

    let mut terminal = TerminalState::new(4, 3).expect("terminal");
    let block = terminal
        .core
        .primary
        .engine
        .open_block(BlockId(4))
        .unwrap()
        .clone();
    let output = block.output.0;
    for (col, ch) in [(0u16, 'H'), (1, 'i')] {
        terminal
            .core
            .primary
            .engine
            .write_at(
                col,
                0,
                Cell::lead_inline(ch, 1, Style::default()),
                block.output,
                LineId(1),
                u64::from(col),
            )
            .unwrap();
    }
    let line_id = terminal.core.primary.line_id(0).unwrap_or(LineId(1));
    terminal
        .core
        .primary
        .engine
        .seal_window_row_indexed(0, line_id)
        .unwrap();
    let row_cells = terminal.core.primary.cell_row(0).expect("row").to_vec();
    let graphemes = crate::grapheme_store::GraphemeStore::default();
    let line = HistoryLine::from_cells(
        line_id,
        HistoryBreakAfter::HardBreak,
        &row_cells,
        &graphemes,
    );
    terminal.core.primary.history_mut().append_line(line);

    assert_eq!(
        terminal.core.primary.engine.store.spill_page_count(),
        0,
        "no spill dual-copy"
    );
    let sealed = terminal.primary_sealed_region_cells(output);
    assert!(
        sealed.iter().any(|(_, _, cell)| cell.character == 'H'),
        "HistoryStore-backed sealed paint must surface owned cells"
    );
    assert_eq!(terminal.primary_sealed_row_count(output), 1);
}

#[test]
fn sealed_region_visitor_is_ordered_and_early_stoppable() {
    use crate::history::{HistoryBreakAfter, HistoryLine};
    use crate::primary_blocks::types::BlockId;
    use crate::{Cell, Style};

    let mut terminal = TerminalState::new(12, 3).expect("terminal");
    let block = terminal
        .core
        .primary
        .engine
        .open_block(BlockId(48))
        .unwrap()
        .clone();
    let output = block.output.0;
    let rows = 2_000u64;
    for i in 1..=rows {
        let line_id = LineId(i);
        let marker = format!("row-{i:05}");
        for (col, ch) in marker.chars().enumerate() {
            terminal
                .core
                .primary
                .engine
                .write_at(
                    col as u16,
                    0,
                    Cell::lead_inline(ch, 1, Style::default()),
                    block.output,
                    line_id,
                    col as u64,
                )
                .unwrap();
        }
        terminal
            .core
            .primary
            .engine
            .seal_window_row_indexed(0, line_id)
            .unwrap();
        let row_cells = terminal.core.primary.cell_row(0).expect("row").to_vec();
        let graphemes = crate::grapheme_store::GraphemeStore::default();
        let line = HistoryLine::from_cells(
            line_id,
            HistoryBreakAfter::HardBreak,
            &row_cells,
            &graphemes,
        );
        terminal.core.primary.history_mut().append_line(line);
    }
    assert_eq!(
        u64::from(terminal.primary_sealed_row_count(output)),
        rows,
        "each sealed row must record Block ownership"
    );
    let mut seen = Vec::new();
    terminal.primary_visit_sealed_region_rows(output, |line_id, owned| {
        let text: String = owned
            .iter()
            .filter(|(_, cell)| cell.role == crate::CellRole::Lead)
            .map(|(_, cell)| cell.character)
            .collect();
        seen.push((line_id.0, text));
        true
    });
    assert_eq!(seen.len() as u64, rows);
    assert_eq!(seen[0].1, "row-00001");
    assert_eq!(seen[rows as usize - 1].1, format!("row-{rows:05}"));
    for window in seen.windows(2) {
        assert!(window[0].0 < window[1].0, "LineIds must increase");
    }
    for (line_id, text) in seen.iter().step_by(250) {
        let lines = terminal
            .primary_history_range(LineId(*line_id), LineId(*line_id), 1)
            .expect("history point lookup");
        let hist: String = lines
            .into_iter()
            .flat_map(|(_, cells)| cells)
            .filter(|cell| cell.role == crate::CellRole::Lead)
            .map(|cell| cell.character)
            .collect();
        assert_eq!(&hist, text, "visitor must match history for {line_id}");
    }
    let mut visits = 0usize;
    terminal.primary_visit_sealed_region_rows(output, |_, _| {
        visits += 1;
        visits < 3
    });
    assert_eq!(visits, 3, "visit false must stop the walk");
}




fn feed_native_block_lines(lines: u32) -> (u128, usize) {
    use crate::primary_blocks::types::BlockId;
    let mut terminal = TerminalState::new(80, 24).expect("terminal");
    let block = terminal
        .core
        .primary
        .engine
        .open_block(BlockId(7))
        .unwrap()
        .clone();
    let output = block.output.0;
    let start = std::time::Instant::now();
    for i in 1..=lines {
        terminal.feed(format!("{i}\r\n").as_bytes()).expect("feed");
        // Simulate Runtime observe drain so the journal never force-ends.
        let _ = terminal.drain_attribution_journal();
    }
    let ms = start.elapsed().as_millis();
    let sealed = terminal.primary_sealed_rows_len(output);
    assert_eq!(terminal.core.primary.engine.active_region(), block.output);
    (ms, sealed)
}

#[test]
fn feed_10k_native_block_finishes_and_seals() {
    let (ms, sealed) = feed_native_block_lines(10_000);
    eprintln!("feed_10k_drained_ms={ms} sealed={sealed}");
    assert!(ms < 10_000, "10k with drain must finish quickly: {ms}ms");
    assert!(sealed >= 9_900, "sealed={sealed}");
}

#[test]
fn feed_100k_native_block_finishes_and_seals() {
    let (ms, sealed) = feed_native_block_lines(100_000);
    eprintln!("feed_100k_drained_ms={ms} sealed={sealed}");
    assert!(ms < 55_000, "100k with drain must finish under UI budget: {ms}ms");
    assert!(sealed >= 99_000, "sealed={sealed}");
}

#[test]
fn feed_1m_native_block_finishes_and_seals() {
    let (ms, sealed) = feed_native_block_lines(1_000_000);
    eprintln!("feed_1m_drained_ms={ms} sealed={sealed}");
    assert!(ms < 600_000, "1M with drain must finish under soak budget: {ms}ms");
    assert!(sealed >= 990_000, "sealed={sealed}");
}

