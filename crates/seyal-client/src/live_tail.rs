//! Flow running-Block live-tail projection (#865).
//!
//! Runtime `BlockTimeline` remains the Block authority. This module only
//! derives how the Pane compositor may project one Block's output:
//!
//! - **Running** Blocks use a damage-driven clip of the prepared primary frame
//!   into the Block output region (SPEC-008 §5.2). Hosts must not invent a history
//!   range such as `start + 511`.
//! - Clip rows are derived from Runtime viewport `LineId`s: only primary rows
//!   whose line id is `>= start_line` are drawn. Preceding prompts/output still
//!   on screen are excluded.
//! - **Completed** Blocks keep the trusted finite history span.
//! - Raw/TUI and conflicting evidence fail closed.

use crate::presentation::PresentationMode;

/// Inclusive canonical history span for a completed Flow Block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistorySpan {
    pub start_line: u64,
    pub end_line: u64,
}

/// Trusted start anchor plus the viewport row slice for a running live-tail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimaryFrameClip {
    pub start_line: u64,
    /// Inclusive first prepared-frame row belonging to this Block.
    pub first_row: u16,
    /// Number of prepared-frame rows to draw (`0` is invalid / fail closed).
    pub row_count: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveTailProjection {
    /// Clip the damage-driven prepared primary frame into the running Block
    /// output region. Not a Pane-wide live grid.
    PrimaryFrame(PrimaryFrameClip),
    /// Inclusive canonical history span for a completed Block.
    History(HistorySpan),
    /// Invalid, stale, or non-Flow evidence. Draw nothing for this Block.
    FailClosed,
}

/// Map a running Block's `start_line` onto the current primary viewport.
///
/// `viewport_line_ids[row]` is the absolute primary `LineId` for that prepared
/// row. Returns `None` when the mapping cannot be established (fail closed):
/// empty viewport, missing ids, or no visible row belongs to the Block yet.
pub fn map_primary_clip(start_line: u64, viewport_line_ids: &[u64]) -> Option<(u16, u16)> {
    if start_line == 0 || viewport_line_ids.is_empty() {
        return None;
    }
    if viewport_line_ids.len() > u16::MAX as usize {
        return None;
    }
    if viewport_line_ids.contains(&0) {
        return None;
    }
    let first_index = viewport_line_ids.iter().position(|&id| id >= start_line)?;
    // Contiguous owned run only. A later row with id < start_line (CSI T /
    // reverse-index) must not be drawn inside this Block; stop before it.
    let mut row_count: u16 = 0;
    for id in viewport_line_ids.iter().skip(first_index) {
        if *id < start_line {
            break;
        }
        row_count = row_count.saturating_add(1);
    }
    if row_count == 0 {
        return None;
    }
    let first = u16::try_from(first_index).ok()?;
    Some((first, row_count))
}

/// Project one Block's Flow output path for the current presentation.
///
/// `end_line` is the Runtime-trusted completed end, or `None` while running.
/// `viewport_line_ids` is required for a running primary clip; pass `&[]` when
/// the mapping is unavailable (projection fails closed).
pub fn project_block_output(
    mode: PresentationMode,
    start_line: u64,
    end_line: Option<u64>,
    running: bool,
    viewport_line_ids: &[u64],
) -> LiveTailProjection {
    if mode != PresentationMode::Flow || start_line == 0 {
        return LiveTailProjection::FailClosed;
    }
    match (running, end_line) {
        (true, None) => match map_primary_clip(start_line, viewport_line_ids) {
            Some((first_row, row_count)) => LiveTailProjection::PrimaryFrame(PrimaryFrameClip {
                start_line,
                first_row,
                row_count,
            }),
            None => LiveTailProjection::FailClosed,
        },
        (false, Some(end)) if end >= start_line => LiveTailProjection::History(HistorySpan {
            start_line,
            end_line: end,
        }),
        _ => LiveTailProjection::FailClosed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_clip_skips_preceding_viewport_rows() {
        // Two sequential commands: rows 0..1 are the previous Block, 2..4 are
        // the running command starting at line 30.
        let ids = [10_u64, 11, 30, 31, 32];
        assert_eq!(map_primary_clip(30, &ids), Some((2, 3)));
        let projection = project_block_output(PresentationMode::Flow, 30, None, true, &ids);
        assert_eq!(
            projection,
            LiveTailProjection::PrimaryFrame(PrimaryFrameClip {
                start_line: 30,
                first_row: 2,
                row_count: 3,
            })
        );
    }

    #[test]
    fn reordered_viewport_line_ids_still_map_from_first_owned_row() {
        // After CSI T / insert-line, unique LineIds need not be monotonic in
        // row order. Mapping still starts at the first id >= start_line.
        let ids = [1_u64, 4, 2];
        assert_eq!(map_primary_clip(2, &ids), Some((1, 2)));
    }

    #[test]
    fn reordered_preceding_row_is_not_drawn_inside_running_block() {
        // CSI T / reverse-index can place an older line between owned rows.
        // The clip stops before that row instead of painting it in the Block.
        let ids = [30_u64, 10, 31];
        assert_eq!(map_primary_clip(30, &ids), Some((0, 1)));
    }

    #[test]
    fn scrolled_off_start_keeps_full_viewport_for_running_block() {
        // start_line has left the viewport; every visible row belongs to the
        // running Block.
        let ids = [100_u64, 101, 102, 103];
        assert_eq!(map_primary_clip(20, &ids), Some((0, 4)));
    }

    #[test]
    fn start_not_yet_on_viewport_fails_closed() {
        let ids = [1_u64, 2, 3];
        assert_eq!(map_primary_clip(50, &ids), None);
        assert_eq!(
            project_block_output(PresentationMode::Flow, 50, None, true, &ids),
            LiveTailProjection::FailClosed
        );
    }

    #[test]
    fn missing_or_zero_line_ids_fail_closed() {
        assert_eq!(map_primary_clip(10, &[]), None);
        assert_eq!(map_primary_clip(10, &[0, 11, 12]), None);
        assert_eq!(
            project_block_output(PresentationMode::Flow, 10, None, true, &[]),
            LiveTailProjection::FailClosed
        );
    }

    #[test]
    fn completion_hands_off_to_trusted_history_span() {
        let ids = [20_u64, 21, 22];
        let running = project_block_output(PresentationMode::Flow, 20, None, true, &ids);
        let completed = project_block_output(PresentationMode::Flow, 20, Some(1019), false, &ids);
        assert_ne!(running, completed);
        assert_eq!(
            completed,
            LiveTailProjection::History(HistorySpan {
                start_line: 20,
                end_line: 1019,
            })
        );
    }

    #[test]
    fn seq_long_output_stays_one_running_clip_identity() {
        // Many lines have scrolled; start is above the viewport. The projection
        // remains one PrimaryFrame clip anchored at the same start_line.
        let early = [50_u64, 51, 52];
        let later = [900_u64, 901, 902, 903];
        let first = project_block_output(PresentationMode::Flow, 20, None, true, &early);
        let after_many = project_block_output(PresentationMode::Flow, 20, None, true, &later);
        assert!(matches!(first, LiveTailProjection::PrimaryFrame(c) if c.start_line == 20));
        assert!(matches!(after_many, LiveTailProjection::PrimaryFrame(c) if c.start_line == 20));
        assert_ne!(first, after_many); // row slice tracks the viewport
        assert_eq!(
            after_many,
            LiveTailProjection::PrimaryFrame(PrimaryFrameClip {
                start_line: 20,
                first_row: 0,
                row_count: 4,
            })
        );
    }

    #[test]
    fn raw_and_tui_fail_closed() {
        let ids = [10_u64, 11];
        assert_eq!(
            project_block_output(PresentationMode::Raw, 10, None, true, &ids),
            LiveTailProjection::FailClosed
        );
        assert_eq!(
            project_block_output(PresentationMode::Tui, 10, Some(12), false, &ids),
            LiveTailProjection::FailClosed
        );
    }

    #[test]
    fn stale_or_conflicting_evidence_fail_closed() {
        let ids = [10_u64, 11];
        assert_eq!(
            project_block_output(PresentationMode::Flow, 0, None, true, &ids),
            LiveTailProjection::FailClosed
        );
        assert_eq!(
            project_block_output(PresentationMode::Flow, 10, Some(9), false, &ids),
            LiveTailProjection::FailClosed
        );
        assert_eq!(
            project_block_output(PresentationMode::Flow, 10, Some(15), true, &ids),
            LiveTailProjection::FailClosed
        );
        assert_eq!(
            project_block_output(PresentationMode::Flow, 10, None, false, &ids),
            LiveTailProjection::FailClosed
        );
    }

    #[test]
    fn host_must_not_receive_invented_history_for_running() {
        let ids = [40_u64, 41];
        match project_block_output(PresentationMode::Flow, 40, None, true, &ids) {
            LiveTailProjection::PrimaryFrame(clip) => {
                assert_eq!(clip.first_row, 0);
                assert_eq!(clip.row_count, 2);
            }
            LiveTailProjection::History(_) | LiveTailProjection::FailClosed => {
                panic!("running Flow must not fall back to history or fail closed")
            }
        }
    }
}
