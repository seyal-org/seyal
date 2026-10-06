//! RunWorkingSet persistence and resume classification (SPEC-014).

use rusqlite::{params, OptionalExtension};
use seyal_agent_core::{
    classify_resume, AgentRunId, AttemptId, ContinuationPlan, ExecutionLivenessHint, MemoryId,
    ResumeClassification, RetentionAvailability, RunWorkingSet, RunWorkingSetId, Sensitivity,
    WorkItemId, WorkingSetEntry, WorkingSetEntryClass, WorkingSetGeneration,
    MAX_WORKING_SET_AGGREGATE_BYTES, MAX_WORKING_SET_BYTES, MAX_WORKING_SET_ENTRIES,
};

use crate::StoreError;

use super::ops::{MemoryAuthority, MemoryError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkingSetError {
    Memory(MemoryError),
    AliasDenied,
    CapExceeded,
    NotFound,
    StaleGeneration,
}

impl From<MemoryError> for WorkingSetError {
    fn from(value: MemoryError) -> Self {
        Self::Memory(value)
    }
}

impl From<StoreError> for WorkingSetError {
    fn from(value: StoreError) -> Self {
        Self::Memory(MemoryError::Store(value))
    }
}

impl MemoryAuthority<'_> {
    pub fn put_working_set(&self, ws: RunWorkingSet) -> Result<RunWorkingSet, WorkingSetError> {
        if ws.entries.len() > MAX_WORKING_SET_ENTRIES {
            return Err(WorkingSetError::CapExceeded);
        }
        let bytes = ws.byte_footprint();
        if bytes > MAX_WORKING_SET_BYTES {
            return Err(WorkingSetError::CapExceeded);
        }

        let conn = self
            .store
            .conn
            .lock()
            .map_err(|_| StoreError::WriteFailed)?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;

        // Sibling run cannot alias this working set as current.
        let existing_run: Option<Vec<u8>> = tx
            .query_row(
                "SELECT agent_run_id FROM run_working_set WHERE id = ?1",
                params![ws.id.to_bytes().as_slice()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| StoreError::Corrupt)?;
        if let Some(run_bytes) = existing_run {
            let mut id = [0u8; 16];
            id.copy_from_slice(&run_bytes);
            if AgentRunId::from_bytes(id) != ws.agent_run_id {
                return Err(WorkingSetError::AliasDenied);
            }
        }

        let aggregate: i64 = tx
            .query_row(
                "SELECT aggregate_working_set_bytes FROM memory_store_meta WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|_| StoreError::Corrupt)?;
        if aggregate as u64 + bytes > MAX_WORKING_SET_AGGREGATE_BYTES {
            return Err(WorkingSetError::CapExceeded);
        }

        // Mark prior current for this run as not current (fresh retry creates new run/id).
        tx.execute(
            "UPDATE run_working_set SET current = 0 WHERE agent_run_id = ?1",
            params![ws.agent_run_id.to_bytes().as_slice()],
        )
        .map_err(|_| StoreError::WriteFailed)?;

        tx.execute(
            "INSERT OR REPLACE INTO run_working_set
             (id, work_item_id, attempt_id, agent_run_id, working_set_generation, policy_fence,
              builder_version, created_at_unix_ms, last_compacted_at_unix_ms,
              provider_continuation_ref, degraded, encoded_bytes, current)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,1)",
            params![
                ws.id.to_bytes().as_slice(),
                ws.work_item_id.to_bytes().as_slice(),
                ws.attempt_id.to_bytes().as_slice(),
                ws.agent_run_id.to_bytes().as_slice(),
                ws.working_set_generation.get() as i64,
                ws.policy_fence,
                ws.builder_version as i64,
                ws.created_at_unix_ms as i64,
                ws.last_compacted_at_unix_ms.map(|v| v as i64),
                ws.provider_continuation_ref,
                i64::from(ws.degraded),
                bytes as i64,
            ],
        )
        .map_err(|_| StoreError::WriteFailed)?;

        tx.execute(
            "DELETE FROM run_working_set_entry WHERE working_set_id = ?1",
            params![ws.id.to_bytes().as_slice()],
        )
        .map_err(|_| StoreError::WriteFailed)?;
        for entry in &ws.entries {
            tx.execute(
                "INSERT INTO run_working_set_entry
                 (working_set_id, entry_id, class, availability, sensitivity, payload,
                  dependency_ref, source_generation, reconstructable)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    ws.id.to_bytes().as_slice(),
                    entry.entry_id.as_slice(),
                    entry.class.code() as i64,
                    entry.availability.code() as i64,
                    entry.sensitivity.code() as i64,
                    entry.payload,
                    entry.dependency_ref,
                    entry.source_generation as i64,
                    i64::from(entry.reconstructable),
                ],
            )
            .map_err(|_| StoreError::WriteFailed)?;
        }

        tx.execute(
            "UPDATE memory_store_meta SET aggregate_working_set_bytes = aggregate_working_set_bytes + ?1
             WHERE singleton = 1",
            params![bytes as i64],
        )
        .map_err(|_| StoreError::WriteFailed)?;

        tx.commit().map_err(|_| StoreError::WriteFailed)?;
        let _ = MemoryId::new(); // keep import warm for fixture refs in tests
        Ok(ws)
    }

    pub fn get_working_set(
        &self,
        id: RunWorkingSetId,
    ) -> Result<Option<RunWorkingSet>, WorkingSetError> {
        let conn = self
            .store
            .conn
            .lock()
            .map_err(|_| StoreError::WriteFailed)?;
        let row = conn
            .query_row(
                "SELECT work_item_id, attempt_id, agent_run_id, working_set_generation, policy_fence,
                        builder_version, created_at_unix_ms, last_compacted_at_unix_ms,
                        provider_continuation_ref, degraded
                 FROM run_working_set WHERE id = ?1",
                params![id.to_bytes().as_slice()],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, Vec<u8>>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, Option<i64>>(7)?,
                        row.get::<_, Option<Vec<u8>>>(8)?,
                        row.get::<_, i64>(9)?,
                    ))
                },
            )
            .optional()
            .map_err(|_| StoreError::Corrupt)?;
        let Some((wi, at, ar, ws_gen, fence, builder, created, compacted, provider, degraded)) =
            row
        else {
            return Ok(None);
        };
        let mut entries = Vec::new();
        let mut stmt = conn
            .prepare(
                "SELECT entry_id, class, availability, sensitivity, payload, dependency_ref,
                        source_generation, reconstructable
                 FROM run_working_set_entry WHERE working_set_id = ?1",
            )
            .map_err(|_| StoreError::Corrupt)?;
        let rows = stmt
            .query_map(params![id.to_bytes().as_slice()], |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<Vec<u8>>>(4)?,
                    row.get::<_, Vec<u8>>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            })
            .map_err(|_| StoreError::Corrupt)?;
        for row in rows {
            let (eid, class, avail, sens, payload, dep, sgen, recon) =
                row.map_err(|_| StoreError::Corrupt)?;
            let mut entry_id = [0u8; 16];
            entry_id.copy_from_slice(&eid);
            entries.push(WorkingSetEntry {
                entry_id,
                class: WorkingSetEntryClass::from_code(class as u8).ok_or(StoreError::Corrupt)?,
                availability: RetentionAvailability::from_code(avail as u8)
                    .ok_or(StoreError::Corrupt)?,
                sensitivity: Sensitivity::from_code(sens as u8).ok_or(StoreError::Corrupt)?,
                payload,
                dependency_ref: dep,
                source_generation: sgen as u64,
                reconstructable: recon != 0,
            });
        }
        Ok(Some(RunWorkingSet {
            id,
            work_item_id: WorkItemId::from_bytes(wi.try_into().map_err(|_| StoreError::Corrupt)?),
            attempt_id: AttemptId::from_bytes(at.try_into().map_err(|_| StoreError::Corrupt)?),
            agent_run_id: AgentRunId::from_bytes(ar.try_into().map_err(|_| StoreError::Corrupt)?),
            working_set_generation: WorkingSetGeneration::from_raw(ws_gen as u64)
                .ok_or(StoreError::Corrupt)?,
            policy_fence: fence,
            builder_version: builder as u16,
            created_at_unix_ms: created as u64,
            last_compacted_at_unix_ms: compacted.map(|v| v as u64),
            provider_continuation_ref: provider,
            entries,
            degraded: degraded != 0,
        }))
    }

    pub fn classify_working_set_resume(
        &self,
        working_set_id: RunWorkingSetId,
        plan: Option<&ContinuationPlan>,
        expected_binding: u64,
        liveness: ExecutionLivenessHint,
        ambiguous_external_effect: bool,
    ) -> Result<ResumeClassification, WorkingSetError> {
        let ws = self
            .get_working_set(working_set_id)?
            .ok_or(WorkingSetError::NotFound)?;
        Ok(classify_resume(
            plan,
            &ws,
            super::ops::MemoryAuthority::now_ms(),
            expected_binding,
            liveness,
            ambiguous_external_effect,
        ))
    }

    pub fn abandon_provider_continuation(
        &self,
        working_set_id: RunWorkingSetId,
        expected_generation: WorkingSetGeneration,
    ) -> Result<RunWorkingSet, WorkingSetError> {
        let mut ws = self
            .get_working_set(working_set_id)?
            .ok_or(WorkingSetError::NotFound)?;
        if ws.working_set_generation != expected_generation {
            return Err(WorkingSetError::StaleGeneration);
        }
        ws.provider_continuation_ref = None;
        ws.working_set_generation = expected_generation
            .next()
            .ok_or(WorkingSetError::Memory(MemoryError::InvalidInput))?;
        // Mark related memory-ref entries revoked if fence advanced — caller supplies eligibility.
        self.put_working_set(ws)
    }

    pub fn acquire_handoff_fence(
        &self,
        agent_run_id: [u8; 16],
        payload_digest: [u8; 32],
        adapter_id: [u8; 16],
        fence: &seyal_agent_core::RevocationFence,
        expires_at_unix_ms: u64,
        enforceable: bool,
    ) -> Result<seyal_agent_core::HandoffFence, seyal_agent_core::HandoffError> {
        seyal_agent_core::HandoffFence::acquire(
            agent_run_id,
            payload_digest,
            adapter_id,
            fence,
            expires_at_unix_ms,
            enforceable,
        )
    }
}
