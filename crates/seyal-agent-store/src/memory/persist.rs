//! SQLite row helpers for MemoryStore.

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use seyal_agent_core::{
    ApplicabilityIdentity, AuthorityClass, CanonicalBytes, MemoryId, MemoryKind, MemoryRecord,
    MemoryState, PolicyGeneration, RecordGeneration, ScopeIdentity, SemanticIdentity, Sensitivity,
    SuppressionIdentity,
};

use crate::StoreError;

use super::codec::{
    decode_evidence, decode_fingerprints, decode_id_list, encode_evidence, encode_fingerprints,
    encode_id_list,
};
use super::ops::{MemoryError, ProposeResult};

pub(super) fn read_replay(
    tx: &Transaction<'_>,
    request_id: &[u8; 16],
    now: u64,
    digest: &[u8],
) -> Result<Option<ProposeResult>, MemoryError> {
    let row = tx
        .query_row(
            "SELECT result_code, result_memory_id, payload_digest, expires_at_unix_ms
             FROM memory_replay_receipt WHERE request_id = ?1",
            params![request_id.as_slice()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<Vec<u8>>>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|_| StoreError::Corrupt)?;
    let Some((code, mem_id, stored_digest, expires)) = row else {
        return Ok(None);
    };
    if now >= expires as u64 {
        return Err(MemoryError::ReplayExpired);
    }
    if stored_digest.as_slice() != digest {
        return Err(MemoryError::ReplayMismatch);
    }
    if code == 2 {
        return Ok(Some(ProposeResult::QualityRejected));
    }
    let id_bytes = mem_id.ok_or(MemoryError::ReceiptIntegrity)?;
    let mut id = [0u8; 16];
    id.copy_from_slice(&id_bytes);
    let record =
        load_record_conn(tx, MemoryId::from_bytes(id))?.ok_or(MemoryError::ReceiptIntegrity)?;
    Ok(Some(ProposeResult::Replay(record)))
}

pub(super) fn validate_policy_fence(
    tx: &Transaction<'_>,
    policy: &PolicyGeneration,
) -> Result<(), MemoryError> {
    for member in policy.members() {
        let current = current_revocation_gen(tx, member.scope)?;
        if current > member.revocation_generation.get() {
            return Err(MemoryError::StalePolicy);
        }
    }
    Ok(())
}

pub(super) fn current_revocation_gen(
    tx: &Transaction<'_>,
    scope: ScopeIdentity,
) -> Result<u64, MemoryError> {
    let current: i64 = tx
        .query_row(
            "SELECT generation FROM revocation_scope_generation
             WHERE scope_kind = ?1 AND scope_id = ?2",
            params![scope.kind.code() as i64, scope.id.as_slice()],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| StoreError::Corrupt)?
        .unwrap_or(1);
    Ok(current as u64)
}

pub(super) fn tombstone_exists(
    tx: &Transaction<'_>,
    s: &SuppressionIdentity,
) -> Result<bool, MemoryError> {
    Ok(tx
        .query_row(
            "SELECT 1 FROM memory_tombstone
             WHERE owning_scope_kind = ?1 AND owning_scope_id = ?2
               AND semantic_token = ?3 AND applicability_token = ?4",
            params![
                s.scope.kind.code() as i64,
                s.scope.id.as_slice(),
                s.semantic_token.as_slice(),
                s.applicability_token.as_slice(),
            ],
            |_| Ok(true),
        )
        .optional()
        .map_err(|_| StoreError::Corrupt)?
        .unwrap_or(false))
}

pub(super) fn scope_quota(
    tx: &Transaction<'_>,
    scope: ScopeIdentity,
) -> Result<(i64, i64, i64, i64), MemoryError> {
    Ok(tx
        .query_row(
            "SELECT proposed_count, durable_bytes, receipt_count, receipt_bytes FROM memory_scope_quota
             WHERE owning_scope_kind = ?1 AND owning_scope_id = ?2",
            params![scope.kind.code() as i64, scope.id.as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(|_| StoreError::Corrupt)?
        .unwrap_or((0, 0, 0, 0)))
}

pub(super) fn upsert_quota(
    tx: &Transaction<'_>,
    scope: ScopeIdentity,
    proposed: i64,
    durable: i64,
    receipts: i64,
    receipt_bytes: i64,
) -> Result<(), MemoryError> {
    tx.execute(
        "INSERT INTO memory_scope_quota
         (owning_scope_kind, owning_scope_id, proposed_count, durable_bytes, receipt_count, receipt_bytes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(owning_scope_kind, owning_scope_id) DO UPDATE SET
           proposed_count = excluded.proposed_count,
           durable_bytes = excluded.durable_bytes,
           receipt_count = excluded.receipt_count,
           receipt_bytes = excluded.receipt_bytes",
        params![
            scope.kind.code() as i64,
            scope.id.as_slice(),
            proposed,
            durable,
            receipts,
            receipt_bytes
        ],
    )
    .map_err(|_| StoreError::WriteFailed)?;
    Ok(())
}

pub(super) fn load_record_conn(
    conn: &Connection,
    id: MemoryId,
) -> Result<Option<MemoryRecord>, MemoryError> {
    let row = conn
        .query_row(
            "SELECT record_generation, schema_version, kind, payload, payload_schema_version,
                    semantic_key_version, semantic_key, applicability_schema_version, applicability,
                    evidence_refs, authority_class, sensitivity, state, created_at_unix_ms,
                    accepted_at_unix_ms, last_validated_at_unix_ms, revalidate_after_unix_ms,
                    expires_at_unix_ms, supersedes, superseded_by, conflicts_with, source_fingerprints,
                    policy_generation, revocation_generation, quarantine_reason
             FROM memory_record WHERE id = ?1",
            params![id.to_bytes().as_slice()],
            |row| {
                Ok(LoadedRow {
                    record_generation: row.get(0)?,
                    schema_version: row.get(1)?,
                    kind: row.get(2)?,
                    payload: row.get(3)?,
                    payload_schema_version: row.get(4)?,
                    semantic_key_version: row.get(5)?,
                    semantic_key: row.get(6)?,
                    applicability_schema_version: row.get(7)?,
                    applicability: row.get(8)?,
                    evidence_refs: row.get(9)?,
                    authority_class: row.get(10)?,
                    sensitivity: row.get(11)?,
                    state: row.get(12)?,
                    created_at_unix_ms: row.get(13)?,
                    accepted_at_unix_ms: row.get(14)?,
                    last_validated_at_unix_ms: row.get(15)?,
                    revalidate_after_unix_ms: row.get(16)?,
                    expires_at_unix_ms: row.get(17)?,
                    supersedes: row.get(18)?,
                    superseded_by: row.get(19)?,
                    conflicts_with: row.get(20)?,
                    source_fingerprints: row.get(21)?,
                    policy_generation: row.get(22)?,
                    revocation_generation: row.get(23)?,
                    quarantine_reason: row.get(24)?,
                })
            },
        )
        .optional()
        .map_err(|_| StoreError::Corrupt)?;
    let Some(r) = row else {
        return Ok(None);
    };
    let policy = PolicyGeneration::decode(&r.policy_generation).map_err(|_| StoreError::Corrupt)?;
    let kind = MemoryKind::from_code(r.kind as u8).ok_or(StoreError::Corrupt)?;
    let authority =
        AuthorityClass::from_code(r.authority_class as u8).ok_or(StoreError::Corrupt)?;
    let sensitivity = Sensitivity::from_code(r.sensitivity as u8).ok_or(StoreError::Corrupt)?;
    let state = MemoryState::from_code(r.state as u8).ok_or(StoreError::Corrupt)?;
    Ok(Some(MemoryRecord {
        id,
        record_generation: RecordGeneration::from_raw(r.record_generation as u64)
            .ok_or(StoreError::Corrupt)?,
        schema_version: r.schema_version as u16,
        kind,
        payload: r.payload,
        payload_schema_version: r.payload_schema_version as u16,
        semantic: SemanticIdentity {
            version: r.semantic_key_version as u16,
            canonical: CanonicalBytes(r.semantic_key),
        },
        applicability: ApplicabilityIdentity {
            schema_version: r.applicability_schema_version as u16,
            canonical: CanonicalBytes(r.applicability),
        },
        evidence_refs: decode_evidence(&r.evidence_refs).map_err(|_| StoreError::Corrupt)?,
        authority_class: authority,
        sensitivity,
        state,
        created_at_unix_ms: r.created_at_unix_ms as u64,
        accepted_at_unix_ms: r.accepted_at_unix_ms.map(|v| v as u64),
        last_validated_at_unix_ms: r.last_validated_at_unix_ms.map(|v| v as u64),
        revalidate_after_unix_ms: r.revalidate_after_unix_ms.map(|v| v as u64),
        expires_at_unix_ms: r.expires_at_unix_ms.map(|v| v as u64),
        supersedes: decode_id_list(&r.supersedes).map_err(|_| StoreError::Corrupt)?,
        superseded_by: decode_id_list(&r.superseded_by).map_err(|_| StoreError::Corrupt)?,
        conflicts_with: decode_id_list(&r.conflicts_with).map_err(|_| StoreError::Corrupt)?,
        source_fingerprints: decode_fingerprints(&r.source_fingerprints)
            .map_err(|_| StoreError::Corrupt)?,
        policy_generation: policy,
        revocation_generation: r.revocation_generation.map(|v| v as u64),
        quarantine_reason: None,
    }))
}

struct LoadedRow {
    record_generation: i64,
    schema_version: i64,
    kind: i64,
    payload: Vec<u8>,
    payload_schema_version: i64,
    semantic_key_version: i64,
    semantic_key: Vec<u8>,
    applicability_schema_version: i64,
    applicability: Vec<u8>,
    evidence_refs: Vec<u8>,
    authority_class: i64,
    sensitivity: i64,
    state: i64,
    created_at_unix_ms: i64,
    accepted_at_unix_ms: Option<i64>,
    last_validated_at_unix_ms: Option<i64>,
    revalidate_after_unix_ms: Option<i64>,
    expires_at_unix_ms: Option<i64>,
    supersedes: Vec<u8>,
    superseded_by: Vec<u8>,
    conflicts_with: Vec<u8>,
    source_fingerprints: Vec<u8>,
    policy_generation: Vec<u8>,
    revocation_generation: Option<i64>,
    #[allow(dead_code)]
    quarantine_reason: Option<String>,
}

pub(super) fn insert_record(
    tx: &Transaction<'_>,
    record: &MemoryRecord,
) -> Result<(), MemoryError> {
    let scope = record.policy_generation.owning_scope();
    tx.execute(
        "INSERT INTO memory_record (
            id, record_generation, schema_version, kind, payload, payload_schema_version,
            semantic_key_version, semantic_key, applicability_schema_version, applicability,
            evidence_refs, authority_class, sensitivity, state, created_at_unix_ms,
            accepted_at_unix_ms, last_validated_at_unix_ms, revalidate_after_unix_ms,
            expires_at_unix_ms, supersedes, superseded_by, conflicts_with, source_fingerprints,
            policy_generation, revocation_generation, quarantine_reason,
            owning_scope_kind, owning_scope_id, encoded_bytes
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29)",
        params![
            record.id.to_bytes().as_slice(),
            record.record_generation.get() as i64,
            record.schema_version as i64,
            record.kind.code() as i64,
            record.payload,
            record.payload_schema_version as i64,
            record.semantic.version as i64,
            record.semantic.canonical.as_slice(),
            record.applicability.schema_version as i64,
            record.applicability.canonical.as_slice(),
            encode_evidence(&record.evidence_refs),
            record.authority_class.code() as i64,
            record.sensitivity.code() as i64,
            record.state.code() as i64,
            record.created_at_unix_ms as i64,
            record.accepted_at_unix_ms.map(|v| v as i64),
            record.last_validated_at_unix_ms.map(|v| v as i64),
            record.revalidate_after_unix_ms.map(|v| v as i64),
            record.expires_at_unix_ms.map(|v| v as i64),
            encode_id_list(&record.supersedes),
            encode_id_list(&record.superseded_by),
            encode_id_list(&record.conflicts_with),
            encode_fingerprints(&record.source_fingerprints),
            record.policy_generation.encode(),
            record.revocation_generation.map(|v| v as i64),
            record.quarantine_reason,
            scope.kind.code() as i64,
            scope.id.as_slice(),
            record.encoded_total_bytes() as i64,
        ],
    )
    .map_err(|_| StoreError::WriteFailed)?;
    Ok(())
}

pub(super) fn update_record(
    tx: &Transaction<'_>,
    record: &MemoryRecord,
) -> Result<(), MemoryError> {
    tx.execute(
        "UPDATE memory_record SET
            record_generation = ?2, state = ?3, accepted_at_unix_ms = ?4,
            last_validated_at_unix_ms = ?5, superseded_by = ?6, policy_generation = ?7,
            revocation_generation = ?8, payload = ?9, evidence_refs = ?10,
            source_fingerprints = ?11, encoded_bytes = ?12
         WHERE id = ?1",
        params![
            record.id.to_bytes().as_slice(),
            record.record_generation.get() as i64,
            record.state.code() as i64,
            record.accepted_at_unix_ms.map(|v| v as i64),
            record.last_validated_at_unix_ms.map(|v| v as i64),
            encode_id_list(&record.superseded_by),
            record.policy_generation.encode(),
            record.revocation_generation.map(|v| v as i64),
            record.payload,
            encode_evidence(&record.evidence_refs),
            encode_fingerprints(&record.source_fingerprints),
            record.encoded_total_bytes() as i64,
        ],
    )
    .map_err(|_| StoreError::WriteFailed)?;
    Ok(())
}
