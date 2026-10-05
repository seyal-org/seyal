//! Frozen M005 MemoryStore / RunWorkingSet resource caps (#1244 / SPEC-012 §19 / SPEC-014 §18).

/// Max encoded payload + control metadata per MemoryRecord (bytes).
pub const MAX_RECORD_BYTES: usize = 256 * 1024;
/// Reserved safety-control metadata per record inside [`MAX_RECORD_BYTES`].
pub const RESERVED_SAFETY_CONTROL_BYTES: usize = 4 * 1024;
/// Max evidence/provenance references per MemoryRecord.
pub const MAX_PROVENANCE_REFS: usize = 64;
/// Max conflict/supersession lineage depth per record.
pub const MAX_LINEAGE_DEPTH: usize = 32;
/// Max Proposed records per owning scope.
pub const MAX_PROPOSED_PER_SCOPE: usize = 1_024;
/// Max durable record + tombstone bytes per owning scope.
pub const MAX_DURABLE_BYTES_PER_SCOPE: u64 = 64 * 1024 * 1024;
/// Max replay-receipt count per owning scope.
pub const MAX_REPLAY_RECEIPTS_PER_SCOPE: usize = 4_096;
/// Max replay-receipt durable bytes per owning scope.
pub const MAX_REPLAY_RECEIPT_BYTES_PER_SCOPE: u64 = 4 * 1024 * 1024;
/// Replay-receipt retention window (seconds).
pub const REPLAY_RECEIPT_TTL_SECS: u64 = 7 * 24 * 60 * 60;
/// Persistence retry attempt budget.
pub const PERSISTENCE_RETRY_ATTEMPTS: u32 = 8;
/// Persistence retry deadline (seconds).
pub const PERSISTENCE_RETRY_DEADLINE_SECS: u64 = 30;

/// Max resident + durable bytes per RunWorkingSet.
pub const MAX_WORKING_SET_BYTES: u64 = 16 * 1024 * 1024;
/// Max aggregate resident + durable working-set bytes.
pub const MAX_WORKING_SET_AGGREGATE_BYTES: u64 = 128 * 1024 * 1024;
/// Max retained entries per RunWorkingSet.
pub const MAX_WORKING_SET_ENTRIES: usize = 4_096;
/// Compaction cooperative time slice (milliseconds).
pub const COMPACTION_SLICE_MS: u64 = 50;

/// Outer MemoryRecord schema understood by this implementation.
pub const MEMORY_SCHEMA_VERSION: u16 = 1;
/// Free-form semantic-key profile: UTF-8 + NFC + LF + whitespace collapse (SPEC-012 §3.2).
pub const SEMANTIC_KEY_PROFILE_V1: u16 = 1;
/// Default structured applicability schema version.
pub const APPLICABILITY_SCHEMA_V1: u16 = 1;
/// Default free-form / statement payload schema.
pub const PAYLOAD_SCHEMA_STATEMENT_V1: u16 = 1;
