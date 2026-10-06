//! Immutable `ContextBundle` contract and dependency completeness (SPEC-013 §§13–14).

use crate::caps::{BUNDLE_BUILDER_VERSION, SELECTION_CONFIG_VERSION};
use crate::digest::IntegrityDigest;
use crate::item::{ContextItem, ContextItemId};
use crate::scope::DiscoveryScope;
use crate::trace::{SelectionTrace, SelectionTraceId};

/// Opaque immutable bundle identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ContextBundleId(pub String);

impl ContextBundleId {
    pub fn new(fingerprint: &IntegrityDigest) -> Self {
        Self(format!("bundle-{}", fingerprint.hex()))
    }
}

/// Dispatch eligibility for an immutable selection record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BundleStatus {
    /// Selection is current and may be dispatched.
    Dispatchable,
    /// Mandatory context could not be satisfied / budget overflow.
    UnableToBuild,
    /// Previously dispatchable selection is now stale.
    Stale,
    /// Payload redacted; selection metadata may remain for audit only.
    Undispatchable,
}

/// Dependency set sufficient for selected + enumeration/negative invalidation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundleDependencies {
    pub catalog_fingerprint: IntegrityDigest,
    pub policy_generation: u64,
    pub privacy_generation: u64,
    pub source_generation: u64,
    pub selected_item_ids: Vec<ContextItemId>,
    pub selected_content_fingerprints: Vec<(ContextItemId, IntegrityDigest)>,
    pub enumeration_roots: Vec<String>,
}

/// Immutable per-build snapshot of selected context (SPEC-013 §13).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextBundle {
    pub id: ContextBundleId,
    pub work_scope_id: seyal_agent_core::WorkScopeId,
    pub status: BundleStatus,
    pub items: Vec<ContextItem>,
    pub dependencies: BundleDependencies,
    pub policy_generation: u64,
    pub privacy_generation: u64,
    pub source_generation: u64,
    pub builder_version: String,
    pub selection_config_version: String,
    pub estimated_tokens: u64,
    pub trace_id: SelectionTraceId,
    /// Retained only while dispatchable; cleared on stale/undispatchable (§23.28).
    pub retains_payload: bool,
}

impl ContextBundle {
    pub fn from_selection(
        scope: &DiscoveryScope,
        items: Vec<ContextItem>,
        status: BundleStatus,
        catalog_fingerprint: IntegrityDigest,
        trace: &SelectionTrace,
    ) -> Self {
        let estimated_tokens = items.iter().map(|i| i.estimated_tokens).sum();
        let selected_item_ids: Vec<_> = items.iter().map(|i| i.id.clone()).collect();
        let selected_content_fingerprints: Vec<_> = items
            .iter()
            .map(|i| (i.id.clone(), i.content_fingerprint))
            .collect();
        let enumeration_roots: Vec<_> = scope
            .roots
            .iter()
            .map(|r| {
                format!(
                    "{}|{}|{}",
                    r.repository_id.0,
                    r.worktree_id.0,
                    r.path.display()
                )
            })
            .collect();
        let fingerprint = crate::digest::digest_bytes(
            format!(
                "{}|{}|{}|{:?}",
                catalog_fingerprint.hex(),
                selected_item_ids
                    .iter()
                    .map(|i| i.0.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
                match status {
                    BundleStatus::Dispatchable => 1u8,
                    BundleStatus::UnableToBuild => 2u8,
                    BundleStatus::Stale => 3u8,
                    BundleStatus::Undispatchable => 4u8,
                },
                estimated_tokens
            )
            .as_bytes(),
        );
        let retains_payload = matches!(status, BundleStatus::Dispatchable);
        let items = if retains_payload {
            items
        } else {
            items
                .into_iter()
                .map(|mut item| {
                    item.payload.clear();
                    item
                })
                .collect()
        };
        Self {
            id: ContextBundleId::new(&fingerprint),
            work_scope_id: scope.work_scope_id,
            status,
            items,
            dependencies: BundleDependencies {
                catalog_fingerprint,
                policy_generation: scope.policy_generation,
                privacy_generation: scope.privacy_generation,
                source_generation: scope.source_generation,
                selected_item_ids,
                selected_content_fingerprints,
                enumeration_roots,
            },
            policy_generation: scope.policy_generation,
            privacy_generation: scope.privacy_generation,
            source_generation: scope.source_generation,
            builder_version: BUNDLE_BUILDER_VERSION.to_string(),
            selection_config_version: SELECTION_CONFIG_VERSION.to_string(),
            estimated_tokens,
            trace_id: trace.id.clone(),
            retains_payload,
        }
    }

    /// Mark stale and strip reconstructable payload (§23.28).
    pub fn mark_stale(&mut self) {
        self.status = BundleStatus::Stale;
        self.retains_payload = false;
        for item in &mut self.items {
            item.payload.clear();
        }
    }

    pub fn is_dispatchable(&self) -> bool {
        self.status == BundleStatus::Dispatchable && self.retains_payload
    }
}
