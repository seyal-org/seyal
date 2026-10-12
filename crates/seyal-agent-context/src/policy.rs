//! Eligibility gates applied before any relevance ranking (SPEC-013 §§5–6).

use crate::provenance::DiscoveredSource;
use crate::scope::DiscoveryScope;
use crate::source::{ExclusionReason, SensitivityClass, SourceClass, VcsMembership};

/// Apply deny-by-default scope/policy/sensitivity gates.
pub fn apply_eligibility(scope: &DiscoveryScope, mut source: DiscoveredSource) -> DiscoveredSource {
    if source.exclusion.is_some() {
        return source;
    }

    // Scope authorization already encoded as OutsideAuthorizedRoot / SiblingWorktreeLeak.
    if source.provenance.worktree_id.0.is_empty() {
        source.exclusion = Some(ExclusionReason::PolicyDenied);
        return source;
    }

    // Ignored files excluded by default unless explicitly authorized.
    if source.provenance.membership == VcsMembership::Ignored {
        if !scope.is_authorized_ignored(&source.provenance.absolute_path) {
            source.exclusion = Some(ExclusionReason::IgnoredByDefault);
            return source;
        }
        // Authorized ignored still receives sensitivity/policy filtering.
        if SensitivityClass::Secret > scope.max_sensitivity {
            // Treat unknown ignored content as Internal unless a stronger policy exists.
        }
    }

    // NormativeInstruction only from authorized-location policy (SPEC-013 §3 / §23.34).
    if source.source_class == SourceClass::NormativeInstruction
        && !scope.is_normative_instruction_path(&source.provenance.absolute_path)
    {
        source.exclusion = Some(ExclusionReason::SelfClassifiedInstruction);
        // Demote rather than keep an illegal classification.
        source.source_class = SourceClass::RepositoryFile;
        source.exclusion = Some(ExclusionReason::SelfClassifiedInstruction);
        return source;
    }

    // Sensitivity ceiling.
    let assumed = assumed_sensitivity(&source);
    if assumed > scope.max_sensitivity {
        source.exclusion = Some(ExclusionReason::SensitivityDenied);
        return source;
    }

    // Generation fence: provenance must match current scope generations.
    if source.provenance.policy_generation != scope.policy_generation
        || source.provenance.privacy_generation != scope.privacy_generation
        || source.provenance.source_generation != scope.source_generation
    {
        source.exclusion = Some(ExclusionReason::GenerationStale);
        return source;
    }

    source
}

/// Assumed sensitivity for discovery/selection (deny-by-default heuristics).
pub fn assumed_sensitivity(source: &DiscoveredSource) -> SensitivityClass {
    let name = source
        .provenance
        .relative_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let lower = name.to_ascii_lowercase();
    if lower.contains("secret")
        || lower.contains(".env")
        || lower.ends_with(".pem")
        || lower.ends_with(".key")
    {
        SensitivityClass::Secret
    } else if source.provenance.membership == VcsMembership::Ignored {
        SensitivityClass::Internal
    } else {
        SensitivityClass::Public
    }
}
