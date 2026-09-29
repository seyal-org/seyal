use rusqlite::params;

use crate::{
    sqlite::{insert_event, AgentStore},
    AggregateId, AggregateSequence, PersistedAgentRun, PersistedLiveness, StoreError,
};

impl AgentStore {
    pub fn commit_work_scope(
        &self,
        id: crate::WorkScopeId,
        kind: u8,
    ) -> Result<AggregateSequence, StoreError> {
        self.commit_identity(
            "INSERT INTO work_scope (id, kind) VALUES (?1, ?2)",
            params![id.to_bytes().to_vec(), kind as i64],
            AggregateId::WorkScope(id),
            &[kind],
        )
    }

    pub fn commit_work_item(
        &self,
        id: crate::WorkItemId,
        work_scope_id: crate::WorkScopeId,
    ) -> Result<AggregateSequence, StoreError> {
        self.commit_identity(
            "INSERT INTO work_item (id, work_scope_id) VALUES (?1, ?2)",
            params![id.to_bytes().to_vec(), work_scope_id.to_bytes().to_vec()],
            AggregateId::WorkItem(id),
            &work_scope_id.to_bytes(),
        )
    }

    pub fn commit_attempt(
        &self,
        id: crate::AttemptId,
        work_item_id: crate::WorkItemId,
    ) -> Result<AggregateSequence, StoreError> {
        self.commit_identity(
            "INSERT INTO attempt (id, work_item_id) VALUES (?1, ?2)",
            params![id.to_bytes().to_vec(), work_item_id.to_bytes().to_vec()],
            AggregateId::Attempt(id),
            &work_item_id.to_bytes(),
        )
    }

    pub fn work_scopes(&self) -> Result<Vec<(crate::WorkScopeId, u8)>, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        let mut statement = conn
            .prepare("SELECT id, kind FROM work_scope")
            .map_err(|_| StoreError::Corrupt)?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|_| StoreError::Corrupt)?;
        let mut records = Vec::new();
        for row in rows {
            let (id, kind) = row.map_err(|_| StoreError::Corrupt)?;
            let kind = u8::try_from(kind).map_err(|_| StoreError::Corrupt)?;
            records.push((id_from_bytes(id)?, kind));
        }
        Ok(records)
    }

    pub fn work_items(&self) -> Result<Vec<(crate::WorkItemId, crate::WorkScopeId)>, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        let mut statement = conn
            .prepare("SELECT id, work_scope_id FROM work_item")
            .map_err(|_| StoreError::Corrupt)?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .map_err(|_| StoreError::Corrupt)?;
        let mut records = Vec::new();
        for row in rows {
            let (id, scope) = row.map_err(|_| StoreError::Corrupt)?;
            records.push((
                crate::WorkItemId::from_bytes(bytes16(id)?),
                crate::WorkScopeId::from_bytes(bytes16(scope)?),
            ));
        }
        Ok(records)
    }

    pub fn attempts(&self) -> Result<Vec<(crate::AttemptId, crate::WorkItemId)>, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        let mut statement = conn
            .prepare("SELECT id, work_item_id FROM attempt")
            .map_err(|_| StoreError::Corrupt)?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .map_err(|_| StoreError::Corrupt)?;
        let mut records = Vec::new();
        for row in rows {
            let (id, item) = row.map_err(|_| StoreError::Corrupt)?;
            records.push((
                crate::AttemptId::from_bytes(bytes16(id)?),
                crate::WorkItemId::from_bytes(bytes16(item)?),
            ));
        }
        Ok(records)
    }

    pub fn agent_runs(&self) -> Result<Vec<(crate::AgentRunId, PersistedAgentRun)>, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        let mut statement = conn
            .prepare(
                "SELECT id, attempt_id, binding_generation, control_generation, liveness FROM agent_run",
            )
            .map_err(|_| StoreError::Corrupt)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .map_err(|_| StoreError::Corrupt)?;
        let mut records = Vec::new();
        for row in rows {
            let (id, attempt, binding, control, liveness) = row.map_err(|_| StoreError::Corrupt)?;
            if liveness != "unknown" || binding < 0 || control < 0 {
                return Err(StoreError::Corrupt);
            }
            records.push((
                crate::AgentRunId::from_bytes(bytes16(id)?),
                PersistedAgentRun {
                    attempt_id: crate::AttemptId::from_bytes(bytes16(attempt)?),
                    binding_generation: binding as u64,
                    control_generation: control as u64,
                    liveness: PersistedLiveness::Unknown,
                },
            ));
        }
        Ok(records)
    }

    fn commit_identity(
        &self,
        sql: &str,
        parameters: impl rusqlite::Params,
        aggregate_id: AggregateId,
        payload: &[u8],
    ) -> Result<AggregateSequence, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;
        tx.execute(sql, parameters)
            .map_err(|_| StoreError::WriteFailed)?;
        let sequence = insert_event(&tx, aggregate_id, 1, payload)?;
        tx.commit().map_err(|_| StoreError::WriteFailed)?;
        Ok(sequence)
    }
}

fn bytes16(bytes: Vec<u8>) -> Result<[u8; 16], StoreError> {
    let mut id = [0; 16];
    if bytes.len() != 16 {
        return Err(StoreError::Corrupt);
    }
    id.copy_from_slice(&bytes);
    Ok(id)
}

fn id_from_bytes(bytes: Vec<u8>) -> Result<crate::WorkScopeId, StoreError> {
    Ok(crate::WorkScopeId::from_bytes(bytes16(bytes)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AgentStore;
    use rusqlite::Connection;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_DIR: AtomicU64 = AtomicU64::new(1);

    fn path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "seyal-agent-identity-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn identity_records_survive_reopen_and_version_two_migrates() {
        let legacy = path("legacy.db");
        let conn = Connection::open(&legacy).unwrap();
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
        .unwrap();
        conn.pragma_update(None, "user_version", 2).unwrap();
        drop(conn);

        let store = AgentStore::open(&legacy).unwrap();
        let scope = crate::WorkScopeId::new();
        let item = crate::WorkItemId::new();
        let attempt = crate::AttemptId::new();
        store.commit_work_scope(scope, 2).unwrap();
        store.commit_work_item(item, scope).unwrap();
        store.commit_attempt(attempt, item).unwrap();
        drop(store);

        let reopened = AgentStore::open(&legacy).unwrap();
        assert_eq!(reopened.work_scopes().unwrap(), vec![(scope, 2)]);
        assert_eq!(reopened.work_items().unwrap(), vec![(item, scope)]);
        assert_eq!(reopened.attempts().unwrap(), vec![(attempt, item)]);
        let scope_events = reopened
            .replay_after(AggregateId::WorkScope(scope), None)
            .unwrap();
        assert_eq!(scope_events.len(), 1);
        assert_eq!(scope_events[0].sequence.get(), 1);
        assert_eq!(scope_events[0].payload, vec![2]);
    }
}
