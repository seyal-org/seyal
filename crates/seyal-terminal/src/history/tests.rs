use super::types::{HistoryUnit, SegmentLine, SegmentUnit};
use super::*;
use super::eviction::{
    MAX_EVICTED_BITMAP_BITS, MAX_EVICTED_BITMAPS, MAX_EVICTED_ID_RANGES, MAX_EVICTED_OVERFLOW_RANGES,
};
use super::reflow::reflow_rows_allocated_bytes;
use super::wrap::{
    wrap_occupancy, wrap_repeating_period, wrap_runs_unit_sum, APERIODIC_EXTEND_CELL_FACTOR,
    APERIODIC_INLINE_RUN_BOUND, WRAP_PATTERN_MAX,
};
use crate::{LineId, Style};
use std::mem::size_of;

fn ascii_line(id: u64, text: &str, break_after: HistoryBreakAfter) -> HistoryLine {
    HistoryLine {
        line_id: LineId(id),
        units: text
            .bytes()
            .map(|byte| HistoryUnit {
                utf8: vec![byte],
                width: 1,
                style: Style::default(),
            })
            .collect(),
        break_after,
        start_offset: 0,
    }
}

#[test]
fn seals_by_payload_bytes_and_reflows_soft_chain() {
    let mut store = HistoryStore::default();
    for id in 0..4_000 {
        store.append_line(ascii_line(id, "abcdefghij", HistoryBreakAfter::SoftWrap));
    }
    assert!(
        store.segments.len() >= 2,
        "canonical UTF-8 payload should exceed {} bytes twice: segments={} payload={}",
        HISTORY_SEGMENT_PAYLOAD_TARGET,
        store.segments.len(),
        store
            .segments
            .iter()
            .map(|s| s.payload.len())
            .sum::<usize>()
    );
    assert!(store.tail_payload_bytes <= HISTORY_SEGMENT_PAYLOAD_TARGET);
    assert!(store.resident_bytes > 0);

    let rows = store.reflow(16, 8);
    assert_eq!(rows.len(), 8);
    assert_eq!(rows[0].cells.len(), 16);
    assert_eq!(
        rows[0].anchors[0],
        HistoryAnchor {
            line_id: LineId(0),
            unit_offset: 0
        }
    );
    assert_eq!(rows[1].anchors[0].unit_offset, 6);
}

#[test]
fn resident_bytes_include_allocation_and_metadata_overhead() {
    let mut store = HistoryStore::default();
    store.append_line(ascii_line(7, "abc", HistoryBreakAfter::HardBreak));

    let payload_bytes = store.tail_payload_bytes;
    assert!(store.resident_bytes() > payload_bytes);
}

#[test]
fn oldest_eviction_reports_actual_resident_delta() {
    let mut store = HistoryStore::default();
    for id in 0..2_000 {
        store.append_line(ascii_line(id, "abcdefghij", HistoryBreakAfter::HardBreak));
    }
    let before = store.resident_bytes();
    let removed = store.evict_oldest_segment();
    assert!(removed > 0);
    assert_eq!(before - removed, store.resident_bytes());
}

#[test]
fn sealed_segments_use_exact_contiguous_payload_and_offset_metadata() {
    let mut store = HistoryStore::default();
    for id in 0..4_000 {
        store.append_line(ascii_line(id, "abcdefghij", HistoryBreakAfter::HardBreak));
    }

    assert!(
        store.segments.len() >= 2,
        "canonical UTF-8 payload should exceed {} bytes twice: segments={}",
        HISTORY_SEGMENT_PAYLOAD_TARGET,
        store.segments.len()
    );
    for segment in &store.segments {
        let exact_content_bytes = segment
            .lines
            .len()
            .saturating_mul(size_of::<SegmentLine>())
            .saturating_add(segment.units.len().saturating_mul(size_of::<SegmentUnit>()))
            .saturating_add(segment.payload.len());
        assert!(segment.payload.len() <= HISTORY_SEGMENT_PAYLOAD_TARGET);
        assert_eq!(segment.resident_bytes, exact_content_bytes);

        let mut next_unit = 0usize;
        for line in &segment.lines {
            assert_eq!(line.unit_start as usize, next_unit);
            next_unit = next_unit.saturating_add(line.unit_len as usize);
        }
        assert_eq!(next_unit, segment.units.len());

        let mut next_payload = 0usize;
        for unit in &segment.units {
            assert_eq!(unit.payload_start as usize, next_payload);
            next_payload = next_payload.saturating_add(unit.payload_len as usize);
        }
        assert_eq!(next_payload, segment.payload.len());
    }
}

#[test]
fn hard_break_stops_reflow_chain_and_wide_units_stay_atomic() {
    let mut store = HistoryStore::default();
    store.append_line(ascii_line(1, "abc", HistoryBreakAfter::HardBreak));
    store.append_line(HistoryLine {
        line_id: LineId(2),
        units: vec![HistoryUnit {
            utf8: "界".as_bytes().to_vec(),
            width: 2,
            style: Style::default(),
        }],
        break_after: HistoryBreakAfter::HardBreak,
        start_offset: 0,
    });
    let rows = store.reflow(2, 8);
    assert_eq!(rows[0].cells.len(), 2);
    let wide = rows
        .iter()
        .find(|row| row.cells.first().is_some_and(|cell| cell.width == 2))
        .expect("wide source unit has a visual row");
    assert_eq!(wide.cells.len(), 2);
    assert!(wide.cells[1].is_continuation());
}

#[test]
fn oversized_fragment_later_source_offset_resolves() {
    let mut store = HistoryStore::default();
    let line = ascii_line(
        7,
        &"a".repeat(HISTORY_SEGMENT_PAYLOAD_TARGET * 2),
        HistoryBreakAfter::HardBreak,
    );
    store.append_line(line);
    let anchor = HistoryAnchor {
        line_id: LineId(7),
        unit_offset: (HISTORY_SEGMENT_PAYLOAD_TARGET + 1) as u32,
    };
    assert!(matches!(
        store.resolve_anchor(anchor),
        HistoryAnchorResolution::Resolved { .. }
    ));
}

#[test]
fn evicted_id_metadata_is_capped_without_coarsening_later_live_ids() {
    let mut store = HistoryStore::default();
    for i in 0..(MAX_EVICTED_ID_RANGES + 8) {
        let first = (i as u64) * 10 + 1;
        store.record_evicted_range_for_test(first, first);
    }
    assert!(store.evicted_id_ranges.len() <= MAX_EVICTED_ID_RANGES);
    // Folded watermark covers the oldest compacted identity, not a later
    // never-evicted primary identity sitting in an alternate-screen gap.
    assert!(!store.line_was_evicted(LineId(MAX_EVICTED_ID_RANGES as u64 * 10 + 50)));
    assert!(store.line_was_evicted(LineId(1)));
    assert!(store.line_was_evicted(LineId(11)));
    assert!(matches!(
        store.resolve_anchor(HistoryAnchor {
            line_id: LineId(11),
            unit_offset: 0,
        }),
        HistoryAnchorResolution::Unavailable
    ));
    // ID 2 was never allocated/evicted; it sits in the gap after the
    // compacted [1,1] run and must not become Unavailable.
    assert!(!store.line_was_evicted(LineId(2)));
    assert!(!store.range_intersects_evicted(LineId(2), LineId(2)));
    assert!(matches!(
        store.resolve_anchor(HistoryAnchor {
            line_id: LineId(2),
            unit_offset: 0,
        }),
        HistoryAnchorResolution::Invalid
    ));
    assert!(store.range_intersects_evicted(LineId(1), LineId(1)));
}

#[test]
fn hostile_sparse_eviction_metadata_is_bounded_and_fail_closes() {
    let mut store = HistoryStore::default();
    let stride = MAX_EVICTED_BITMAP_BITS.saturating_add(1);
    let extra = 32u64;
    let total = MAX_EVICTED_ID_RANGES
        + 1
        + MAX_EVICTED_BITMAPS
        + MAX_EVICTED_OVERFLOW_RANGES
        + extra as usize;
    for i in 0..total {
        let id = (i as u64).saturating_mul(stride).saturating_add(1);
        store.record_evicted_range_for_test(id, id);
    }
    assert!(store.evicted_id_ranges.len() <= MAX_EVICTED_ID_RANGES);
    assert!(store.evicted_bitmaps.len() <= MAX_EVICTED_BITMAPS);
    assert!(store.evicted_overflow_ranges.len() <= MAX_EVICTED_OVERFLOW_RANGES);
    assert!(store.evicted_index_saturated);

    let overflowed = (1 + MAX_EVICTED_BITMAPS as u64 + MAX_EVICTED_OVERFLOW_RANGES as u64)
        .saturating_mul(stride)
        .saturating_add(1);
    assert!(store.line_was_evicted(LineId(overflowed)));
    assert!(matches!(
        store.resolve_anchor(HistoryAnchor {
            line_id: LineId(overflowed),
            unit_offset: 0,
        }),
        HistoryAnchorResolution::Unavailable
    ));
    assert!(!store.line_was_evicted(LineId(2)));
    assert!(matches!(
        store.resolve_anchor(HistoryAnchor {
            line_id: LineId(2),
            unit_offset: 0,
        }),
        HistoryAnchorResolution::Invalid
    ));
}

#[test]
fn reflow_cache_accounting_counts_allocation_capacity() {
    let mut rows = Vec::with_capacity(128);
    rows.push(ReflowRow::default());
    rows[0].cells.reserve(64);
    rows[0].anchors.reserve(32);
    let bytes = reflow_rows_allocated_bytes(&rows);
    let minimum = size_of::<Vec<ReflowRow>>()
        .saturating_add(128usize.saturating_mul(size_of::<ReflowRow>()))
        .saturating_add(64usize.saturating_mul(size_of::<HistoryWireCell>()))
        .saturating_add(32usize.saturating_mul(size_of::<HistoryAnchor>()));
    assert!(
        bytes >= minimum,
        "derived-cache accounting undercounted allocations: {bytes} < {minimum}"
    );
}

fn ascii_fragment(
    id: u64,
    start_offset: u32,
    text: &str,
    break_after: HistoryBreakAfter,
) -> HistoryLine {
    HistoryLine {
        line_id: LineId(id),
        units: text
            .bytes()
            .map(|byte| HistoryUnit {
                utf8: vec![byte],
                width: 1,
                style: Style::default(),
            })
            .collect(),
        break_after,
        start_offset,
    }
}

#[test]
fn wrap_column_before_walks_only_the_current_soft_wrap_chain() {
    let mut store = HistoryStore::default();
    for id in 0..8_000 {
        store.append_line(ascii_line(id, "xxxx", HistoryBreakAfter::HardBreak));
    }
    store.append_line(ascii_fragment(
        8_000,
        0,
        "abcde",
        HistoryBreakAfter::SoftWrap,
    ));
    store.append_line(ascii_fragment(8_000, 5, "fg", HistoryBreakAfter::HardBreak));
    let (suffix, from, start_col) = store.eager_resize_suffix(2, 1);
    assert!(
        suffix.len() <= 8,
        "eager suffix cloned {} lines including hard-broken prefix",
        suffix.len()
    );
    assert_eq!(from.unwrap().line_id, LineId(8_000));
    assert_eq!(from.unwrap().unit_offset, 5);
    assert_eq!(start_col, 1);
}

#[test]
fn wrap_column_before_is_closed_form_for_a_resident_soft_wrap_chain() {
    let mut store = HistoryStore::default();
    for id in 0..8_000 {
        store.append_line(ascii_line(id, "x", HistoryBreakAfter::SoftWrap));
    }
    let chain = store.wrap_chains.back().expect("open wrap chain");
    assert_eq!(chain.runs, [(1, 8_000)]);
    assert_eq!(chain.fragments.len(), 8_000);
    assert_eq!(chain.fragments.last().unwrap().prefix_units, 7_999);
    let (suffix, from, start_col) = store.eager_resize_suffix(8, 6);
    assert!(
        suffix.len() <= 48,
        "eager suffix cloned {} lines from a 8000-unit SoftWrap chain",
        suffix.len()
    );
    assert_eq!(from.unwrap().line_id, LineId(8_000 - 48));
    assert_eq!(start_col, 0);
    assert_eq!(
        store.wrap_column_before(from, 8),
        wrap_occupancy(&[1; 7_952], 8)
    );
}

#[test]
fn blank_rows_seal_and_stay_inside_the_resident_cap() {
    let mut store = HistoryStore::default();
    for id in 0..50_000 {
        store.append_line(HistoryLine {
            line_id: LineId(id),
            units: Vec::new(),
            break_after: HistoryBreakAfter::HardBreak,
            start_offset: 0,
        });
    }
    assert!(
        !store.segments.is_empty(),
        "blank rows must seal instead of remaining an unbounded tail"
    );
    assert!(store.tail_resident_bytes <= HISTORY_TAIL_PAYLOAD_LIMIT);
    assert!(store.resident_bytes <= HISTORY_PER_EXECUTION_BYTE_CAP);
    assert!(store.derived_cache_bytes() <= HISTORY_PER_EXECUTION_DERIVED_INDEX_CAP);
}

#[test]
fn eager_resize_suffix_is_bounded_for_hard_broken_history() {
    let mut store = HistoryStore::default();
    for id in 0..8_000 {
        store.append_line(ascii_line(id, "x", HistoryBreakAfter::HardBreak));
    }
    let (suffix, from, start_col) = store.eager_resize_suffix(8, 6);
    assert!(
        suffix.len() <= 48,
        "eager suffix cloned {} lines from 8000 hard-broken rows",
        suffix.len()
    );
    assert!(suffix.len() < 8_000);
    assert_eq!(start_col, 0);
    assert!(from.expect("suffix").line_id.0 > 0);
    store.truncate_from(from.unwrap());
    assert!(matches!(
        store.resolve_anchor(HistoryAnchor {
            line_id: LineId(0),
            unit_offset: 0
        }),
        HistoryAnchorResolution::Resolved { .. }
    ));
}

#[test]
fn eager_resize_suffix_is_bounded_for_blank_hard_broken_history() {
    let mut store = HistoryStore::default();
    for id in 0..8_000 {
        store.append_line(HistoryLine {
            line_id: LineId(id),
            units: Vec::new(),
            break_after: HistoryBreakAfter::HardBreak,
            start_offset: 0,
        });
    }
    let (suffix, from, start_col) = store.eager_resize_suffix(8, 6);
    assert!(
        suffix.len() <= 48,
        "eager suffix cloned {} blank lines from 8000 empty rows",
        suffix.len()
    );
    assert_eq!(start_col, 0);
    store.truncate_from(from.expect("suffix"));
    assert!(
        store.entries().count() < 8_000,
        "truncate_from must drop the blank suffix"
    );
}

#[test]
fn truncate_from_drops_an_empty_line_at_the_cut() {
    let mut store = HistoryStore::default();
    store.append_line(ascii_line(1, "x", HistoryBreakAfter::HardBreak));
    store.append_line(HistoryLine {
        line_id: LineId(2),
        units: Vec::new(),
        break_after: HistoryBreakAfter::HardBreak,
        start_offset: 0,
    });
    store.truncate_from(HistoryAnchor {
        line_id: LineId(2),
        unit_offset: 0,
    });
    assert_eq!(
        store.entries().count(),
        1,
        "empty line at the cut must be dropped"
    );
    assert!(matches!(
        store.resolve_anchor(HistoryAnchor {
            line_id: LineId(1),
            unit_offset: 0
        }),
        HistoryAnchorResolution::Resolved { .. }
    ));
}

#[test]
fn wrap_index_is_derived_and_omits_closed_hard_broken_rows() {
    let mut store = HistoryStore::default();
    for id in 0..8_000 {
        store.append_line(ascii_line(id, "x", HistoryBreakAfter::HardBreak));
    }
    assert!(store.wrap_chains.is_empty());
    store.append_line(ascii_line(8_000, "x", HistoryBreakAfter::SoftWrap));
    let wrap = store.wrap_index_allocated_bytes();
    assert!(wrap > 0);
    assert!(store.derived_cache_bytes() >= wrap);
    store.append_line(ascii_line(8_001, "y", HistoryBreakAfter::HardBreak));
    let (_, from, start_col) = store.eager_resize_suffix(8, 6);
    store.truncate_from(from.unwrap());
    assert_eq!(start_col, 0);
    assert!(matches!(
        store.resolve_anchor(HistoryAnchor {
            line_id: LineId(0),
            unit_offset: 0
        }),
        HistoryAnchorResolution::Resolved { .. }
    ));
}

#[test]
fn wrap_column_before_is_closed_form_for_alternating_widths() {
    let mut store = HistoryStore::default();
    let mut widths = Vec::new();
    for id in 0..8_000 {
        let width = if id % 2 == 0 { 1 } else { 2 };
        widths.push(width);
        store.append_line(HistoryLine {
            line_id: LineId(id),
            units: vec![HistoryUnit {
                utf8: vec![b'x'],
                width,
                style: Style::default(),
            }],
            break_after: HistoryBreakAfter::SoftWrap,
            start_offset: 0,
        });
    }
    {
        let chain = store.wrap_chains.back().expect("open wrap chain");
        assert_eq!(chain.pattern_len, 2);
        assert_eq!(chain.runs.as_slice(), &[(1, 1), (2, 1)]);
    }
    let (suffix, from, start_col) = store.eager_resize_suffix(8, 6);
    assert!(
        suffix.len() <= 48,
        "eager suffix cloned {} lines from an 8000-unit mixed SoftWrap chain",
        suffix.len()
    );
    let from = from.expect("suffix cut");
    let prefix = from.line_id.0 as usize;
    assert_eq!(start_col, wrap_occupancy(&widths[..prefix], 8));
    assert_eq!(store.wrap_column_before(Some(from), 8), start_col);
    store.truncate_from(from);
    assert!(matches!(
        store.resolve_anchor(HistoryAnchor {
            line_id: LineId(0),
            unit_offset: 0
        }),
        HistoryAnchorResolution::Resolved { .. }
    ));
}

#[test]
fn wrap_column_before_preserves_period_two_run_counts() {
    let mut store = HistoryStore::default();
    let mut widths = Vec::new();
    for id in 0..2_000 {
        widths.extend_from_slice(&[1, 1, 2]);
        store.append_line(HistoryLine {
            line_id: LineId(id),
            units: vec![
                HistoryUnit {
                    utf8: vec![b'a'],
                    width: 1,
                    style: Style::default(),
                },
                HistoryUnit {
                    utf8: vec![b'b'],
                    width: 1,
                    style: Style::default(),
                },
                HistoryUnit {
                    utf8: vec![b'c'],
                    width: 2,
                    style: Style::default(),
                },
            ],
            break_after: HistoryBreakAfter::SoftWrap,
            start_offset: 0,
        });
    }
    {
        let chain = store.wrap_chains.back().expect("open wrap chain");
        assert_eq!(chain.pattern_len, 2);
        assert_eq!(chain.runs.as_slice(), &[(1, 2), (2, 1)]);
    }
    let (suffix, from, start_col) = store.eager_resize_suffix(8, 6);
    assert!(suffix.len() < 2_000);
    let from = from.expect("suffix cut");
    let prefix_units = store
        .wrap_chains
        .back()
        .map(|chain| chain.units_before(from))
        .unwrap_or(0) as usize;
    assert_eq!(start_col, wrap_occupancy(&widths[..prefix_units], 8));
    assert_eq!(store.wrap_column_before(Some(from), 8), start_col);
    store.truncate_from(from);
    let run_units = store
        .wrap_chains
        .back()
        .map(|chain| wrap_runs_unit_sum(&chain.runs) as usize)
        .unwrap_or(0);
    if run_units < prefix_units {
        assert_eq!(
            store.wrap_chains.back().map(|chain| chain.runs.len()),
            Some(2)
        );
    } else {
        assert_eq!(run_units, prefix_units);
    }
    assert_eq!(store.wrap_column_before(Some(from), 8), start_col);
    for line in suffix {
        store.append_line(line);
    }
    let chain = store.wrap_chains.back().expect("re-appended wrap chain");
    assert_eq!(chain.runs.first().copied(), Some((1, 2)));
    assert_eq!(chain.runs.get(1).copied(), Some((2, 1)));
    assert_eq!(chain.pattern_len, 2);
}

#[test]
fn wrap_suffix_trim_drops_stale_two_run_tail_before_reappend() {
    let mut store = HistoryStore::default();
    store.append_line(HistoryLine {
        line_id: LineId(0),
        units: vec![
            HistoryUnit {
                utf8: vec![b'a'],
                width: 1,
                style: Style::default(),
            },
            HistoryUnit {
                utf8: vec![b'b'],
                width: 1,
                style: Style::default(),
            },
            HistoryUnit {
                utf8: vec![b'c'],
                width: 2,
                style: Style::default(),
            },
        ],
        break_after: HistoryBreakAfter::SoftWrap,
        start_offset: 0,
    });
    {
        let chain = store.wrap_chains.back().expect("open wrap chain");
        assert_eq!(chain.pattern_len, 2);
        assert_eq!(chain.runs.as_slice(), &[(1, 2), (2, 1)]);
    }
    let from = HistoryAnchor {
        line_id: LineId(0),
        unit_offset: 1,
    };
    store.truncate_from(from);
    assert_eq!(
        store
            .wrap_chains
            .back()
            .expect("trimmed wrap chain")
            .runs
            .as_slice(),
        &[(1, 1)]
    );
    store.append_line(HistoryLine {
        line_id: LineId(0),
        units: vec![
            HistoryUnit {
                utf8: vec![b'b'],
                width: 1,
                style: Style::default(),
            },
            HistoryUnit {
                utf8: vec![b'c'],
                width: 2,
                style: Style::default(),
            },
        ],
        break_after: HistoryBreakAfter::SoftWrap,
        start_offset: 1,
    });
    let chain = store.wrap_chains.back().expect("re-appended wrap chain");
    assert_eq!(chain.runs.as_slice(), &[(1, 2), (2, 1)]);
    assert_eq!(chain.pattern_len, 2);
    assert_eq!(
        store.wrap_column_before(
            Some(HistoryAnchor {
                line_id: LineId(0),
                unit_offset: 3
            }),
            8
        ),
        wrap_occupancy(&[1, 1, 2], 8)
    );
}

#[test]
fn drop_derived_cache_drops_wrap_index_without_touching_payload() {
    let mut store = HistoryStore::default();
    for id in 0..8_000 {
        store.append_line(ascii_line(id, "x", HistoryBreakAfter::SoftWrap));
    }
    assert!(store.wrap_index_allocated_bytes() > 0);
    let resident = store.resident_bytes();
    let from = HistoryAnchor {
        line_id: LineId(4_001),
        unit_offset: 0,
    };
    assert_eq!(store.wrap_column_before(Some(from), 8), 1);
    store.drop_derived_cache();
    assert_eq!(store.wrap_index_allocated_bytes(), 0);
    assert_eq!(store.derived_cache_bytes(), 0);
    assert_eq!(store.resident_bytes(), resident);
    assert_eq!(store.wrap_column_before(Some(from), 8), 1);
    assert!(matches!(
        store.resolve_anchor(HistoryAnchor {
            line_id: LineId(0),
            unit_offset: 0
        }),
        HistoryAnchorResolution::Resolved { .. }
    ));
    store.append_line(ascii_line(8_000, "y", HistoryBreakAfter::SoftWrap));
    assert!(store.wrap_index_allocated_bytes() > 0);
}

fn width_line(id: u64, widths: &[u8], break_after: HistoryBreakAfter) -> HistoryLine {
    HistoryLine {
        line_id: LineId(id),
        units: widths
            .iter()
            .map(|&width| HistoryUnit {
                utf8: vec![b'x'],
                width,
                style: Style::default(),
            })
            .collect(),
        break_after,
        start_offset: 0,
    }
}

#[test]
fn wrap_repeating_period_finds_small_periods() {
    assert_eq!(
        wrap_repeating_period(&[(1, 1), (2, 1), (1, 1), (2, 1)]),
        Some(2)
    );
    assert_eq!(
        wrap_repeating_period(&[
            (1, 1),
            (2, 2),
            (1, 3),
            (2, 1),
            (1, 1),
            (2, 2),
            (1, 3),
            (2, 1)
        ]),
        Some(4)
    );
    assert_eq!(
        wrap_repeating_period(&[
            (1, 1),
            (2, 1),
            (1, 2),
            (2, 1),
            (1, 3),
            (2, 1),
            (1, 4),
            (2, 1),
            (1, 5),
            (2, 1)
        ]),
        None
    );
}

#[test]
fn wrap_column_before_is_closed_form_for_period_four_mixed_widths() {
    let mut store = HistoryStore::default();
    let period: &[u8] = &[1, 2, 2, 1, 1, 1, 2];
    let mut widths = Vec::new();
    for id in 0..2_000 {
        widths.extend_from_slice(period);
        store.append_line(width_line(id, period, HistoryBreakAfter::SoftWrap));
    }
    {
        let chain = store.wrap_chains.back().expect("open wrap chain");
        assert_eq!(chain.pattern_len, 4);
        assert_eq!(chain.runs.as_slice(), &[(1, 1), (2, 2), (1, 3), (2, 1)]);
    }
    let (suffix, from, start_col) = store.eager_resize_suffix(8, 6);
    assert!(suffix.len() < 2_000);
    let from = from.expect("suffix cut");
    let prefix_units = store
        .wrap_chains
        .back()
        .map(|chain| chain.units_before(from))
        .unwrap_or(0) as usize;
    assert_eq!(start_col, wrap_occupancy(&widths[..prefix_units], 8));
    assert_eq!(store.wrap_column_before(Some(from), 8), start_col);
    store.drop_derived_cache();
    assert_eq!(store.wrap_column_before(Some(from), 8), start_col);
}

#[test]
fn wrap_column_before_matches_occupancy_for_aperiodic_mixed_widths() {
    let mut store = HistoryStore::default();
    let mut widths = Vec::new();
    for id in 0..256u64 {
        let ones = u8::try_from((id % 9) + 1).unwrap();
        let mut line = vec![1u8; usize::from(ones)];
        line.push(2);
        widths.extend_from_slice(&line);
        store.append_line(width_line(id, &line, HistoryBreakAfter::SoftWrap));
    }
    {
        let chain = store.wrap_chains.back().expect("open wrap chain");
        assert_eq!(chain.pattern_len, 0);
        assert!(chain.runs.len() > WRAP_PATTERN_MAX);
        assert!(chain.runs.len() > APERIODIC_INLINE_RUN_BOUND);
    }
    let (suffix, from, start_col) = store.eager_resize_suffix(8, 6);
    assert!(suffix.len() < 256);
    let from = from.expect("suffix cut");
    let prefix_units = store
        .wrap_chains
        .back()
        .map(|chain| chain.units_before(from))
        .unwrap_or(0) as usize;
    assert_eq!(start_col, wrap_occupancy(&widths[..prefix_units], 8));
    assert_eq!(store.wrap_column_before(Some(from), 8), start_col);
    // Spine must answer a second query without depending on a fresh linear walk.
    assert_eq!(store.wrap_column_before(Some(from), 8), start_col);
    {
        let chain = store.wrap_chains.back().expect("open wrap chain");
        let spine = chain.spine.borrow();
        assert!(
            spine
                .as_ref()
                .is_some_and(|spine| spine.cols == 8 && spine.run_len == chain.runs.len()),
            "aperiodic mid-chain cuts must retain a cols-dependent occupancy spine"
        );
    }
    // Miss path after derived-cache drop must stay exact (ephemeral spine).
    store.drop_derived_cache();
    assert_eq!(store.wrap_column_before(Some(from), 8), start_col);
}

#[test]
fn eager_resize_extends_aperiodic_soft_wrap_to_hard_break_when_budget_allows() {
    let mut store = HistoryStore::default();
    store.append_line(ascii_line(0, "hard-prefix", HistoryBreakAfter::HardBreak));
    // Truly aperiodic run stream (no period ≤ WRAP_PATTERN_MAX), but cell
    // count fits the extend budget so resize can snap to HardBreak.
    for id in 1..50u64 {
        let ones = u8::try_from((id % 9) + 1).unwrap();
        let mut line = vec![1u8; usize::from(ones)];
        line.push(2);
        store.append_line(width_line(id, &line, HistoryBreakAfter::SoftWrap));
    }
    {
        let chain = store.wrap_chains.back().expect("open wrap chain");
        assert_eq!(chain.pattern_len, 0);
        assert!(chain.runs.len() > APERIODIC_INLINE_RUN_BOUND);
        let soft_cells: usize = chain
            .runs
            .iter()
            .map(|(width, count)| usize::from(*width) * (*count as usize))
            .sum();
        assert!(
            soft_cells
                <= usize::from(8u16)
                    .saturating_mul(6)
                    .saturating_mul(APERIODIC_EXTEND_CELL_FACTOR),
            "fixture must fit extend budget; soft_cells={soft_cells}"
        );
    }
    let (suffix, from, start_col) = store.eager_resize_suffix(8, 6);
    assert_eq!(
        start_col, 0,
        "extend-to-HardBreak must clear mid-chain start_col"
    );
    let from = from.expect("suffix");
    assert_eq!(from.line_id, LineId(1));
    assert!(suffix.iter().any(|line| line.line_id == LineId(1)));
    assert!(!suffix.iter().any(|line| line.line_id == LineId(0)));
}
