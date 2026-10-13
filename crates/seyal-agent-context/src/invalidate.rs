//! Bundle dependency invalidation (SPEC-013 §§11, 14 / §23.4–7).

use crate::bundle::{BundleStatus, ContextBundle};
use crate::digest::IntegrityDigest;
use crate::walk::{catalog_fingerprint, DiscoveryReport};

/// Why a bundle became stale (policy-safe).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidationReason {
    SelectedContentChanged,
    CatalogEnumerationChanged,
    GenerationFence,
    HigherAuthorityAppeared,
    NegativeDependencyChanged,
    Unrelated,
}

/// Compare an existing bundle against a fresh discovery report.
pub fn evaluate_invalidation(
    bundle: &ContextBundle,
    fresh: &DiscoveryReport,
    scope_policy: u64,
    scope_privacy: u64,
    scope_source: u64,
) -> InvalidationReason {
    if bundle.policy_generation != scope_policy
        || bundle.privacy_generation != scope_privacy
        || bundle.source_generation != scope_source
    {
        return InvalidationReason::GenerationFence;
    }

    let fresh_catalog = catalog_fingerprint(&fresh.sources);
    if fresh_catalog != bundle.dependencies.catalog_fingerprint {
        // Distinguish selected-edit vs higher-authority appearance vs unrelated catalog churn.
        if selected_fingerprint_changed(bundle, fresh) {
            return InvalidationReason::SelectedContentChanged;
        }
        if higher_authority_candidate_appeared(bundle, fresh) {
            return InvalidationReason::HigherAuthorityAppeared;
        }
        if negative_dependency_changed(bundle, fresh) {
            return InvalidationReason::NegativeDependencyChanged;
        }
        // Catalog changed but not selected and not higher-authority — still
        // enumeration-complete invalidation when catalog fingerprint differs.
        return InvalidationReason::CatalogEnumerationChanged;
    }

    if selected_fingerprint_changed(bundle, fresh) {
        return InvalidationReason::SelectedContentChanged;
    }

    InvalidationReason::Unrelated
}

fn selected_fingerprint_changed(bundle: &ContextBundle, fresh: &DiscoveryReport) -> bool {
    for (id, expected_fp) in &bundle.dependencies.selected_content_fingerprints {
        // Match by relative path embedded in item id when possible.
        let Some(selected) = bundle.items.iter().find(|i| &i.id == id) else {
            return true;
        };
        let rel = &selected.provenance.relative_path;
        let Some(src) = fresh
            .sources
            .iter()
            .find(|s| s.provenance.relative_path == *rel && s.exclusion.is_none())
        else {
            return true;
        };
        let actual = src
            .provenance
            .content_fingerprint
            .unwrap_or(IntegrityDigest([0; 16]));
        if actual != *expected_fp {
            return true;
        }
    }
    false
}

fn higher_authority_candidate_appeared(bundle: &ContextBundle, fresh: &DiscoveryReport) -> bool {
    use crate::item::authority_for_source;
    let max_selected = bundle
        .items
        .iter()
        .map(|i| i.authority)
        .min() // AuthorityClass: lower discriminant = higher authority
        .unwrap_or(crate::source::AuthorityClass::OptionalSemantic);
    for src in fresh.sources.iter().filter(|s| s.exclusion.is_none()) {
        let auth = authority_for_source(src.source_class);
        if auth < max_selected {
            let already = bundle.items.iter().any(|i| {
                i.provenance.relative_path == src.provenance.relative_path
                    && i.source_class == src.source_class
            });
            if !already {
                return true;
            }
        }
    }
    false
}

fn negative_dependency_changed(bundle: &ContextBundle, fresh: &DiscoveryReport) -> bool {
    // Reclassification / removal of a previously absent-or-ineligible source that
    // affects the catalog fingerprint is already caught; this helper detects
    // membership flips for paths that were excluded at build time.
    let old_catalog = bundle.dependencies.catalog_fingerprint;
    let new_catalog = catalog_fingerprint(&fresh.sources);
    old_catalog != new_catalog
}

/// Apply invalidation: mark stale and strip payload when needed.
pub fn apply_invalidation(bundle: &mut ContextBundle, reason: InvalidationReason) -> bool {
    if reason == InvalidationReason::Unrelated {
        return false;
    }
    if bundle.status == BundleStatus::Dispatchable || bundle.status == BundleStatus::UnableToBuild {
        bundle.mark_stale();
        return true;
    }
    false
}

/// Unrelated path edit: true when the edited relative path is outside selected
/// deps and does not change catalog in a way that adds higher authority.
pub fn is_unrelated_edit(
    bundle: &ContextBundle,
    edited_relative: &std::path::Path,
    fresh: &DiscoveryReport,
) -> bool {
    let selected = bundle
        .items
        .iter()
        .any(|i| i.provenance.relative_path == edited_relative);
    if selected {
        return false;
    }
    evaluate_invalidation(
        bundle,
        fresh,
        bundle.policy_generation,
        bundle.privacy_generation,
        bundle.source_generation,
    ) == InvalidationReason::Unrelated
        || (evaluate_invalidation(
            bundle,
            fresh,
            bundle.policy_generation,
            bundle.privacy_generation,
            bundle.source_generation,
        ) == InvalidationReason::CatalogEnumerationChanged
            && !higher_authority_candidate_appeared(bundle, fresh)
            && !selected_fingerprint_changed(bundle, fresh))
}
