//! Rebuildable discovery index/cache with generation and integrity fencing.

use seyal_agent_core::WorkScopeId;
use seyal_agent_store::{AgentStore, ContextIndexRecord, StoreError};

use crate::caps::{DURABLE_INDEX_MIB_PER_WORKSPACE, INDEX_PRODUCER_ID, INDEX_SCHEMA_VERSION};
use crate::digest::{digest_bytes, IntegrityDigest};
use crate::scope::DiscoveryScope;
use crate::source::ExclusionReason;
use crate::walk::{catalog_fingerprint, DiscoveryReport};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexCacheKey {
    pub work_scope_id: WorkScopeId,
    pub policy_generation: u64,
    pub privacy_generation: u64,
    pub source_generation: u64,
    pub catalog_digest: IntegrityDigest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexCacheEntry {
    pub key: IndexCacheKey,
    pub producer_id: String,
    pub schema_version: u32,
    pub integrity: IntegrityDigest,
    pub payload: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheLookup {
    Hit,
    Miss(ExclusionReason),
}

impl IndexCacheEntry {
    pub fn from_report(scope: &DiscoveryScope, report: &DiscoveryReport) -> Self {
        let catalog = catalog_fingerprint(&report.sources);
        let payload = encode_report_payload(report);
        let integrity = digest_bytes(&payload);
        Self {
            key: IndexCacheKey {
                work_scope_id: scope.work_scope_id,
                policy_generation: scope.policy_generation,
                privacy_generation: scope.privacy_generation,
                source_generation: scope.source_generation,
                catalog_digest: catalog,
            },
            producer_id: INDEX_PRODUCER_ID.to_string(),
            schema_version: INDEX_SCHEMA_VERSION,
            integrity,
            payload,
        }
    }

    pub fn validate_for(&self, scope: &DiscoveryScope) -> CacheLookup {
        if self.producer_id != INDEX_PRODUCER_ID {
            return CacheLookup::Miss(ExclusionReason::IntegrityMismatch);
        }
        if self.schema_version != INDEX_SCHEMA_VERSION {
            return CacheLookup::Miss(ExclusionReason::IntegrityMismatch);
        }
        if digest_bytes(&self.payload) != self.integrity {
            return CacheLookup::Miss(ExclusionReason::IntegrityMismatch);
        }
        if self.key.policy_generation != scope.policy_generation
            || self.key.privacy_generation != scope.privacy_generation
            || self.key.source_generation != scope.source_generation
            || self.key.work_scope_id != scope.work_scope_id
        {
            return CacheLookup::Miss(ExclusionReason::GenerationStale);
        }
        CacheLookup::Hit
    }
}

fn encode_report_payload(report: &DiscoveryReport) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(report.sources.len() as u64).to_le_bytes());
    for source in &report.sources {
        let line = format!(
            "{}\t{}\t{}\t{}\t{:?}\n",
            source.source_class.code(),
            source.provenance.repository_id.0,
            source.provenance.worktree_id.0,
            source.provenance.relative_path.display(),
            source.exclusion
        );
        out.extend_from_slice(line.as_bytes());
    }
    out
}

/// Persist a rebuildable index entry into the agent-domain store.
pub fn store_index(store: &AgentStore, entry: &IndexCacheEntry) -> Result<(), StoreError> {
    let max_bytes = DURABLE_INDEX_MIB_PER_WORKSPACE.saturating_mul(1024 * 1024);
    if entry.payload.len() as u64 > max_bytes {
        // Shed rebuildable cache under pressure rather than grow unbounded.
        let _ = store.delete_context_index(entry.key.work_scope_id);
        return Err(StoreError::PayloadTooLarge);
    }
    store.put_context_index(&ContextIndexRecord {
        work_scope_id: entry.key.work_scope_id,
        producer_id: entry.producer_id.clone(),
        schema_version: entry.schema_version,
        policy_generation: entry.key.policy_generation,
        privacy_generation: entry.key.privacy_generation,
        source_generation: entry.key.source_generation,
        catalog_digest_hex: entry.key.catalog_digest.hex(),
        integrity_hex: entry.integrity.hex(),
        payload: entry.payload.clone(),
    })
}

/// Load and validate a cached index; generation/integrity mismatch is a miss.
pub fn load_index(
    store: &AgentStore,
    scope: &DiscoveryScope,
) -> Result<Option<IndexCacheEntry>, StoreError> {
    let Some(row) = store.get_context_index(scope.work_scope_id)? else {
        return Ok(None);
    };
    let integrity = IntegrityDigest::from_hex(&row.integrity_hex).ok_or(StoreError::Corrupt)?;
    let catalog = IntegrityDigest::from_hex(&row.catalog_digest_hex).ok_or(StoreError::Corrupt)?;
    let entry = IndexCacheEntry {
        key: IndexCacheKey {
            work_scope_id: row.work_scope_id,
            policy_generation: row.policy_generation,
            privacy_generation: row.privacy_generation,
            source_generation: row.source_generation,
            catalog_digest: catalog,
        },
        producer_id: row.producer_id,
        schema_version: row.schema_version,
        integrity,
        payload: row.payload,
    };
    match entry.validate_for(scope) {
        CacheLookup::Hit => Ok(Some(entry)),
        CacheLookup::Miss(_) => {
            let _ = store.delete_context_index(scope.work_scope_id);
            Ok(None)
        }
    }
}

/// Probe whether a stored index is usable under the current scope generations.
pub fn probe_cache(store: &AgentStore, scope: &DiscoveryScope) -> CacheLookup {
    match load_index(store, scope) {
        Ok(Some(entry)) => entry.validate_for(scope),
        Ok(None) => CacheLookup::Miss(ExclusionReason::GenerationStale),
        Err(_) => CacheLookup::Miss(ExclusionReason::IntegrityMismatch),
    }
}
