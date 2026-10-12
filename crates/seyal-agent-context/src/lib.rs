//! Local Context Engine — source discovery, provenance, index, freshness,
//! and permanent ContextBundle / SelectionTrace assembly.
//!
//! This crate owns the permanent production SPEC-013 path for M005:
//! - discovery/index (#1271)
//! - ContextBundle / SelectionTrace assembly (#1272)
//!
//! It does **not** own `MemoryStore` (#1273), evaluation (#1274), or ranking
//! / a second router (#1275).
//!
//! Authority: ADR-013, SPEC-013. Resource caps:
//! `docs/evidence/m005-context-memory-production-calibration.md`.
//!
//! Terminal Runtime (`seyal-runtime` / PTY / VT / Metal) is never a dependency.

mod assemble;
mod budget;
mod bundle;
mod caps;
mod digest;
mod engine;
mod git;
mod ignore;
mod index;
mod invalidate;
mod item;
mod lsp;
mod policy;
mod provenance;
mod request;
mod scope;
mod semantic;
mod source;
mod trace;
mod walk;

pub use assemble::{
    assemble_bundle, assemble_from_discovery, BundleBuildGuard, BundleBuildOutcome,
    BundleBuildSlots,
};
pub use budget::DiscoveryBudget;
pub use bundle::{BundleDependencies, BundleStatus, ContextBundle, ContextBundleId};
pub use caps::{
    BUNDLE_BUILDER_VERSION, DURABLE_INDEX_MIB_AGGREGATE, DURABLE_INDEX_MIB_PER_WORKSPACE,
    INDEX_PRODUCER_ID, INDEX_SCHEMA_VERSION, MAX_CONCURRENT_BUNDLE_BUILDS, MAX_QUEUE_DEPTH,
    MAX_RETRY_ATTEMPTS, MAX_RETRY_DEADLINE_SECS, MAX_SYMLINK_HOPS, MAX_TRAVERSAL_DEPTH,
    MAX_TRAVERSAL_ENTRIES, MAX_VISITED_IDENTITIES, SELECTION_CONFIG_VERSION,
    SEMANTIC_ENHANCEMENT_TIMEOUT_MS, WARM_RSS_MIB_PER_WORKSPACE,
};
pub use digest::{digest_bytes, digest_file, IntegrityDigest};
pub use engine::{ContextBundleEngine, ContextDiscoveryEngine, DiscoveryOutcome};
pub use index::{
    load_index, probe_cache, store_index, CacheLookup, IndexCacheEntry, IndexCacheKey,
};
pub use invalidate::{
    apply_invalidation, evaluate_invalidation, is_unrelated_edit, InvalidationReason,
};
pub use item::{authority_for_source, estimate_tokens, ContentRange, ContextItem, ContextItemId};
pub use lsp::{
    materialize_lsp_item, overlay_and_disk_are_distinct, reject_stale_lsp_generation,
    LspDocumentSource, LspSourceChoice,
};
pub use policy::assumed_sensitivity;
pub use provenance::{
    join_under_root, validate_relative_identity, DiscoveredSource, ObjectIdentity, SourceProvenance,
};
pub use request::{BuildRequest, RequiredSource, TokenBudget};
pub use scope::{AuthorizedRoot, DiscoveryScope, RepositoryId, WorktreeId};
pub use semantic::{
    apply_semantic_enhancement, apply_semantic_outcome, SemanticEnhancer, SemanticOutcome,
    UnusedSemanticEnhancer,
};
pub use source::{
    AuthorityClass, DiscoveryHealth, ExclusionReason, SensitivityClass, SourceClass, VcsMembership,
};
pub use trace::{SelectionTrace, SelectionTraceId, TraceEntry, TraceReason};
pub use walk::{catalog_fingerprint, discover, reject_malformed_relative, DiscoveryReport};
