//! Pin / singleton hard resolution (SPEC-027 §4.3).
//!
//! Remains a valid selection path after V1 ranking is composed. Pins never
//! bypass SPEC-020 §5 hard constraints.

use crate::lifecycle::SelectionKind;
use crate::{AdapterId, RouteOfferingId};

/// One eligible-or-not RouteOffering fact, precomputed by the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdapterCandidate {
    pub adapter_id: AdapterId,
    pub route_offering_id: RouteOfferingId,
    pub adapter_enabled: bool,
    pub adapter_manifest_generation: u64,
    /// SPEC-020 §5 hard-constraint evaluation (including TTY/enforcement
    /// class). `false` means this offering cannot be selected at all.
    pub hard_constraint_satisfied: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedTarget {
    pub adapter_id: AdapterId,
    pub route_offering_id: RouteOfferingId,
    pub adapter_manifest_generation: u64,
    pub selection_kind: SelectionKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolveFailure {
    /// Pin names an offering that is not present in the candidate set
    /// (unknown/uninstalled), or it fails a hard constraint.
    TargetUnavailable,
    /// Pin names a present, hard-constraint-satisfying offering whose
    /// adapter is disabled.
    AdapterDisabled { adapter_id: AdapterId },
    /// Unpinned resolution saw zero eligible offerings, or more than one
    /// without a ranking stage result.
    AmbiguousOrNoTarget,
    /// Explicit NoRoute after hard-constraint / floor / budget conflict.
    NoRoute,
    /// BaselineCalibrationArtifact integrity or cold-start bind failed.
    BaselineIntegrity,
}

/// SPEC-027 §4.3 pin/singleton resolver (no soft ranking).
pub fn resolve_pin_or_singleton(
    pin: Option<RouteOfferingId>,
    candidates: &[AdapterCandidate],
) -> Result<ResolvedTarget, ResolveFailure> {
    if let Some(pinned) = pin {
        let candidate = candidates
            .iter()
            .find(|candidate| candidate.route_offering_id == pinned)
            .ok_or(ResolveFailure::TargetUnavailable)?;
        if !candidate.hard_constraint_satisfied {
            return Err(ResolveFailure::TargetUnavailable);
        }
        if !candidate.adapter_enabled {
            return Err(ResolveFailure::AdapterDisabled {
                adapter_id: candidate.adapter_id,
            });
        }
        return Ok(ResolvedTarget {
            adapter_id: candidate.adapter_id,
            route_offering_id: candidate.route_offering_id,
            adapter_manifest_generation: candidate.adapter_manifest_generation,
            selection_kind: SelectionKind::Pinned,
        });
    }

    let mut eligible = candidates
        .iter()
        .filter(|candidate| candidate.adapter_enabled && candidate.hard_constraint_satisfied);
    let first = eligible.next();
    let second = eligible.next();
    match (first, second) {
        (Some(only), None) => Ok(ResolvedTarget {
            adapter_id: only.adapter_id,
            route_offering_id: only.route_offering_id,
            adapter_manifest_generation: only.adapter_manifest_generation,
            selection_kind: SelectionKind::Singleton,
        }),
        _ => Err(ResolveFailure::AmbiguousOrNoTarget),
    }
}

/// Backward-compatible name used by Agent Backend pin/singleton composition.
pub fn resolve_execution_target(
    pin: Option<RouteOfferingId>,
    candidates: &[AdapterCandidate],
) -> Result<ResolvedTarget, ResolveFailure> {
    resolve_pin_or_singleton(pin, candidates)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(enabled: bool, hard_ok: bool) -> AdapterCandidate {
        AdapterCandidate {
            adapter_id: AdapterId::new(),
            route_offering_id: RouteOfferingId::new(),
            adapter_enabled: enabled,
            adapter_manifest_generation: 1,
            hard_constraint_satisfied: hard_ok,
        }
    }

    #[test]
    fn unpinned_zero_eligible_is_unavailable() {
        let candidates = [candidate(false, true), candidate(true, false)];
        assert_eq!(
            resolve_execution_target(None, &candidates),
            Err(ResolveFailure::AmbiguousOrNoTarget)
        );
    }

    #[test]
    fn unpinned_exactly_one_eligible_is_singleton() {
        let only = candidate(true, true);
        let candidates = [candidate(false, true), only, candidate(true, false)];
        let resolved = resolve_execution_target(None, &candidates).unwrap();
        assert_eq!(resolved.selection_kind, SelectionKind::Singleton);
        assert_eq!(resolved.adapter_id, only.adapter_id);
    }

    #[test]
    fn unpinned_two_eligible_without_ranking_is_unavailable() {
        let candidates = [candidate(true, true), candidate(true, true)];
        assert_eq!(
            resolve_execution_target(None, &candidates),
            Err(ResolveFailure::AmbiguousOrNoTarget)
        );
    }

    #[test]
    fn pin_unknown_offering_is_unavailable() {
        let candidates = [candidate(true, true)];
        let unknown_pin = RouteOfferingId::new();
        assert_eq!(
            resolve_execution_target(Some(unknown_pin), &candidates),
            Err(ResolveFailure::TargetUnavailable)
        );
    }

    #[test]
    fn pin_hard_constraint_miss_is_unavailable() {
        let hit = candidate(true, false);
        let candidates = [hit];
        assert_eq!(
            resolve_execution_target(Some(hit.route_offering_id), &candidates),
            Err(ResolveFailure::TargetUnavailable)
        );
    }

    #[test]
    fn pin_of_disabled_adapter_is_adapter_not_enabled() {
        let disabled = candidate(false, true);
        let candidates = [disabled];
        assert_eq!(
            resolve_execution_target(Some(disabled.route_offering_id), &candidates),
            Err(ResolveFailure::AdapterDisabled {
                adapter_id: disabled.adapter_id
            })
        );
    }

    #[test]
    fn pin_of_enabled_offering_is_pinned_even_with_other_eligible_offerings() {
        let pinned = candidate(true, true);
        let candidates = [pinned, candidate(true, true), candidate(true, true)];
        let resolved =
            resolve_execution_target(Some(pinned.route_offering_id), &candidates).unwrap();
        assert_eq!(resolved.selection_kind, SelectionKind::Pinned);
        assert_eq!(resolved.route_offering_id, pinned.route_offering_id);
    }
}
