//! Runtime-owned per-execution composer command Block timeline.
//!
//! Owns `CAP_COMMAND_BLOCKS` anchors and lifecycle truth only. It never
//! receives terminal cells, parses prompts, or owns a PTY/renderer.
//! Distinct from `activity_block_timeline` (Pass-8 TerminalActivity metadata).

use std::collections::VecDeque;

use seyal_protocol::framing::MAX_FRAME_PAYLOAD;

/// Per-command text admission limit (matches wire `MAX_COMPOSER_COMMAND_BYTES`).
pub(crate) const MAX_COMMAND_BYTES: usize = 16 * 1024;

/// Maximum retained records. The authoritative bound is encoded size ≤
/// [`MAX_FRAME_PAYLOAD`]; this count is a secondary guard only.
pub(crate) const MAX_BLOCKS_PER_EXECUTION: usize = 128;

const TIMELINE_HEADER_BYTES: usize = 16;
const RECORD_HEADER_BYTES: usize = 36;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct CommandBlockId(u64);

impl CommandBlockId {
    pub(crate) const fn raw(self) -> u64 {
        self.0
    }

    #[cfg(test)]
    pub(crate) const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommandBlockLifecycle {
    Running,
    /// `exit_status` is `None` when the finishing marker was never observed
    /// (ADR-009 mechanism 5); it is never reported as `0`.
    Completed {
        exit_status: Option<i32>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommandBlockRecord {
    pub(crate) id: CommandBlockId,
    pub(crate) command: String,
    pub(crate) start_line: u64,
    pub(crate) end_line: Option<u64>,
    pub(crate) lifecycle: CommandBlockLifecycle,
}

/// Bounded, append-only logical timeline for one `ExecutionId`.
///
/// Callers must invoke `start` only after a trusted shell-integration start
/// event correlates a pending composer admission.
#[derive(Default)]
pub(crate) struct CommandBlockTimeline {
    next_id: u64,
    records: VecDeque<CommandBlockRecord>,
}

impl CommandBlockTimeline {
    /// Reserve a stable Block identity before the trusted start transition.
    pub(crate) fn allocate_id(&mut self) -> Result<CommandBlockId, CommandBlockTimelineError> {
        let id = CommandBlockId(
            self.next_id
                .checked_add(1)
                .ok_or(CommandBlockTimelineError::Exhausted)?,
        );
        self.next_id = id.raw();
        Ok(id)
    }

    pub(crate) fn start(
        &mut self,
        id: CommandBlockId,
        command: String,
        start_line: u64,
    ) -> Result<(), CommandBlockTimelineError> {
        if command.is_empty() || command.len() > MAX_COMMAND_BYTES {
            return Err(CommandBlockTimelineError::InvalidCommand);
        }
        if self.records.iter().any(|record| record.id == id) {
            return Err(CommandBlockTimelineError::InvalidCommand);
        }
        self.make_room_for(command.len())?;
        self.records.push_back(CommandBlockRecord {
            id,
            command,
            start_line,
            end_line: None,
            lifecycle: CommandBlockLifecycle::Running,
        });
        debug_assert!(self.encoded_len() <= MAX_FRAME_PAYLOAD as usize);
        Ok(())
    }

    /// Complete the matching running record. A completion line before
    /// `start_line` is clamped to `start_line`: a command with no output
    /// finishes on the row it started on, and the parser's completion line
    /// then backs up past it. The Block must still complete rather than stay
    /// `Running` with no owner (#1015). The wire range cannot express an empty
    /// output yet; that marker is proposed in the ADR-009 amendment (#1041).
    pub(crate) fn complete(
        &mut self,
        id: CommandBlockId,
        end_line: u64,
        exit_status: Option<i32>,
    ) -> Result<(), CommandBlockTimelineError> {
        let record = self
            .records
            .iter_mut()
            .find(|record| record.id == id)
            .ok_or(CommandBlockTimelineError::UnknownBlock)?;
        if record.lifecycle != CommandBlockLifecycle::Running {
            return Err(CommandBlockTimelineError::InvalidCompletion);
        }
        record.end_line = Some(end_line.max(record.start_line));
        record.lifecycle = CommandBlockLifecycle::Completed { exit_status };
        Ok(())
    }

    pub(crate) fn records(&self) -> impl ExactSizeIterator<Item = &CommandBlockRecord> {
        self.records.iter()
    }

    pub(crate) fn encoded_len(&self) -> usize {
        TIMELINE_HEADER_BYTES
            + self
                .records
                .iter()
                .map(|record| RECORD_HEADER_BYTES + record.command.len())
                .sum::<usize>()
    }

    fn make_room_for(&mut self, command_len: usize) -> Result<(), CommandBlockTimelineError> {
        let needed = RECORD_HEADER_BYTES + command_len;
        if TIMELINE_HEADER_BYTES + needed > MAX_FRAME_PAYLOAD as usize {
            return Err(CommandBlockTimelineError::Capacity);
        }
        while self.records.len() == MAX_BLOCKS_PER_EXECUTION
            || self.encoded_len() + needed > MAX_FRAME_PAYLOAD as usize
        {
            let Some(index) = self.records.iter().position(|record| {
                matches!(record.lifecycle, CommandBlockLifecycle::Completed { .. })
            }) else {
                return Err(CommandBlockTimelineError::Capacity);
            };
            self.records.remove(index);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommandBlockTimelineError {
    InvalidCommand,
    Capacity,
    Exhausted,
    UnknownBlock,
    InvalidCompletion,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_one_ordered_record_per_accepted_command() {
        let mut timeline = CommandBlockTimeline::default();
        let first = timeline.allocate_id().unwrap();
        timeline.start(first, "printf one".into(), 41).unwrap();
        let second = timeline.allocate_id().unwrap();
        timeline.start(second, "printf two".into(), 44).unwrap();

        assert_ne!(first, second);
        assert_eq!(
            timeline
                .records()
                .map(|record| record.command.as_str())
                .collect::<Vec<_>>(),
            ["printf one", "printf two"]
        );
        assert!(timeline
            .records()
            .all(|record| record.lifecycle == CommandBlockLifecycle::Running));
    }

    #[test]
    fn only_runtime_completion_can_close_the_matching_running_record() {
        let mut timeline = CommandBlockTimeline::default();
        let id = timeline.allocate_id().unwrap();
        timeline.start(id, "false".into(), 5).unwrap();
        assert_eq!(
            timeline.complete(CommandBlockId(99), 7, Some(1)),
            Err(CommandBlockTimelineError::UnknownBlock)
        );
        timeline.complete(id, 7, Some(1)).unwrap();
        let record = timeline.records().next().unwrap();
        assert_eq!(record.end_line, Some(7));
        assert_eq!(
            record.lifecycle,
            CommandBlockLifecycle::Completed {
                exit_status: Some(1)
            }
        );
        assert_eq!(
            timeline.complete(id, 8, Some(0)),
            Err(CommandBlockTimelineError::InvalidCompletion),
            "a completed record never completes again"
        );
    }

    #[test]
    fn zero_output_completion_before_start_clamps_and_completes() {
        // `true`/`cd`: C and D land on the same empty row; the parser's
        // completion line backs up one row past start_line (#1015 review).
        let mut timeline = CommandBlockTimeline::default();
        let id = timeline.allocate_id().unwrap();
        timeline.start(id, "true".into(), 5).unwrap();
        timeline.complete(id, 4, Some(0)).unwrap();
        let record = timeline.records().next().unwrap();
        assert_eq!(
            record.end_line,
            Some(5),
            "end_line is never before start_line"
        );
        assert_eq!(
            record.lifecycle,
            CommandBlockLifecycle::Completed {
                exit_status: Some(0)
            },
            "a zero-output Block completes instead of staying Running"
        );
    }

    #[test]
    fn completed_records_roll_forward_without_evicting_active_work() {
        let mut timeline = CommandBlockTimeline::default();
        let mut first = None;
        for index in 0..MAX_BLOCKS_PER_EXECUTION {
            let id = timeline.allocate_id().unwrap();
            timeline
                .start(id, format!("printf {index}"), index as u64 + 1)
                .unwrap();
            timeline.complete(id, index as u64 + 2, Some(0)).unwrap();
            first.get_or_insert(id);
        }
        let active = timeline.allocate_id().unwrap();
        timeline
            .start(active, "printf active".into(), 10_000)
            .unwrap();
        assert!(timeline.records().any(|record| record.id == active));
        assert!(!timeline.records().any(|record| Some(record.id) == first));
    }

    #[test]
    fn cumulative_wire_budget_evicts_completed_before_exceeding_frame() {
        let mut timeline = CommandBlockTimeline::default();
        let large = "x".repeat(MAX_COMMAND_BYTES);
        for index in 0..16 {
            let id = timeline.allocate_id().unwrap();
            timeline.start(id, large.clone(), index as u64 + 1).unwrap();
            timeline.complete(id, index as u64 + 2, Some(0)).unwrap();
        }
        assert!(timeline.encoded_len() <= MAX_FRAME_PAYLOAD as usize);
        assert!(timeline.records().len() < 16);
        let active = timeline.allocate_id().unwrap();
        timeline
            .start(active, large, 100)
            .expect("running command fits after eviction");
        assert!(timeline.encoded_len() <= MAX_FRAME_PAYLOAD as usize);
    }

    #[test]
    fn near_limit_running_blocks_reject_additional_admission() {
        let mut timeline = CommandBlockTimeline::default();
        let large = "y".repeat(MAX_COMMAND_BYTES);
        let first = timeline.allocate_id().unwrap();
        timeline.start(first, large.clone(), 1).unwrap();
        // Do not complete — only running records remain, so eviction cannot help.
        loop {
            let id = timeline.allocate_id().unwrap();
            match timeline.start(id, large.clone(), 2) {
                Ok(()) => continue,
                Err(CommandBlockTimelineError::Capacity) => return,
                Err(other) => panic!("unexpected error: {other:?}"),
            }
        }
    }
}
