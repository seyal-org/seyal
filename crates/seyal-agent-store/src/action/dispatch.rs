//! Atomic Authorized → Dispatching with exact approval consumption (SPEC-016 §6).

use rusqlite::{params, OptionalExtension, Transaction};
use seyal_agent_core::{
    evaluate_consume, evaluate_dispatch, human_approval_required, ActionId, ActionLifecycle,
    ApprovalDecision, ApprovalError, ApprovalId, ApprovalVerdict, ConsumptionWitness, ControlMode,
    DispatchError, DispatchEval, PersistHealth, RevocationFence, RevocationFenceMember,
    RevocationGeneration,
};

use super::ops::{ActionAuthority, ActionError, PersistedAction};
use crate::approval::{consume_in_tx, load_decision, load_request};
use crate::StoreError;

/// Worker-supplied dispatch environment. Binding generation is the worker's
/// cached AgentRun fence; current policy is the live policy engine value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DispatchInput {
    pub now_ms: u64,
    pub current_policy_generation: u64,
    pub expected_binding_generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DispatchOutcome {
    pub action_id: ActionId,
    pub dispatch_generation: u64,
    pub consumed_approval: Option<ApprovalDecision>,
}

impl ActionError {
    fn from_dispatch(err: DispatchError) -> Self {
        Self::Dispatch(err)
    }
}

impl<'a> ActionAuthority<'a> {
    /// Bind exact authorization without consuming an approval (SPEC-016 §4).
    /// `Prepared -> Dispatching` is forbidden.
    pub fn authorize(&self, action_id: ActionId, now_ms: u64) -> Result<(), ActionError> {
        self.begin_action_write()?;
        match self.authorize_committed(action_id, now_ms) {
            Ok(()) => {
                self.store.note_action_write_ok();
                Ok(())
            }
            Err(ActionError::Store(StoreError::WriteFailed)) => {
                self.store.note_action_write_failed();
                Err(ActionError::Store(StoreError::WriteFailed))
            }
            Err(err) => Err(err),
        }
    }

    fn authorize_committed(&self, action_id: ActionId, now_ms: u64) -> Result<(), ActionError> {
        self.store.gate_write()?;
        let record = self
            .get_record(action_id)?
            .ok_or(ActionError::Store(StoreError::Corrupt))?;
        if record.runtime.lifecycle != ActionLifecycle::Prepared {
            return Err(ActionError::IllegalLifecycle);
        }
        if record
            .intent
            .expires_at_ms()
            .is_some_and(|expiry| now_ms >= expiry)
        {
            return Err(ActionError::Dispatch(DispatchError::Expired));
        }
        if human_approval_required(&record.intent) {
            let _ = self.require_unconsumed_approval(&record, now_ms)?;
        }
        let conn = self.store.conn.lock().expect("agent store lock");
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;
        let updated = tx
            .execute(
                "UPDATE action_intent SET lifecycle = ?1, authorization_invalidated = 0
                 WHERE action_id = ?2 AND digest = ?3 AND lifecycle = ?4",
                params![
                    ActionLifecycle::Authorized.code() as i64,
                    action_id.to_bytes().to_vec(),
                    record.intent.digest().to_vec(),
                    ActionLifecycle::Prepared.code() as i64,
                ],
            )
            .map_err(|_| StoreError::WriteFailed)?;
        if updated != 1 {
            return Err(ActionError::IllegalLifecycle);
        }
        self.append_history(
            &tx,
            action_id,
            ActionLifecycle::Prepared,
            ActionLifecycle::Authorized,
            0,
        )?;
        tx.commit().map_err(|_| StoreError::WriteFailed)?;
        Ok(())
    }

    /// Atomic consume + `Dispatching`. Never commits consumed-without-Dispatching.
    pub fn dispatch(
        &self,
        action_id: ActionId,
        input: DispatchInput,
    ) -> Result<DispatchOutcome, ActionError> {
        if !self.store.action_persist_health().allows_new_effect()
            && self.store.action_persist_health() != PersistHealth::Paused
        {
            return Err(ActionError::from_dispatch(
                DispatchError::PersistHealthNotMinting,
            ));
        }
        self.begin_action_write()?;
        match self.dispatch_committed(action_id, input) {
            Ok(outcome) => {
                self.store.note_action_write_ok();
                Ok(outcome)
            }
            Err(ActionError::Store(StoreError::WriteFailed)) => {
                self.store.note_action_write_failed();
                Err(ActionError::Store(StoreError::WriteFailed))
            }
            Err(err) => Err(err),
        }
    }

    fn dispatch_committed(
        &self,
        action_id: ActionId,
        input: DispatchInput,
    ) -> Result<DispatchOutcome, ActionError> {
        self.store.gate_write()?;
        let record = self
            .get_record(action_id)?
            .ok_or(ActionError::Store(StoreError::Corrupt))?;
        let run = self.store.agent_run(record.intent.agent_run_id())?;
        let conn = self.store.conn.lock().expect("agent store lock");
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;
        let current_fence = current_fence_for(&tx, record.intent.revocation_fence())?;
        let evaluated = evaluate_dispatch(DispatchEval {
            intent: &record.intent,
            runtime: &record.runtime,
            persist_health: self.store.action_persist_health(),
            now_ms: input.now_ms,
            current_policy_generation: input.current_policy_generation,
            current_fence: &current_fence,
            current_binding_generation: run.binding_generation,
            expected_binding_generation: input.expected_binding_generation,
        });
        let generation = match evaluated {
            Err(DispatchError::AlreadyDispatching) => {
                return Err(ActionError::from_dispatch(
                    DispatchError::AlreadyDispatching,
                ));
            }
            Err(err) => {
                invalidate_authorized(&tx, &record)?;
                tx.commit().map_err(|_| StoreError::WriteFailed)?;
                return Err(ActionError::from_dispatch(err));
            }
            Ok(generation) => generation,
        };
        let consumed = if human_approval_required(&record.intent) {
            match consume_matching(&tx, &record, &current_fence, input) {
                Ok(decision) => Some(decision),
                Err(err) => {
                    invalidate_authorized(&tx, &record)?;
                    tx.commit().map_err(|_| StoreError::WriteFailed)?;
                    return Err(err);
                }
            }
        } else {
            None
        };
        let updated = tx
            .execute(
                "UPDATE action_intent
                 SET lifecycle = ?1, dispatch_generation = ?2, authorization_invalidated = 0
                 WHERE action_id = ?3 AND digest = ?4 AND lifecycle = ?5
                   AND cancel_requested = 0 AND authorization_invalidated = 0",
                params![
                    ActionLifecycle::Dispatching.code() as i64,
                    generation as i64,
                    action_id.to_bytes().to_vec(),
                    record.intent.digest().to_vec(),
                    ActionLifecycle::Authorized.code() as i64,
                ],
            )
            .map_err(|_| StoreError::WriteFailed)?;
        if updated != 1 {
            return Err(ActionError::IllegalLifecycle);
        }
        self.append_history(
            &tx,
            action_id,
            ActionLifecycle::Authorized,
            ActionLifecycle::Dispatching,
            0,
        )?;
        tx.commit().map_err(|_| StoreError::WriteFailed)?;
        Ok(DispatchOutcome {
            action_id,
            dispatch_generation: generation,
            consumed_approval: consumed,
        })
    }

    fn require_unconsumed_approval(
        &self,
        record: &PersistedAction,
        now_ms: u64,
    ) -> Result<ApprovalId, ActionError> {
        let conn = self.store.conn.lock().expect("agent store lock");
        find_unconsumed_approval_id(&conn, record, now_ms)
    }
}

fn consume_matching(
    tx: &Transaction<'_>,
    record: &PersistedAction,
    current_fence: &RevocationFence,
    input: DispatchInput,
) -> Result<ApprovalDecision, ActionError> {
    let approval_id = find_unconsumed_approval_id(tx, record, input.now_ms)?;
    let request = load_request(tx, approval_id)?
        .ok_or(ActionError::Dispatch(DispatchError::ApprovalRequired))?;
    if request.control_mode != ControlMode::SeyalControlled {
        return Err(ActionError::Approval(
            ApprovalError::ExternalObservedForbidden,
        ));
    }
    let witness = ConsumptionWitness::from_live_intent(
        &record.intent,
        input.current_policy_generation,
        current_fence.clone(),
    );
    consume_in_tx(tx, approval_id, &witness, input.now_ms).map_err(ActionError::from)
}

fn find_unconsumed_approval_id(
    conn: &rusqlite::Connection,
    record: &PersistedAction,
    now_ms: u64,
) -> Result<ApprovalId, ActionError> {
    let rows: Vec<Vec<u8>> = {
        let mut statement = conn
            .prepare(
                "SELECT r.approval_id FROM approval_request r
                 INNER JOIN approval_decision d ON d.approval_id = r.approval_id
                 WHERE r.action_id = ?1 AND d.consumed = 0 AND d.decision = ?2",
            )
            .map_err(|_| StoreError::Corrupt)?;
        let mapped = statement
            .query_map(
                params![
                    record.intent.action_id().to_bytes().to_vec(),
                    ApprovalVerdict::Approved.as_u8()
                ],
                |row| row.get(0),
            )
            .map_err(|_| StoreError::Corrupt)?;
        let mut out = Vec::new();
        for row in mapped {
            out.push(row.map_err(|_| StoreError::Corrupt)?);
        }
        out
    };
    if rows.len() != 1 {
        return Err(ActionError::Dispatch(DispatchError::ApprovalRequired));
    }
    let approval_id = ApprovalId::from_bytes(
        rows[0]
            .as_slice()
            .try_into()
            .map_err(|_| StoreError::Corrupt)?,
    );
    let request = load_request(conn, approval_id)?
        .ok_or(ActionError::Dispatch(DispatchError::ApprovalRequired))?;
    let decision = load_decision(conn, approval_id)?
        .ok_or(ActionError::Dispatch(DispatchError::ApprovalRequired))?;
    let witness = ConsumptionWitness::from_live_intent(
        &record.intent,
        record.intent.policy_generation(),
        record.intent.revocation_fence().clone(),
    );
    evaluate_consume(&request, &decision, &witness, now_ms).map_err(ActionError::Approval)?;
    Ok(approval_id)
}

fn current_fence_for(
    tx: &Transaction<'_>,
    bound: &RevocationFence,
) -> Result<RevocationFence, ActionError> {
    if bound.members().is_empty() {
        return Err(ActionError::from_dispatch(DispatchError::IncompleteFence));
    }
    let mut members = Vec::new();
    for member in bound.members() {
        let current: i64 = tx
            .query_row(
                "SELECT generation FROM revocation_scope_generation
                 WHERE scope_kind = ?1 AND scope_id = ?2",
                params![member.scope.kind.code() as i64, member.scope.id.as_slice()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| StoreError::Corrupt)?
            .unwrap_or(1);
        members.push(RevocationFenceMember {
            scope: member.scope,
            generation: RevocationGeneration::from_raw(current as u64)
                .ok_or(ActionError::Store(StoreError::Corrupt))?,
        });
    }
    RevocationFence::new(members).map_err(|err| ActionError::Intent(err.into()))
}

fn invalidate_authorized(
    tx: &Transaction<'_>,
    record: &PersistedAction,
) -> Result<(), ActionError> {
    if record.runtime.lifecycle != ActionLifecycle::Authorized {
        return Ok(());
    }
    let updated = tx
        .execute(
            "UPDATE action_intent
             SET lifecycle = ?1, authorization_invalidated = 1, dispatch_generation = NULL
             WHERE action_id = ?2 AND digest = ?3 AND lifecycle = ?4",
            params![
                ActionLifecycle::Prepared.code() as i64,
                record.intent.action_id().to_bytes().to_vec(),
                record.intent.digest().to_vec(),
                ActionLifecycle::Authorized.code() as i64,
            ],
        )
        .map_err(|_| StoreError::WriteFailed)?;
    if updated != 1 {
        return Err(ActionError::IllegalLifecycle);
    }
    let next_seq: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM action_recovery_history WHERE action_id = ?1",
            params![record.intent.action_id().to_bytes().to_vec()],
            |row| row.get(0),
        )
        .map_err(|_| StoreError::Corrupt)?;
    tx.execute(
        "INSERT INTO action_recovery_history (
            action_id, seq, from_lifecycle, to_lifecycle, evidence_kind, recorded_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            record.intent.action_id().to_bytes().to_vec(),
            next_seq,
            ActionLifecycle::Authorized.code() as i64,
            ActionLifecycle::Prepared.code() as i64,
            0i64,
            1i64
        ],
    )
    .map_err(|_| StoreError::WriteFailed)?;
    Ok(())
}
