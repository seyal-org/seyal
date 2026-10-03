//! Durable AgentRun lifecycle column persistence (SPEC-026).

use rusqlite::params;

use crate::{AggregateId, AggregateSequence, StoreError};

use super::{insert_event, AgentStore, MAX_EVENT_PAYLOAD};

impl AgentStore {
    /// Persist orthogonal AgentRun lifecycle facts after a domain transition.
    #[allow(clippy::too_many_arguments)]
    pub fn update_agent_run_lifecycle(
        &self,
        run_id: crate::AgentRunId,
        run_lifecycle: u8,
        execution_liveness: u8,
        observation: u8,
        resumability: u8,
        run_revision: u64,
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
        Self::write_lifecycle_columns(
            &tx,
            run_id,
            run_lifecycle,
            execution_liveness,
            observation,
            resumability,
            run_revision,
        )?;
        let sequence = insert_event(
            &tx,
            AggregateId::AgentRun(run_id),
            event_kind,
            event_payload,
        )?;
        tx.commit().map_err(|_| StoreError::WriteFailed)?;
        Ok(sequence)
    }

    /// Column-only lifecycle persist (no outbox event). Used when generations
    /// were already fenced in the same recovery commit path.
    #[allow(clippy::too_many_arguments)]
    pub fn set_agent_run_lifecycle_columns(
        &self,
        run_id: crate::AgentRunId,
        run_lifecycle: u8,
        execution_liveness: u8,
        observation: u8,
        resumability: u8,
        run_revision: u64,
    ) -> Result<(), StoreError> {
        self.gate_write()?;
        let conn = self.conn.lock().expect("agent store lock");
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;
        Self::write_lifecycle_columns(
            &tx,
            run_id,
            run_lifecycle,
            execution_liveness,
            observation,
            resumability,
            run_revision,
        )?;
        tx.commit().map_err(|_| StoreError::WriteFailed)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn write_lifecycle_columns(
        tx: &rusqlite::Transaction<'_>,
        run_id: crate::AgentRunId,
        run_lifecycle: u8,
        execution_liveness: u8,
        observation: u8,
        resumability: u8,
        run_revision: u64,
    ) -> Result<(), StoreError> {
        let changed = tx
            .execute(
                "UPDATE agent_run SET
                    run_lifecycle = ?1,
                    execution_liveness = ?2,
                    observation = ?3,
                    resumability = ?4,
                    run_revision = ?5
                 WHERE id = ?6",
                params![
                    run_lifecycle as i64,
                    execution_liveness as i64,
                    observation as i64,
                    resumability as i64,
                    run_revision as i64,
                    run_id.to_bytes().to_vec()
                ],
            )
            .map_err(|_| StoreError::WriteFailed)?;
        if changed != 1 {
            return Err(StoreError::Corrupt);
        }
        Ok(())
    }
}
