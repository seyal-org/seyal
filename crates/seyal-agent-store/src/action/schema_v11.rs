//! ActionIntent table (SPEC-016 §3). Schema version 11.

pub(crate) const ACTION_INTENT_TABLES_V11: &str = "
CREATE TABLE IF NOT EXISTS action_intent (
    action_id BLOB PRIMARY KEY,
    agent_run_id BLOB NOT NULL,
    digest BLOB NOT NULL,
    lifecycle INTEGER NOT NULL,
    canonical_intent BLOB NOT NULL,
    created_at INTEGER NOT NULL
);
";
