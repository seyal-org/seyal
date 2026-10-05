//! Local Context Engine — source discovery, provenance, index, and freshness.
//!
//! This crate owns the permanent production SPEC-013 discovery/index path for
//! M005 (#1271). It does **not** assemble `ContextBundle` / `SelectionTrace`,
//! own `MemoryStore`, or implement ranking / a second router.
//!
//! Authority: ADR-013, SPEC-013 §§3, 5–6, 10–12, 18–23. Resource caps:
//! `docs/evidence/m005-context-memory-production-calibration.md`.
//!
//! Terminal Runtime (`seyal-runtime` / PTY / VT / Metal) is never a dependency.

mod budget;
mod caps;
mod digest;
mod engine;
mod git;
mod ignore;
mod index;
mod policy;
mod provenance;
mod scope;
mod source;
mod walk;

pub use budget::DiscoveryBudget;
pub use caps::{
    DURABLE_INDEX_MIB_AGGREGATE, DURABLE_INDEX_MIB_PER_WORKSPACE, INDEX_PRODUCER_ID,
    INDEX_SCHEMA_VERSION, MAX_QUEUE_DEPTH, MAX_RETRY_ATTEMPTS, MAX_RETRY_DEADLINE_SECS,
    MAX_SYMLINK_HOPS, MAX_TRAVERSAL_DEPTH, MAX_TRAVERSAL_ENTRIES, MAX_VISITED_IDENTITIES,
    WARM_RSS_MIB_PER_WORKSPACE,
};
pub use digest::{digest_bytes, digest_file, IntegrityDigest};
pub use engine::{ContextDiscoveryEngine, DiscoveryOutcome};
pub use index::{
    load_index, probe_cache, store_index, CacheLookup, IndexCacheEntry, IndexCacheKey,
};
pub use provenance::{
    join_under_root, validate_relative_identity, DiscoveredSource, ObjectIdentity, SourceProvenance,
};
pub use scope::{AuthorizedRoot, DiscoveryScope, RepositoryId, WorktreeId};
pub use source::{
    AuthorityClass, DiscoveryHealth, ExclusionReason, SensitivityClass, SourceClass, VcsMembership,
};
pub use walk::{catalog_fingerprint, discover, reject_malformed_relative, DiscoveryReport};
