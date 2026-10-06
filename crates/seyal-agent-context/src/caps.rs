//! Frozen SPEC-013 §22 / M005 calibration caps for discovery and index work.

/// Maximum directory depth from an authorized root (calibration).
pub const MAX_TRAVERSAL_DEPTH: u32 = 64;
/// Maximum filesystem entries visited in one discovery run.
pub const MAX_TRAVERSAL_ENTRIES: u64 = 100_000;
/// Maximum symlink hops when resolving a single path chain.
pub const MAX_SYMLINK_HOPS: u32 = 32;
/// Maximum distinct resolved object identities retained in the visited set.
pub const MAX_VISITED_IDENTITIES: u64 = 100_000;
/// Maximum queued discovery/index jobs per workspace.
pub const MAX_QUEUE_DEPTH: usize = 64;
/// Finite automatic retry attempts for persistent source/index failure.
pub const MAX_RETRY_ATTEMPTS: u32 = 5;
/// Finite automatic retry deadline for persistent source/index failure.
pub const MAX_RETRY_DEADLINE_SECS: u64 = 60;
/// Warm context-index RSS ceiling per workspace (MiB).
pub const WARM_RSS_MIB_PER_WORKSPACE: u64 = 64;
/// Durable rebuildable index/cache ceiling per workspace (MiB).
pub const DURABLE_INDEX_MIB_PER_WORKSPACE: u64 = 256;
/// Aggregate durable rebuildable index/cache ceiling (MiB).
pub const DURABLE_INDEX_MIB_AGGREGATE: u64 = 1024;

/// Producer identity for the permanent discovery/index path.
pub const INDEX_PRODUCER_ID: &str = "seyal-agent-context/discovery-index";
/// Cache schema version for integrity/producer fencing.
pub const INDEX_SCHEMA_VERSION: u32 = 1;

/// Concurrent independent ContextBundle builds per workspace (calibration).
pub const MAX_CONCURRENT_BUNDLE_BUILDS: usize = 8;
/// Optional semantic enhancement timeout then deterministic fallback (ms).
pub const SEMANTIC_ENHANCEMENT_TIMEOUT_MS: u64 = 2_000;
/// Builder/version fencing for ContextBundle / SelectionTrace.
pub const BUNDLE_BUILDER_VERSION: &str = "seyal-agent-context/bundle-v1";
/// Selection configuration version for deterministic baseline.
pub const SELECTION_CONFIG_VERSION: &str = "deterministic-baseline-v1";
