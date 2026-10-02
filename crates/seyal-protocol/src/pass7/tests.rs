//! Pass 7 composer/block/history wire encode-decode tests.

use super::command_blocks::MAX_COMMAND_BLOCK_RECORDS;
use super::composer::MAX_COMPOSER_COMMAND_BYTES;
use super::*;
use crate::AttachmentId;

fn attachment() -> AttachmentId {
    AttachmentId::from_bytes(7u128.to_le_bytes())
}

#[test]
fn composer_command_requires_exact_bounded_utf8_payload() {
    let request = ComposerCommandRef {
        attachment_id: attachment(),
        request_id: 3,
        command: "printf hello",
    };
    assert_eq!(ComposerCommandRef::decode(&request.encode()), Ok(request));
    let mut malformed = request.encode();
    malformed[24..28].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        ComposerCommandRef::decode(&malformed),
        Err(FramingError::MalformedPayload)
    );
}

#[test]
fn timeline_round_trip_preserves_only_metadata_and_anchors() {
    let timeline = BlockTimeline {
        revision: 9,
        records: vec![
            CommandBlock {
                id: 1,
                command: "printf one".into(),
                start_line: 31,
                end_line: None,
                state: CommandBlockState::Running,
            },
            CommandBlock {
                id: 2,
                command: "false".into(),
                start_line: 34,
                end_line: Some(36),
                state: CommandBlockState::Completed {
                    exit_status: Some(1),
                },
            },
        ],
    };
    assert_eq!(BlockTimeline::decode(&timeline.encode()), Ok(timeline));
}

#[test]
fn timeline_state_tag_2_round_trips_completed_without_exit_status() {
    let timeline = BlockTimeline {
        revision: 11,
        records: vec![CommandBlock {
            id: 3,
            command: "sleep 1".into(),
            start_line: 40,
            end_line: Some(42),
            state: CommandBlockState::Completed { exit_status: None },
        }],
    };
    let encoded = timeline.encode();
    // Fixed record header: id(8)+start(8)+end(8)+tag(1)+pad(3)+exit(4)+cmd_len(2)+rsv(2).
    let record = &encoded[16..];
    assert_eq!(
        record[24], 2,
        "Completed {{ exit_status: None }} encodes as tag 2"
    );
    assert_eq!(
        i32::from_le_bytes(record[28..32].try_into().unwrap()),
        0,
        "tag-2 wire exit field stays zero"
    );
    assert_eq!(BlockTimeline::decode(&encoded), Ok(timeline));
}

#[test]
fn timeline_state_tag_2_rejects_nonzero_exit_status_and_inverted_end_line() {
    let timeline = BlockTimeline {
        revision: 12,
        records: vec![CommandBlock {
            id: 4,
            command: "echo".into(),
            start_line: 50,
            end_line: Some(52),
            state: CommandBlockState::Completed { exit_status: None },
        }],
    };
    let good = timeline.encode();
    let record_offset = 16;

    let mut nonzero_exit = good.clone();
    nonzero_exit[record_offset + 28..record_offset + 32].copy_from_slice(&1i32.to_le_bytes());
    assert_eq!(
        BlockTimeline::decode(&nonzero_exit),
        Err(FramingError::MalformedPayload),
        "tag-2 with non-zero exit_status is malformed"
    );

    let mut inverted_end = good;
    // end_raw at record+16; start_line is 50, so end 49 is inverted.
    inverted_end[record_offset + 16..record_offset + 24].copy_from_slice(&49u64.to_le_bytes());
    assert_eq!(
        BlockTimeline::decode(&inverted_end),
        Err(FramingError::MalformedPayload),
        "tag-2 with end_raw < start_line is malformed"
    );
}

#[test]
fn timeline_try_encode_rejects_unbounded_record_count() {
    let record = CommandBlock {
        id: 1,
        command: "echo".into(),
        start_line: 1,
        end_line: None,
        state: CommandBlockState::Running,
    };
    let timeline = BlockTimeline {
        revision: 1,
        records: vec![record; MAX_COMMAND_BLOCK_RECORDS + 1],
    };
    assert_eq!(timeline.try_encode(), Err(FramingError::OversizedPayload));
}

#[test]
fn timeline_try_encode_rejects_cumulative_command_bytes_over_frame() {
    let large = "x".repeat(MAX_COMPOSER_COMMAND_BYTES);
    let timeline = BlockTimeline {
        revision: 1,
        records: (1..=16)
            .map(|id| CommandBlock {
                id,
                command: large.clone(),
                start_line: id,
                end_line: Some(id + 1),
                state: CommandBlockState::Completed {
                    exit_status: Some(0),
                },
            })
            .collect(),
    };
    assert!(timeline.records.len() <= MAX_COMMAND_BLOCK_RECORDS);
    assert_eq!(timeline.try_encode(), Err(FramingError::OversizedPayload));
}

#[test]
fn composer_status_and_result_are_correlated_to_attachment() {
    let status = ComposerStatus {
        attachment_id: attachment(),
        eligibility: ComposerEligibility::Busy,
        revision: 12,
    };
    assert_eq!(ComposerStatus::decode(&status.encode()), Ok(status));
    let result = ComposerResult {
        attachment_id: attachment(),
        code: ComposerResultCode::Accepted,
        block_id: 44,
        request_id: 3,
    };
    assert_eq!(ComposerResult::decode(&result.encode()), Ok(result));
}

#[test]
fn history_range_request_rejects_unbounded_or_reversed_ranges() {
    let request = HistoryRangeRequest {
        attachment_id: attachment(),
        request_id: 9,
        block_id: 44,
        start_line: 4,
        end_line: 8,
        max_lines: 32,
        max_cells: 4096,
        start_unit: 12,
    };
    assert_eq!(HistoryRangeRequest::decode(&request.encode()), Ok(request));
    let mut reversed = request.encode();
    reversed[40..48].copy_from_slice(&3u64.to_le_bytes());
    assert_eq!(
        HistoryRangeRequest::decode(&reversed),
        Err(FramingError::MalformedPayload)
    );
    let mut oversized = request.encode();
    oversized[48..50].copy_from_slice(&u16::MAX.to_le_bytes());
    assert_eq!(
        HistoryRangeRequest::decode(&oversized),
        Err(FramingError::MalformedPayload)
    );
}

#[test]
fn history_snapshot_round_trip_is_bounded_and_keeps_rows_distinct() {
    let snapshot = HistoryRangeSnapshot {
        request_id: 9,
        block_id: 44,
        revision: 17,
        status: HistoryRangeStatus::Complete,
        rows: vec![
            HistoryRow {
                line_id: 4,
                cells: "café"
                    .chars()
                    .map(|c| HistoryCell {
                        scalar: c as u32,
                        foreground: 0x0200_00ff,
                        background: 0,
                        flags: 1,
                        reserved: 0,
                    })
                    .collect(),
            },
            HistoryRow {
                line_id: 5,
                cells: vec![
                    HistoryCell {
                        scalar: b' ' as u32,
                        foreground: 0,
                        background: 0,
                        flags: 0,
                        reserved: 0,
                    };
                    3
                ],
            },
        ],
        sidecar: Vec::new(),
    };
    assert_eq!(
        HistoryRangeSnapshot::decode(&snapshot.encode()),
        Ok(snapshot)
    );
    let too_many = HistoryRangeSnapshot {
        request_id: 1,
        block_id: 1,
        revision: 1,
        status: HistoryRangeStatus::Complete,
        rows: vec![HistoryRow {
            line_id: 1,
            cells: vec![
                HistoryCell {
                    scalar: 0,
                    foreground: 0,
                    background: 0,
                    flags: 0,
                    reserved: 0,
                };
                MAX_HISTORY_RANGE_CELLS + 1
            ],
        }],
        sidecar: Vec::new(),
    };
    assert_eq!(too_many.try_encode(), Err(FramingError::OversizedPayload));
}

#[test]
fn history_admit_rows_truncates_before_wire_byte_budget() {
    fn full_row(line_id: u64, cols: usize) -> HistoryRow {
        HistoryRow {
            line_id,
            cells: vec![
                HistoryCell {
                    scalar: b'x' as u32,
                    foreground: 0,
                    background: 0,
                    flags: 0,
                    reserved: 0,
                };
                cols
            ],
        }
    }

    // Production native requests max_lines=512 / max_cells=131072. At 80
    // columns, ~152 full-width rows exceed MAX_HISTORY_RANGE_BYTES while
    // still inside the line/cell admit limits — the P1 disconnect path.
    let dense: Vec<_> = (1..=200).map(|id| full_row(id, 80)).collect();
    let without_wire = HistoryRangeSnapshot {
        request_id: 1,
        block_id: 2,
        revision: 3,
        status: HistoryRangeStatus::Complete,
        rows: dense.clone(),
        sidecar: Vec::new(),
    };
    assert_eq!(
        without_wire.try_encode(),
        Err(FramingError::OversizedPayload)
    );

    let (admitted, truncated) = HistoryRangeSnapshot::admit_rows(
        dense,
        MAX_HISTORY_RANGE_LINES,
        MAX_HISTORY_RANGE_CELLS,
        0,
    );
    assert!(truncated);
    assert!(admitted.len() < 200);
    assert!(!admitted.is_empty());
    let snapshot = HistoryRangeSnapshot {
        request_id: 1,
        block_id: 2,
        revision: 3,
        status: HistoryRangeStatus::Truncated,
        rows: admitted,
        sidecar: Vec::new(),
    };
    let encoded = snapshot
        .try_encode()
        .expect("wire-admitted snapshot encodes");
    assert!(encoded.len() <= MAX_HISTORY_RANGE_BYTES);
    assert_eq!(HistoryRangeSnapshot::decode(&encoded), Ok(snapshot));
}

#[test]
fn history_admit_marks_truncated_when_line_window_fills() {
    fn row(line_id: u64) -> HistoryRow {
        HistoryRow {
            line_id,
            cells: vec![HistoryCell {
                scalar: b'x' as u32,
                foreground: 0,
                background: 0,
                flags: 0,
                reserved: 0,
            }],
        }
    }
    let rows: Vec<_> = (1..=5).map(row).collect();
    let (admitted, truncated) = HistoryRangeSnapshot::admit_rows(rows, 3, 4096, 0);
    assert!(truncated);
    assert_eq!(admitted.len(), 3);
}

#[test]
fn history_snapshot_rejects_nonzero_cell_reserved() {
    let snapshot = HistoryRangeSnapshot {
        request_id: 1,
        block_id: 2,
        revision: 3,
        status: HistoryRangeStatus::Complete,
        rows: vec![HistoryRow {
            line_id: 1,
            cells: vec![HistoryCell {
                scalar: b'x' as u32,
                foreground: 0,
                background: 0,
                flags: 0,
                reserved: 0,
            }],
        }],
        sidecar: Vec::new(),
    };
    let mut encoded = snapshot.encode();
    // scalar(4)+fg(4)+bg(4)+flags(2)+reserved(2) — flip reserved.
    // Sidecar is empty, so reserved sits at the last two payload bytes.
    let reserved_at = encoded.len() - 2;
    encoded[reserved_at] = 1;
    assert_eq!(
        HistoryRangeSnapshot::decode(&encoded),
        Err(FramingError::MalformedPayload)
    );
}

#[test]
fn history_snapshot_round_trips_combining_grapheme_sidecar() {
    let mut sidecar = Vec::new();
    let cell = HistoryCell::from_text("e\u{301}", 0, 0, 0, &mut sidecar).expect("combining cell");
    assert_eq!(
        cell.flags & HISTORY_CELL_SIDECAR_FLAG,
        HISTORY_CELL_SIDECAR_FLAG
    );
    assert_eq!(
        cell.sidecar_utf8(&sidecar).expect("sidecar"),
        Some("e\u{301}".as_bytes())
    );
    let snapshot = HistoryRangeSnapshot {
        request_id: 11,
        block_id: 2,
        revision: 4,
        status: HistoryRangeStatus::Complete,
        rows: vec![HistoryRow {
            line_id: 1,
            cells: vec![cell],
        }],
        sidecar,
    };
    assert_eq!(
        HistoryRangeSnapshot::decode(&snapshot.encode()),
        Ok(snapshot)
    );
}

#[test]
fn pack_source_rows_keeps_a_sidecar_prefix_when_the_row_exceeds_the_budget() {
    let grapheme = "\u{10000}".repeat(MAX_HISTORY_GRAPHEME_BYTES / 4);
    assert_eq!(grapheme.len(), MAX_HISTORY_GRAPHEME_BYTES);
    let cell = HistorySourceCell {
        text: grapheme,
        width: 2,
        continuation: false,
        foreground: 0,
        background: 0,
        style_flags: 0,
    };
    let continuation = HistorySourceCell {
        text: String::new(),
        width: 0,
        continuation: true,
        foreground: 0,
        background: 0,
        style_flags: 0,
    };
    let mut row = Vec::new();
    for _ in 0..8 {
        row.push(cell.clone());
        row.push(continuation.clone());
    }
    let (rows, sidecar, truncated) = HistoryRangeSnapshot::pack_source_rows([(1, row)]);
    assert!(truncated);
    assert_eq!(rows.len(), 1);
    let leads: Vec<_> = rows[0]
        .cells
        .iter()
        .filter(|cell| !cell.is_continuation())
        .collect();
    assert_eq!(leads.len(), 7);
    assert!(leads.iter().all(|cell| cell.cell_width() == 2));
    assert!(rows[0].cells.iter().any(|cell| cell.is_continuation()));
    HistoryRangeSnapshot {
        request_id: 1,
        block_id: 2,
        revision: 1,
        status: HistoryRangeStatus::Truncated,
        rows: rows.clone(),
        sidecar: sidecar.clone(),
    }
    .try_encode()
    .expect("packed prefix encodes");
    assert!(sidecar.len() <= MAX_HISTORY_SIDECAR_BYTES);
    assert!(!leads.is_empty());
}

#[test]
fn truncated_sidecar_prefix_continues_with_start_unit_without_zero_progress() {
    let grapheme = "\u{10000}".repeat(MAX_HISTORY_GRAPHEME_BYTES / 4);
    let lead = HistorySourceCell {
        text: grapheme,
        width: 2,
        continuation: false,
        foreground: 0,
        background: 0,
        style_flags: 0,
    };
    let continuation = HistorySourceCell {
        text: String::new(),
        width: 0,
        continuation: true,
        foreground: 0,
        background: 0,
        style_flags: 0,
    };
    let mut source = Vec::new();
    for _ in 0..8 {
        source.push(lead.clone());
        source.push(continuation.clone());
    }

    let (first_rows, first_sidecar, first_truncated) =
        HistoryRangeSnapshot::pack_source_rows([(1, source.clone())]);
    assert!(first_truncated);
    let (first_rows, first_budget) = HistoryRangeSnapshot::admit_rows(
        first_rows,
        MAX_HISTORY_RANGE_LINES,
        MAX_HISTORY_RANGE_CELLS,
        first_sidecar.len(),
    );
    assert!(!first_budget || first_truncated);
    let first_leads = HistoryRangeSnapshot::lead_count(&first_rows);
    assert!(
        first_leads > 0,
        "truncated chunk must expose leads for start_unit"
    );
    let first = HistoryRangeSnapshot {
        request_id: 1,
        block_id: 2,
        revision: 1,
        status: HistoryRangeStatus::Truncated,
        rows: first_rows,
        sidecar: first_sidecar,
    };
    first.try_encode().expect("first truncated chunk encodes");

    let skip = first_leads as usize;
    let mut remaining = Vec::new();
    let mut seen_leads = 0usize;
    for cell in source {
        if cell.continuation {
            if seen_leads > skip {
                remaining.push(cell);
            }
            continue;
        }
        let include = seen_leads >= skip;
        seen_leads += 1;
        if include {
            remaining.push(cell);
        }
    }
    assert!(!remaining.is_empty(), "unconsumed suffix must remain");

    let (second_rows, second_sidecar, second_truncated) =
        HistoryRangeSnapshot::pack_source_rows([(1, remaining)]);
    let (second_rows, _) = HistoryRangeSnapshot::admit_rows(
        second_rows,
        MAX_HISTORY_RANGE_LINES,
        MAX_HISTORY_RANGE_CELLS,
        second_sidecar.len(),
    );
    let second_leads = HistoryRangeSnapshot::lead_count(&second_rows);
    assert!(
        second_leads > 0,
        "continuation chunk must not be zero-progress"
    );
    let second = HistoryRangeSnapshot {
        request_id: 2,
        block_id: 2,
        revision: 1,
        status: if second_truncated {
            HistoryRangeStatus::Truncated
        } else {
            HistoryRangeStatus::Complete
        },
        rows: second_rows,
        sidecar: second_sidecar,
    };
    second.try_encode().expect("continuation chunk encodes");
    assert_eq!(first_leads + second_leads, 8);
}

#[test]
fn admit_rows_reserves_sidecar_bytes_so_packed_prefix_encodes() {
    let grapheme = "\u{10000}".repeat(MAX_HISTORY_GRAPHEME_BYTES / 4);
    let lead = HistorySourceCell {
        text: grapheme,
        width: 2,
        continuation: false,
        foreground: 0,
        background: 0,
        style_flags: 0,
    };
    let continuation = HistorySourceCell {
        text: String::new(),
        width: 0,
        continuation: true,
        foreground: 0,
        background: 0,
        style_flags: 0,
    };
    let mut row = Vec::new();
    for _ in 0..8 {
        row.push(lead.clone());
        row.push(continuation.clone());
    }
    let (packed, sidecar, truncated) = HistoryRangeSnapshot::pack_source_rows([(1, row)]);
    assert!(truncated);
    assert!(!sidecar.is_empty());
    let (admitted, _) = HistoryRangeSnapshot::admit_rows(
        packed,
        MAX_HISTORY_RANGE_LINES,
        MAX_HISTORY_RANGE_CELLS,
        sidecar.len(),
    );
    assert!(!admitted.is_empty());
    assert!(HistoryRangeSnapshot::lead_count(&admitted) > 0);
    HistoryRangeSnapshot {
        request_id: 1,
        block_id: 2,
        revision: 1,
        status: HistoryRangeStatus::Truncated,
        rows: admitted,
        sidecar,
    }
    .try_encode()
    .expect("sidecar-aware admit must encode");
}
