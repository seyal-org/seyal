//! Runtime-owned per-execution composer command Block timeline.
//!
//! Owns `CAP_COMMAND_BLOCKS` anchors and lifecycle truth only. It never
//! receives terminal cells, parses prompts, or owns a PTY/renderer.
//! Distinct from `activity_block_timeline` (Pass-8 TerminalActivity metadata).

use std::collections::VecDeque;
use std::time::Instant;

use seyal_protocol::framing::{BlockTimelineSchema, MAX_FRAME_PAYLOAD};

/// Per-command text admission limit (matches wire `MAX_COMPOSER_COMMAND_BYTES`).
pub(crate) const MAX_COMMAND_BYTES: usize = 16 * 1024;

/// Maximum retained records. The authoritative bound is encoded size ≤
/// [`MAX_FRAME_PAYLOAD`]; this count is a secondary guard only.
pub(crate) const MAX_BLOCKS_PER_EXECUTION: usize = 128;

const TIMELINE_HEADER_BYTES: usize = 16;
/// Admission reserves the largest supported record shape so every retained
/// timeline encodes within one frame for both schemas (ADR-009 duration
/// amendment).
const ADMISSION_SCHEMA: BlockTimelineSchema = BlockTimelineSchema::Duration;

/// A trusted shell-integration boundary observed by Runtime: the monotonic
/// instant it was drained and the PTY read it arrived in. Callers supply it,
/// so tests inject the clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BoundaryStamp {
    pub(crate) at: Instant,
    pub(crate) read: u64,
}

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
    /// Runtime-measured trusted `C`→`D` interval; `Some` only after a matching
    /// trusted `D` in a later PTY read. Immutable once set.
    pub(crate) duration_ns: Option<u64>,
    /// The trusted `C` stamp, held only while the Block is Running.
    started: Option<BoundaryStamp>,
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
        started: Option<BoundaryStamp>,
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
            duration_ns: None,
            started,
        });
        debug_assert!(self.encoded_len(ADMISSION_SCHEMA) <= MAX_FRAME_PAYLOAD as usize);
        Ok(())
    }

    /// Complete the matching running record. A completion line before
    /// `start_line` is clamped to `start_line`: a command with no output
    /// finishes on the row it started on, and the parser's completion line
    /// then backs up past it. The Block must still complete rather than stay
    /// `Running` with no owner (#1015). The wire range cannot express an empty
    /// output yet; that marker is proposed in the ADR-009 amendment (#1041).
    ///
    /// `ended` is the trusted `D` stamp. Duration is recorded only when a
    /// trusted `D` (exit status present) arrives in a later PTY read than the
    /// trusted `C`; same-read, missing, untrusted or lifecycle-only ends stay
    /// unknown (ADR-009 2026-09-19 amendment items 3–4).
    pub(crate) fn complete(
        &mut self,
        id: CommandBlockId,
        end_line: u64,
        exit_status: Option<i32>,
        ended: Option<BoundaryStamp>,
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
        record.duration_ns = match (record.started.take(), ended, exit_status) {
            (Some(start), Some(end), Some(_)) if end.read != start.read => Some(
                u64::try_from(end.at.saturating_duration_since(start.at).as_nanos())
                    .unwrap_or(u64::MAX),
            ),
            _ => None,
        };
        Ok(())
    }

    pub(crate) fn records(&self) -> impl ExactSizeIterator<Item = &CommandBlockRecord> {
        self.records.iter()
    }

    pub(crate) fn encoded_len(&self, schema: BlockTimelineSchema) -> usize {
        TIMELINE_HEADER_BYTES
            + self
                .records
                .iter()
                .map(|record| schema.record_header_len() + record.command.len())
                .sum::<usize>()
    }

    /// Plan the oldest-completed evictions that let one more record fit in
    /// both schemas, then apply them only if the admission succeeds; a
    /// rejection leaves the timeline unchanged. Running records are never
    /// evicted.
    fn make_room_for(&mut self, command_len: usize) -> Result<(), CommandBlockTimelineError> {
        let header = ADMISSION_SCHEMA.record_header_len();
        let budget = MAX_FRAME_PAYLOAD as usize;
        let needed = header + command_len;
        if TIMELINE_HEADER_BYTES + needed > budget {
            return Err(CommandBlockTimelineError::Capacity);
        }
        let mut len = self.encoded_len(ADMISSION_SCHEMA);
        let mut count = self.records.len();
        let mut evict = Vec::new();
        let mut completed = self.records.iter().enumerate().filter(|(_, record)| {
            matches!(record.lifecycle, CommandBlockLifecycle::Completed { .. })
        });
        while count == MAX_BLOCKS_PER_EXECUTION || len + needed > budget {
            let Some((index, record)) = completed.next() else {
                return Err(CommandBlockTimelineError::Capacity);
            };
            len -= header + record.command.len();
            count -= 1;
            evict.push(index);
        }
        for index in evict.into_iter().rev() {
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
    use std::time::Duration;

    use super::*;

    #[test]
    fn creates_one_ordered_record_per_accepted_command() {
        let mut timeline = CommandBlockTimeline::default();
        let first = timeline.allocate_id().unwrap();
        timeline
            .start(first, "printf one".into(), 41, None)
            .unwrap();
        let second = timeline.allocate_id().unwrap();
        timeline
            .start(second, "printf two".into(), 44, None)
            .unwrap();

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
        timeline.start(id, "false".into(), 5, None).unwrap();
        assert_eq!(
            timeline.complete(CommandBlockId(99), 7, Some(1), None),
            Err(CommandBlockTimelineError::UnknownBlock)
        );
        timeline.complete(id, 7, Some(1), None).unwrap();
        let record = timeline.records().next().unwrap();
        assert_eq!(record.end_line, Some(7));
        assert_eq!(
            record.lifecycle,
            CommandBlockLifecycle::Completed {
                exit_status: Some(1)
            }
        );
        assert_eq!(
            timeline.complete(id, 8, Some(0), None),
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
        timeline.start(id, "true".into(), 5, None).unwrap();
        timeline.complete(id, 4, Some(0), None).unwrap();
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
                .start(id, format!("printf {index}"), index as u64 + 1, None)
                .unwrap();
            timeline
                .complete(id, index as u64 + 2, Some(0), None)
                .unwrap();
            first.get_or_insert(id);
        }
        let active = timeline.allocate_id().unwrap();
        timeline
            .start(active, "printf active".into(), 10_000, None)
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
            timeline
                .start(id, large.clone(), index as u64 + 1, None)
                .unwrap();
            timeline
                .complete(id, index as u64 + 2, Some(0), None)
                .unwrap();
        }
        assert!(timeline.encoded_len(BlockTimelineSchema::Duration) <= MAX_FRAME_PAYLOAD as usize);
        assert!(timeline.records().len() < 16);
        let active = timeline.allocate_id().unwrap();
        timeline
            .start(active, large, 100, None)
            .expect("running command fits after eviction");
        assert!(timeline.encoded_len(BlockTimelineSchema::Duration) <= MAX_FRAME_PAYLOAD as usize);
    }

    fn stamp(base: Instant, after: Duration, read: u64) -> BoundaryStamp {
        BoundaryStamp {
            at: base + after,
            read,
        }
    }

    #[test]
    fn trusted_d_in_a_later_read_records_monotonic_duration() {
        let base = Instant::now();
        let mut timeline = CommandBlockTimeline::default();
        let id = timeline.allocate_id().unwrap();
        timeline
            .start(
                id,
                "sleep 2".into(),
                3,
                Some(stamp(base, Duration::ZERO, 7)),
            )
            .unwrap();
        assert_eq!(
            timeline.records().next().unwrap().duration_ns,
            None,
            "Running has none"
        );
        timeline
            .complete(
                id,
                5,
                Some(0),
                Some(stamp(base, Duration::from_millis(2_013), 9)),
            )
            .unwrap();
        assert_eq!(
            timeline.records().next().unwrap().duration_ns,
            Some(2_013_000_000)
        );
    }

    #[test]
    fn same_read_c_and_d_leave_duration_unknown() {
        let base = Instant::now();
        let mut timeline = CommandBlockTimeline::default();
        let id = timeline.allocate_id().unwrap();
        timeline
            .start(id, "true".into(), 3, Some(stamp(base, Duration::ZERO, 4)))
            .unwrap();
        timeline
            .complete(
                id,
                3,
                Some(0),
                Some(stamp(base, Duration::from_micros(40), 4)),
            )
            .unwrap();
        assert_eq!(
            timeline.records().next().unwrap().duration_ns,
            None,
            "coalesced C/D must be unknown, never a near-zero value"
        );
    }

    #[test]
    fn completions_without_a_trusted_d_have_unknown_duration() {
        let base = Instant::now();
        let mut timeline = CommandBlockTimeline::default();
        // Lifecycle-only / lost D: no exit status and no end stamp.
        let lost = timeline.allocate_id().unwrap();
        timeline
            .start(lost, "make".into(), 1, Some(stamp(base, Duration::ZERO, 1)))
            .unwrap();
        timeline.complete(lost, 2, None, None).unwrap();
        // An end stamp without an exit status (not a trusted D) still yields none.
        let stamped = timeline.allocate_id().unwrap();
        timeline
            .start(
                stamped,
                "make".into(),
                3,
                Some(stamp(base, Duration::ZERO, 2)),
            )
            .unwrap();
        timeline
            .complete(
                stamped,
                4,
                None,
                Some(stamp(base, Duration::from_secs(1), 3)),
            )
            .unwrap();
        // A Block started without a stamp has no measurable interval.
        let unstamped = timeline.allocate_id().unwrap();
        timeline.start(unstamped, "make".into(), 5, None).unwrap();
        timeline
            .complete(
                unstamped,
                6,
                Some(0),
                Some(stamp(base, Duration::from_secs(1), 4)),
            )
            .unwrap();
        assert!(timeline
            .records()
            .all(|record| record.duration_ns.is_none()));
    }

    #[test]
    fn a_completed_duration_is_immutable() {
        let base = Instant::now();
        let mut timeline = CommandBlockTimeline::default();
        let id = timeline.allocate_id().unwrap();
        timeline
            .start(
                id,
                "sleep 1".into(),
                1,
                Some(stamp(base, Duration::ZERO, 1)),
            )
            .unwrap();
        timeline
            .complete(id, 2, Some(0), Some(stamp(base, Duration::from_secs(1), 2)))
            .unwrap();
        assert_eq!(
            timeline.complete(id, 9, Some(0), Some(stamp(base, Duration::from_secs(5), 3))),
            Err(CommandBlockTimelineError::InvalidCompletion)
        );
        assert_eq!(
            timeline.records().next().unwrap().duration_ns,
            Some(1_000_000_000)
        );
    }

    #[test]
    fn rejected_admission_leaves_the_timeline_unchanged() {
        let mut timeline = CommandBlockTimeline::default();
        let large = "z".repeat(MAX_COMMAND_BYTES);
        // One completed record that could be evicted, then running records that
        // cannot: the final admission still cannot fit even after evicting it.
        let done = timeline.allocate_id().unwrap();
        timeline.start(done, large.clone(), 1, None).unwrap();
        timeline.complete(done, 2, Some(0), None).unwrap();
        loop {
            let id = timeline.allocate_id().unwrap();
            let before: Vec<_> = timeline.records().cloned().collect();
            match timeline.start(id, large.clone(), 3, None) {
                Ok(()) => continue,
                Err(CommandBlockTimelineError::Capacity) => {
                    let after: Vec<_> = timeline.records().cloned().collect();
                    assert_eq!(after, before, "no partial eviction on rejection");
                    return;
                }
                Err(other) => panic!("unexpected error: {other:?}"),
            }
        }
    }

    #[test]
    fn admission_keeps_every_retained_timeline_within_both_schemas() {
        let mut timeline = CommandBlockTimeline::default();
        let large = "w".repeat(MAX_COMMAND_BYTES);
        for index in 0..40u64 {
            let id = timeline.allocate_id().unwrap();
            timeline.start(id, large.clone(), index + 1, None).unwrap();
            timeline.complete(id, index + 2, Some(0), None).unwrap();
            for schema in [BlockTimelineSchema::Legacy, BlockTimelineSchema::Duration] {
                assert!(
                    timeline.encoded_len(schema) <= MAX_FRAME_PAYLOAD as usize,
                    "{schema:?} after {index}"
                );
            }
        }
    }

    #[test]
    fn near_limit_running_blocks_reject_additional_admission() {
        let mut timeline = CommandBlockTimeline::default();
        let large = "y".repeat(MAX_COMMAND_BYTES);
        let first = timeline.allocate_id().unwrap();
        timeline.start(first, large.clone(), 1, None).unwrap();
        // Do not complete — only running records remain, so eviction cannot help.
        loop {
            let id = timeline.allocate_id().unwrap();
            match timeline.start(id, large.clone(), 2, None) {
                Ok(()) => continue,
                Err(CommandBlockTimelineError::Capacity) => return,
                Err(other) => panic!("unexpected error: {other:?}"),
            }
        }
    }
}
