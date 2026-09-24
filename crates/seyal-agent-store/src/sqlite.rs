use std::{num::NonZeroU64, path::Path, sync::Mutex};

use rusqlite::{params, Connection, OptionalExtension};

use crate::{
    AggregateEventEnvelopeV1, AggregateId, AggregateSequence, HistoryGap, SnapshotPosition,
};

const SCHEMA_VERSION: i32 = 1;
const MAX_EVENT_PAYLOAD: usize = 64 * 1024;
pub const OUTPUT_SEGMENT_LEN: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PersistedLiveness {
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistedAgentRun {
    pub attempt_id: crate::AttemptId,
    pub binding_generation: u64,
    pub control_generation: u64,
    pub liveness: PersistedLiveness,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreError {
    UnsupportedSchema,
    PayloadTooLarge,
    Corrupt,
    WriteFailed,
    History(HistoryGap),
}

pub struct AgentStore {
    conn: Mutex<Connection>,
}

impl AgentStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let conn = Connection::open(path).map_err(|_| StoreError::WriteFailed)?;
        conn.busy_timeout(std::time::Duration::from_millis(500))
            .map_err(|_| StoreError::WriteFailed)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|_| StoreError::WriteFailed)?;
        conn.pragma_update(None, "synchronous", "FULL")
            .map_err(|_| StoreError::WriteFailed)?;
        let version: i32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|_| StoreError::Corrupt)?;
        if version > SCHEMA_VERSION {
            return Err(StoreError::UnsupportedSchema);
        }
        if version == 0 {
            initialize(&conn)?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn append_event(
        &self,
        aggregate_id: AggregateId,
        payload: &[u8],
    ) -> Result<AggregateSequence, StoreError> {
        if payload.len() > MAX_EVENT_PAYLOAD {
            return Err(StoreError::PayloadTooLarge);
        }
        self.commit_insert(aggregate_id, payload, true)
    }

    pub fn snapshot(
        &self,
        aggregate_id: AggregateId,
        incorporated_through: AggregateSequence,
        payload: &[u8],
    ) -> Result<SnapshotPosition, StoreError> {
        if payload.len() > MAX_EVENT_PAYLOAD {
            return Err(StoreError::PayloadTooLarge);
        }
        let conn = self.conn.lock().expect("agent store lock");
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;
        let (kind, id) = aggregate_key(aggregate_id);
        tx.execute(
            "INSERT INTO aggregate_snapshot (aggregate_kind, aggregate_id, incorporated_through, payload)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (aggregate_kind, aggregate_id) DO UPDATE SET
               incorporated_through = excluded.incorporated_through,
               payload = excluded.payload",
            params![kind, id, incorporated_through.get() as i64, payload],
        )
        .map_err(|_| StoreError::WriteFailed)?;
        tx.commit().map_err(|_| StoreError::WriteFailed)?;
        Ok(SnapshotPosition {
            aggregate_id,
            incorporated_through,
        })
    }

    pub fn replay_after(
        &self,
        aggregate_id: AggregateId,
        requested_after: Option<AggregateSequence>,
    ) -> Result<Vec<AggregateEventEnvelopeV1>, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        let (kind, id) = aggregate_key(aggregate_id);
        let earliest: Option<i64> = conn
            .query_row(
                "SELECT MIN(sequence) FROM aggregate_event WHERE aggregate_kind = ?1 AND aggregate_id = ?2",
                params![kind, id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| StoreError::Corrupt)?
            .flatten();
        let Some(earliest) = earliest else {
            return Ok(Vec::new());
        };
        let after = requested_after
            .map(|sequence| sequence.get() as i64)
            .unwrap_or(0);
        if after + 1 < earliest {
            let earliest_available =
                AggregateSequence::from_raw(earliest as u64).ok_or(StoreError::Corrupt)?;
            let requested = requested_after.unwrap_or(AggregateSequence::FIRST);
            return Err(StoreError::History(HistoryGap {
                aggregate_id,
                requested_after: requested,
                earliest_available,
            }));
        }
        let mut statement = conn
            .prepare(
                "SELECT sequence, payload FROM aggregate_event
                 WHERE aggregate_kind = ?1 AND aggregate_id = ?2 AND sequence > ?3
                 ORDER BY sequence",
            )
            .map_err(|_| StoreError::Corrupt)?;
        let rows = statement
            .query_map(params![kind, id, after], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .map_err(|_| StoreError::Corrupt)?;
        let mut events = Vec::new();
        for row in rows {
            let (sequence, payload) = row.map_err(|_| StoreError::Corrupt)?;
            events.push(AggregateEventEnvelopeV1 {
                aggregate_id,
                sequence: AggregateSequence::from_raw(sequence as u64)
                    .ok_or(StoreError::Corrupt)?,
                payload,
            });
        }
        Ok(events)
    }

    pub fn drop_events_before(
        &self,
        aggregate_id: AggregateId,
        earliest_keep: AggregateSequence,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        let (kind, id) = aggregate_key(aggregate_id);
        conn.execute(
            "DELETE FROM aggregate_event
             WHERE aggregate_kind = ?1 AND aggregate_id = ?2 AND sequence < ?3",
            params![kind, id, earliest_keep.get() as i64],
        )
        .map_err(|_| StoreError::WriteFailed)?;
        Ok(())
    }

    pub fn append_output(
        &self,
        run_id: crate::AgentRunId,
        bytes: &[u8],
    ) -> Result<u64, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;
        let mut index: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(segment_index), -1) FROM output_segment WHERE agent_run_id = ?1",
                params![run_id.to_bytes().to_vec()],
                |row| row.get(0),
            )
            .map_err(|_| StoreError::Corrupt)?;
        let mut segments = 0_u64;
        for chunk in bytes.chunks(OUTPUT_SEGMENT_LEN) {
            index += 1;
            segments += 1;
            tx.execute(
                "INSERT INTO output_segment (agent_run_id, segment_index, payload) VALUES (?1, ?2, ?3)",
                params![run_id.to_bytes().to_vec(), index, chunk],
            )
            .map_err(|_| StoreError::WriteFailed)?;
        }
        tx.commit().map_err(|_| StoreError::WriteFailed)?;
        Ok(segments)
    }

    pub fn output_segment_count(&self, run_id: crate::AgentRunId) -> Result<u64, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM output_segment WHERE agent_run_id = ?1",
                params![run_id.to_bytes().to_vec()],
                |row| row.get(0),
            )
            .map_err(|_| StoreError::Corrupt)?;
        Ok(count as u64)
    }

    pub fn put_agent_run(
        &self,
        run_id: crate::AgentRunId,
        attempt_id: crate::AttemptId,
        binding_generation: u64,
        control_generation: u64,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        conn.execute(
            "INSERT INTO agent_run (id, attempt_id, binding_generation, control_generation, liveness)
             VALUES (?1, ?2, ?3, ?4, 'unknown')
             ON CONFLICT (id) DO UPDATE SET
               attempt_id = excluded.attempt_id,
               binding_generation = excluded.binding_generation,
               control_generation = excluded.control_generation,
               liveness = 'unknown'",
            params![
                run_id.to_bytes().to_vec(),
                attempt_id.to_bytes().to_vec(),
                binding_generation as i64,
                control_generation as i64
            ],
        )
        .map_err(|_| StoreError::WriteFailed)?;
        Ok(())
    }

    pub fn agent_run(&self, run_id: crate::AgentRunId) -> Result<PersistedAgentRun, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        let row = conn
            .query_row(
                "SELECT attempt_id, binding_generation, control_generation, liveness FROM agent_run WHERE id = ?1",
                params![run_id.to_bytes().to_vec()],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .map_err(|_| StoreError::Corrupt)?;
        if row.3 != "unknown" {
            return Err(StoreError::Corrupt);
        }
        let mut attempt_bytes = [0; 16];
        if row.0.len() != 16 {
            return Err(StoreError::Corrupt);
        }
        attempt_bytes.copy_from_slice(&row.0);
        Ok(PersistedAgentRun {
            attempt_id: crate::AttemptId::from_bytes(attempt_bytes),
            binding_generation: row.1 as u64,
            control_generation: row.2 as u64,
            liveness: PersistedLiveness::Unknown,
        })
    }

    pub fn confine_database(&self) -> Result<(), StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        let pages: i64 = conn
            .query_row("PRAGMA page_count", [], |row| row.get(0))
            .map_err(|_| StoreError::Corrupt)?;
        conn.pragma_update(None, "max_page_count", pages)
            .map_err(|_| StoreError::WriteFailed)?;
        Ok(())
    }

    #[cfg(test)]
    pub fn abandon_uncommitted(
        &self,
        aggregate_id: AggregateId,
        payload: &[u8],
    ) -> Result<(), StoreError> {
        self.commit_insert(aggregate_id, payload, false).map(|_| ())
    }

    fn commit_insert(
        &self,
        aggregate_id: AggregateId,
        payload: &[u8],
        commit: bool,
    ) -> Result<AggregateSequence, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;
        let (kind, id) = aggregate_key(aggregate_id);
        let current: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(sequence), 0) FROM aggregate_event WHERE aggregate_kind = ?1 AND aggregate_id = ?2",
                params![kind, id],
                |row| row.get(0),
            )
            .map_err(|_| StoreError::Corrupt)?;
        let next = current.checked_add(1).ok_or(StoreError::Corrupt)?;
        tx.execute(
            "INSERT INTO aggregate_event (aggregate_kind, aggregate_id, sequence, payload)
             VALUES (?1, ?2, ?3, ?4)",
            params![kind, id, next, payload],
        )
        .map_err(|_| StoreError::WriteFailed)?;
        if commit {
            tx.commit().map_err(|_| StoreError::WriteFailed)?;
        }
        AggregateSequence::from_raw(next as u64).ok_or(StoreError::Corrupt)
    }
}

fn initialize(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE aggregate_event (
            aggregate_kind INTEGER NOT NULL,
            aggregate_id BLOB NOT NULL,
            sequence INTEGER NOT NULL,
            payload BLOB NOT NULL,
            PRIMARY KEY (aggregate_kind, aggregate_id, sequence)
        );
        CREATE TABLE aggregate_snapshot (
            aggregate_kind INTEGER NOT NULL,
            aggregate_id BLOB NOT NULL,
            incorporated_through INTEGER NOT NULL,
            payload BLOB NOT NULL,
            PRIMARY KEY (aggregate_kind, aggregate_id)
        );
        CREATE TABLE output_segment (
            agent_run_id BLOB NOT NULL,
            segment_index INTEGER NOT NULL,
            payload BLOB NOT NULL,
            PRIMARY KEY (agent_run_id, segment_index)
        );
        CREATE TABLE agent_run (
            id BLOB PRIMARY KEY,
            attempt_id BLOB NOT NULL,
            binding_generation INTEGER NOT NULL,
            control_generation INTEGER NOT NULL,
            liveness TEXT NOT NULL CHECK (liveness = 'unknown')
        );",
    )
    .map_err(|_| StoreError::WriteFailed)?;
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|_| StoreError::WriteFailed)?;
    Ok(())
}

fn aggregate_key(id: AggregateId) -> (i64, Vec<u8>) {
    match id {
        AggregateId::WorkScope(value) => (1, value.to_bytes().to_vec()),
        AggregateId::WorkItem(value) => (2, value.to_bytes().to_vec()),
        AggregateId::Attempt(value) => (3, value.to_bytes().to_vec()),
        AggregateId::AgentRun(value) => (4, value.to_bytes().to_vec()),
    }
}

impl AggregateSequence {
    fn from_raw(value: u64) -> Option<Self> {
        NonZeroU64::new(value).map(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AggregateId;
    use std::{
        fs,
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc,
        },
        thread,
    };

    static NEXT_DIR: AtomicU64 = AtomicU64::new(1);

    fn path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "seyal-agent-store-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn uncommitted_insert_is_invisible_and_commit_survives_reopen() {
        let file = path("events.db");
        let aggregate = AggregateId::WorkItem(crate::WorkItemId::new());
        let store = AgentStore::open(&file).unwrap();
        store.abandon_uncommitted(aggregate, b"nope").unwrap();
        drop(store);
        let store = AgentStore::open(&file).unwrap();
        assert!(store.replay_after(aggregate, None).unwrap().is_empty());
        let sequence = store.append_event(aggregate, b"kept").unwrap();
        drop(store);
        let store = AgentStore::open(&file).unwrap();
        let events = store.replay_after(aggregate, None).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].sequence, sequence);
        assert_eq!(events[0].payload, b"kept");
    }

    #[test]
    fn sequences_are_per_aggregate_and_concurrent_appends_do_not_duplicate() {
        let file = path("concurrent.db");
        let store = Arc::new(AgentStore::open(&file).unwrap());
        let first = AggregateId::WorkScope(crate::WorkScopeId::new());
        let second = AggregateId::AgentRun(crate::AgentRunId::new());
        assert_eq!(store.append_event(first, b"a").unwrap().get(), 1);
        assert_eq!(store.append_event(second, b"b").unwrap().get(), 1);

        let mut joins = Vec::new();
        for _ in 0..8 {
            let store = Arc::clone(&store);
            joins.push(thread::spawn(move || {
                store.append_event(first, b"x").unwrap().get()
            }));
        }
        let mut sequences = joins
            .into_iter()
            .map(|join| join.join().unwrap())
            .collect::<Vec<_>>();
        sequences.sort_unstable();
        sequences.dedup();
        assert_eq!(sequences.len(), 8);
    }

    #[test]
    fn snapshot_plus_replay_converges_and_retention_returns_history_gap() {
        let file = path("replay.db");
        let store = AgentStore::open(&file).unwrap();
        let aggregate = AggregateId::Attempt(crate::AttemptId::new());
        let first = store.append_event(aggregate, b"one").unwrap();
        let second = store.append_event(aggregate, b"two").unwrap();
        let third = store.append_event(aggregate, b"three").unwrap();
        store.snapshot(aggregate, second, b"onetwo").unwrap();
        let replay = store.replay_after(aggregate, Some(second)).unwrap();
        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].payload, b"three");
        store.drop_events_before(aggregate, third).unwrap();
        match store.replay_after(aggregate, Some(first)) {
            Err(StoreError::History(gap)) => {
                assert_eq!(gap.requested_after, first);
                assert_eq!(gap.earliest_available, third);
            }
            other => panic!("expected history gap, got {other:?}"),
        }
    }

    #[test]
    fn newer_schema_is_quarantined_and_live_liveness_cannot_be_read() {
        let file = path("schema.db");
        let conn = Connection::open(&file).unwrap();
        conn.pragma_update(None, "user_version", 99).unwrap();
        drop(conn);
        assert_eq!(
            AgentStore::open(&file).err(),
            Some(StoreError::UnsupportedSchema)
        );

        let runs = path("runs.db");
        let store = AgentStore::open(&runs).unwrap();
        let run = crate::AgentRunId::new();
        let attempt = crate::AttemptId::new();
        store.put_agent_run(run, attempt, 1, 1).unwrap();
        let loaded = store.agent_run(run).unwrap();
        assert_eq!(loaded.liveness, PersistedLiveness::Unknown);
        assert_eq!(loaded.attempt_id, attempt);
        drop(store);
        let conn = Connection::open(&runs).unwrap();
        conn.execute("UPDATE agent_run SET liveness = 'live'", [])
            .unwrap_err();
    }

    #[test]
    fn output_is_segmented_and_page_exhaustion_does_not_publish_a_new_event() {
        let file = path("output.db");
        let store = AgentStore::open(&file).unwrap();
        let run = crate::AgentRunId::new();
        let segments = store
            .append_output(run, &vec![7; OUTPUT_SEGMENT_LEN * 2 + 10])
            .unwrap();
        assert_eq!(segments, 3);
        assert_eq!(store.output_segment_count(run).unwrap(), 3);
        let aggregate = AggregateId::AgentRun(run);
        let committed = store.append_event(aggregate, b"before").unwrap();
        store.confine_database().unwrap();
        assert_eq!(
            store.append_event(aggregate, &vec![1; 8192]),
            Err(StoreError::WriteFailed)
        );
        let events = store.replay_after(aggregate, None).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].sequence, committed);
    }
}
