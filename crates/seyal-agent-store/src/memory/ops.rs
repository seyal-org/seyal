//! MemoryStore propose / transition / eligibility operations.

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, OptionalExtension, Transaction};
use seyal_agent_core::{
    allowed_transition, is_safety_maintenance, ApplicabilityIdentity, AuthorityClass,
    CallerScopeContext, CanonicalBytes, Eligibility, EvidenceRef, MemoryId, MemoryKind,
    MemoryRecord, MemoryState, PolicyGeneration, RecordGeneration, ScopeIdentity, SemanticIdentity,
    Sensitivity, SuppressionIdentity, TransitionReason, MAX_DURABLE_BYTES_PER_SCOPE,
    MAX_PROPOSED_PER_SCOPE, MAX_PROVENANCE_REFS, MAX_RECORD_BYTES, MAX_REPLAY_RECEIPTS_PER_SCOPE,
    MAX_REPLAY_RECEIPT_BYTES_PER_SCOPE, MEMORY_SCHEMA_VERSION, PAYLOAD_SCHEMA_STATEMENT_V1,
    RESERVED_SAFETY_CONTROL_BYTES, SEMANTIC_KEY_PROFILE_V1,
};

use crate::sqlite::AgentStore;
use crate::StoreError;

use super::codec::opaque_token;
use super::persist::{
    current_revocation_gen, insert_record, load_record_conn, read_replay, scope_quota,
    tombstone_exists, update_record, upsert_quota, validate_policy_fence,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryError {
    Store(StoreError),
    ModeForbidden,
    ScopeNotAuthorized,
    UnsupportedTransition,
    StaleGeneration,
    StalePolicy,
    CapExceeded,
    Suppressed,
    NotFound,
    ReplayMismatch,
    ReplayExpired,
    ReceiptIntegrity,
    InvalidInput,
}

impl From<StoreError> for MemoryError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

pub struct MemoryAuthority<'a> {
    pub(super) store: &'a AgentStore,
}

impl<'a> MemoryAuthority<'a> {
    pub(super) fn new(store: &'a AgentStore) -> Self {
        Self { store }
    }

    pub(super) fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    fn suppression_key_tx(tx: &Transaction<'_>) -> Result<[u8; 32], MemoryError> {
        let key: Vec<u8> = tx
            .query_row(
                "SELECT suppression_key FROM memory_store_meta WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|_| StoreError::Corrupt)?;
        if key.len() != 32 {
            return Err(MemoryError::Store(StoreError::Corrupt));
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&key);
        Ok(out)
    }

    pub(super) fn derive_suppression_tx(
        tx: &Transaction<'_>,
        scope: ScopeIdentity,
        semantic: &SemanticIdentity,
        applicability: &ApplicabilityIdentity,
    ) -> Result<SuppressionIdentity, MemoryError> {
        let key = Self::suppression_key_tx(tx)?;
        Ok(SuppressionIdentity {
            scope,
            semantic_token: opaque_token(&key, scope.kind.code(), &scope.id, &semantic.canonical.0),
            applicability_token: opaque_token(
                &key,
                scope.kind.code(),
                &scope.id,
                &applicability.canonical.0,
            ),
        })
    }

    pub fn propose(&self, input: ProposeInput) -> Result<ProposeResult, MemoryError> {
        let now = Self::now_ms();
        if now >= input.request_expires_at_unix_ms {
            return Err(MemoryError::ReplayExpired);
        }
        input
            .caller
            .authorize_owning_scope(input.owning_scope)
            .map_err(|_| MemoryError::ScopeNotAuthorized)?;
        let mode = input.policy_generation.effective_mode();
        if !mode.allows_ordinary_write() {
            return Err(MemoryError::ModeForbidden);
        }
        if input.evidence_refs.len() > MAX_PROVENANCE_REFS {
            return Err(MemoryError::CapExceeded);
        }

        let statement_nfc: String = {
            use unicode_normalization::UnicodeNormalization;
            input.statement.nfc().collect()
        };
        let semantic = SemanticIdentity::freeform_v1(&statement_nfc);
        let payload = semantic.canonical.0.clone();
        let payload_digest = blake3::hash(
            &[
                input.request_id.as_slice(),
                payload.as_slice(),
                &input.policy_generation.encode(),
            ]
            .concat(),
        );

        let conn = self
            .store
            .conn
            .lock()
            .map_err(|_| StoreError::WriteFailed)?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;

        if let Some(replay) = read_replay(&tx, &input.request_id, now, payload_digest.as_bytes())? {
            return Ok(replay);
        }

        validate_policy_fence(&tx, &input.policy_generation)?;

        let suppression =
            Self::derive_suppression_tx(&tx, input.owning_scope, &semantic, &input.applicability)?;
        let suppressed = tombstone_exists(&tx, &suppression)?;
        if suppressed && !input.independent_post_revocation {
            return Err(MemoryError::Suppressed);
        }

        let (proposed_count, durable_bytes, receipt_count, receipt_bytes) =
            scope_quota(&tx, input.owning_scope)?;
        if proposed_count as usize >= MAX_PROPOSED_PER_SCOPE
            || receipt_count as usize >= MAX_REPLAY_RECEIPTS_PER_SCOPE
            || receipt_bytes as u64 >= MAX_REPLAY_RECEIPT_BYTES_PER_SCOPE
        {
            return Err(MemoryError::CapExceeded);
        }

        let record = MemoryRecord {
            id: MemoryId::new(),
            record_generation: RecordGeneration::FIRST,
            schema_version: MEMORY_SCHEMA_VERSION,
            kind: input.kind,
            payload: payload.clone(),
            payload_schema_version: PAYLOAD_SCHEMA_STATEMENT_V1,
            semantic: SemanticIdentity {
                version: SEMANTIC_KEY_PROFILE_V1,
                canonical: CanonicalBytes(payload),
            },
            applicability: input.applicability.clone(),
            evidence_refs: input.evidence_refs.clone(),
            authority_class: input.authority_class,
            sensitivity: input.sensitivity,
            state: MemoryState::Proposed,
            created_at_unix_ms: now,
            accepted_at_unix_ms: None,
            last_validated_at_unix_ms: None,
            revalidate_after_unix_ms: input.revalidate_after_unix_ms,
            expires_at_unix_ms: input.expires_at_unix_ms,
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            conflicts_with: Vec::new(),
            source_fingerprints: input.source_fingerprints.clone(),
            policy_generation: input.policy_generation.clone(),
            revocation_generation: None,
            quarantine_reason: None,
        };
        let encoded = record.encoded_total_bytes();
        if encoded > MAX_RECORD_BYTES
            || encoded > MAX_RECORD_BYTES.saturating_sub(0)
            || durable_bytes as u64 + encoded as u64 > MAX_DURABLE_BYTES_PER_SCOPE
        {
            return Err(MemoryError::CapExceeded);
        }
        let _ = RESERVED_SAFETY_CONTROL_BYTES;

        insert_record(&tx, &record)?;
        upsert_quota(
            &tx,
            input.owning_scope,
            proposed_count + 1,
            durable_bytes + encoded as i64,
            receipt_count + 1,
            receipt_bytes + 64,
        )?;
        tx.execute(
            "INSERT INTO memory_replay_receipt
             (request_id, payload_digest, result_code, result_memory_id, result_generation,
              expires_at_unix_ms, owning_scope_kind, owning_scope_id, encoded_bytes)
             VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, ?7, 64)",
            params![
                input.request_id.as_slice(),
                payload_digest.as_bytes().as_slice(),
                record.id.to_bytes().as_slice(),
                record.record_generation.get() as i64,
                input.request_expires_at_unix_ms as i64,
                input.owning_scope.kind.code() as i64,
                input.owning_scope.id.as_slice(),
            ],
        )
        .map_err(|_| StoreError::WriteFailed)?;
        tx.commit().map_err(|_| StoreError::WriteFailed)?;
        Ok(ProposeResult::Created(record))
    }

    pub fn quality_reject(&self, id: MemoryId) -> Result<(), MemoryError> {
        let conn = self
            .store
            .conn
            .lock()
            .map_err(|_| StoreError::WriteFailed)?;
        let state: i64 = conn
            .query_row(
                "SELECT state FROM memory_record WHERE id = ?1",
                params![id.to_bytes().as_slice()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| StoreError::Corrupt)?
            .ok_or(MemoryError::NotFound)?;
        if state != MemoryState::Proposed.code() as i64 {
            return Err(MemoryError::UnsupportedTransition);
        }
        Ok(())
    }

    pub fn transition(
        &self,
        id: MemoryId,
        expected_generation: RecordGeneration,
        to: MemoryState,
        reason: TransitionReason,
        current_policy: &PolicyGeneration,
        successor: Option<MemoryId>,
    ) -> Result<MemoryRecord, MemoryError> {
        let now = Self::now_ms();
        let conn = self
            .store
            .conn
            .lock()
            .map_err(|_| StoreError::WriteFailed)?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;
        let mut record = load_record_conn(&tx, id)?.ok_or(MemoryError::NotFound)?;
        if record.record_generation != expected_generation {
            return Err(MemoryError::StaleGeneration);
        }
        allowed_transition(record.state, to, reason)
            .map_err(|_| MemoryError::UnsupportedTransition)?;

        let mode = current_policy.effective_mode();
        let safety = is_safety_maintenance(to, reason);
        if matches!(to, MemoryState::Accepted | MemoryState::Superseded)
            && !mode.allows_ordinary_write()
        {
            return Err(MemoryError::ModeForbidden);
        }
        if !safety && !record.policy_generation.matches(current_policy) {
            return Err(MemoryError::StalePolicy);
        }
        validate_policy_fence(&tx, current_policy)?;

        let next_gen = record
            .record_generation
            .next()
            .ok_or(MemoryError::InvalidInput)?;
        record.record_generation = next_gen;
        record.state = to;
        if !safety {
            record.policy_generation = current_policy.clone();
        }
        if to == MemoryState::Accepted {
            record.accepted_at_unix_ms = Some(now);
            record.last_validated_at_unix_ms = Some(now);
        }
        if to == MemoryState::Superseded
            && let Some(succ) = successor
        {
            record.superseded_by.push(succ);
        }
        if to == MemoryState::Revoked {
            let scope = record.policy_generation.owning_scope();
            let suppression =
                Self::derive_suppression_tx(&tx, scope, &record.semantic, &record.applicability)?;
            let rev_gen = current_revocation_gen(&tx, scope)?;
            record.revocation_generation = Some(rev_gen);
            tx.execute(
                "INSERT OR REPLACE INTO memory_tombstone
                 (semantic_token, applicability_token, owning_scope_kind, owning_scope_id,
                  revoked_memory_id, revoked_kind, revocation_generation, revoked_at_unix_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    suppression.semantic_token.as_slice(),
                    suppression.applicability_token.as_slice(),
                    scope.kind.code() as i64,
                    scope.id.as_slice(),
                    record.id.to_bytes().as_slice(),
                    record.kind.code() as i64,
                    rev_gen as i64,
                    now as i64,
                ],
            )
            .map_err(|_| StoreError::WriteFailed)?;
            record.payload.clear();
            record.evidence_refs.clear();
            record.source_fingerprints.clear();
        }
        update_record(&tx, &record)?;
        tx.commit().map_err(|_| StoreError::WriteFailed)?;
        Ok(record)
    }

    pub fn get(&self, id: MemoryId) -> Result<Option<MemoryRecord>, MemoryError> {
        let conn = self
            .store
            .conn
            .lock()
            .map_err(|_| StoreError::WriteFailed)?;
        load_record_conn(&conn, id)
    }

    pub fn use_time_eligibility(
        &self,
        id: MemoryId,
        current_policy: &PolicyGeneration,
    ) -> Result<Eligibility, MemoryError> {
        let record = self.get(id)?.ok_or(MemoryError::NotFound)?;
        let mode = current_policy.effective_mode();
        Ok(
            record.use_time_eligibility(
                Self::now_ms(),
                mode.allows_ordinary_read(),
                current_policy,
            ),
        )
    }

    pub fn is_suppressed(
        &self,
        scope: ScopeIdentity,
        semantic: &SemanticIdentity,
        applicability: &ApplicabilityIdentity,
    ) -> Result<bool, MemoryError> {
        let conn = self
            .store
            .conn
            .lock()
            .map_err(|_| StoreError::WriteFailed)?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;
        let suppression = Self::derive_suppression_tx(&tx, scope, semantic, applicability)?;
        tombstone_exists(&tx, &suppression)
    }

    pub fn advance_revocation(
        &self,
        scopes: &[ScopeIdentity],
        subject: Option<MemoryId>,
        reason: TransitionReason,
    ) -> Result<(RevocationBundle, Option<MemoryRecord>), MemoryError> {
        use seyal_agent_core::{
            forgetting_transition, ForgettingState, RevocationEventId, RevocationFence,
            RevocationFenceMember, RevocationGeneration,
        };

        if !reason.is_forget_or_privacy() {
            return Err(MemoryError::UnsupportedTransition);
        }
        let now = Self::now_ms();
        let conn = self
            .store
            .conn
            .lock()
            .map_err(|_| StoreError::WriteFailed)?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;

        let mut members = Vec::new();
        for scope in scopes {
            let current = current_revocation_gen(&tx, *scope)?;
            let next =
                RevocationGeneration::from_raw(current + 1).ok_or(MemoryError::InvalidInput)?;
            tx.execute(
                "INSERT INTO revocation_scope_generation (scope_kind, scope_id, generation)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(scope_kind, scope_id) DO UPDATE SET generation = excluded.generation",
                params![
                    scope.kind.code() as i64,
                    scope.id.as_slice(),
                    next.get() as i64
                ],
            )
            .map_err(|_| StoreError::WriteFailed)?;
            members.push(RevocationFenceMember {
                scope: *scope,
                generation: next,
            });
            // Invalidate derived caches for this scope.
            tx.execute(
                "INSERT OR REPLACE INTO derived_cache_invalidation (cache_key, fence, invalidated_at_unix_ms)
                 VALUES (?1, ?2, ?3)",
                params![
                    scope.id.as_slice(),
                    next.get().to_le_bytes().as_slice(),
                    now as i64
                ],
            )
            .map_err(|_| StoreError::WriteFailed)?;
        }
        let fence = RevocationFence::new(members).map_err(|_| MemoryError::InvalidInput)?;
        let event_id = RevocationEventId::new();
        let mut state = ForgettingState::RevocationCommitted;
        forgetting_transition(ForgettingState::RevocationRequested, state)
            .map_err(|_| MemoryError::UnsupportedTransition)?;
        state = ForgettingState::CleanupPending;
        forgetting_transition(ForgettingState::RevocationCommitted, state)
            .map_err(|_| MemoryError::UnsupportedTransition)?;

        tx.execute(
            "INSERT INTO revocation_request
             (event_id, subject_memory_id, state, fence, provider_deletion, attempts,
              created_at_unix_ms, updated_at_unix_ms)
             VALUES (?1, ?2, ?3, ?4, 0, 0, ?5, ?5)",
            params![
                event_id.to_bytes().as_slice(),
                subject.map(|s| s.to_bytes().to_vec()),
                state.code() as i64,
                fence.encode(),
                now as i64,
            ],
        )
        .map_err(|_| StoreError::WriteFailed)?;

        let mut revoked = None;
        if let Some(id) = subject {
            let record = load_record_conn(&tx, id)?.ok_or(MemoryError::NotFound)?;
            // Build a policy that matches the record but with advanced revocation gens.
            let policy = record.policy_generation.clone();
            // Use transition path inline for revoke.
            drop(policy);
            let mut record = record;
            let expected = record.record_generation;
            allowed_transition(record.state, MemoryState::Revoked, reason)
                .map_err(|_| MemoryError::UnsupportedTransition)?;
            let next_gen = expected.next().ok_or(MemoryError::InvalidInput)?;
            record.record_generation = next_gen;
            record.state = MemoryState::Revoked;
            let scope = record.policy_generation.owning_scope();
            let suppression =
                Self::derive_suppression_tx(&tx, scope, &record.semantic, &record.applicability)?;
            let rev_gen = current_revocation_gen(&tx, scope)?;
            record.revocation_generation = Some(rev_gen);
            tx.execute(
                "INSERT OR REPLACE INTO memory_tombstone
                 (semantic_token, applicability_token, owning_scope_kind, owning_scope_id,
                  revoked_memory_id, revoked_kind, revocation_generation, revoked_at_unix_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    suppression.semantic_token.as_slice(),
                    suppression.applicability_token.as_slice(),
                    scope.kind.code() as i64,
                    scope.id.as_slice(),
                    record.id.to_bytes().as_slice(),
                    record.kind.code() as i64,
                    rev_gen as i64,
                    now as i64,
                ],
            )
            .map_err(|_| StoreError::WriteFailed)?;
            record.payload.clear();
            record.evidence_refs.clear();
            record.source_fingerprints.clear();
            update_record(&tx, &record)?;
            revoked = Some(record);
        }

        tx.commit().map_err(|_| StoreError::WriteFailed)?;
        Ok((
            RevocationBundle {
                event_id,
                fence,
                state,
                provider_deletion: seyal_agent_core::ProviderDeletionTruth::NotAttempted,
            },
            revoked,
        ))
    }

    pub fn complete_local_forgetting(
        &self,
        event_id: seyal_agent_core::RevocationEventId,
    ) -> Result<seyal_agent_core::ForgettingState, MemoryError> {
        use seyal_agent_core::{forgetting_transition, ForgettingState};
        let now = Self::now_ms();
        let conn = self
            .store
            .conn
            .lock()
            .map_err(|_| StoreError::WriteFailed)?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;
        let state_code: i64 = tx
            .query_row(
                "SELECT state FROM revocation_request WHERE event_id = ?1",
                params![event_id.to_bytes().as_slice()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| StoreError::Corrupt)?
            .ok_or(MemoryError::NotFound)?;
        let from = ForgettingState::from_code(state_code as u8).ok_or(MemoryError::InvalidInput)?;
        let to = ForgettingState::LocalForgotten;
        forgetting_transition(from, to).map_err(|_| MemoryError::UnsupportedTransition)?;
        tx.execute(
            "UPDATE revocation_request SET state = ?1, updated_at_unix_ms = ?2 WHERE event_id = ?3",
            params![to.code() as i64, now as i64, event_id.to_bytes().as_slice()],
        )
        .map_err(|_| StoreError::WriteFailed)?;
        tx.commit().map_err(|_| StoreError::WriteFailed)?;
        Ok(to)
    }

    pub fn cache_eligible(&self, cache_key: &[u8], fence: &[u8]) -> Result<bool, MemoryError> {
        let conn = self
            .store
            .conn
            .lock()
            .map_err(|_| StoreError::WriteFailed)?;
        let invalidated: Option<Vec<u8>> = conn
            .query_row(
                "SELECT fence FROM derived_cache_invalidation WHERE cache_key = ?1",
                params![cache_key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| StoreError::Corrupt)?;
        match invalidated {
            None => Ok(true),
            Some(current) => Ok(current == fence),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProposeInput {
    pub request_id: [u8; 16],
    pub request_expires_at_unix_ms: u64,
    pub kind: MemoryKind,
    pub statement: String,
    pub applicability: ApplicabilityIdentity,
    pub evidence_refs: Vec<EvidenceRef>,
    pub authority_class: AuthorityClass,
    pub sensitivity: Sensitivity,
    pub source_fingerprints: Vec<Vec<u8>>,
    pub policy_generation: PolicyGeneration,
    pub caller: CallerScopeContext,
    pub owning_scope: ScopeIdentity,
    pub revalidate_after_unix_ms: Option<u64>,
    pub expires_at_unix_ms: Option<u64>,
    pub independent_post_revocation: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProposeResult {
    Created(MemoryRecord),
    Replay(MemoryRecord),
    QualityRejected,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevocationBundle {
    pub event_id: seyal_agent_core::RevocationEventId,
    pub fence: seyal_agent_core::RevocationFence,
    pub state: seyal_agent_core::ForgettingState,
    pub provider_deletion: seyal_agent_core::ProviderDeletionTruth,
}
