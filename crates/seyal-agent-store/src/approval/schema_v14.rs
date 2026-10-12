//! ApprovalRequest / ApprovalDecision tables (SPEC-028 §5). Schema version 14.

pub(crate) const APPROVAL_TABLES_V14: &str = "
CREATE TABLE IF NOT EXISTS approval_request (
    approval_id BLOB PRIMARY KEY,
    action_id BLOB NOT NULL,
    action_intent_digest BLOB NOT NULL,
    agent_run_id BLOB NOT NULL,
    capability BLOB NOT NULL,
    resource_type BLOB NOT NULL,
    resource_canonical_id BLOB NOT NULL,
    resource_scope BLOB NOT NULL,
    resource_version_or_fingerprint BLOB NOT NULL,
    argument_fingerprint BLOB NOT NULL,
    effect_class INTEGER NOT NULL,
    policy_generation INTEGER NOT NULL,
    revocation_fence BLOB NOT NULL,
    expires_at_unix_ms INTEGER,
    requested_at_unix_ms INTEGER NOT NULL,
    attention_id BLOB NOT NULL,
    control_mode INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS approval_request_action
    ON approval_request(action_id);
CREATE TABLE IF NOT EXISTS approval_decision (
    approval_id BLOB PRIMARY KEY,
    action_id BLOB NOT NULL,
    agent_run_id BLOB NOT NULL,
    decision INTEGER NOT NULL,
    authority INTEGER NOT NULL,
    decided_at_unix_ms INTEGER NOT NULL,
    decision_policy_generation INTEGER NOT NULL,
    decision_principal_id BLOB,
    consumed INTEGER NOT NULL DEFAULT 0
);
";
