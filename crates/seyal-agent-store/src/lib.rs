//! Agent-domain persistence ownership boundary.
//!
//! AB-0.3 starts from explicit aggregate-local sequence and replay value types.
//! No global event clock is defined here. SQLite/WAL durability, snapshots,
//! retention and fault-injection are layered onto these primitives in this slice.

use std::num::NonZeroU64;

mod sqlite;

pub use seyal_agent_core::{AgentRunId, AttemptId, WorkItemId, WorkScopeId};
pub use sqlite::{
    AgentStore, PersistedAgentRun, PersistedLiveness, StoreError, OUTPUT_SEGMENT_LEN,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AggregateId {
    WorkScope(WorkScopeId),
    WorkItem(WorkItemId),
    Attempt(AttemptId),
    AgentRun(AgentRunId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AggregateSequence(NonZeroU64);

impl AggregateSequence {
    pub const FIRST: Self = Self(NonZeroU64::MIN);

    pub const fn get(self) -> u64 {
        self.0.get()
    }

    pub fn next(self) -> Option<Self> {
        self.get()
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .map(Self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryGap {
    pub aggregate_id: AggregateId,
    pub requested_after: AggregateSequence,
    pub earliest_available: AggregateSequence,
    pub current_snapshot_sequence: Option<AggregateSequence>,
    pub reason: HistoryGapReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryGapReason {
    RetentionTruncation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AggregateEventEnvelopeV1 {
    pub aggregate_id: AggregateId,
    pub sequence: AggregateSequence,
    pub event_id: u128,
    pub kind: u16,
    pub payload: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotPosition {
    pub aggregate_id: AggregateId,
    pub incorporated_through: AggregateSequence,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_sequence_is_monotonic_and_non_zero() {
        let first = AggregateSequence::FIRST;
        let second = first.next().unwrap();
        let third = second.next().unwrap();

        assert_eq!(first.get(), 1);
        assert!(first < second);
        assert!(second < third);
    }

    #[test]
    fn event_order_is_scoped_to_its_aggregate() {
        let scope = AggregateId::WorkScope(WorkScopeId::new());
        let run = AggregateId::AgentRun(AgentRunId::new());

        let scope_event = AggregateEventEnvelopeV1 {
            aggregate_id: scope,
            sequence: AggregateSequence::FIRST,
            event_id: 1,
            kind: 1,
            payload: b"scope".to_vec(),
        };
        let run_event = AggregateEventEnvelopeV1 {
            aggregate_id: run,
            sequence: AggregateSequence::FIRST,
            event_id: 2,
            kind: 1,
            payload: b"run".to_vec(),
        };

        assert_ne!(scope_event.aggregate_id, run_event.aggregate_id);
        assert_eq!(scope_event.sequence, run_event.sequence);
        assert_ne!(scope_event.event_id, run_event.event_id);
    }

    #[test]
    fn snapshot_position_names_the_exact_aggregate_sequence() {
        let aggregate_id = AggregateId::WorkItem(WorkItemId::new());
        let position = SnapshotPosition {
            aggregate_id,
            incorporated_through: AggregateSequence::FIRST.next().unwrap(),
        };

        assert_eq!(position.aggregate_id, aggregate_id);
        assert_eq!(position.incorporated_through.get(), 2);
    }
}
