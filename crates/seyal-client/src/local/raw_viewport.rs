//! Rust-owned scroll state for the conventional Raw terminal presentation.

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
    use super::{RawViewport, RawViewportPlan};

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
}
