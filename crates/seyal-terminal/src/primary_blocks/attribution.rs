//! Ordered attribution journal (StreamSequence + UnitVersion).
//!
//! Production capacity must cover one full reactor read plus marker/rewrite
//! margin. When a burst still exceeds that window, production refuses the new
//! event (no silent drops of prior markers/mutations — F06) and keeps Block VT
//! ownership; the refused write stays on the active region without forged
//! journal admission (F05). Artificially smaller capacities are F06
//! fault-injection only (exempt F05): overflow classifies unresolved ownership
//! as Unstructured.

use std::collections::VecDeque;

use super::types::{BlockId, CellHandle, RegionId};
use crate::LineId;

/// Monotonic per-execution sequence, independent of OS read boundaries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct StreamSequence(pub u64);

/// Stable identity for a source unit version across relocate/overwrite.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UnitVersion {
    pub line_id: LineId,
    pub unit_offset: u64,
    /// Last-write generation; overwrite mints a new value.
    pub content_generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerKind {
    PromptStart,
    CommandStart,
    CommandFinished,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrimaryEvent {
    CellMutation {
        sequence: StreamSequence,
        version: UnitVersion,
        handle: CellHandle,
        region: RegionId,
    },
    ShellBoundaryCandidate {
        sequence: StreamSequence,
        marker: MarkerKind,
        /// Pre-armed Runtime Block correlation when known.
        block: Option<BlockId>,
        version: UnitVersion,
        column: u16,
        /// Nonce bytes already filtered against the pre-armed fence when present.
        token_accepted: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JournalOverflowPolicy {
    /// Production: capacity sized so overflow is unreachable for a full read.
    ProductionUnreachable,
    /// F06 fault injection: overflow classifies unresolved as Unstructured.
    FaultInject,
}

pub(crate) struct AttributionJournal {
    next_sequence: u64,
    capacity: usize,
    events: VecDeque<PrimaryEvent>,
    policy: JournalOverflowPolicy,
    overflowed: bool,
}

impl AttributionJournal {
    pub(crate) fn production(capacity: usize) -> Self {
        Self {
            next_sequence: 1,
            capacity,
            events: VecDeque::with_capacity(capacity),
            policy: JournalOverflowPolicy::ProductionUnreachable,
            overflowed: false,
        }
    }

    pub(crate) fn fault_inject(capacity: usize) -> Self {
        Self {
            next_sequence: 1,
            capacity,
            events: VecDeque::with_capacity(capacity),
            policy: JournalOverflowPolicy::FaultInject,
            overflowed: false,
        }
    }

    pub(crate) fn next_sequence(&mut self) -> StreamSequence {
        let seq = StreamSequence(self.next_sequence);
        self.next_sequence = self.next_sequence.saturating_add(1);
        seq
    }

    /// Soft-cap for Finding F / D3 mid-feed yield: leave headroom for markers
    /// before hard capacity refuse. Not a capacity raise of the hard limit.
    pub(crate) fn soft_cap(&self) -> usize {
        let headroom = (self.capacity / 8).max(1);
        self.capacity.saturating_sub(headroom).max(1)
    }

    pub(crate) fn at_or_above_soft_cap(&self) -> bool {
        !self.events.is_empty() && self.events.len() >= self.soft_cap()
    }

    pub(crate) fn push(&mut self, event: PrimaryEvent) -> Result<(), ()> {
        if self.events.len() >= self.capacity {
            self.overflowed = true;
            // Both policies refuse the new event and retain prior journal
            // contents. FaultInject callers fail-closed ownership; production
            // callers keep Block ownership without silent marker/event drops.
            return Err(());
        }
        self.events.push_back(event);
        Ok(())
    }

    /// Admit a shell boundary without discarding unresolved journal events.
    ///
    /// Preferential CellMutation eviction is forbidden until a Runtime consumer
    /// proves those mutations were already finalized (#1484 Finding F / D2).
    /// At capacity the marker is refused (`Err`) and prior events are retained;
    /// callers must not enqueue the paired `shell_events` entry (atomic admit).
    /// ShellBoundaryCandidate entries already in the journal are never dropped.
    pub(crate) fn push_shell_boundary(&mut self, event: PrimaryEvent) -> Result<(), ()> {
        debug_assert!(matches!(event, PrimaryEvent::ShellBoundaryCandidate { .. }));
        if self.events.len() >= self.capacity {
            self.overflowed = true;
            return Err(());
        }
        self.events.push_back(event);
        Ok(())
    }

    pub(crate) fn overflowed(&self) -> bool {
        self.overflowed
    }

    pub(crate) fn policy(&self) -> JournalOverflowPolicy {
        self.policy
    }

    /// F06 fault-inject: overflow must fail-closed to Unstructured. Production
    /// overflow refuses journal admission but must not end Block ownership (#1484).
    pub(crate) fn fail_closed_on_overflow(&self) -> bool {
        self.overflowed && matches!(self.policy, JournalOverflowPolicy::FaultInject)
    }

    pub(crate) fn drain(&mut self) -> Vec<PrimaryEvent> {
        // Clear the sticky flag after Runtime has observed this window so a
        // single tall burst cannot re-trigger fail-closed on every later turn.
        self.overflowed = false;
        self.events.drain(..).collect()
    }

    pub(crate) fn events(&self) -> &VecDeque<PrimaryEvent> {
        &self.events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primary_blocks::types::{CellHandle, RegionId};

    #[test]
    fn stream_sequence_is_monotonic_and_independent_of_capacity() {
        let mut journal = AttributionJournal::production(4);
        let a = journal.next_sequence();
        let b = journal.next_sequence();
        assert!(b > a);
    }

    #[test]
    fn soft_cap_is_below_hard_capacity() {
        let journal = AttributionJournal::production(64);
        assert!(journal.soft_cap() < 64);
        assert!(journal.soft_cap() >= 64 - (64 / 8));
        assert!(!journal.at_or_above_soft_cap());
    }

    #[test]
    fn fault_inject_overflow_sets_flag_without_dropping_prior_events() {
        let mut journal = AttributionJournal::fault_inject(1);
        let seq = journal.next_sequence();
        let version = UnitVersion {
            line_id: LineId(1),
            unit_offset: 0,
            content_generation: 1,
        };
        journal
            .push(PrimaryEvent::CellMutation {
                sequence: seq,
                version,
                handle: CellHandle {
                    page: 0,
                    slot: 0,
                    generation: 1,
                },
                region: RegionId(1),
            })
            .unwrap();
        let seq2 = journal.next_sequence();
        assert!(journal
            .push(PrimaryEvent::CellMutation {
                sequence: seq2,
                version: UnitVersion {
                    content_generation: 2,
                    ..version
                },
                handle: CellHandle {
                    page: 0,
                    slot: 1,
                    generation: 1,
                },
                region: RegionId(1),
            })
            .is_err());
        assert!(journal.overflowed());
        assert_eq!(journal.drain().len(), 1);
    }

    #[test]
    fn f06_d_marker_refused_when_journal_full_without_evicting_cell_mutations() {
        // Finding F / D2: do not discard unresolved CellMutations to admit a marker.
        let mut journal = AttributionJournal::fault_inject(1);
        let version = UnitVersion {
            line_id: LineId(1),
            unit_offset: 0,
            content_generation: 1,
        };
        let cell_seq = journal.next_sequence();
        journal
            .push(PrimaryEvent::CellMutation {
                sequence: cell_seq,
                version,
                handle: CellHandle {
                    page: 0,
                    slot: 0,
                    generation: 1,
                },
                region: RegionId(1),
            })
            .unwrap();
        let marker_seq = journal.next_sequence();
        assert!(journal
            .push_shell_boundary(PrimaryEvent::ShellBoundaryCandidate {
                sequence: marker_seq,
                marker: MarkerKind::CommandStart,
                block: None,
                version,
                column: 0,
                token_accepted: true,
            })
            .is_err());
        assert!(journal.overflowed());
        let drained = journal.drain();
        assert_eq!(drained.len(), 1);
        assert!(matches!(drained[0], PrimaryEvent::CellMutation { .. }));
    }

    #[test]
    fn f06_d_marker_only_full_journal_refuses_additional_marker() {
        let mut journal = AttributionJournal::fault_inject(1);
        let version = UnitVersion {
            line_id: LineId(1),
            unit_offset: 0,
            content_generation: 1,
        };
        let first = journal.next_sequence();
        journal
            .push_shell_boundary(PrimaryEvent::ShellBoundaryCandidate {
                sequence: first,
                marker: MarkerKind::PromptStart,
                block: None,
                version,
                column: 0,
                token_accepted: true,
            })
            .unwrap();
        let second = journal.next_sequence();
        assert!(journal
            .push_shell_boundary(PrimaryEvent::ShellBoundaryCandidate {
                sequence: second,
                marker: MarkerKind::CommandStart,
                block: None,
                version,
                column: 0,
                token_accepted: true,
            })
            .is_err());
        assert_eq!(journal.drain().len(), 1);
    }
}
