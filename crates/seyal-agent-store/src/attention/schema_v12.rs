//! Attention / Artifact tables (SPEC-028). Schema version 12 (v11 is ActionIntent).

pub(crate) const ATTENTION_TABLES_V12: &str = "
CREATE TABLE IF NOT EXISTS attention_item (
    attention_id BLOB PRIMARY KEY,
    work_item_id BLOB,
    attempt_id BLOB,
    agent_run_id BLOB,
    action_id BLOB,
    approval_id BLOB,
    kind INTEGER NOT NULL,
    state INTEGER NOT NULL,
    priority INTEGER NOT NULL,
    summary TEXT NOT NULL,
    requires_spatial_focus INTEGER NOT NULL,
    resource_address BLOB,
    coalesce_key BLOB,
    created_at_unix_ms INTEGER NOT NULL,
    updated_at_unix_ms INTEGER NOT NULL,
    resolved_at_unix_ms INTEGER,
    expires_at_unix_ms INTEGER
);
CREATE INDEX IF NOT EXISTS attention_item_run_state
    ON attention_item(agent_run_id, state);
CREATE INDEX IF NOT EXISTS attention_item_coalesce
    ON attention_item(coalesce_key);

CREATE TABLE IF NOT EXISTS attention_artifact_link (
    attention_id BLOB NOT NULL,
    artifact_id BLOB NOT NULL,
    PRIMARY KEY (attention_id, artifact_id)
);

CREATE TABLE IF NOT EXISTS artifact_ref (
    artifact_id BLOB PRIMARY KEY,
    producer_agent_run_id BLOB,
    producer_attempt_id BLOB,
    kind INTEGER NOT NULL,
    content_address_or_version BLOB NOT NULL,
    sensitivity_class INTEGER NOT NULL,
    created_at_unix_ms INTEGER NOT NULL
);
";
