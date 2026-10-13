//! Optional semantic/model enhancement boundary (SPEC-013 §17).

use std::time::{Duration, Instant};

use crate::caps::SEMANTIC_ENHANCEMENT_TIMEOUT_MS;
use crate::item::ContextItem;
use crate::item::ContextItemId;

/// Result of an optional semantic reorder attempt.
#[derive(Clone, Debug)]
pub enum SemanticOutcome {
    /// Reordered subset of already-eligible candidates only.
    Reordered(Vec<ContextItemId>),
    /// Enhancer failed or timed out — caller must use deterministic baseline.
    Fallback,
}

/// Pluggable semantic enhancer. Production default is unused/fail-closed.
pub trait SemanticEnhancer: Send + Sync {
    fn enhance(&self, eligible: &[ContextItem]) -> SemanticOutcome;
}

/// Apply enhancer with timeout; reject any id not in the eligible set (§23.31–32).
pub fn apply_semantic_enhancement(
    enhancer: &dyn SemanticEnhancer,
    eligible: &[ContextItem],
) -> (Vec<ContextItem>, bool /* used_fallback */) {
    let started = Instant::now();
    let timeout = Duration::from_millis(SEMANTIC_ENHANCEMENT_TIMEOUT_MS);
    let outcome = enhancer.enhance(eligible);
    if started.elapsed() > timeout {
        return (eligible.to_vec(), true);
    }
    apply_semantic_outcome(outcome, eligible)
}

/// Apply a pre-computed semantic outcome (single enhance call for callers).
pub fn apply_semantic_outcome(
    outcome: SemanticOutcome,
    eligible: &[ContextItem],
) -> (Vec<ContextItem>, bool /* used_fallback */) {
    match outcome {
        SemanticOutcome::Fallback => (eligible.to_vec(), true),
        SemanticOutcome::Reordered(order) => {
            let eligible_ids: std::collections::HashSet<_> =
                eligible.iter().map(|i| i.id.clone()).collect();
            // Reject reintroduction or unknown ids.
            if order.iter().any(|id| !eligible_ids.contains(id)) {
                return (eligible.to_vec(), true);
            }
            if order.len() != eligible.len() {
                // Partial reorder still must only use eligible ids; fill remaining
                // in deterministic baseline order for unspecified members.
            }
            let mut by_id: std::collections::BTreeMap<_, _> =
                eligible.iter().map(|i| (i.id.clone(), i.clone())).collect();
            let mut out = Vec::with_capacity(eligible.len());
            let mut seen = std::collections::HashSet::new();
            for id in order {
                if let Some(item) = by_id.remove(&id) {
                    seen.insert(item.id.clone());
                    out.push(item);
                }
            }
            // Append any eligible not mentioned, preserving baseline relative order.
            for item in eligible {
                if !seen.contains(&item.id) {
                    out.push(item.clone());
                }
            }
            // Authority must not widen: if enhancer order violates authority
            // precedence against baseline, fall back.
            if violates_authority_ceiling(&out, eligible) {
                return (eligible.to_vec(), true);
            }
            (out, false)
        }
    }
}

fn violates_authority_ceiling(enhanced: &[ContextItem], baseline: &[ContextItem]) -> bool {
    // A lower-authority item must not move ahead of a higher-authority item that
    // was ahead in the deterministic baseline (§23.3 / §23.31).
    let baseline_pos: std::collections::HashMap<_, _> = baseline
        .iter()
        .enumerate()
        .map(|(i, item)| (item.id.clone(), i))
        .collect();
    for i in 0..enhanced.len() {
        for j in (i + 1)..enhanced.len() {
            let a = &enhanced[i];
            let b = &enhanced[j];
            if a.authority > b.authority {
                // lower authority (higher ordinal) before higher authority
                if let (Some(&pa), Some(&pb)) = (baseline_pos.get(&a.id), baseline_pos.get(&b.id))
                    && pa > pb
                {
                    return true;
                }
            }
        }
    }
    false
}

/// Default unused enhancer — always falls back (no provider required).
#[derive(Debug, Default)]
pub struct UnusedSemanticEnhancer;

impl SemanticEnhancer for UnusedSemanticEnhancer {
    fn enhance(&self, _eligible: &[ContextItem]) -> SemanticOutcome {
        SemanticOutcome::Fallback
    }
}
