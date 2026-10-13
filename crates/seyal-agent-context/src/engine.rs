//! Public Local Context Engine discovery/index + bundle assembly API.

use seyal_agent_store::AgentStore;

use crate::assemble::{assemble_bundle, BundleBuildOutcome, BundleBuildSlots};
use crate::budget::DiscoveryBudget;
use crate::index::{probe_cache, store_index, CacheLookup, IndexCacheEntry};
use crate::request::BuildRequest;
use crate::scope::DiscoveryScope;
use crate::source::{DiscoveryHealth, ExclusionReason};
use crate::walk::{discover, DiscoveryReport};

/// Permanent production Local Context Engine discovery surface.
#[derive(Debug, Default)]
pub struct ContextDiscoveryEngine {
    pub budget: DiscoveryBudget,
}

#[derive(Clone, Debug)]
pub struct DiscoveryOutcome {
    pub report: DiscoveryReport,
    pub cache: CacheLookup,
    pub persisted: bool,
}

impl ContextDiscoveryEngine {
    pub fn new() -> Self {
        Self {
            budget: DiscoveryBudget::new(),
        }
    }

    pub fn cancel(&self) {
        self.budget.cancel();
    }

    /// Discover authorized sources. Never executes discovered content. Optionally
    /// refreshes the rebuildable index in the agent-domain store.
    pub fn discover_with_store(
        &self,
        scope: &DiscoveryScope,
        store: Option<&AgentStore>,
    ) -> DiscoveryOutcome {
        if self.budget.is_cancelled() {
            return DiscoveryOutcome {
                report: DiscoveryReport {
                    sources: Vec::new(),
                    health: DiscoveryHealth::Cancelled,
                    entries_visited: 0,
                    visited_identities: 0,
                    max_depth_seen: 0,
                },
                cache: CacheLookup::Miss(ExclusionReason::Cancelled),
                persisted: false,
            };
        }

        let prior = store
            .map(|s| probe_cache(s, scope))
            .unwrap_or(CacheLookup::Miss(ExclusionReason::GenerationStale));

        if let Err(reason) = self.budget.try_enqueue() {
            return DiscoveryOutcome {
                report: DiscoveryReport {
                    sources: Vec::new(),
                    health: DiscoveryHealth::Degraded,
                    entries_visited: 0,
                    visited_identities: 0,
                    max_depth_seen: 0,
                },
                cache: CacheLookup::Miss(reason),
                persisted: false,
            };
        }

        let report = discover(scope, &self.budget);
        self.budget.dequeue();

        let mut persisted = false;
        if let Some(store) = store {
            let entry = IndexCacheEntry::from_report(scope, &report);
            if store_index(store, &entry).is_ok() {
                persisted = true;
            }
        }

        DiscoveryOutcome {
            report,
            cache: prior,
            persisted,
        }
    }

    /// Read-only cache probe used by freshness/invalidation callers.
    pub fn probe(&self, scope: &DiscoveryScope, store: &AgentStore) -> CacheLookup {
        probe_cache(store, scope)
    }

    /// Inject a persistent failure into the retry/deadline budget.
    pub fn inject_persistent_failure(&self) -> DiscoveryHealth {
        self.budget.record_failure()
    }
}

/// Permanent production ContextBundle / SelectionTrace assembly surface (#1272).
///
/// Consumes #1271 discovery. Does not own MemoryStore, eval, or ranking.
#[derive(Debug, Default)]
pub struct ContextBundleEngine {
    pub slots: BundleBuildSlots,
}

impl ContextBundleEngine {
    pub fn new() -> Self {
        Self {
            slots: BundleBuildSlots::new(),
        }
    }

    /// Build an immutable ContextBundle + policy-safe SelectionTrace.
    pub fn build(&self, request: &BuildRequest) -> Result<BundleBuildOutcome, ExclusionReason> {
        assemble_bundle(request, &self.slots)
    }

    pub fn active_builds(&self) -> usize {
        self.slots.active()
    }
}
