use std::sync::atomic::Ordering;

use rusqlite::params;

use crate::{
    sqlite::{insert_event, AgentStore},
    AggregateId, AggregateSequence, PersistedAgentRun, PersistedLiveness, PersistedPrincipal,
    StoreError,
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

    pub fn upsert_principal(&self, principal: &PersistedPrincipal) -> Result<(), StoreError> {
        self.gate_write()?;
        let conn = self.conn.lock().expect("agent store lock");
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;
        tx.execute(
            "INSERT INTO client_principal (id, kind, status, scopes, evidence_key)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (id) DO UPDATE SET
               kind = excluded.kind,
               status = excluded.status,
               scopes = excluded.scopes,
               evidence_key = excluded.evidence_key",
            params![
                principal.id.to_bytes().to_vec(),
                principal.kind as i64,
                principal.status as i64,
                principal.scopes,
                principal.evidence_key
            ],
        )
        .map_err(|_| StoreError::WriteFailed)?;
        tx.commit().map_err(|_| StoreError::WriteFailed)?;
        Ok(())
    }

    pub fn set_principal_status(
        &self,
        id: seyal_agent_core::ClientPrincipalId,
        status: u8,
    ) -> Result<(), StoreError> {
        self.gate_write()?;
        let conn = self.conn.lock().expect("agent store lock");
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;
        let changed = tx
            .execute(
                "UPDATE client_principal SET status = ?1 WHERE id = ?2",
                params![status as i64, id.to_bytes().to_vec()],
            )
            .map_err(|_| StoreError::WriteFailed)?;
        if changed != 1 {
            return Err(StoreError::Corrupt);
        }
        tx.commit().map_err(|_| StoreError::WriteFailed)?;
        Ok(())
    }

    pub fn client_principals(&self) -> Result<Vec<PersistedPrincipal>, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        let mut statement = conn
            .prepare("SELECT id, kind, status, scopes, evidence_key FROM client_principal")
            .map_err(|_| StoreError::Corrupt)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                ))
            })
            .map_err(|_| StoreError::Corrupt)?;
        let mut records = Vec::new();
        for row in rows {
            let (id, kind, status, scopes, evidence_key) = row.map_err(|_| StoreError::Corrupt)?;
            let kind = u8::try_from(kind).map_err(|_| StoreError::Corrupt)?;
            let status = u8::try_from(status).map_err(|_| StoreError::Corrupt)?;
            records.push(PersistedPrincipal {
                id: seyal_agent_core::ClientPrincipalId::from_bytes(bytes16(id)?),
                kind,
                status,
                scopes,
                evidence_key,
            });
        }
        Ok(records)
    }

    pub fn agent_runs(&self) -> Result<Vec<(crate::AgentRunId, PersistedAgentRun)>, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        let mut statement = conn
            .prepare(
                "SELECT id, attempt_id, binding_generation, control_generation, liveness,
                        run_lifecycle, execution_liveness, observation, resumability, run_revision
                 FROM agent_run",
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
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                ))
            })
            .map_err(|_| StoreError::Corrupt)?;
        let mut records = Vec::new();
        for row in rows {
            let (
                id,
                attempt,
                binding,
                control,
                liveness,
                run_lifecycle,
                execution_liveness,
                observation,
                resumability,
                run_revision,
            ) = row.map_err(|_| StoreError::Corrupt)?;
            if liveness != "unknown" || binding < 0 || control < 0 || run_revision < 1 {
                return Err(StoreError::Corrupt);
            }
            records.push((
                crate::AgentRunId::from_bytes(bytes16(id)?),
                PersistedAgentRun {
                    attempt_id: crate::AttemptId::from_bytes(bytes16(attempt)?),
                    binding_generation: binding as u64,
                    control_generation: control as u64,
                    liveness: PersistedLiveness::Unknown,
                    run_lifecycle: u8::try_from(run_lifecycle).map_err(|_| StoreError::Corrupt)?,
                    execution_liveness: u8::try_from(execution_liveness)
                        .map_err(|_| StoreError::Corrupt)?,
                    observation: u8::try_from(observation).map_err(|_| StoreError::Corrupt)?,
                    resumability: u8::try_from(resumability).map_err(|_| StoreError::Corrupt)?,
                    run_revision: run_revision as u64,
                },
            ));
        }
        Ok(records)
    }

    /// The next `allowed` commits proceed. The following commit fails before
    /// its transaction starts, so a refused write publishes nothing.
    #[cfg(any(test, feature = "test-fault-injection"))]
    pub fn fail_after_writes(&self, allowed: u64) {
        self.writes_before_fault.store(allowed, Ordering::Relaxed);
    }

    pub(crate) fn gate_write(&self) -> Result<(), StoreError> {
        let remaining = self.writes_before_fault.load(Ordering::Relaxed);
        if remaining == u64::MAX {
            return Ok(());
        }
        if remaining == 0 {
            return Err(StoreError::WriteFailed);
        }
        self.writes_before_fault.fetch_sub(1, Ordering::Relaxed);
        Ok(())
    }

    fn commit_identity(
        &self,
        sql: &str,
        parameters: impl rusqlite::Params,
        aggregate_id: AggregateId,
        payload: &[u8],
    ) -> Result<AggregateSequence, StoreError> {
        self.gate_write()?;
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
    use rusqlite::{params, Connection};
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
    fn client_principals_survive_reopen_and_status_update() {
        let file = path("principals.db");
        let store = AgentStore::open(&file).unwrap();
        let id = seyal_agent_core::ClientPrincipalId::new();
        store
            .upsert_principal(&PersistedPrincipal {
                id,
                kind: 1,
                status: 1,
                scopes: vec![1, 2, 4],
                evidence_key: b"cli".to_vec(),
            })
            .unwrap();
        drop(store);
        let reopened = AgentStore::open(&file).unwrap();
        let rows = reopened.client_principals().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, id);
        assert_eq!(rows[0].status, 1);
        reopened.set_principal_status(id, 3).unwrap();
        drop(reopened);
        let again = AgentStore::open(&file).unwrap();
        assert_eq!(again.client_principals().unwrap()[0].status, 3);
        let _ = fs::remove_file(&file);
    }

    #[test]
    fn principal_write_fault_before_commit_publishes_nothing() {
        let file = path("principal-fault.db");
        let store = AgentStore::open(&file).unwrap();
        store.fail_after_writes(0);
        let id = seyal_agent_core::ClientPrincipalId::new();
        assert_eq!(
            store.upsert_principal(&PersistedPrincipal {
                id,
                kind: 1,
                status: 1,
                scopes: vec![1],
                evidence_key: b"cli".to_vec(),
            }),
            Err(StoreError::WriteFailed)
        );
        store.fail_after_writes(u64::MAX);
        assert!(store.client_principals().unwrap().is_empty());
        let _ = fs::remove_file(&file);
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
        let prior_run = [9u8; 16];
        let prior_attempt = [8u8; 16];
        conn.execute(
            "INSERT INTO agent_run (id, attempt_id, binding_generation, control_generation, liveness)
             VALUES (?1, ?2, 4, 5, 'unknown')",
            params![prior_run.to_vec(), prior_attempt.to_vec()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO aggregate_event
                (aggregate_kind, aggregate_id, sequence, event_id, kind, payload)
             VALUES (4, ?1, 1, 1, 9, ?2)",
            params![prior_run.to_vec(), b"kept".to_vec()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO aggregate_sequence_hwm (aggregate_kind, aggregate_id, high_water)
             VALUES (4, ?1, 1)",
            params![prior_run.to_vec()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO aggregate_snapshot
                (aggregate_kind, aggregate_id, incorporated_through, payload)
             VALUES (4, ?1, 1, ?2)",
            params![prior_run.to_vec(), b"snap".to_vec()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO output_segment (agent_run_id, segment_index, payload)
             VALUES (?1, 0, ?2)",
            params![prior_run.to_vec(), b"seg".to_vec()],
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 2).unwrap();
        drop(conn);

        let store = AgentStore::open(&legacy).unwrap();
        let prior_run = crate::AgentRunId::from_bytes([9u8; 16]);
        let prior = store.agent_run(prior_run).unwrap();
        assert_eq!(prior.attempt_id, crate::AttemptId::from_bytes([8u8; 16]));
        assert_eq!(prior.binding_generation, 4);
        assert_eq!(prior.control_generation, 5);
        assert_eq!(prior.liveness, PersistedLiveness::Unknown);
        let prior_events = store
            .replay_after(AggregateId::AgentRun(prior_run), None)
            .unwrap();
        assert_eq!(prior_events.len(), 1);
        assert_eq!(prior_events[0].sequence.get(), 1);
        assert_eq!(prior_events[0].payload, b"kept");
        let (position, payload) = store
            .get_snapshot(AggregateId::AgentRun(prior_run))
            .unwrap()
            .unwrap();
        assert_eq!(position.incorporated_through.get(), 1);
        assert_eq!(payload, b"snap");
        assert_eq!(store.output_segment_count(prior_run).unwrap(), 1);
        assert!(store.work_scopes().unwrap().is_empty());
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
        assert_eq!(reopened.agent_run(prior_run).unwrap().binding_generation, 4);
        drop(reopened);
        let conn = Connection::open(&legacy).unwrap();
        let version: i32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 8);
        let migrated = AgentStore::open(&legacy)
            .unwrap()
            .agent_run(prior_run)
            .unwrap();
        assert_eq!(migrated.run_lifecycle, 1);
        assert_eq!(migrated.execution_liveness, 1);
        assert_eq!(migrated.run_revision, 1);
    }
}
