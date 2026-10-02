use std::sync::atomic::AtomicU64;
use std::{num::NonZeroU64, path::Path, sync::Mutex};

use rusqlite::{params, Connection, OptionalExtension};

use crate::{
    AggregateEventEnvelopeV1, AggregateId, AggregateSequence, HistoryGap, HistoryGapReason,
    SnapshotPosition,
};
use seyal_agent_core::{
    decode_output_ref, encode_output_ref, FingerprintRef, OutputRef, RetentionPolicyRef, StreamKind,
};

mod schema;
use schema::{initialize, migrate_to_current, SCHEMA_VERSION};

const MAX_EVENT_PAYLOAD: usize = 64 * 1024;
pub const OUTPUT_SEGMENT_LEN: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputAppend {
    pub sequence: AggregateSequence,
    pub first_segment_index: u32,
    pub segment_count: u32,
    pub byte_offset: u32,
    pub byte_length: u64,
    pub output_ref: OutputRef,
}

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

/// Durable ClientPrincipal row (SPEC-017 §5). Sessions are never persisted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistedPrincipal {
    pub id: seyal_agent_core::ClientPrincipalId,
    pub kind: u8,
    pub status: u8,
    pub scopes: Vec<u8>,
    pub evidence_key: Vec<u8>,
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
    pub(crate) writes_before_fault: AtomicU64,
    /// Bytes loaded by `replay_after` / `replay_page` on this store instance.
    /// Test-fault instrumentation only; production builds keep a cheap zeroed cell.
    #[cfg(feature = "test-fault-injection")]
    replay_payload_bytes_loaded: AtomicU64,
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
            writes_before_fault: AtomicU64::new(u64::MAX),
            #[cfg(feature = "test-fault-injection")]
            replay_payload_bytes_loaded: AtomicU64::new(0),
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
        self.gate_write()?;
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
        self.gate_write()?;
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
        self.gate_write()?;
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
        self.replay_after_limited(aggregate_id, requested_after, i64::MAX)
    }

    /// Replay at most `limit` events after the cursor. History gaps are unchanged.
    pub fn replay_page(
        &self,
        aggregate_id: AggregateId,
        requested_after: Option<AggregateSequence>,
        limit: usize,
    ) -> Result<Vec<AggregateEventEnvelopeV1>, StoreError> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.replay_after_limited(aggregate_id, requested_after, limit)
    }

    fn replay_after_limited(
        &self,
        aggregate_id: AggregateId,
        requested_after: Option<AggregateSequence>,
        limit: i64,
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
                 ORDER BY sequence
                 LIMIT ?4",
            )
            .map_err(|_| StoreError::Corrupt)?;
        let rows = statement
            .query_map(params![kind, id, after, limit], |row| {
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
            #[cfg(feature = "test-fault-injection")]
            {
                self.replay_payload_bytes_loaded
                    .fetch_add(payload.len() as u64, std::sync::atomic::Ordering::Relaxed);
            }
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

    /// Aggregate-local high-water sequence (event count when sequences are dense from 1).
    ///
    /// Does not load event payloads. Used by start_agent_run for event_count.
    pub fn high_water(&self, aggregate_id: AggregateId) -> Result<u64, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        let (kind, id) = aggregate_key(aggregate_id);
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
        if hwm < 0 {
            return Err(StoreError::Corrupt);
        }
        Ok(hwm as u64)
    }

    /// True when a committed observation payload records KnownSuccess (3) or
    /// KnownFailure (4). Recovery-only; not a hot-path scan of full history into
    /// memory — SQLite stops at the first match.
    pub fn has_committed_terminal_observation(
        &self,
        run_id: crate::AgentRunId,
    ) -> Result<bool, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        let id = run_id.to_bytes().to_vec();
        // aggregate_kind 4 = AgentRun; event kind 2 = observation payload.
        // Observation wire: 8-byte ordinal + kind byte (substr is 1-based).
        let found: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM aggregate_event
                 WHERE aggregate_kind = 4 AND aggregate_id = ?1 AND kind = 2
                   AND length(payload) >= 9
                   AND substr(payload, 9, 1) IN (x'03', x'04')
                 LIMIT 1",
                params![id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| StoreError::Corrupt)?;
        Ok(found.is_some())
    }

    #[cfg(feature = "test-fault-injection")]
    pub fn take_replay_payload_bytes_loaded(&self) -> u64 {
        self.replay_payload_bytes_loaded
            .swap(0, std::sync::atomic::Ordering::Relaxed)
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

    /// Persist high-volume output as bounded segments plus exactly one RunEvent
    /// whose payload is a fixed-size segment reference (SPEC-017 §10).
    ///
    /// Cross-batch appends continue an open (partial) last segment when capacity
    /// remains. Segments and the referencing event share one transaction: a fault
    /// before commit leaves neither behind.
    pub fn append_output_event(
        &self,
        run_id: crate::AgentRunId,
        event_kind: u16,
        bytes: &[u8],
        first_ordinal: u64,
        last_ordinal: u64,
    ) -> Result<OutputAppend, StoreError> {
        self.append_output_event_with_refs(
            run_id,
            event_kind,
            bytes,
            first_ordinal,
            last_ordinal,
            StreamKind::Stdout,
            FingerprintRef::public_content_digest(bytes),
            RetentionPolicyRef::retained_stream(),
        )
    }

    /// Like [`Self::append_output_event`] with explicit §10/§12 refs.
    #[allow(clippy::too_many_arguments)]
    pub fn append_output_event_with_refs(
        &self,
        run_id: crate::AgentRunId,
        event_kind: u16,
        bytes: &[u8],
        first_ordinal: u64,
        last_ordinal: u64,
        stream_kind: StreamKind,
        fingerprint_ref: FingerprintRef,
        retention_policy_ref: RetentionPolicyRef,
    ) -> Result<OutputAppend, StoreError> {
        let probe = OutputRef {
            first_segment_index: 0,
            segment_count: 0,
            byte_offset: 0,
            byte_length: bytes.len() as u64,
            first_ordinal,
            last_ordinal,
            stream_kind,
            fingerprint_ref,
            retention_policy_ref,
        };
        let payload_probe = encode_output_ref(&probe);
        if payload_probe.len() > MAX_EVENT_PAYLOAD {
            return Err(StoreError::PayloadTooLarge);
        }
        self.gate_write()?;
        let conn = self.conn.lock().expect("agent store lock");
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;
        let run_bytes = run_id.to_bytes().to_vec();
        let last_row: Option<(i64, Vec<u8>)> = tx
            .query_row(
                "SELECT segment_index, payload FROM output_segment
                 WHERE agent_run_id = ?1
                 ORDER BY segment_index DESC LIMIT 1",
                params![run_bytes.clone()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|_| StoreError::Corrupt)?;

        let mut remaining = bytes;
        let mut first_segment_index = 0_u32;
        let mut byte_offset = 0_u32;
        let mut segments = 0_u32;
        let mut next_index = last_row.as_ref().map(|(index, _)| *index + 1).unwrap_or(0);

        if let Some((index, mut payload)) = last_row
            && !remaining.is_empty()
            && payload.len() < OUTPUT_SEGMENT_LEN
        {
            let room = OUTPUT_SEGMENT_LEN - payload.len();
            let take = remaining.len().min(room);
            first_segment_index = index as u32;
            byte_offset = payload.len() as u32;
            payload.extend_from_slice(&remaining[..take]);
            tx.execute(
                "UPDATE output_segment SET payload = ?1
                 WHERE agent_run_id = ?2 AND segment_index = ?3",
                params![payload, run_bytes.clone(), index],
            )
            .map_err(|_| StoreError::WriteFailed)?;
            remaining = &remaining[take..];
            segments = 1;
        }

        if segments == 0 && !remaining.is_empty() {
            first_segment_index = next_index as u32;
            byte_offset = 0;
        } else if segments == 0 && remaining.is_empty() {
            first_segment_index = 0;
            byte_offset = 0;
        }

        while !remaining.is_empty() {
            let take = remaining.len().min(OUTPUT_SEGMENT_LEN);
            tx.execute(
                "INSERT INTO output_segment (agent_run_id, segment_index, payload)
                 VALUES (?1, ?2, ?3)",
                params![run_bytes.clone(), next_index, &remaining[..take]],
            )
            .map_err(|_| StoreError::WriteFailed)?;
            if segments == 0 {
                first_segment_index = next_index as u32;
                byte_offset = 0;
            }
            segments += 1;
            next_index += 1;
            remaining = &remaining[take..];
        }

        let output_ref = OutputRef {
            first_segment_index,
            segment_count: segments,
            byte_offset,
            byte_length: bytes.len() as u64,
            first_ordinal,
            last_ordinal,
            stream_kind,
            fingerprint_ref,
            retention_policy_ref,
        };
        let payload = encode_output_ref(&output_ref);
        let sequence = insert_event(&tx, AggregateId::AgentRun(run_id), event_kind, &payload)?;
        tx.commit().map_err(|_| StoreError::WriteFailed)?;
        Ok(OutputAppend {
            sequence,
            first_segment_index,
            segment_count: segments,
            byte_offset,
            byte_length: bytes.len() as u64,
            output_ref,
        })
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

    /// Materialize bytes addressed by an [`OutputRef`] without loading unrelated runs.
    pub fn materialize_output_ref(
        &self,
        run_id: crate::AgentRunId,
        output_ref: &OutputRef,
    ) -> Result<Vec<u8>, StoreError> {
        if output_ref.byte_length == 0 {
            return Ok(Vec::new());
        }
        if output_ref.segment_count == 0 {
            return Err(StoreError::Corrupt);
        }
        let conn = self.conn.lock().expect("agent store lock");
        let run_bytes = run_id.to_bytes().to_vec();
        let mut out = Vec::with_capacity(output_ref.byte_length as usize);
        let mut needed = output_ref.byte_length as usize;
        let offset_in_first = output_ref.byte_offset as usize;
        for step in 0..output_ref.segment_count {
            let index = output_ref.first_segment_index as i64 + i64::from(step);
            let payload: Vec<u8> = conn
                .query_row(
                    "SELECT payload FROM output_segment
                     WHERE agent_run_id = ?1 AND segment_index = ?2",
                    params![run_bytes.clone(), index],
                    |row| row.get(0),
                )
                .map_err(|_| StoreError::Corrupt)?;
            let start = if step == 0 { offset_in_first } else { 0 };
            if start > payload.len() {
                return Err(StoreError::Corrupt);
            }
            let available = &payload[start..];
            let take = available.len().min(needed);
            out.extend_from_slice(&available[..take]);
            needed -= take;
            if needed == 0 {
                break;
            }
        }
        if needed != 0 {
            return Err(StoreError::Corrupt);
        }
        Ok(out)
    }

    /// Decode a persisted RunEvent payload as an output ref (§10). Explicit errors.
    pub fn decode_persisted_output_ref(
        payload: &[u8],
    ) -> Result<OutputRef, seyal_agent_core::OutputRefError> {
        decode_output_ref(payload)
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
