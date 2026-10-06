//! Rebuildable Local Context Engine index/cache metadata (SPEC-013 §18).
//!
//! Derived indexes are disposable and never source truth. Stored only in the
//! agent-domain store (ADR-016); never on the terminal Runtime hot path.

use rusqlite::{params, OptionalExtension};

use seyal_agent_core::WorkScopeId;

use super::{AgentStore, StoreError};

const MAX_CONTEXT_INDEX_PAYLOAD: usize = 16 * 1024 * 1024; // soft per-row guard; workspace caps enforced by caller

/// Durable rebuildable context-index row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextIndexRecord {
    pub work_scope_id: WorkScopeId,
    pub producer_id: String,
    pub schema_version: u32,
    pub policy_generation: u64,
    pub privacy_generation: u64,
    pub source_generation: u64,
    pub catalog_digest_hex: String,
    pub integrity_hex: String,
    pub payload: Vec<u8>,
}

impl AgentStore {
    pub fn put_context_index(&self, record: &ContextIndexRecord) -> Result<(), StoreError> {
        if record.producer_id.is_empty() || record.payload.len() > MAX_CONTEXT_INDEX_PAYLOAD {
            return Err(StoreError::PayloadTooLarge);
        }
        self.gate_write()?;
        let conn = self.conn.lock().expect("agent store lock");
        conn.execute(
            "INSERT INTO context_index_cache (
                work_scope_id, producer_id, schema_version,
                policy_generation, privacy_generation, source_generation,
                catalog_digest_hex, integrity_hex, payload, bytes
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(work_scope_id) DO UPDATE SET
                producer_id = excluded.producer_id,
                schema_version = excluded.schema_version,
                policy_generation = excluded.policy_generation,
                privacy_generation = excluded.privacy_generation,
                source_generation = excluded.source_generation,
                catalog_digest_hex = excluded.catalog_digest_hex,
                integrity_hex = excluded.integrity_hex,
                payload = excluded.payload,
                bytes = excluded.bytes",
            params![
                record.work_scope_id.to_bytes().to_vec(),
                record.producer_id,
                record.schema_version as i64,
                record.policy_generation as i64,
                record.privacy_generation as i64,
                record.source_generation as i64,
                record.catalog_digest_hex,
                record.integrity_hex,
                record.payload,
                record.payload.len() as i64,
            ],
        )
        .map_err(|_| StoreError::WriteFailed)?;
        Ok(())
    }

    pub fn get_context_index(
        &self,
        work_scope_id: WorkScopeId,
    ) -> Result<Option<ContextIndexRecord>, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        conn.query_row(
            "SELECT producer_id, schema_version, policy_generation, privacy_generation,
                    source_generation, catalog_digest_hex, integrity_hex, payload
             FROM context_index_cache WHERE work_scope_id = ?1",
            params![work_scope_id.to_bytes().to_vec()],
            |row| {
                Ok(ContextIndexRecord {
                    work_scope_id,
                    producer_id: row.get(0)?,
                    schema_version: row.get::<_, i64>(1)? as u32,
                    policy_generation: row.get::<_, i64>(2)? as u64,
                    privacy_generation: row.get::<_, i64>(3)? as u64,
                    source_generation: row.get::<_, i64>(4)? as u64,
                    catalog_digest_hex: row.get(5)?,
                    integrity_hex: row.get(6)?,
                    payload: row.get(7)?,
                })
            },
        )
        .optional()
        .map_err(|_| StoreError::Corrupt)
    }

    pub fn delete_context_index(&self, work_scope_id: WorkScopeId) -> Result<(), StoreError> {
        self.gate_write()?;
        let conn = self.conn.lock().expect("agent store lock");
        conn.execute(
            "DELETE FROM context_index_cache WHERE work_scope_id = ?1",
            params![work_scope_id.to_bytes().to_vec()],
        )
        .map_err(|_| StoreError::WriteFailed)?;
        Ok(())
    }

    /// Aggregate durable context-index bytes (for shed/pressure checks).
    pub fn context_index_total_bytes(&self) -> Result<u64, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        let total: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(bytes), 0) FROM context_index_cache",
                [],
                |row| row.get(0),
            )
            .map_err(|_| StoreError::Corrupt)?;
        Ok(total as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(1);

    fn temp_store() -> AgentStore {
        let dir = std::env::temp_dir().join(format!(
            "seyal-agent-store-ctx-idx-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        AgentStore::open(dir.join("agent.db")).unwrap()
    }

    #[test]
    fn context_index_round_trip_and_delete() {
        let store = temp_store();
        let scope = WorkScopeId::new();
        let record = ContextIndexRecord {
            work_scope_id: scope,
            producer_id: "seyal-agent-context/discovery-index".into(),
            schema_version: 1,
            policy_generation: 2,
            privacy_generation: 3,
            source_generation: 4,
            catalog_digest_hex: "aa".repeat(16),
            integrity_hex: "bb".repeat(16),
            payload: b"catalog".to_vec(),
        };
        store.put_context_index(&record).unwrap();
        let loaded = store.get_context_index(scope).unwrap().unwrap();
        assert_eq!(loaded.payload, b"catalog");
        assert_eq!(store.context_index_total_bytes().unwrap(), 7);
        store.delete_context_index(scope).unwrap();
        assert_eq!(store.get_context_index(scope).unwrap(), None);
    }
}
