//! Rust-owned scroll state for the conventional Raw terminal presentation.

use seyal_runtime::local_ipc::framing::{
    HistoryCell, HistoryRangeSnapshot, HISTORY_CELL_CONTINUATION_FLAG, HISTORY_CELL_SIDECAR_FLAG,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RawViewport {
    anchor_line: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RawViewportPlan {
    pub offset_rows: u64,
    pub start_line: u64,
    pub end_line: u64,
}

/// Clip a history row without leaving half of a width-two grapheme visible.
/// The Raw viewport width is presentation policy, so the canonical row is
/// retained and only the disposable client snapshot is shortened.
pub(crate) fn clip_history_row_at_viewport_width(row: &mut Vec<HistoryCell>, columns: usize) {
    if columns == 0 || row.len() <= columns {
        return;
    }

    let split_continuation = row[columns].flags & HISTORY_CELL_CONTINUATION_FLAG != 0;
    if !split_continuation {
        row.truncate(columns);
        return;
    }

    let mut glyph_start = columns;
    while glyph_start > 0 && row[glyph_start].flags & HISTORY_CELL_CONTINUATION_FLAG != 0 {
        glyph_start -= 1;
    }
    row.truncate(glyph_start);
}

/// Apply Raw viewport clipping to the disposable history response while
/// preserving its correlation, revision, status, and sidecar metadata.
pub(crate) fn clip_history_snapshot(snapshot: &mut HistoryRangeSnapshot, columns: u16) -> u32 {
    let original_lead_count = snapshot
        .rows
        .iter()
        .flat_map(|row| &row.cells)
        .filter(|cell| cell.scalar != 0 || cell.flags & HISTORY_CELL_SIDECAR_FLAG != 0)
        .count() as u32;
    if columns > 0 {
        for row in &mut snapshot.rows {
            clip_history_row_at_viewport_width(&mut row.cells, usize::from(columns));
        }
    }
    original_lead_count
}

impl RawViewport {
    pub fn clear(&mut self) {
        self.anchor_line = None;
    }

    pub fn apply_delta(
        &mut self,
        start_line_id: u64,
        rows: u16,
        delta_rows: i32,
    ) -> Option<RawViewportPlan> {
        if start_line_id == 0 || rows == 0 {
            self.clear();
            return None;
        }
        let max_offset = start_line_id - 1;
        let current_offset = self
            .anchor_line
            .map_or(0, |anchor| start_line_id.saturating_sub(anchor));
        let next_offset = if delta_rows >= 0 {
            current_offset
                .saturating_add(delta_rows as u64)
                .min(max_offset)
        } else {
            current_offset.saturating_sub(delta_rows.unsigned_abs() as u64)
        };
        self.anchor_line = (next_offset > 0).then(|| start_line_id - next_offset);
        self.plan(start_line_id, rows)
    }

    pub fn plan(&mut self, start_line_id: u64, rows: u16) -> Option<RawViewportPlan> {
        if start_line_id == 0 || rows == 0 {
            self.clear();
            return None;
        }
        if let Some(anchor) = self.anchor_line {
            self.anchor_line = Some(anchor.min(start_line_id));
        }
        let Some(anchor_line) = self.anchor_line else {
            return None;
        };
        if anchor_line >= start_line_id {
            self.clear();
            return None;
        }
        let offset_rows = start_line_id - anchor_line;
        let start_line = anchor_line.max(1);
        let row_count = u64::from(rows);
        // At absolute history top request one extra viewport so Rust can omit
        // sealed leading blank rows without returning a partially filled page.
        let span = if start_line == 1 {
            row_count.saturating_mul(2)
        } else {
            row_count
        };
        let end_line = start_line.saturating_add(span.saturating_sub(1));
        Some(RawViewportPlan {
            offset_rows,
            start_line,
            end_line,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        clip_history_row_at_viewport_width, clip_history_snapshot, RawViewport, RawViewportPlan,
    };
    use seyal_runtime::local_ipc::framing::{
        HistoryCell, HistoryRangeSnapshot, HistoryRangeStatus, HistoryRow,
        HISTORY_CELL_CONTINUATION_FLAG,
    };

    fn cell(scalar: u32, flags: u16) -> HistoryCell {
        HistoryCell {
            scalar,
            foreground: 0,
            background: 0,
            flags,
            reserved: 0,
        }
    }

    #[test]
    fn viewport_clamps_scroll_and_anchors_the_same_history_window() {
        let mut viewport = RawViewport::default();
        assert_eq!(
            viewport.apply_delta(101, 20, 7),
            Some(RawViewportPlan {
                offset_rows: 7,
                start_line: 94,
                end_line: 113,
            })
        );

        // New live output advances the offset while retaining the same
        // absolute history window and reader position.
        let later = viewport.plan(108, 20).unwrap();
        assert_eq!(
            (later.start_line, later.end_line, later.offset_rows),
            (94, 113, 14)
        );
        assert_eq!(viewport.apply_delta(108, 20, i32::MIN), None);
        assert_eq!(viewport.plan(108, 20), None);
    }

    #[test]
    fn top_window_is_bounded_and_invalid_geometry_clears_state() {
        let mut viewport = RawViewport::default();
        let plan = viewport.apply_delta(2, 12, 99).unwrap();
        assert_eq!(plan.offset_rows, 1);
        assert_eq!((plan.start_line, plan.end_line), (1, 24));
        assert_eq!(viewport.plan(0, 12), None);
    }

    #[test]
    fn history_clip_removes_a_wide_grapheme_crossing_the_right_edge() {
        let mut row = vec![
            cell(b'a' as u32, 0),
            cell(0x4e2d, 0),
            cell(0, HISTORY_CELL_CONTINUATION_FLAG),
            cell(b'z' as u32, 0),
        ];

        clip_history_row_at_viewport_width(&mut row, 2);

        assert_eq!(row, vec![cell(b'a' as u32, 0)]);
    }

    #[test]
    fn history_clip_keeps_a_wide_grapheme_that_fits_inside_the_viewport() {
        let mut row = vec![
            cell(b'a' as u32, 0),
            cell(0x4e2d, 0),
            cell(0, HISTORY_CELL_CONTINUATION_FLAG),
            cell(b'z' as u32, 0),
        ];

        clip_history_row_at_viewport_width(&mut row, 3);

        assert_eq!(
            row,
            vec![
                cell(b'a' as u32, 0),
                cell(0x4e2d, 0),
                cell(0, HISTORY_CELL_CONTINUATION_FLAG)
            ]
        );
    }

    #[test]
    fn raw_snapshot_clip_preserves_identity_and_grapheme_sidecar() {
        let mut sidecar = Vec::new();
        let wide_grapheme = HistoryCell::from_text("👩‍💻", 0, 0, 0, &mut sidecar)
            .unwrap()
            .with_cell_metrics(2, false);
        let continuation = cell(0, HISTORY_CELL_CONTINUATION_FLAG);
        let mut snapshot = HistoryRangeSnapshot {
            request_id: 9,
            block_id: u64::MAX,
            revision: 17,
            status: HistoryRangeStatus::Complete,
            rows: vec![
                HistoryRow {
                    line_id: 42,
                    cells: vec![
                        cell(b'a' as u32, 0),
                        wide_grapheme,
                        continuation,
                        cell(b'z' as u32, 0),
                    ],
                },
                HistoryRow {
                    line_id: 43,
                    cells: vec![wide_grapheme, continuation],
                },
            ],
            sidecar,
        };

        let original_lead_count = clip_history_snapshot(&mut snapshot, 2);

        assert_eq!(original_lead_count, 4);
        assert_eq!(snapshot.request_id, 9);
        assert_eq!(snapshot.block_id, u64::MAX);
        assert_eq!(snapshot.revision, 17);
        assert_eq!(snapshot.status, HistoryRangeStatus::Complete);
        assert_eq!(snapshot.rows[0].line_id, 42);
        assert_eq!(snapshot.rows[0].cells, vec![cell(b'a' as u32, 0)]);
        assert_eq!(snapshot.rows[1].line_id, 43);
        assert_eq!(snapshot.rows[1].cells, vec![wide_grapheme, continuation]);
        assert_eq!(
            wide_grapheme.sidecar_utf8(&snapshot.sidecar).unwrap(),
            Some("👩‍💻".as_bytes())
        );
    }
}
