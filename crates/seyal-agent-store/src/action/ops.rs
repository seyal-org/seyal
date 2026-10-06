//! Persist immutable ActionIntent before any dispatch attempt.

use rusqlite::{params, OptionalExtension};
use seyal_agent_core::{ActionId, ActionIntent, ActionIntentError, ActionLifecycle, AgentRunId};

use crate::sqlite::AgentStore;
use crate::StoreError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionError {
    Store(StoreError),
    Intent(ActionIntentError),
    UnknownAgentRun,
    IdentityMismatch,
}

impl From<StoreError> for ActionError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<ActionIntentError> for ActionError {
    fn from(value: ActionIntentError) -> Self {
        Self::Intent(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrepareOutcome {
    NewlyPrepared,
    Duplicate,
}

pub struct ActionAuthority<'a> {
    store: &'a AgentStore,
}

impl<'a> ActionAuthority<'a> {
    pub(super) fn new(store: &'a AgentStore) -> Self {
        Self { store }
    }

    /// Persist immutable intent in `Prepared` before dispatch. Never mutates an
    /// existing row. Same ActionId + digest is a duplicate; digest mismatch
    /// fails closed (SPEC-016 §20).
    pub fn prepare(&self, intent: &ActionIntent) -> Result<PrepareOutcome, ActionError> {
        if intent.lifecycle() != ActionLifecycle::Prepared {
            return Err(ActionError::Intent(ActionIntentError::InvalidField));
        }
        self.store.gate_write()?;
        let conn = self.store.conn.lock().expect("agent store lock");
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;
        let run_id = intent.agent_run_id().to_bytes().to_vec();
        let run_exists: Option<i64> = tx
            .query_row(
                "SELECT 1 FROM agent_run WHERE id = ?1",
                params![run_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| StoreError::Corrupt)?;
        if run_exists.is_none() {
            return Err(ActionError::UnknownAgentRun);
        }
        let action_id = intent.action_id().to_bytes().to_vec();
        let existing: Option<(Vec<u8>, i64)> = tx
            .query_row(
                "SELECT digest, lifecycle FROM action_intent WHERE action_id = ?1",
                params![action_id.clone()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|_| StoreError::Corrupt)?;
        let digest = intent.digest().to_vec();
        if let Some((stored_digest, lifecycle)) = existing {
            if lifecycle != ActionLifecycle::Prepared.code() as i64 {
                return Err(ActionError::Store(StoreError::Corrupt));
            }
            if stored_digest != digest {
                return Err(ActionError::IdentityMismatch);
            }
            tx.commit().map_err(|_| StoreError::WriteFailed)?;
            return Ok(PrepareOutcome::Duplicate);
        }
        let canonical = intent.encode();
        tx.execute(
            "INSERT INTO action_intent (
                action_id, agent_run_id, digest, lifecycle, canonical_intent, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                action_id,
                run_id,
                digest,
                ActionLifecycle::Prepared.code() as i64,
                canonical,
                intent.created_at_ms() as i64
            ],
        )
        .map_err(|_| StoreError::WriteFailed)?;
        tx.commit().map_err(|_| StoreError::WriteFailed)?;
        Ok(PrepareOutcome::NewlyPrepared)
    }

    pub fn get(&self, action_id: ActionId) -> Result<Option<ActionIntent>, ActionError> {
        let conn = self.store.conn.lock().expect("agent store lock");
        let row: Option<(Vec<u8>, Vec<u8>)> = conn
            .query_row(
                "SELECT digest, canonical_intent FROM action_intent WHERE action_id = ?1",
                params![action_id.to_bytes().to_vec()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|_| StoreError::Corrupt)?;
        let Some((digest, canonical)) = row else {
            return Ok(None);
        };
        let intent = ActionIntent::decode(&canonical)?;
        if intent.digest().as_slice() != digest.as_slice() {
            return Err(ActionError::Store(StoreError::Corrupt));
        }
        if intent.action_id() != action_id {
            return Err(ActionError::Store(StoreError::Corrupt));
        }
        Ok(Some(intent))
    }

    pub fn list_for_run(&self, run_id: AgentRunId) -> Result<Vec<ActionIntent>, ActionError> {
        let conn = self.store.conn.lock().expect("agent store lock");
        let mut statement = conn
            .prepare(
                "SELECT canonical_intent FROM action_intent WHERE agent_run_id = ?1 ORDER BY created_at, action_id",
            )
            .map_err(|_| StoreError::Corrupt)?;
        let rows = statement
            .query_map(params![run_id.to_bytes().to_vec()], |row| {
                row.get::<_, Vec<u8>>(0)
            })
            .map_err(|_| StoreError::Corrupt)?;
        let mut out = Vec::new();
        for row in rows {
            let canonical = row.map_err(|_| StoreError::Corrupt)?;
            out.push(ActionIntent::decode(&canonical)?);
        }
        Ok(out)
    }
}
