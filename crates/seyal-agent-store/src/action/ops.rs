//! Persist immutable ActionIntent before any dispatch attempt.

use rusqlite::{params, OptionalExtension};
use seyal_agent_core::{
    linearize_cancel, reconcile, recover, ActionId, ActionIntent, ActionIntentError,
    ActionLifecycle, ActionRuntime, AgentRunId, CrashBoundary, RecoveryDecision, RecoveryError,
    RecoveryEvidence, DEFAULT_AUTOMATIC_RECONCILIATION_BUDGET,
};

use crate::sqlite::AgentStore;
use crate::StoreError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionError {
    Store(StoreError),
    Intent(ActionIntentError),
    UnknownAgentRun,
    IdentityMismatch,
    Recovery(RecoveryError),
    IllegalLifecycle,
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

impl From<RecoveryError> for ActionError {
    fn from(value: RecoveryError) -> Self {
        Self::Recovery(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrepareOutcome {
    NewlyPrepared,
    Duplicate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistedAction {
    pub intent: ActionIntent,
    pub runtime: ActionRuntime,
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
        if let Some((stored_digest, _lifecycle)) = existing {
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

    /// Load identity via `get()` digest check, then overlay mutable lifecycle.
    /// Dispatch/authorization must not trust `list_for_run` alone.
    pub fn get_record(&self, action_id: ActionId) -> Result<Option<PersistedAction>, ActionError> {
        let intent = match self.get(action_id)? {
            Some(intent) => intent,
            None => return Ok(None),
        };
        let conn = self.store.conn.lock().expect("agent store lock");
        struct RuntimeRow {
            lifecycle: i64,
            dispatch_generation: Option<i64>,
            invalidated: i64,
            cancel: i64,
            attempts: i64,
        }
        let row: Option<RuntimeRow> = conn
            .query_row(
                "SELECT lifecycle, dispatch_generation, authorization_invalidated,
                        cancel_requested, reconciliation_attempts
                 FROM action_intent WHERE action_id = ?1",
                params![action_id.to_bytes().to_vec()],
                |row| {
                    Ok(RuntimeRow {
                        lifecycle: row.get(0)?,
                        dispatch_generation: row.get(1)?,
                        invalidated: row.get(2)?,
                        cancel: row.get(3)?,
                        attempts: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(|_| StoreError::Corrupt)?;
        let Some(row) = row else {
            return Err(ActionError::Store(StoreError::Corrupt));
        };
        let lifecycle = ActionLifecycle::from_code(row.lifecycle as u8)
            .ok_or(ActionError::Store(StoreError::Corrupt))?;
        Ok(Some(PersistedAction {
            intent,
            runtime: ActionRuntime {
                lifecycle,
                dispatch_generation: row.dispatch_generation.map(|v| v as u64),
                authorization_invalidated: row.invalidated != 0,
                cancel_requested: row.cancel != 0,
                reconciliation_attempts: row.attempts as u32,
                automatic_budget: DEFAULT_AUTOMATIC_RECONCILIATION_BUDGET,
            },
        }))
    }

    /// Fixture seam for #1310: policy/human authorization is recorded without
    /// consuming an ApprovalDecision in this Issue.
    pub fn fixture_mark_authorized(&self, action_id: ActionId) -> Result<(), ActionError> {
        self.transition_fixture(
            action_id,
            ActionLifecycle::Prepared,
            ActionLifecycle::Authorized,
            None,
        )
    }

    /// Fixture seam for #1310: durable Dispatching without approval consumption.
    pub fn fixture_mark_dispatching(
        &self,
        action_id: ActionId,
        dispatch_generation: u64,
    ) -> Result<(), ActionError> {
        self.transition_fixture(
            action_id,
            ActionLifecycle::Authorized,
            ActionLifecycle::Dispatching,
            Some(dispatch_generation),
        )
    }

    pub fn recover(
        &self,
        action_id: ActionId,
        boundary: CrashBoundary,
        evidence: &RecoveryEvidence,
    ) -> Result<RecoveryDecision, ActionError> {
        let record = self
            .get_record(action_id)?
            .ok_or(ActionError::Store(StoreError::Corrupt))?;
        if matches!(boundary, CrashBoundary::BeforeIntentPersist) {
            return Err(ActionError::IllegalLifecycle);
        }
        let decision = recover(
            record.intent.action_id(),
            record.intent.agent_run_id(),
            record.intent.effect_class(),
            &record.runtime,
            boundary,
            evidence,
        )?;
        self.persist_decision(&record, decision, evidence)
    }

    pub fn reconcile(
        &self,
        action_id: ActionId,
        evidence: &RecoveryEvidence,
    ) -> Result<RecoveryDecision, ActionError> {
        let record = self
            .get_record(action_id)?
            .ok_or(ActionError::Store(StoreError::Corrupt))?;
        let decision = reconcile(
            record.intent.action_id(),
            record.intent.agent_run_id(),
            record.intent.effect_class(),
            &record.runtime,
            evidence,
        )?;
        self.persist_decision(&record, decision, evidence)
    }

    pub fn cancel(&self, action_id: ActionId) -> Result<RecoveryDecision, ActionError> {
        let record = self
            .get_record(action_id)?
            .ok_or(ActionError::Store(StoreError::Corrupt))?;
        let decision = linearize_cancel(&record.runtime);
        self.persist_decision(&record, decision, &RecoveryEvidence::none())
    }

    fn transition_fixture(
        &self,
        action_id: ActionId,
        expected: ActionLifecycle,
        next: ActionLifecycle,
        dispatch_generation: Option<u64>,
    ) -> Result<(), ActionError> {
        self.store.gate_write()?;
        let record = self
            .get_record(action_id)?
            .ok_or(ActionError::Store(StoreError::Corrupt))?;
        if record.runtime.lifecycle != expected {
            return Err(ActionError::IllegalLifecycle);
        }
        let conn = self.store.conn.lock().expect("agent store lock");
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;
        tx.execute(
            "UPDATE action_intent SET lifecycle = ?1, dispatch_generation = ?2
             WHERE action_id = ?3 AND digest = ?4",
            params![
                next.code() as i64,
                dispatch_generation.map(|g| g as i64),
                action_id.to_bytes().to_vec(),
                record.intent.digest().to_vec()
            ],
        )
        .map_err(|_| StoreError::WriteFailed)?;
        self.append_history(&tx, action_id, expected, next, 0)?;
        tx.commit().map_err(|_| StoreError::WriteFailed)?;
        Ok(())
    }

    fn persist_decision(
        &self,
        record: &PersistedAction,
        decision: RecoveryDecision,
        evidence: &RecoveryEvidence,
    ) -> Result<RecoveryDecision, ActionError> {
        self.store.gate_write()?;
        let from = record.runtime.lifecycle;
        let conn = self.store.conn.lock().expect("agent store lock");
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;
        let encoded = evidence.encode();
        let expected_generation = record.runtime.dispatch_generation.map(|g| g as i64);
        let updated = tx
            .execute(
                "UPDATE action_intent
             SET lifecycle = ?1,
                 dispatch_generation = ?2,
                 authorization_invalidated = ?3,
                 cancel_requested = ?4,
                 reconciliation_attempts = ?5,
                 last_evidence = ?6
             WHERE action_id = ?7 AND digest = ?8 AND lifecycle = ?9
               AND (
                    (?10 IS NULL AND dispatch_generation IS NULL)
                    OR dispatch_generation = ?10
               )",
                params![
                    decision.runtime.lifecycle.code() as i64,
                    decision.runtime.dispatch_generation.map(|g| g as i64),
                    i64::from(decision.runtime.authorization_invalidated),
                    i64::from(decision.runtime.cancel_requested),
                    decision.runtime.reconciliation_attempts as i64,
                    encoded,
                    record.intent.action_id().to_bytes().to_vec(),
                    record.intent.digest().to_vec(),
                    from.code() as i64,
                    expected_generation,
                ],
            )
            .map_err(|_| StoreError::WriteFailed)?;
        if updated != 1 {
            return Err(ActionError::IllegalLifecycle);
        }
        self.append_history(
            &tx,
            record.intent.action_id(),
            from,
            decision.runtime.lifecycle,
            evidence.kind.code() as i64,
        )?;
        tx.commit().map_err(|_| StoreError::WriteFailed)?;
        Ok(decision)
    }

    fn append_history(
        &self,
        tx: &rusqlite::Transaction<'_>,
        action_id: ActionId,
        from: ActionLifecycle,
        to: ActionLifecycle,
        evidence_kind: i64,
    ) -> Result<(), ActionError> {
        let next_seq: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(seq), 0) + 1 FROM action_recovery_history WHERE action_id = ?1",
                params![action_id.to_bytes().to_vec()],
                |row| row.get(0),
            )
            .map_err(|_| StoreError::Corrupt)?;
        tx.execute(
            "INSERT INTO action_recovery_history (
                action_id, seq, from_lifecycle, to_lifecycle, evidence_kind, recorded_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                action_id.to_bytes().to_vec(),
                next_seq,
                from.code() as i64,
                to.code() as i64,
                evidence_kind,
                1_i64
            ],
        )
        .map_err(|_| StoreError::WriteFailed)?;
        Ok(())
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
