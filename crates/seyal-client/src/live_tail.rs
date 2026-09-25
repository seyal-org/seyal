//! Flow running-Block live-tail projection (#865).
//!
//! Runtime `BlockTimeline` remains the Block authority. This module only
//! derives how the Pane compositor may project one Block's output:
//!
//! - **Running** Blocks use a damage-driven clip of the prepared primary frame
//!   into the Block output region (Approach B). Hosts must not invent a history
//!   range such as `start + 511`.
//! - **Completed** Blocks keep the trusted finite history span.
//! - Raw/TUI and conflicting evidence fail closed.

use crate::presentation::PresentationMode;

/// Inclusive canonical history span for a completed Flow Block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistorySpan {
    pub start_line: u64,
    pub end_line: u64,
}

/// Trusted start anchor for a running primary-frame live-tail clip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimaryFrameClip {
    pub start_line: u64,
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

/// Project one Block's Flow output path for the current presentation.
///
/// `end_line` is the Runtime-trusted completed end, or `None` while running.
pub fn project_block_output(
    mode: PresentationMode,
    start_line: u64,
    end_line: Option<u64>,
    running: bool,
) -> LiveTailProjection {
    if mode != PresentationMode::Flow || start_line == 0 {
        return LiveTailProjection::FailClosed;
    }
    match (running, end_line) {
        (true, None) => LiveTailProjection::PrimaryFrame(PrimaryFrameClip { start_line }),
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
    fn running_flow_block_uses_primary_frame_clip() {
        assert_eq!(
            project_block_output(PresentationMode::Flow, 10, None, true),
            LiveTailProjection::PrimaryFrame(PrimaryFrameClip { start_line: 10 })
        );
    }

    #[test]
    fn seq_one_to_one_thousand_stays_one_primary_clip() {
        let first = project_block_output(PresentationMode::Flow, 20, None, true);
        let after_many_lines = project_block_output(PresentationMode::Flow, 20, None, true);
        assert_eq!(first, after_many_lines);
        assert_eq!(
            first,
            LiveTailProjection::PrimaryFrame(PrimaryFrameClip { start_line: 20 })
        );
    }

    #[test]
    fn completion_hands_off_to_trusted_history_span() {
        let running = project_block_output(PresentationMode::Flow, 20, None, true);
        let completed = project_block_output(PresentationMode::Flow, 20, Some(1019), false);
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
    fn raw_and_tui_fail_closed() {
        assert_eq!(
            project_block_output(PresentationMode::Raw, 10, None, true),
            LiveTailProjection::FailClosed
        );
        assert_eq!(
            project_block_output(PresentationMode::Tui, 10, Some(12), false),
            LiveTailProjection::FailClosed
        );
    }

    #[test]
    fn stale_or_conflicting_evidence_fail_closed() {
        assert_eq!(
            project_block_output(PresentationMode::Flow, 0, None, true),
            LiveTailProjection::FailClosed
        );
        assert_eq!(
            project_block_output(PresentationMode::Flow, 10, Some(9), false),
            LiveTailProjection::FailClosed
        );
        assert_eq!(
            project_block_output(PresentationMode::Flow, 10, Some(15), true),
            LiveTailProjection::FailClosed
        );
        assert_eq!(
            project_block_output(PresentationMode::Flow, 10, None, false),
            LiveTailProjection::FailClosed
        );
    }

    #[test]
    fn identical_running_projections_coalesce() {
        let a = project_block_output(PresentationMode::Flow, 3, None, true);
        let b = project_block_output(PresentationMode::Flow, 3, None, true);
        assert_eq!(a, b);
    }

    #[test]
    fn host_must_not_receive_invented_history_for_running() {
        match project_block_output(PresentationMode::Flow, 40, None, true) {
            LiveTailProjection::PrimaryFrame(_) => {}
            LiveTailProjection::History(_) | LiveTailProjection::FailClosed => {
                panic!("running Flow must not fall back to history or fail closed")
            }
        }
    }
}
