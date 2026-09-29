use std::{num::NonZeroU64, path::Path, sync::Mutex};

use rusqlite::{params, Connection, OptionalExtension};

use crate::{
    AggregateEventEnvelopeV1, AggregateId, AggregateSequence, HistoryGap, HistoryGapReason,
    SnapshotPosition,
};

const SCHEMA_VERSION: i32 = 3;
const IDENTITY_TABLES: &str = "
CREATE TABLE IF NOT EXISTS work_scope (
    id BLOB PRIMARY KEY,
    kind INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS work_item (
    id BLOB PRIMARY KEY,
    work_scope_id BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS attempt (
    id BLOB PRIMARY KEY,
    work_item_id BLOB NOT NULL
);";
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
    pub(crate) conn: Mutex<Connection>,
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

pub(crate) fn insert_event(
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
    if from < 2 {
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
    }
    if from < 3 {
        conn.execute_batch(IDENTITY_TABLES)
            .map_err(|_| StoreError::WriteFailed)?;
    }
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
        );
        CREATE TABLE work_scope (
            id BLOB PRIMARY KEY,
            kind INTEGER NOT NULL
        );
        CREATE TABLE work_item (
            id BLOB PRIMARY KEY,
            work_scope_id BLOB NOT NULL
        );
        CREATE TABLE attempt (
            id BLOB PRIMARY KEY,
            work_item_id BLOB NOT NULL
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
    pub fn from_raw(value: u64) -> Option<Self> {
        NonZeroU64::new(value).map(Self)
    }
}

#[cfg(test)]
mod tests;
