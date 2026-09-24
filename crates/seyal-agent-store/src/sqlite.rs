use std::{num::NonZeroU64, path::Path, sync::Mutex};

use rusqlite::{params, Connection, OptionalExtension};

use crate::{
    AggregateEventEnvelopeV1, AggregateId, AggregateSequence, HistoryGap, HistoryGapReason,
    SnapshotPosition,
};

const SCHEMA_VERSION: i32 = 2;
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
        } else if version < SCHEMA_VERSION {
            migrate_to_current(&conn, version)?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn append_event(
        &self,
        aggregate_id: AggregateId,
        kind: u16,
        payload: &[u8],
    ) -> Result<AggregateSequence, StoreError> {
        if payload.len() > MAX_EVENT_PAYLOAD {
            return Err(StoreError::PayloadTooLarge);
        }
        self.commit_insert(aggregate_id, kind, payload, true)
    }

    /// Authoritative AgentRun mutation and its outbox event share one commit.
    pub fn mutate_agent_run_and_append(
        &self,
        run_id: crate::AgentRunId,
        attempt_id: crate::AttemptId,
        binding_generation: u64,
        control_generation: u64,
        event_kind: u16,
        event_payload: &[u8],
    ) -> Result<AggregateSequence, StoreError> {
        if event_payload.len() > MAX_EVENT_PAYLOAD {
            return Err(StoreError::PayloadTooLarge);
        }
        let conn = self.conn.lock().expect("agent store lock");
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;
        tx.execute(
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
        let aggregate_id = AggregateId::AgentRun(run_id);
        let sequence = insert_event(&tx, aggregate_id, event_kind, event_payload)?;
        tx.commit().map_err(|_| StoreError::WriteFailed)?;
        Ok(sequence)
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
        // Snapshot must name an exact existing aggregate sequence.
        let exists: bool = tx
            .query_row(
                "SELECT 1 FROM aggregate_event
                 WHERE aggregate_kind = ?1 AND aggregate_id = ?2 AND sequence = ?3",
                params![kind, id, incorporated_through.get() as i64],
                |_| Ok(true),
            )
            .optional()
            .map_err(|_| StoreError::Corrupt)?
            .unwrap_or(false);
        if !exists {
            return Err(StoreError::Corrupt);
        }
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

    /// Loads the current aggregate snapshot payload and position, if any.
    pub fn get_snapshot(
        &self,
        aggregate_id: AggregateId,
    ) -> Result<Option<(SnapshotPosition, Vec<u8>)>, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        let (kind, id) = aggregate_key(aggregate_id);
        let row = conn
            .query_row(
                "SELECT incorporated_through, payload FROM aggregate_snapshot
                 WHERE aggregate_kind = ?1 AND aggregate_id = ?2",
                params![kind, id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )
            .optional()
            .map_err(|_| StoreError::Corrupt)?;
        let Some((incorporated_through, payload)) = row else {
            return Ok(None);
        };
        let incorporated_through =
            AggregateSequence::from_raw(incorporated_through as u64).ok_or(StoreError::Corrupt)?;
        Ok(Some((
            SnapshotPosition {
                aggregate_id,
                incorporated_through,
            },
            payload,
        )))
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
        let current_snapshot_sequence = conn
            .query_row(
                "SELECT incorporated_through FROM aggregate_snapshot
                 WHERE aggregate_kind = ?1 AND aggregate_id = ?2",
                params![kind, id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(|_| StoreError::Corrupt)?
            .and_then(|value| AggregateSequence::from_raw(value as u64));
        let hwm: i64 = conn
            .query_row(
                "SELECT COALESCE(high_water, 0) FROM aggregate_sequence_hwm
                 WHERE aggregate_kind = ?1 AND aggregate_id = ?2",
                params![kind, id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| StoreError::Corrupt)?
            .unwrap_or(0);
        let Some(earliest) = earliest else {
            // No retained events. Absent cursor is treated as 0 (from the start).
            let cursor = requested_after.map(|s| s.get() as i64).unwrap_or(0);
            if let Some(snapshot_sequence) = current_snapshot_sequence {
                if cursor < snapshot_sequence.get() as i64 {
                    let earliest_available =
                        snapshot_sequence.next().unwrap_or(AggregateSequence::FIRST);
                    let requested = requested_after.unwrap_or(AggregateSequence::FIRST);
                    return Err(StoreError::History(HistoryGap {
                        aggregate_id,
                        requested_after: requested,
                        earliest_available,
                        current_snapshot_sequence: Some(snapshot_sequence),
                        reason: HistoryGapReason::RetentionTruncation,
                    }));
                }
                return Ok(Vec::new());
            }
            // No snapshot: gap only when HWM proves prior history existed.
            if hwm > 0 && cursor < hwm {
                let requested = requested_after.unwrap_or(AggregateSequence::FIRST);
                let earliest_available = AggregateSequence::from_raw((hwm + 1) as u64)
                    .unwrap_or(AggregateSequence::FIRST);
                return Err(StoreError::History(HistoryGap {
                    aggregate_id,
                    requested_after: requested,
                    earliest_available,
                    current_snapshot_sequence: None,
                    reason: HistoryGapReason::RetentionTruncation,
                }));
            }
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
                current_snapshot_sequence,
                reason: HistoryGapReason::RetentionTruncation,
            }));
        }
        let mut statement = conn
            .prepare(
                "SELECT sequence, event_id, kind, payload FROM aggregate_event
                 WHERE aggregate_kind = ?1 AND aggregate_id = ?2 AND sequence > ?3
                 ORDER BY sequence",
            )
            .map_err(|_| StoreError::Corrupt)?;
        let rows = statement
            .query_map(params![kind, id, after], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                ))
            })
            .map_err(|_| StoreError::Corrupt)?;
        let mut events = Vec::new();
        for row in rows {
            let (sequence, event_id, kind, payload) = row.map_err(|_| StoreError::Corrupt)?;
            events.push(AggregateEventEnvelopeV1 {
                aggregate_id,
                sequence: AggregateSequence::from_raw(sequence as u64)
                    .ok_or(StoreError::Corrupt)?,
                event_id: event_id as u128,
                kind: kind as u16,
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

    /// Test-only AgentRun insert that skips the mutate+append commit boundary.
    /// Production writers must use [`Self::mutate_agent_run_and_append`].
    #[cfg(test)]
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
        self.commit_insert(aggregate_id, 1, payload, false)
            .map(|_| ())
    }

    /// Begin mutate+append then roll back — proves no false durable success.
    #[cfg(test)]
    pub fn abandon_mutate_agent_run_and_append(
        &self,
        run_id: crate::AgentRunId,
        attempt_id: crate::AttemptId,
        binding_generation: u64,
        control_generation: u64,
        event_kind: u16,
        event_payload: &[u8],
    ) -> Result<(), StoreError> {
        if event_payload.len() > MAX_EVENT_PAYLOAD {
            return Err(StoreError::PayloadTooLarge);
        }
        let conn = self.conn.lock().expect("agent store lock");
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;
        tx.execute(
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
        let _ = insert_event(
            &tx,
            AggregateId::AgentRun(run_id),
            event_kind,
            event_payload,
        )?;
        // Intentionally drop `tx` without commit.
        Ok(())
    }

    fn commit_insert(
        &self,
        aggregate_id: AggregateId,
        event_kind: u16,
        payload: &[u8],
        commit: bool,
    ) -> Result<AggregateSequence, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;
        let sequence = insert_event(&tx, aggregate_id, event_kind, payload)?;
        if commit {
            tx.commit().map_err(|_| StoreError::WriteFailed)?;
        }
        Ok(sequence)
    }

    pub fn page_count(&self) -> Result<u64, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        let pages: i64 = conn
            .query_row("PRAGMA page_count", [], |row| row.get(0))
            .map_err(|_| StoreError::Corrupt)?;
        Ok(pages as u64)
    }

    pub fn database_bytes(&self) -> Result<u64, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        let page_size: i64 = conn
            .query_row("PRAGMA page_size", [], |row| row.get(0))
            .map_err(|_| StoreError::Corrupt)?;
        let pages: i64 = conn
            .query_row("PRAGMA page_count", [], |row| row.get(0))
            .map_err(|_| StoreError::Corrupt)?;
        Ok((pages as u64).saturating_mul(page_size as u64))
    }
}

fn insert_event(
    tx: &rusqlite::Transaction<'_>,
    aggregate_id: AggregateId,
    event_kind: u16,
    payload: &[u8],
) -> Result<AggregateSequence, StoreError> {
    let (kind, id) = aggregate_key(aggregate_id);
    let hwm: i64 = tx
        .query_row(
            "SELECT COALESCE(high_water, 0) FROM aggregate_sequence_hwm
             WHERE aggregate_kind = ?1 AND aggregate_id = ?2",
            params![kind, id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| StoreError::Corrupt)?
        .unwrap_or(0);
    let next = hwm.checked_add(1).ok_or(StoreError::Corrupt)?;
    let event_id = next;
    tx.execute(
        "INSERT INTO aggregate_event (aggregate_kind, aggregate_id, sequence, event_id, kind, payload)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![kind, id, next, event_id, event_kind as i64, payload],
    )
    .map_err(|_| StoreError::WriteFailed)?;
    tx.execute(
        "INSERT INTO aggregate_sequence_hwm (aggregate_kind, aggregate_id, high_water)
         VALUES (?1, ?2, ?3)
         ON CONFLICT (aggregate_kind, aggregate_id) DO UPDATE SET
           high_water = excluded.high_water",
        params![kind, id, next],
    )
    .map_err(|_| StoreError::WriteFailed)?;
    AggregateSequence::from_raw(next as u64).ok_or(StoreError::Corrupt)
}

fn migrate_to_current(conn: &Connection, from: i32) -> Result<(), StoreError> {
    if from >= SCHEMA_VERSION {
        return Ok(());
    }
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS aggregate_sequence_hwm (
            aggregate_kind INTEGER NOT NULL,
            aggregate_id BLOB NOT NULL,
            high_water INTEGER NOT NULL,
            PRIMARY KEY (aggregate_kind, aggregate_id)
        );",
    )
    .map_err(|_| StoreError::WriteFailed)?;
    // Backfill from the max of retained events and snapshot frontiers.
    conn.execute_batch(
        "INSERT OR REPLACE INTO aggregate_sequence_hwm (aggregate_kind, aggregate_id, high_water)
         SELECT aggregate_kind, aggregate_id, MAX(seq) FROM (
           SELECT aggregate_kind, aggregate_id, sequence AS seq FROM aggregate_event
           UNION ALL
           SELECT aggregate_kind, aggregate_id, incorporated_through AS seq FROM aggregate_snapshot
         ) GROUP BY aggregate_kind, aggregate_id;",
    )
    .map_err(|_| StoreError::WriteFailed)?;
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|_| StoreError::WriteFailed)?;
    Ok(())
}

fn initialize(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE aggregate_event (
            aggregate_kind INTEGER NOT NULL,
            aggregate_id BLOB NOT NULL,
            sequence INTEGER NOT NULL,
            event_id INTEGER NOT NULL,
            kind INTEGER NOT NULL,
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
        CREATE TABLE aggregate_sequence_hwm (
            aggregate_kind INTEGER NOT NULL,
            aggregate_id BLOB NOT NULL,
            high_water INTEGER NOT NULL,
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
        time::{Duration, Instant},
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
        let sequence = store.append_event(aggregate, 7, b"kept").unwrap();
        drop(store);
        let store = AgentStore::open(&file).unwrap();
        let events = store.replay_after(aggregate, None).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].sequence, sequence);
        assert_eq!(events[0].kind, 7);
        assert_eq!(events[0].event_id, 1);
        assert_eq!(events[0].payload, b"kept");
    }

    #[test]
    fn sequences_are_per_aggregate_and_concurrent_appends_do_not_duplicate() {
        let file = path("concurrent.db");
        let store = Arc::new(AgentStore::open(&file).unwrap());
        let first = AggregateId::WorkScope(crate::WorkScopeId::new());
        let second = AggregateId::AgentRun(crate::AgentRunId::new());
        assert_eq!(store.append_event(first, 1, b"a").unwrap().get(), 1);
        assert_eq!(store.append_event(second, 1, b"b").unwrap().get(), 1);

        let mut joins = Vec::new();
        for _ in 0..8 {
            let store = Arc::clone(&store);
            joins.push(thread::spawn(move || {
                store.append_event(first, 1, b"x").unwrap().get()
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
        let first = store.append_event(aggregate, 1, b"one").unwrap();
        let second = store.append_event(aggregate, 1, b"two").unwrap();
        let third = store.append_event(aggregate, 1, b"three").unwrap();
        store.snapshot(aggregate, second, b"onetwo").unwrap();
        let (position, payload) = store.get_snapshot(aggregate).unwrap().unwrap();
        assert_eq!(position.incorporated_through, second);
        assert_eq!(payload, b"onetwo");
        let replay = store.replay_after(aggregate, Some(second)).unwrap();
        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].payload, b"three");
        store.drop_events_before(aggregate, third).unwrap();
        match store.replay_after(aggregate, Some(first)) {
            Err(StoreError::History(gap)) => {
                assert_eq!(gap.requested_after, first);
                assert_eq!(gap.earliest_available, third);
                assert_eq!(gap.current_snapshot_sequence, Some(second));
                assert_eq!(gap.reason, HistoryGapReason::RetentionTruncation);
            }
            other => panic!("expected history gap, got {other:?}"),
        }

        // Full truncation: cursor behind snapshot → gap; at snapshot → Ok([]);
        // None (from start) → gap; append must continue past HWM (not renumber).
        let past_all = AggregateSequence::from_raw(third.get() + 1).unwrap();
        store.drop_events_before(aggregate, past_all).unwrap();
        match store.replay_after(aggregate, Some(first)) {
            Err(StoreError::History(gap)) => {
                assert_eq!(gap.requested_after, first);
                assert_eq!(gap.current_snapshot_sequence, Some(second));
                assert_eq!(gap.reason, HistoryGapReason::RetentionTruncation);
            }
            other => panic!("expected full-truncation history gap, got {other:?}"),
        }
        assert!(matches!(
            store.replay_after(aggregate, None),
            Err(StoreError::History(_))
        ));
        let (position, payload) = store.get_snapshot(aggregate).unwrap().unwrap();
        assert_eq!(position.incorporated_through, second);
        assert_eq!(payload, b"onetwo");
        assert_eq!(
            store.replay_after(aggregate, Some(second)).unwrap(),
            Vec::new()
        );
        let resumed = store.append_event(aggregate, 1, b"after-wipe").unwrap();
        assert_eq!(resumed.get(), third.get() + 1);

        // Wipe with no snapshot: HWM still prevents renumbering and fabrications.
        let wiped = AggregateId::WorkScope(crate::WorkScopeId::new());
        let a = store.append_event(wiped, 1, b"a").unwrap();
        let b = store.append_event(wiped, 1, b"b").unwrap();
        let past_b = AggregateSequence::from_raw(b.get() + 1).unwrap();
        store.drop_events_before(wiped, past_b).unwrap();
        assert!(matches!(
            store.replay_after(wiped, None),
            Err(StoreError::History(_))
        ));
        assert!(matches!(
            store.replay_after(wiped, Some(a)),
            Err(StoreError::History(_))
        ));
        assert_eq!(store.replay_after(wiped, Some(b)).unwrap(), Vec::new());
        let next = store.append_event(wiped, 1, b"again").unwrap();
        assert_eq!(next.get(), b.get() + 1);

        // Virgin aggregate: Some cursor with no HWM is empty, not a gap.
        let virgin = AggregateId::WorkItem(crate::WorkItemId::new());
        assert_eq!(
            store
                .replay_after(virgin, Some(AggregateSequence::FIRST))
                .unwrap(),
            Vec::new()
        );

        // Snapshot must name an exact existing sequence.
        assert_eq!(
            store.snapshot(aggregate, AggregateSequence::from_raw(999).unwrap(), b"lie"),
            Err(StoreError::Corrupt)
        );
    }

    #[test]
    fn mutate_and_append_share_one_commit_boundary() {
        let file = path("atomic.db");
        let store = AgentStore::open(&file).unwrap();
        let run = crate::AgentRunId::new();
        let attempt = crate::AttemptId::new();
        store
            .abandon_mutate_agent_run_and_append(run, attempt, 2, 3, 9, b"started")
            .unwrap();
        drop(store);
        let store = AgentStore::open(&file).unwrap();
        assert!(store.agent_run(run).is_err());
        assert!(store
            .replay_after(AggregateId::AgentRun(run), None)
            .unwrap()
            .is_empty());

        let sequence = store
            .mutate_agent_run_and_append(run, attempt, 2, 3, 9, b"started")
            .unwrap();
        let loaded = store.agent_run(run).unwrap();
        assert_eq!(loaded.attempt_id, attempt);
        assert_eq!(loaded.binding_generation, 2);
        assert_eq!(loaded.control_generation, 3);
        assert_eq!(loaded.liveness, PersistedLiveness::Unknown);
        let events = store
            .replay_after(AggregateId::AgentRun(run), None)
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].sequence, sequence);
        assert_eq!(events[0].kind, 9);
        assert_eq!(events[0].payload, b"started");
        drop(store);
        let reopened = AgentStore::open(&file).unwrap();
        assert_eq!(
            reopened.agent_run(run).unwrap().liveness,
            PersistedLiveness::Unknown
        );
        assert_eq!(
            reopened
                .replay_after(AggregateId::AgentRun(run), None)
                .unwrap()
                .len(),
            1
        );
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
        let committed = store.append_event(aggregate, 1, b"before").unwrap();
        store.confine_database().unwrap();
        assert_eq!(
            store.append_event(aggregate, 1, &vec![1; 8192]),
            Err(StoreError::WriteFailed)
        );
        let events = store.replay_after(aggregate, None).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].sequence, committed);
    }

    #[test]
    fn records_append_throughput_snapshot_latency_db_growth_and_recovery() {
        let file = path("measure.db");
        let opened = Instant::now();
        let store = AgentStore::open(&file).unwrap();
        let cold_open = opened.elapsed();
        let aggregate = AggregateId::WorkItem(crate::WorkItemId::new());
        let append_started = Instant::now();
        for index in 0..256_u16 {
            store
                .append_event(aggregate, index, &index.to_le_bytes())
                .unwrap();
        }
        let append = append_started.elapsed();
        let snapshot_started = Instant::now();
        let through = AggregateSequence::from_raw(256).unwrap();
        store.snapshot(aggregate, through, b"snap").unwrap();
        let snapshot = snapshot_started.elapsed();
        let bytes = store.database_bytes().unwrap();
        drop(store);
        let reopen_started = Instant::now();
        let store = AgentStore::open(&file).unwrap();
        let recovery = reopen_started.elapsed();
        assert_eq!(store.replay_after(aggregate, None).unwrap().len(), 256);
        let rss = resident_kib();
        assert!(append < Duration::from_secs(2));
        assert!(snapshot < Duration::from_secs(1));
        assert!(recovery < Duration::from_secs(1));
        assert!(bytes > 0);
        let rss = rss.expect("RSS sample required for AB-0.3 measurement budget");
        assert!(rss < 512 * 1024, "RSS ceiling exceeded: {rss} KiB");
        eprintln!(
            "ab-0.3 measurement cold_open_us={} append_256_us={} snapshot_us={} recovery_us={} db_bytes={} rss_kib={}",
            cold_open.as_micros(),
            append.as_micros(),
            snapshot.as_micros(),
            recovery.as_micros(),
            bytes,
            rss
        );
    }

    fn resident_kib() -> Option<u64> {
        let output = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()?;
        String::from_utf8(output.stdout).ok()?.trim().parse().ok()
    }
}
