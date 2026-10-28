use seyal_terminal::{
    CellRole, Color, HistoryAnchor, HistoryAnchorResolution, HistoryBreakAfter, HistoryRangeError,
    HistoryUnitView, LineId, TerminalState, HISTORY_PER_EXECUTION_BYTE_CAP,
};

#[test]
fn search_does_not_cross_hard_break_separator() {
    let mut terminal = TerminalState::new(4, 1).expect("terminal");
    terminal.feed(b"ab\r\ncd\r\n").expect("feed");
    assert!(terminal.primary_history_search("bc", 8).is_empty());
}

#[test]
fn canonical_search_keeps_multiscalar_grapheme_units_atomic() {
    let mut terminal = TerminalState::new(8, 1).expect("terminal");
    terminal.feed("x e\u{301} y\r\n".as_bytes()).expect("feed");
    let matches = terminal.primary_history_search("e\u{301}", 1);
    assert_eq!(matches.len(), 1);
    assert_eq!(
        terminal.primary_history_unit(matches[0].start),
        HistoryAnchorResolution::Resolved {
            text: "e\u{301}".to_owned(),
            width: 1,
            style: Default::default()
        }
    );
}

#[test]
fn primary_history_reflow_keeps_multiscalar_grapheme_payload() {
    let mut terminal = TerminalState::new(8, 1).expect("terminal");
    terminal
        .feed("e\u{301} 👩\u{200d}💻\r\n".as_bytes())
        .expect("feed");
    let rows = terminal.primary_history_reflow(8, 8);
    let leads: Vec<&str> = rows
        .iter()
        .flat_map(|row| {
            row.cells
                .iter()
                .filter(|cell| !cell.continuation)
                .map(|cell| cell.text.as_str())
        })
        .collect();
    assert!(
        leads.contains(&"e\u{301}"),
        "reflow truncated combining grapheme: {leads:?}"
    );
    assert!(
        leads.contains(&"👩\u{200d}💻"),
        "reflow truncated ZWJ grapheme: {leads:?}"
    );
}

#[test]
fn canonical_search_matches_across_soft_wrap_and_returns_source_anchors() {
    let mut terminal = TerminalState::new(4, 1).expect("terminal");
    terminal.feed(b"abcdefgh\r\n").expect("feed");

    let matches = terminal.primary_history_search("de", 1);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].start.unit_offset, 3);
    assert_eq!(matches[0].end.unit_offset, 0);
    assert_ne!(matches[0].start.line_id, matches[0].end.line_id);
}

#[test]
fn canonical_selection_resolves_source_units_after_reflow() {
    let mut terminal = TerminalState::new(4, 1).expect("terminal");
    terminal.feed(b"abcdef").expect("feed");
    terminal.feed(b"\r\n").expect("scroll source into history");
    let source = terminal.primary_history_units_range(LineId(1), LineId(u64::MAX), 8);
    let start = HistoryAnchor {
        line_id: source[1].anchor.line_id,
        unit_offset: source[1].anchor.unit_offset,
    };
    let end = HistoryAnchor {
        line_id: source[2].anchor.line_id,
        unit_offset: source[2].anchor.unit_offset,
    };
    terminal.resize(8, 1).expect("reflow");
    let selected = terminal
        .primary_history_selection(start, end)
        .expect("selection");
    assert_eq!(
        selected
            .iter()
            .map(|unit| unit.text.as_str())
            .collect::<String>(),
        "bc"
    );
}

#[test]
fn reflow_cache_matches_rebuild_from_canonical_history() {
    let mut terminal = TerminalState::new(4, 2).expect("terminal");
    terminal.feed(b"abcdefgh\r\nijkl\r\n").expect("feed");
    let cached = terminal.primary_history_reflow(3, 32);
    let rebuilt = terminal.primary_history_reflow_uncached(3, 32);
    assert_eq!(cached, rebuilt);
}

#[test]
fn reflow_at_one_column_never_emits_an_orphan_wide_unit() {
    let mut terminal = TerminalState::new(2, 1).expect("terminal");
    terminal.feed("界\r\n".as_bytes()).expect("feed");

    let rows = terminal.primary_history_reflow(1, 8);
    assert!(rows.iter().all(|row| row.cells.len() <= 1));
    assert!(rows.iter().all(|row| {
        row.cells
            .first()
            .is_none_or(|cell| cell.continuation || cell.width < 2)
    }));
}

#[test]
fn narrowing_active_wide_unit_to_one_column_preserves_source_without_an_orphan() {
    let mut terminal = TerminalState::new(2, 1).expect("terminal");
    let source_id = terminal.line_id(0).expect("source line");
    terminal.feed("界\u{301}".as_bytes()).expect("feed");

    terminal.resize(1, 1).expect("resize");

    let visible = terminal.cell(0, 0).expect("cell");
    assert_ne!(visible.role, CellRole::Lead);
    assert!(terminal
        .primary_history_units_range(source_id, source_id, 8)
        .iter()
        .any(|unit| unit.text == "界\u{301}" && unit.width == 2));
}

#[test]
fn shrinking_rows_preserves_every_active_source_line() {
    let mut terminal = TerminalState::new(4, 4).expect("terminal");
    terminal.feed(b"a\r\nb\r\nc\r\nd").expect("feed");
    let source_ids = (0..4)
        .map(|row| terminal.line_id(row).expect("line id"))
        .collect::<Vec<_>>();

    terminal.resize(4, 2).expect("resize");

    for (line_id, expected) in source_ids.into_iter().zip(["a", "b", "c", "d"]) {
        let text = terminal
            .primary_history_units_range(line_id, line_id, 8)
            .into_iter()
            .map(|unit| unit.text)
            .collect::<String>();
        assert_eq!(text, expected, "source line {line_id:?} was lost");
    }
}

#[test]
fn resize_does_not_join_rows_separated_by_cursor_motion() {
    let mut terminal = TerminalState::new(4, 3).expect("terminal");
    terminal.feed(b"a\x1b[3;1Hb").expect("feed");
    let first = terminal.line_id(0).expect("first line");
    let third = terminal.line_id(2).expect("third line");

    terminal.resize(8, 3).expect("resize");

    assert_eq!(terminal.row_text(0).as_deref(), Some("a       "));
    assert_eq!(terminal.row_text(2).as_deref(), Some("b       "));
    assert_eq!(terminal.line_id(0), Some(first));
    assert_eq!(terminal.line_id(2), Some(third));
}

#[test]
fn resize_oscillation_preserves_multiscalar_payload_and_source_anchor() {
    let mut terminal = TerminalState::new(8, 2).expect("terminal");
    terminal
        .feed("ab界\u{301}cd".as_bytes())
        .expect("unicode feed");
    let source_id = terminal.line_id(0).expect("source line");

    for cols in [4, 8, 3, 8, 1, 8] {
        terminal.resize(cols, 2).expect("resize");
        let units = terminal.primary_history_units_range(source_id, source_id, 32);
        assert_eq!(
            units
                .iter()
                .map(|unit| unit.text.as_str())
                .collect::<String>(),
            "ab界\u{301}cd",
            "canonical payload changed at {cols} columns"
        );
        assert_eq!(
            units.first().map(|unit| unit.anchor.line_id),
            Some(source_id)
        );
    }
}

#[test]
fn output_and_scroll_after_resize_preserve_source_ids_offsets_lineage_and_style() {
    let mut terminal = TerminalState::new(4, 2).expect("terminal");
    terminal
        .feed(b"\x1b[31mabcdef")
        .expect("soft-wrapped styled input");
    let first = terminal.line_id(0).expect("first source id");
    let second = terminal.line_id(1).expect("second source id");

    terminal.resize(8, 2).expect("widen");
    terminal.feed(b"g\r\nh\r\n").expect("output after resize");

    let first_units = terminal.primary_history_units_range(first, first, 16);
    assert_eq!(
        first_units
            .iter()
            .map(|unit| unit.text.as_str())
            .collect::<String>(),
        "abcd"
    );
    assert_eq!(
        first_units
            .iter()
            .map(|unit| unit.anchor.unit_offset)
            .collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
    assert!(first_units
        .iter()
        .all(|unit| unit.style.fg == Color::Indexed(1)));

    let second_units = terminal.primary_history_units_range(second, second, 16);
    assert_eq!(
        second_units
            .iter()
            .map(|unit| unit.text.as_str())
            .collect::<String>(),
        "efg"
    );
    assert_eq!(
        second_units
            .iter()
            .map(|unit| unit.anchor.unit_offset)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert!(second_units
        .iter()
        .all(|unit| unit.style.fg == Color::Indexed(1)));
}

#[test]
fn evicted_range_and_anchor_are_explicitly_unavailable() {
    let mut terminal = TerminalState::new(512, 1).expect("terminal");
    let line = format!("{}\r\n", "a".repeat(512));
    for _ in 0..128 {
        terminal.feed(line.as_bytes()).expect("history feed");
    }
    let anchor = HistoryAnchor {
        line_id: LineId(1),
        unit_offset: 0,
    };
    assert!(matches!(
        terminal.primary_history_unit(anchor),
        HistoryAnchorResolution::Resolved { .. }
    ));

    assert!(terminal.evict_oldest_primary_history_segment() > 0);

    assert_eq!(
        terminal.primary_history_unit(anchor),
        HistoryAnchorResolution::Unavailable
    );
    assert_eq!(
        terminal.primary_history_range(LineId(1), LineId(1), 8),
        Err(HistoryRangeError::Stale)
    );
    assert_eq!(
        terminal.primary_history_unit(HistoryAnchor {
            line_id: LineId(u64::MAX),
            unit_offset: 0,
        }),
        HistoryAnchorResolution::Invalid
    );
}

#[test]
fn never_allocated_line_id_is_invalid_after_history_eviction() {
    let mut terminal = TerminalState::new(512, 1).expect("terminal");
    let line = format!("{}\r\n", "a".repeat(512));
    for _ in 0..128 {
        terminal.feed(line.as_bytes()).expect("history feed");
    }
    assert!(terminal.evict_oldest_primary_history_segment() > 0);
    assert_eq!(
        terminal.primary_history_unit(HistoryAnchor {
            line_id: LineId(0),
            unit_offset: 0,
        }),
        HistoryAnchorResolution::Invalid
    );
}

#[test]
fn alternate_screen_line_id_gap_is_invalid_after_primary_history_eviction() {
    let mut terminal = TerminalState::new(512, 1).expect("terminal");
    let line = format!("{}\r\n", "a".repeat(512));
    for _ in 0..128 {
        terminal.feed(line.as_bytes()).expect("primary history");
    }
    let before_max = terminal
        .primary_history_range(LineId(1), LineId(u64::MAX), 512)
        .expect("history range")
        .into_iter()
        .map(|(id, _)| id)
        .max()
        .expect("primary ids");

    terminal.feed(b"\x1b[?1049h").expect("enter alternate");
    for _ in 0..128 {
        terminal.feed(line.as_bytes()).expect("alternate output");
    }
    terminal.feed(b"\x1b[?1049l").expect("leave alternate");
    for _ in 0..128 {
        terminal
            .feed(line.as_bytes())
            .expect("more primary history");
    }

    let gap = LineId(before_max.0 + 1);
    let primary_ids = terminal
        .primary_history_range(LineId(1), LineId(u64::MAX), 512)
        .expect("history range")
        .into_iter()
        .map(|(id, _)| id)
        .collect::<Vec<_>>();
    assert!(!primary_ids.contains(&gap));
    while terminal.evict_oldest_primary_history_segment() > 0 {}

    assert_eq!(
        terminal.primary_history_unit(HistoryAnchor {
            line_id: gap,
            unit_offset: 0,
        }),
        HistoryAnchorResolution::Invalid
    );
}

#[test]
fn hostile_unicode_resize_and_eviction_sequence_preserves_canonical_units() {
    let mut one_shot = TerminalState::new(13, 3).expect("one-shot terminal");
    let mut chunked = TerminalState::new(13, 3).expect("chunked terminal");
    let mut input = String::new();
    for index in 0..96 {
        input.push_str(if index % 2 == 0 {
            "\x1b[31me\u{301}界👩\u{200d}💻🙂"
        } else {
            "\x1b[34m🏳️\u{200d}🌈Z\u{308}語"
        });
        input.push_str("\x1b[0m\r\n");
    }
    one_shot.feed(input.as_bytes()).expect("one-shot feed");
    for chunk in input.as_bytes().chunks(3) {
        chunked.feed(chunk).expect("chunked feed");
    }

    for (step, cols) in [1, 2, 5, 17, 3, 13, 40, 1, 13].into_iter().enumerate() {
        let rows = 1 + (step % 5) as u16;
        one_shot.resize(cols, rows).expect("one-shot resize");
        chunked.resize(cols, rows).expect("chunked resize");
        let suffix = format!("\x1b[3{}mstep-{step}-界\u{301}\r\n", step % 8);
        one_shot.feed(suffix.as_bytes()).expect("one-shot suffix");
        for chunk in suffix.as_bytes().chunks(2) {
            chunked.feed(chunk).expect("chunked suffix");
        }

        assert_eq!(
            one_shot.primary_history_units_range(LineId(1), LineId(u64::MAX), 32_768),
            chunked.primary_history_units_range(LineId(1), LineId(u64::MAX), 32_768),
            "canonical history diverged after resize step {step}"
        );
        for terminal in [&one_shot, &chunked] {
            for row in 0..terminal.rows() {
                for col in 0..terminal.cols() {
                    let Some(cell) = terminal.cell(col, row) else {
                        continue;
                    };
                    if cell.role == CellRole::Lead && cell.width == 2 {
                        assert!(
                            col + 1 < terminal.cols(),
                            "orphan lead at resize step {step}, row {row}, col {col}, cols {cols}"
                        );
                        assert!(terminal
                            .cell(col + 1, row)
                            .is_some_and(|next| next.role == CellRole::Continuation));
                    }
                }
            }
        }
    }

    for _ in 0..8 {
        let one_removed = one_shot.evict_oldest_primary_history_segment();
        let chunked_removed = chunked.evict_oldest_primary_history_segment();
        assert_eq!(one_removed, chunked_removed);
        assert_eq!(
            one_shot.primary_history_units_range(LineId(1), LineId(u64::MAX), 32_768),
            chunked.primary_history_units_range(LineId(1), LineId(u64::MAX), 32_768)
        );
        if one_removed == 0 {
            break;
        }
    }
    assert!(one_shot.primary_history_resident_bytes() <= HISTORY_PER_EXECUTION_BYTE_CAP);
    assert!(chunked.primary_history_resident_bytes() <= HISTORY_PER_EXECUTION_BYTE_CAP);
}

#[test]
fn narrowing_alternate_screen_discards_a_wide_unit_atomically() {
    let mut terminal = TerminalState::new(21, 2).expect("terminal");
    terminal.feed(b"\x1b[?1049h\x1b[20G").expect("position");
    terminal.feed("界".as_bytes()).expect("wide unit");
    assert_eq!(terminal.cell(19, 0).map(|cell| cell.width), Some(2));
    assert!(terminal
        .cell(20, 0)
        .is_some_and(|cell| cell.role == CellRole::Continuation));

    terminal.resize(20, 2).expect("narrow");

    assert!(terminal
        .cell(19, 0)
        .is_some_and(|cell| cell.role != CellRole::Lead));
}

#[test]
fn primary_history_range_retains_combining_grapheme_rows() {
    let mut terminal = TerminalState::new(8, 1).expect("terminal");
    terminal
        .feed("e\u{301}x\r\n".as_bytes())
        .expect("combining history");
    let rows = terminal
        .primary_history_range(LineId(1), LineId(u64::MAX), 8)
        .expect("representable");
    assert!(!rows.is_empty());
    let units = terminal.primary_history_units_range(LineId(1), LineId(u64::MAX), 8);
    assert!(units.iter().any(|unit| unit.text == "e\u{301}"));
    let wire = terminal
        .primary_history_wire_range(LineId(1), LineId(u64::MAX), 8, 0)
        .expect("wire");
    assert!(wire
        .iter()
        .flat_map(|(_, cells)| cells)
        .any(|cell| cell.text == "e\u{301}"));
}

#[test]
fn history_wire_skip_leads_returns_the_unconsumed_suffix() {
    let mut terminal = TerminalState::new(8, 1).expect("terminal");
    terminal.feed(b"abcdef\r\n").expect("feed");
    let all = terminal
        .primary_history_wire_range(LineId(1), LineId(u64::MAX), 8, 0)
        .expect("all");
    let skipped = terminal
        .primary_history_wire_range(LineId(1), LineId(u64::MAX), 8, 2)
        .expect("skip 2");
    let all_leads: Vec<&str> = all
        .iter()
        .flat_map(|(_, cells)| cells.iter().filter(|cell| !cell.continuation))
        .map(|cell| cell.text.as_str())
        .collect();
    let skipped_leads: Vec<&str> = skipped
        .iter()
        .flat_map(|(_, cells)| cells.iter().filter(|cell| !cell.continuation))
        .map(|cell| cell.text.as_str())
        .collect();
    assert!(all_leads.len() > 2, "need a prefix to skip: {all_leads:?}");
    assert_eq!(skipped_leads, all_leads[2..]);
}

#[test]
fn resize_reflows_soft_wrap_across_retained_and_active_boundary() {
    let mut terminal = TerminalState::new(4, 2).expect("terminal");
    terminal
        .feed(b"abcdefghij")
        .expect("soft wrap into history");
    let before = terminal
        .primary_history_units_range(LineId(1), LineId(u64::MAX), 64)
        .iter()
        .map(|unit| unit.text.as_str())
        .collect::<String>();
    assert_eq!(before, "abcdefghij");
    terminal.resize(10, 2).expect("widen across boundary");
    let after = terminal
        .primary_history_units_range(LineId(1), LineId(u64::MAX), 64)
        .iter()
        .map(|unit| unit.text.as_str())
        .collect::<String>();
    assert_eq!(
        after, "abcdefghij",
        "resize duplicated or dropped source text"
    );
    terminal.resize(3, 2).expect("narrow");
    terminal.resize(8, 2).expect("restore");
    let restored = terminal
        .primary_history_units_range(LineId(1), LineId(u64::MAX), 64)
        .iter()
        .map(|unit| unit.text.as_str())
        .collect::<String>();
    assert_eq!(restored, "abcdefghij");
}

#[test]
fn resize_keeps_early_hard_broken_history_without_rewriting_it() {
    let mut terminal = TerminalState::new(8, 2).expect("terminal");
    terminal.feed(b"early\r\n").expect("first line");
    for i in 0..400 {
        let row = format!("row{i:03}\r\n");
        terminal.feed(row.as_bytes()).expect("fill");
    }
    terminal.resize(12, 2).expect("widen");
    terminal.resize(6, 2).expect("narrow");
    let matches = terminal.primary_history_search("early", 1);
    assert_eq!(matches.len(), 1);
}

/// SPEC-010 `hist-resize-oscillation`: exact-width and width-oscillation reflow
/// at 40/48/64/80/96/132/160 columns must preserve canonical units, `LineId`s,
/// hard/soft lineage, and source anchors.
#[test]
fn hist_resize_oscillation_at_spec_column_ladder() {
    const WIDTHS: [u16; 7] = [40, 48, 64, 80, 96, 132, 160];
    let mut terminal = TerminalState::new(40, 2).expect("terminal");
    terminal.feed(b"HARD-A\r\nHARD-B\r\n").expect("hard breaks");

    let mut soft = String::from("ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789xxxx");
    soft.push('界');
    soft.push('😀');
    soft.push_str("e\u{301}");
    soft.push_str("YYYYYYYYYY");
    terminal
        .feed(soft.as_bytes())
        .expect("soft-wrapped payload");
    terminal
        .feed(b"\r\n\r\n\r\n\r\n")
        .expect("scroll the soft-wrapped chain fully into retained history");

    let expected_units = terminal.primary_history_units_range(LineId(1), LineId(u64::MAX), 4_096);
    assert!(
        expected_units
            .iter()
            .any(|unit| unit.text == "H" && unit.anchor.unit_offset == 0),
        "hard-broken prefix must be retained before oscillation"
    );
    let expected_text = units_text(&expected_units);
    assert!(
        expected_text.contains("HARD-AHARD-B")
            && expected_text.contains("界")
            && expected_text.contains("😀"),
        "canonical snapshot missing hard prefix or wide units: {expected_text}"
    );
    let expected_anchors = expected_units
        .iter()
        .map(|unit| (unit.anchor, unit.text.clone(), unit.width))
        .collect::<Vec<_>>();
    let mut prefix = String::new();
    let soft_start = expected_units
        .iter()
        .position(|unit| {
            if prefix == "HARD-AHARD-B" {
                true
            } else {
                prefix.push_str(&unit.text);
                false
            }
        })
        .expect("soft-wrapped payload follows hard-broken prefix");
    let soft_line_ids = expected_units[soft_start..]
        .iter()
        .map(|unit| unit.anchor.line_id)
        .collect::<std::collections::HashSet<_>>();

    let mut ladder = Vec::from(WIDTHS);
    ladder.extend(WIDTHS.iter().copied().rev());
    ladder.extend(WIDTHS);

    for cols in ladder {
        terminal.resize(cols, 2).expect("oscillation resize");
        let units = terminal.primary_history_units_range(LineId(1), LineId(u64::MAX), 4_096);
        assert_eq!(
            units_text(&units),
            expected_text,
            "canonical payload changed at {cols} columns"
        );
        assert_eq!(
            units
                .iter()
                .map(|unit| (unit.anchor, unit.text.as_str(), unit.width))
                .collect::<Vec<_>>(),
            expected_anchors
                .iter()
                .map(|(anchor, text, width)| (*anchor, text.as_str(), *width))
                .collect::<Vec<_>>(),
            "LineId/offset/width anchors changed at {cols} columns"
        );
        assert!(
            units.iter().all(|unit| !unit.text.is_empty()),
            "wide/grapheme unit split into an empty independent half at {cols} columns"
        );

        let rows = terminal.primary_history_reflow(cols, 256);
        let hard_a = rows.iter().find(|row| row_text(row).starts_with("HARD-A"));
        let hard_b = rows.iter().find(|row| row_text(row).starts_with("HARD-B"));
        let hard_a = hard_a.expect("HARD-A visual row");
        let hard_b = hard_b.expect("HARD-B visual row");
        assert_eq!(hard_a.break_after, Some(HistoryBreakAfter::HardBreak));
        assert_eq!(hard_b.break_after, Some(HistoryBreakAfter::HardBreak));
        assert_ne!(
            hard_a.source_line_id, hard_b.source_line_id,
            "hard newline must remain a source boundary at {cols} columns"
        );
        assert!(
            !row_text(hard_a).contains("HARD-B") && !row_text(hard_b).contains("HARD-A"),
            "hard-broken rows joined at {cols} columns"
        );

        let soft_rows = rows
            .iter()
            .filter(|row| {
                row.source_line_id
                    .is_some_and(|id| soft_line_ids.contains(&id))
                    || row
                        .anchors
                        .iter()
                        .any(|anchor| soft_line_ids.contains(&anchor.line_id))
            })
            .collect::<Vec<_>>();
        assert!(
            !soft_rows.is_empty(),
            "soft-wrapped payload missing from reflow at {cols} columns"
        );
        if cols >= 64 {
            assert_eq!(
                soft_rows.len(),
                1,
                "soft wraps must rejoin at {cols} columns, got {:?}",
                soft_rows
                    .iter()
                    .map(|row| row_text(row))
                    .collect::<Vec<_>>()
            );
            assert_eq!(soft_rows[0].break_after, Some(HistoryBreakAfter::HardBreak));
        } else {
            assert!(
                soft_rows.len() > 1,
                "soft-wrapped payload should occupy multiple visual rows at {cols} columns, got {:?}",
                soft_rows.iter().map(|row| row_text(row)).collect::<Vec<_>>()
            );
            assert!(
                soft_rows
                    .iter()
                    .any(|row| row.break_after == Some(HistoryBreakAfter::SoftWrap)),
                "autowrapped visual rows lost SoftWrap lineage at {cols} columns"
            );
            assert_eq!(
                soft_rows.last().and_then(|row| row.break_after),
                Some(HistoryBreakAfter::HardBreak)
            );
        }
    }
}

#[test]
fn empty_grid_column_oscillation_stays_bounded_and_cheap() {
    let mut terminal = TerminalState::new(120, 40).expect("terminal");
    let started = std::time::Instant::now();
    for i in 0..120 {
        let cols = if i % 2 == 0 { 121 } else { 120 };
        terminal.resize(cols, 40).expect("oscillation resize");
    }
    let elapsed = started.elapsed();
    let resident = terminal.primary_history_resident_bytes();
    let visual = terminal.primary_history_reflow(120, 256);
    assert!(
        resident <= 64 * 1024,
        "empty 120x40 oscillation retained {resident} history bytes"
    );
    assert!(
        visual.len() <= 8,
        "empty column oscillation leaked {} blank history rows",
        visual.len()
    );
    assert!(
        elapsed.as_millis() < 1_000,
        "120 empty-grid 120/121 column resizes took {elapsed:?} (pass7 CI budget cannot absorb ~100ms/sample)"
    );
}

fn units_text(units: &[HistoryUnitView]) -> String {
    units.iter().map(|unit| unit.text.as_str()).collect()
}

fn row_text(row: &seyal_terminal::ReflowRow) -> String {
    row.cells
        .iter()
        .filter(|cell| !cell.continuation)
        .map(|cell| cell.text.as_str())
        .collect()
}

/// SPEC-010 §17 `hist-hard-soft-lineage`: LF/VT/FF are HardBreak; CR alone is not.
#[test]
fn hist_hard_soft_lineage_cr_alone_vs_lf_vt_ff() {
    let mut terminal = TerminalState::new(4, 2).expect("terminal");
    // SoftWrap chain then LF HardBreak.
    terminal.feed(b"abcd\n").expect("soft+lf");
    // VT and FF hard breaks.
    terminal.feed(b"B\x0bC\x0c").expect("vt/ff");
    // CR alone overwrites; LF then seals.
    terminal.feed(b"XY\rZ\n").expect("cr alone");

    let units = terminal.primary_history_units_range(LineId(1), LineId(u64::MAX), 64);
    assert!(
        units
            .iter()
            .any(|unit| unit.break_after == HistoryBreakAfter::SoftWrap),
        "autowrap must record SoftWrap lineage"
    );
    assert!(
        units
            .iter()
            .any(|unit| unit.break_after == HistoryBreakAfter::HardBreak),
        "LF/VT/FF must record HardBreak lineage"
    );
    // Search must not cross HardBreak separators invented by LF.
    assert!(terminal.primary_history_search("dB", 8).is_empty());
    // CR alone should overwrite rather than invent an extra HardBreak between X and Z.
    let text = units_text(&units);
    assert!(
        text.contains('Z'),
        "CR overwrite should leave Z in retained text, got {text:?}"
    );
}

/// SPEC-010 §17 `hist-search-soft-span`: matches stay width-independent across resize.
#[test]
fn hist_search_soft_span_is_width_independent() {
    let mut terminal = TerminalState::new(4, 1).expect("terminal");
    terminal.feed(b"abcdefgh\r\n").expect("soft wrap history");
    let before = terminal.primary_history_search("de", 1);
    assert_eq!(before.len(), 1);
    let start = before[0].start;
    let end = before[0].end;
    for cols in [8u16, 6, 40, 4] {
        terminal.resize(cols, 1).expect("resize");
        let after = terminal.primary_history_search("de", 1);
        assert_eq!(after.len(), 1, "soft-span search must survive width {cols}");
        assert_eq!(after[0].start, start);
        assert_eq!(after[0].end, end);
    }
}

/// SPEC-010 §17 `hist-cache-rebuild-equiv`: drop derived caches then rebuild.
#[test]
fn hist_cache_drop_rebuild_equals_prior_projection() {
    let mut terminal = TerminalState::new(4, 2).expect("terminal");
    terminal.feed(b"abcdefgh\r\nijkl\r\n").expect("feed");
    let prior = terminal.primary_history_reflow(3, 32);
    terminal.drop_primary_history_derived_cache();
    let rebuilt = terminal.primary_history_reflow(3, 32);
    assert_eq!(prior, rebuilt);
    let uncached = terminal.primary_history_reflow_uncached(3, 32);
    assert_eq!(rebuilt, uncached);
}

/// SPEC-010 §17 `hist-wide-grapheme`: overflow sentinel retained in history.
#[test]
fn hist_wide_grapheme_overflow_sentinel_is_retained() {
    let mut terminal = TerminalState::new(40, 2).expect("terminal");
    let mut payload = String::from("a");
    for _ in 0..4096 {
        payload.push('\u{0301}');
    }
    assert!(payload.len() > 8192);
    terminal
        .feed(payload.as_bytes())
        .expect("overflow grapheme");
    assert!(terminal.diagnostics().grapheme_payload_overflow_count >= 1);
    terminal.feed(b"\r\n").expect("retain");
    let units = terminal.primary_history_units_range(LineId(1), LineId(u64::MAX), 8);
    assert!(
        units.iter().any(|unit| unit.text == "\u{FFFD}"),
        "overflow sentinel must be retained in canonical history, got {:?}",
        units_text(&units)
    );
}

/// Gate 1 / #1379: completed Block history spans are LineId ranges that often
/// still sit on the live primary grid. CSI ED2 (`clear`) must seal those rows
/// and mint fresh LineIds so a later rewrite cannot contaminate history queries
/// used by Flow Block bodies.
#[test]
fn ed2_clear_preserves_prior_primary_lines_in_history() {
    let mut terminal = TerminalState::new(20, 4).expect("terminal");
    terminal.feed(b"block-one\r\n").expect("feed");
    terminal.feed(b"block-two-output\r\n").expect("feed");
    let first = terminal.line_id(0).expect("line 0");
    let second = terminal.line_id(1).expect("line 1");
    let before_first: String = terminal
        .primary_history_units_range(first, first, 32)
        .into_iter()
        .map(|u| u.text)
        .collect();
    let before_second: String = terminal
        .primary_history_units_range(second, second, 32)
        .into_iter()
        .map(|u| u.text)
        .collect();
    assert_eq!(before_first, "block-one");
    assert_eq!(before_second, "block-two-output");

    terminal.feed(b"\x1b[H\x1b[2J").expect("clear");
    terminal
        .feed(b"RED GREEN\r\n")
        .expect("rewrite after clear");
    terminal.feed(b"sleep-bleed\r\n").expect("more raw");

    let after_first: String = terminal
        .primary_history_units_range(first, first, 32)
        .into_iter()
        .map(|u| u.text)
        .collect();
    let after_second: String = terminal
        .primary_history_units_range(second, second, 32)
        .into_iter()
        .map(|u| u.text)
        .collect();
    assert_eq!(
        after_first, "block-one",
        "ED2 must not let later primary text replace Block line {first:?}: got {after_first:?}"
    );
    assert_eq!(
        after_second, "block-two-output",
        "ED2 must not let later primary text replace Block line {second:?}: got {after_second:?}"
    );
    assert_ne!(
        terminal.line_id(0).expect("fresh row 0"),
        first,
        "ED2 must mint a fresh LineId for row 0"
    );
}

/// #1379: a Block that starts on the primary, runs ED2, then finishes on a
/// reminted row must not own the numeric span through every reminted blank.
#[test]
fn ed2_mid_command_block_end_clamps_before_remint() {
    let mut terminal = TerminalState::new(20, 6).expect("terminal");
    terminal.feed(b"before-clear\r\n").expect("feed");
    // Simulate the clear command's start row still on the primary.
    terminal.feed(b"clear-command").expect("feed");
    let start = terminal
        .line_id(terminal.cursor().row)
        .expect("clear start line");
    terminal.feed(b"\x1b[H\x1b[2J").expect("ED2");
    let finish = terminal.line_id(0).expect("reminted finish line");
    assert!(
        finish.0 > start.0 + 1,
        "ED2 must remint multiple LineIds after start ({start:?} → {finish:?})"
    );
    let clamped = terminal.clamp_block_end_across_ed2_remint(start, finish);
    assert_eq!(
        clamped, start,
        "ED2 remint finish must clamp end to the sealed start row, not {finish:?}"
    );
    assert!(
        finish.0 - start.0 >= 5,
        "unclamped numeric span must be remint-tall ({start:?}..{finish:?})"
    );
}
