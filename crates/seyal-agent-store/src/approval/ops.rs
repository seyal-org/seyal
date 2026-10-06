//! Durable approval request/decision recording and SPEC-016 consume seam.

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, OptionalExtension};
use seyal_agent_core::{
    allowed_attention_transition, authorize_decide, evaluate_consume, mint_from_trusted_source,
    ActionId, AgentRunId, ApprovalDecision, ApprovalError, ApprovalId, ApprovalRequest,
    ApprovalRequestSpec, ApprovalVerdict, ArgumentFingerprint, AttentionId, AttentionKind,
    AttentionState, AttentionTarget, CapabilityRef, ClientPrincipalId, ClientSessionId,
    ConsumptionWitness, ControlMode, DecisionAuthority, EffectClass, ResourceIdentity,
    RevocationFence, TrustedMintSpec,
};

use crate::sqlite::AgentStore;
use crate::StoreError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalStoreError {
    Domain(ApprovalError),
    Store(StoreError),
}

impl From<ApprovalError> for ApprovalStoreError {
    fn from(value: ApprovalError) -> Self {
        Self::Domain(value)
    }
}

impl From<StoreError> for ApprovalStoreError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

#[derive(Clone, Debug)]
pub struct DecideInput {
    pub approval_id: ApprovalId,
    pub action_id: ActionId,
    pub agent_run_id: AgentRunId,
    pub verdict: ApprovalVerdict,
    pub authority: DecisionAuthority,
    pub decision_policy_generation: u64,
    pub decision_principal_id: Option<ClientPrincipalId>,
    pub require_session: bool,
    pub client_session: Option<ClientSessionId>,
    pub session_valid: bool,
    pub has_approval_decide_scope: bool,
    pub now_unix_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedApproval {
    pub request: ApprovalRequest,
    pub decision: Option<ApprovalDecision>,
}

pub struct ApprovalAuthority<'a> {
    store: &'a AgentStore,
}

impl<'a> ApprovalAuthority<'a> {
    pub(super) fn new(store: &'a AgentStore) -> Self {
        Self { store }
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// Persist a complete §5.1 request and linked ApprovalRequired Attention.
    pub fn record_request(
        &self,
        mut spec: ApprovalRequestSpec,
        summary: impl Into<String>,
    ) -> Result<ApprovalRequest, ApprovalStoreError> {
        self.store.gate_write()?;
        if spec.attention_id.is_none() {
            spec.attention_id = Some(AttentionId::new());
        }
        let request = ApprovalRequest::try_build(spec)?;
        let conn = self
            .store
            .conn
            .lock()
            .map_err(|_| StoreError::WriteFailed)?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;
        let open: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM attention_item
                 WHERE agent_run_id = ?1 AND state IN (1, 2)",
                params![request.agent_run_id.to_bytes().as_slice()],
                |row| row.get(0),
            )
            .map_err(|_| StoreError::Corrupt)?;
        let item = mint_from_trusted_source(TrustedMintSpec {
            kind: AttentionKind::ApprovalRequired,
            summary: summary.into(),
            target: AttentionTarget {
                resource_address: None,
                agent_run_id: Some(request.agent_run_id),
                action_id: Some(request.action_id),
                artifact_id: None,
                requires_spatial_focus: false,
            },
            agent_run_id: Some(request.agent_run_id),
            action_id: Some(request.action_id),
            approval_id: Some(request.approval_id),
            now_unix_ms: request.requested_at_unix_ms,
            open_count_for_run: open as usize,
        })
        .map_err(|_| ApprovalError::IncompleteBinding)?;
        tx.execute(
            "INSERT INTO attention_item (
                attention_id, work_item_id, attempt_id, agent_run_id, action_id, approval_id,
                kind, state, priority, summary, requires_spatial_focus, resource_address,
                coalesce_key, created_at_unix_ms, updated_at_unix_ms, resolved_at_unix_ms,
                expires_at_unix_ms
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
            params![
                request.attention_id.to_bytes().as_slice(),
                item.work_item_id.map(|id| id.to_bytes().to_vec()),
                item.attempt_id.map(|id| id.to_bytes().to_vec()),
                request.agent_run_id.to_bytes().as_slice(),
                request.action_id.to_bytes().as_slice(),
                request.approval_id.to_bytes().as_slice(),
                item.kind.as_u8(),
                item.state.as_u8(),
                item.priority.as_u8(),
                item.summary.as_str(),
                item.target.requires_spatial_focus as i64,
                item.target.resource_address.as_deref(),
                item.coalesce_key.as_deref(),
                item.created_at_unix_ms as i64,
                item.updated_at_unix_ms as i64,
                item.resolved_at_unix_ms.map(|v| v as i64),
                request.expires_at_unix_ms.map(|v| v as i64),
            ],
        )
        .map_err(|_| StoreError::WriteFailed)?;
        insert_request(&tx, &request)?;
        tx.commit().map_err(|_| StoreError::WriteFailed)?;
        Ok(request)
    }

    pub fn get(&self, approval_id: ApprovalId) -> Result<RecordedApproval, ApprovalStoreError> {
        let conn = self
            .store
            .conn
            .lock()
            .map_err(|_| StoreError::WriteFailed)?;
        let request = load_request(&conn, approval_id)?.ok_or(ApprovalError::UnknownApproval)?;
        let decision = load_decision(&conn, approval_id)?;
        Ok(RecordedApproval { request, decision })
    }

    /// Record Approved|Rejected. Duplicate concurrent Approve collapses to one
    /// consumable decision. Resolves the linked AttentionItem.
    pub fn decide(&self, input: DecideInput) -> Result<ApprovalDecision, ApprovalStoreError> {
        self.store.gate_write()?;
        if input.require_session {
            if input.client_session.is_none() {
                return Err(ApprovalError::StaleSession.into());
            }
            authorize_decide(input.session_valid, input.has_approval_decide_scope)?;
        }
        let now = input.now_unix_ms.unwrap_or_else(Self::now_ms);
        let conn = self
            .store
            .conn
            .lock()
            .map_err(|_| StoreError::WriteFailed)?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;
        let request =
            load_request(&tx, input.approval_id)?.ok_or(ApprovalError::UnknownApproval)?;
        if request.action_id != input.action_id || request.agent_run_id != input.agent_run_id {
            return Err(ApprovalError::BindingMismatch.into());
        }
        if request.is_expired(now) {
            return Err(ApprovalError::Expired.into());
        }
        if let Some(existing) = load_decision(&tx, input.approval_id)? {
            if existing.decision == ApprovalVerdict::Approved
                && input.verdict == ApprovalVerdict::Approved
                && !existing.consumed
            {
                tx.commit().map_err(|_| StoreError::WriteFailed)?;
                return Ok(existing);
            }
            return Err(ApprovalError::AlreadyDecided.into());
        }
        let decision = ApprovalDecision {
            approval_id: input.approval_id,
            action_id: input.action_id,
            agent_run_id: input.agent_run_id,
            decision: input.verdict,
            authority: input.authority,
            decided_at_unix_ms: now,
            decision_policy_generation: input.decision_policy_generation,
            decision_principal_id: input.decision_principal_id,
            consumed: false,
        };
        tx.execute(
            "INSERT INTO approval_decision (
                approval_id, action_id, agent_run_id, decision, authority,
                decided_at_unix_ms, decision_policy_generation, decision_principal_id, consumed
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                decision.approval_id.to_bytes().as_slice(),
                decision.action_id.to_bytes().as_slice(),
                decision.agent_run_id.to_bytes().as_slice(),
                decision.decision.as_u8(),
                decision.authority.as_u8(),
                decision.decided_at_unix_ms as i64,
                decision.decision_policy_generation as i64,
                decision
                    .decision_principal_id
                    .map(|id| id.to_bytes().to_vec()),
                0i64,
            ],
        )
        .map_err(|_| StoreError::WriteFailed)?;
        resolve_attention(&tx, request.attention_id, now)?;
        tx.commit().map_err(|_| StoreError::WriteFailed)?;
        Ok(decision)
    }

    /// SPEC-016 fixture consumer: exact single-use consume without dispatch.
    pub fn consume_exact(
        &self,
        approval_id: ApprovalId,
        witness: &ConsumptionWitness,
        now_unix_ms: u64,
    ) -> Result<ApprovalDecision, ApprovalStoreError> {
        self.store.gate_write()?;
        let conn = self
            .store
            .conn
            .lock()
            .map_err(|_| StoreError::WriteFailed)?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;
        let request = load_request(&tx, approval_id)?.ok_or(ApprovalError::UnknownApproval)?;
        let mut decision =
            load_decision(&tx, approval_id)?.ok_or(ApprovalError::UnknownApproval)?;
        evaluate_consume(&request, &decision, witness, now_unix_ms)?;
        let changed = tx
            .execute(
                "UPDATE approval_decision SET consumed = 1 WHERE approval_id = ?1 AND consumed = 0",
                params![approval_id.to_bytes().as_slice()],
            )
            .map_err(|_| StoreError::WriteFailed)?;
        if changed != 1 {
            return Err(ApprovalError::AlreadyConsumed.into());
        }
        tx.commit().map_err(|_| StoreError::WriteFailed)?;
        decision.consumed = true;
        Ok(decision)
    }
}

fn insert_request(
    conn: &rusqlite::Connection,
    request: &ApprovalRequest,
) -> Result<(), ApprovalStoreError> {
    conn.execute(
        "INSERT INTO approval_request (
            approval_id, action_id, action_intent_digest, agent_run_id, capability,
            resource_type, resource_canonical_id, resource_scope, resource_version_or_fingerprint,
            argument_fingerprint, effect_class, policy_generation, revocation_fence,
            expires_at_unix_ms, requested_at_unix_ms, attention_id, control_mode
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
        params![
            request.approval_id.to_bytes().as_slice(),
            request.action_id.to_bytes().as_slice(),
            request.action_intent_digest.as_slice(),
            request.agent_run_id.to_bytes().as_slice(),
            request.capability.as_bytes(),
            request.resource.resource_type(),
            request.resource.canonical_id().as_slice(),
            request.resource.scope().as_slice(),
            request.resource.version_or_fingerprint().as_slice(),
            request.argument_fingerprint.0.as_slice(),
            request.effect_class.code(),
            request.policy_generation as i64,
            request.revocation_fence.encode(),
            request.expires_at_unix_ms.map(|v| v as i64),
            request.requested_at_unix_ms as i64,
            request.attention_id.to_bytes().as_slice(),
            request.control_mode.as_u8(),
        ],
    )
    .map_err(|_| StoreError::WriteFailed)?;
    Ok(())
}

fn load_request(
    conn: &rusqlite::Connection,
    approval_id: ApprovalId,
) -> Result<Option<ApprovalRequest>, ApprovalStoreError> {
    let row = conn
        .query_row(
            "SELECT action_id, action_intent_digest, agent_run_id, capability,
                    resource_type, resource_canonical_id, resource_scope,
                    resource_version_or_fingerprint, argument_fingerprint, effect_class,
                    policy_generation, revocation_fence, expires_at_unix_ms,
                    requested_at_unix_ms, attention_id, control_mode
             FROM approval_request WHERE approval_id = ?1",
            params![approval_id.to_bytes().as_slice()],
            |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                    row.get::<_, Vec<u8>>(5)?,
                    row.get::<_, Vec<u8>>(6)?,
                    row.get::<_, Vec<u8>>(7)?,
                    row.get::<_, Vec<u8>>(8)?,
                    row.get::<_, u8>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, Vec<u8>>(11)?,
                    row.get::<_, Option<i64>>(12)?,
                    row.get::<_, i64>(13)?,
                    row.get::<_, Vec<u8>>(14)?,
                    row.get::<_, u8>(15)?,
                ))
            },
        )
        .optional()
        .map_err(|_| StoreError::Corrupt)?;
    let Some((
        action_id,
        digest,
        run,
        cap,
        resource_type,
        canonical,
        scope,
        version,
        args,
        effect,
        policy,
        fence,
        expires,
        requested,
        attention,
        mode,
    )) = row
    else {
        return Ok(None);
    };
    let request = ApprovalRequest {
        approval_id,
        action_id: ActionId::from_bytes(id16(&action_id)?),
        action_intent_digest: arr32(&digest)?,
        agent_run_id: AgentRunId::from_bytes(id16(&run)?),
        capability: CapabilityRef::new(cap).map_err(ApprovalError::from)?,
        resource: ResourceIdentity::new(
            resource_type,
            id16(&canonical)?,
            id16(&scope)?,
            arr32(&version)?,
        )
        .map_err(ApprovalError::from)?,
        argument_fingerprint: ArgumentFingerprint(arr32(&args)?),
        effect_class: EffectClass::from_code(effect).ok_or(StoreError::Corrupt)?,
        policy_generation: policy as u64,
        revocation_fence: RevocationFence::decode(&fence).map_err(ApprovalError::from)?,
        expires_at_unix_ms: expires.map(|v| v as u64),
        requested_at_unix_ms: requested as u64,
        attention_id: AttentionId::from_bytes(id16(&attention)?),
        control_mode: ControlMode::from_u8(mode).ok_or(StoreError::Corrupt)?,
    };
    Ok(Some(request))
}

fn load_decision(
    conn: &rusqlite::Connection,
    approval_id: ApprovalId,
) -> Result<Option<ApprovalDecision>, ApprovalStoreError> {
    let row = conn
        .query_row(
            "SELECT action_id, agent_run_id, decision, authority, decided_at_unix_ms,
                    decision_policy_generation, decision_principal_id, consumed
             FROM approval_decision WHERE approval_id = ?1",
            params![approval_id.to_bytes().as_slice()],
            |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, u8>(2)?,
                    row.get::<_, u8>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<Vec<u8>>>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            },
        )
        .optional()
        .map_err(|_| StoreError::Corrupt)?;
    let Some((action_id, run, verdict, authority, decided, policy, principal, consumed)) = row
    else {
        return Ok(None);
    };
    Ok(Some(ApprovalDecision {
        approval_id,
        action_id: ActionId::from_bytes(id16(&action_id)?),
        agent_run_id: AgentRunId::from_bytes(id16(&run)?),
        decision: ApprovalVerdict::from_u8(verdict).ok_or(StoreError::Corrupt)?,
        authority: DecisionAuthority::from_u8(authority).ok_or(StoreError::Corrupt)?,
        decided_at_unix_ms: decided as u64,
        decision_policy_generation: policy as u64,
        decision_principal_id: match principal {
            Some(bytes) => Some(ClientPrincipalId::from_bytes(id16(&bytes)?)),
            None => None,
        },
        consumed: consumed != 0,
    }))
}

fn resolve_attention(
    conn: &rusqlite::Connection,
    attention_id: AttentionId,
    now: u64,
) -> Result<(), ApprovalStoreError> {
    let state: u8 = conn
        .query_row(
            "SELECT state FROM attention_item WHERE attention_id = ?1",
            params![attention_id.to_bytes().as_slice()],
            |row| row.get(0),
        )
        .map_err(|_| StoreError::Corrupt)?;
    let from = AttentionState::from_u8(state).ok_or(StoreError::Corrupt)?;
    allowed_attention_transition(from, AttentionState::Resolved)
        .map_err(|_| ApprovalError::BindingMismatch)?;
    conn.execute(
        "UPDATE attention_item SET state = ?1, updated_at_unix_ms = ?2, resolved_at_unix_ms = ?3
         WHERE attention_id = ?4",
        params![
            AttentionState::Resolved.as_u8(),
            now as i64,
            now as i64,
            attention_id.to_bytes().as_slice()
        ],
    )
    .map_err(|_| StoreError::WriteFailed)?;
    Ok(())
}

fn id16(bytes: &[u8]) -> Result<[u8; 16], ApprovalStoreError> {
    bytes
        .try_into()
        .map_err(|_| ApprovalStoreError::Store(StoreError::Corrupt))
}

fn arr32(bytes: &[u8]) -> Result<[u8; 32], ApprovalStoreError> {
    bytes
        .try_into()
        .map_err(|_| ApprovalStoreError::Store(StoreError::Corrupt))
}
