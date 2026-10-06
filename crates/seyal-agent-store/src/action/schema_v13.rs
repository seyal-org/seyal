//! Action recovery columns (SPEC-016 EffectUnknown). Schema version 13.
//!
//! Attention tables occupy schema v12 on master (#1313). Recovery columns are
//! additive on `action_intent` plus history.

pub(crate) const ACTION_RECOVERY_V13: &str = "
ALTER TABLE action_intent ADD COLUMN dispatch_generation INTEGER;
ALTER TABLE action_intent ADD COLUMN authorization_invalidated INTEGER NOT NULL DEFAULT 0;
ALTER TABLE action_intent ADD COLUMN cancel_requested INTEGER NOT NULL DEFAULT 0;
ALTER TABLE action_intent ADD COLUMN reconciliation_attempts INTEGER NOT NULL DEFAULT 0;
ALTER TABLE action_intent ADD COLUMN last_evidence BLOB;
CREATE TABLE IF NOT EXISTS action_recovery_history (
    action_id BLOB NOT NULL,
    seq INTEGER NOT NULL,
    from_lifecycle INTEGER NOT NULL,
    to_lifecycle INTEGER NOT NULL,
    evidence_kind INTEGER NOT NULL,
    recorded_at INTEGER NOT NULL,
    PRIMARY KEY (action_id, seq)
);
";
