//! Schema v10 — MemoryStore / RunWorkingSet / revocation tables (ADR-013 / SPEC-012/014/015).

pub(crate) const MEMORY_TABLES_V10: &str = "
CREATE TABLE IF NOT EXISTS memory_store_meta (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    suppression_key BLOB NOT NULL,
    aggregate_working_set_bytes INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS memory_record (
    id BLOB PRIMARY KEY,
    record_generation INTEGER NOT NULL,
    schema_version INTEGER NOT NULL,
    kind INTEGER NOT NULL,
    payload BLOB NOT NULL,
    payload_schema_version INTEGER NOT NULL,
    semantic_key_version INTEGER NOT NULL,
    semantic_key BLOB NOT NULL,
    applicability_schema_version INTEGER NOT NULL,
    applicability BLOB NOT NULL,
    evidence_refs BLOB NOT NULL,
    authority_class INTEGER NOT NULL,
    sensitivity INTEGER NOT NULL,
    state INTEGER NOT NULL,
    created_at_unix_ms INTEGER NOT NULL,
    accepted_at_unix_ms INTEGER,
    last_validated_at_unix_ms INTEGER,
    revalidate_after_unix_ms INTEGER,
    expires_at_unix_ms INTEGER,
    supersedes BLOB NOT NULL,
    superseded_by BLOB NOT NULL,
    conflicts_with BLOB NOT NULL,
    source_fingerprints BLOB NOT NULL,
    policy_generation BLOB NOT NULL,
    revocation_generation INTEGER,
    quarantine_reason TEXT,
    owning_scope_kind INTEGER NOT NULL,
    owning_scope_id BLOB NOT NULL,
    encoded_bytes INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS memory_record_semantic_slot
    ON memory_record(
        owning_scope_kind, owning_scope_id,
        semantic_key_version, semantic_key,
        applicability_schema_version, applicability
    )
    WHERE state IN (1, 2);
CREATE TABLE IF NOT EXISTS memory_tombstone (
    semantic_token BLOB NOT NULL,
    applicability_token BLOB NOT NULL,
    owning_scope_kind INTEGER NOT NULL,
    owning_scope_id BLOB NOT NULL,
    revoked_memory_id BLOB NOT NULL,
    revoked_kind INTEGER NOT NULL,
    revocation_generation INTEGER NOT NULL,
    revoked_at_unix_ms INTEGER NOT NULL,
    PRIMARY KEY (owning_scope_kind, owning_scope_id, semantic_token, applicability_token)
);
CREATE TABLE IF NOT EXISTS memory_replay_receipt (
    request_id BLOB PRIMARY KEY,
    payload_digest BLOB NOT NULL,
    result_code INTEGER NOT NULL,
    result_memory_id BLOB,
    result_generation INTEGER,
    expires_at_unix_ms INTEGER NOT NULL,
    owning_scope_kind INTEGER NOT NULL,
    owning_scope_id BLOB NOT NULL,
    encoded_bytes INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS memory_scope_quota (
    owning_scope_kind INTEGER NOT NULL,
    owning_scope_id BLOB NOT NULL,
    proposed_count INTEGER NOT NULL DEFAULT 0,
    durable_bytes INTEGER NOT NULL DEFAULT 0,
    receipt_count INTEGER NOT NULL DEFAULT 0,
    receipt_bytes INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (owning_scope_kind, owning_scope_id)
);
CREATE TABLE IF NOT EXISTS revocation_scope_generation (
    scope_kind INTEGER NOT NULL,
    scope_id BLOB NOT NULL,
    generation INTEGER NOT NULL,
    PRIMARY KEY (scope_kind, scope_id)
);
CREATE TABLE IF NOT EXISTS revocation_request (
    event_id BLOB PRIMARY KEY,
    subject_memory_id BLOB,
    state INTEGER NOT NULL,
    fence BLOB NOT NULL,
    provider_deletion INTEGER NOT NULL DEFAULT 0,
    attempts INTEGER NOT NULL DEFAULT 0,
    created_at_unix_ms INTEGER NOT NULL,
    updated_at_unix_ms INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS run_working_set (
    id BLOB PRIMARY KEY,
    work_item_id BLOB NOT NULL,
    attempt_id BLOB NOT NULL,
    agent_run_id BLOB NOT NULL,
    working_set_generation INTEGER NOT NULL,
    policy_fence BLOB NOT NULL,
    builder_version INTEGER NOT NULL,
    created_at_unix_ms INTEGER NOT NULL,
    last_compacted_at_unix_ms INTEGER,
    provider_continuation_ref BLOB,
    degraded INTEGER NOT NULL DEFAULT 0,
    encoded_bytes INTEGER NOT NULL,
    current INTEGER NOT NULL DEFAULT 1
);
CREATE UNIQUE INDEX IF NOT EXISTS run_working_set_current_run
    ON run_working_set(agent_run_id) WHERE current = 1;
CREATE TABLE IF NOT EXISTS run_working_set_entry (
    working_set_id BLOB NOT NULL,
    entry_id BLOB NOT NULL,
    class INTEGER NOT NULL,
    availability INTEGER NOT NULL,
    sensitivity INTEGER NOT NULL,
    payload BLOB,
    dependency_ref BLOB NOT NULL,
    source_generation INTEGER NOT NULL,
    reconstructable INTEGER NOT NULL,
    PRIMARY KEY (working_set_id, entry_id)
);
CREATE TABLE IF NOT EXISTS derived_cache_invalidation (
    cache_key BLOB PRIMARY KEY,
    fence BLOB NOT NULL,
    invalidated_at_unix_ms INTEGER NOT NULL
);
";
