//! Durable `WorkScope.bindings` (SPEC-027 §6).
//!
//! One canonical root path per WorkScope, keyed by `WorkScopeId`. Written
//! only through this trusted store API — never a `CreateWorkScope` /
//! `StartAgentRun` field (SPEC-027 fixture 5).

use rusqlite::{params, OptionalExtension};

use seyal_agent_core::WorkScopeId;

use super::{AgentStore, StoreError};

const MAX_BOUND_ROOT_CHARS: usize = 4096;

impl AgentStore {
    /// Associate a canonical root path with a WorkScope that already exists
    /// in `work_scope`. Replaces any previous binding for that id.
    pub fn bind_work_scope_root(
        &self,
        work_scope_id: WorkScopeId,
        bound_root: &str,
    ) -> Result<(), StoreError> {
        if bound_root.is_empty() || bound_root.chars().count() > MAX_BOUND_ROOT_CHARS {
            return Err(StoreError::WriteFailed);
        }
        self.gate_write()?;
        let conn = self.conn.lock().expect("agent store lock");
        let exists: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM work_scope WHERE id = ?1",
                params![work_scope_id.to_bytes().to_vec()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| StoreError::Corrupt)?;
        if exists.is_none() {
            return Err(StoreError::Corrupt);
        }
        conn.execute(
            "INSERT INTO work_scope_binding (work_scope_id, bound_root)
             VALUES (?1, ?2)
             ON CONFLICT (work_scope_id) DO UPDATE SET bound_root = excluded.bound_root",
            params![work_scope_id.to_bytes().to_vec(), bound_root],
        )
        .map_err(|_| StoreError::WriteFailed)?;
        Ok(())
    }

    pub fn work_scope_bound_root(
        &self,
        work_scope_id: WorkScopeId,
    ) -> Result<Option<String>, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        conn.query_row(
            "SELECT bound_root FROM work_scope_binding WHERE work_scope_id = ?1",
            params![work_scope_id.to_bytes().to_vec()],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| StoreError::Corrupt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(1);

    fn temp_store() -> AgentStore {
        let dir = std::env::temp_dir().join(format!(
            "seyal-agent-store-bindings-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        AgentStore::open(dir.join("agent.db")).unwrap()
    }

    #[test]
    fn bind_round_trips_keyed_by_work_scope_id() {
        let store = temp_store();
        let scope = WorkScopeId::new();
        let other = WorkScopeId::new();
        store.commit_work_scope(scope, 2).unwrap();
        store.commit_work_scope(other, 2).unwrap();
        store
            .bind_work_scope_root(scope, "/tmp/seyal-bound-a")
            .unwrap();
        assert_eq!(
            store.work_scope_bound_root(scope).unwrap().as_deref(),
            Some("/tmp/seyal-bound-a")
        );
        assert_eq!(store.work_scope_bound_root(other).unwrap(), None);
        store
            .bind_work_scope_root(scope, "/tmp/seyal-bound-b")
            .unwrap();
        assert_eq!(
            store.work_scope_bound_root(scope).unwrap().as_deref(),
            Some("/tmp/seyal-bound-b")
        );
    }

    #[test]
    fn bind_without_a_work_scope_row_fails_closed() {
        let store = temp_store();
        let scope = WorkScopeId::new();
        assert_eq!(
            store.bind_work_scope_root(scope, "/tmp/missing-scope"),
            Err(StoreError::Corrupt)
        );
    }

    #[test]
    fn empty_or_oversized_root_is_rejected() {
        let store = temp_store();
        let scope = WorkScopeId::new();
        store.commit_work_scope(scope, 2).unwrap();
        assert_eq!(
            store.bind_work_scope_root(scope, ""),
            Err(StoreError::WriteFailed)
        );
        let huge = "x".repeat(MAX_BOUND_ROOT_CHARS + 1);
        assert_eq!(
            store.bind_work_scope_root(scope, &huge),
            Err(StoreError::WriteFailed)
        );
    }
}
