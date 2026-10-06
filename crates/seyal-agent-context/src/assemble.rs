//! Deterministic ContextBundle / SelectionTrace assembly (SPEC-013 §§7–9, 13–17).

use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::budget::DiscoveryBudget;
use crate::bundle::{BundleStatus, ContextBundle};
use crate::caps::{BUNDLE_BUILDER_VERSION, MAX_CONCURRENT_BUNDLE_BUILDS, SELECTION_CONFIG_VERSION};
use crate::digest::digest_bytes;
use crate::item::{
    authority_for_source, estimate_tokens, ContentRange, ContextItem, ContextItemId,
};
use crate::policy::assumed_sensitivity;
use crate::provenance::DiscoveredSource;
use crate::request::BuildRequest;
use crate::source::{AuthorityClass, ExclusionReason, SensitivityClass, SourceClass};
use crate::trace::{SelectionTrace, TraceReason};
use crate::walk::{catalog_fingerprint, discover, DiscoveryReport};

/// Outcome of one permanent production bundle build.
#[derive(Clone, Debug)]
pub struct BundleBuildOutcome {
    pub bundle: ContextBundle,
    pub trace: SelectionTrace,
    pub discovery: DiscoveryReport,
    pub pre_semantic_order: Vec<ContextItemId>,
}

/// Slot counter for concurrent independent builds (≤ 8/workspace).
#[derive(Debug, Default)]
pub struct BundleBuildSlots {
    active: AtomicUsize,
}

impl BundleBuildSlots {
    pub fn new() -> Self {
        Self {
            active: AtomicUsize::new(0),
        }
    }

    pub fn try_acquire(&self) -> Result<BundleBuildGuard<'_>, ExclusionReason> {
        loop {
            let cur = self.active.load(Ordering::SeqCst);
            if cur >= MAX_CONCURRENT_BUNDLE_BUILDS {
                return Err(ExclusionReason::Degraded);
            }
            if self
                .active
                .compare_exchange(cur, cur + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return Ok(BundleBuildGuard { slots: self });
            }
        }
    }

    pub fn active(&self) -> usize {
        self.active.load(Ordering::SeqCst)
    }
}

pub struct BundleBuildGuard<'a> {
    slots: &'a BundleBuildSlots,
}

impl Drop for BundleBuildGuard<'_> {
    fn drop(&mut self) {
        self.slots.active.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Assemble an immutable ContextBundle + SelectionTrace from #1271 discovery.
pub fn assemble_bundle(
    request: &BuildRequest,
    slots: &BundleBuildSlots,
) -> Result<BundleBuildOutcome, ExclusionReason> {
    let _guard = slots.try_acquire()?;
    let budget = DiscoveryBudget::new();
    let discovery = discover(&request.scope, &budget);
    assemble_from_discovery(request, discovery)
}

/// Assemble from an already-produced discovery report (tests / callers).
pub fn assemble_from_discovery(
    request: &BuildRequest,
    discovery: DiscoveryReport,
) -> Result<BundleBuildOutcome, ExclusionReason> {
    let catalog = catalog_fingerprint(&discovery.sources);
    let mut trace = SelectionTrace::new(BUNDLE_BUILDER_VERSION, SELECTION_CONFIG_VERSION);

    // Materialize candidates from discovery + fixtures.
    let mut candidates: Vec<Candidate> = Vec::new();
    for src in &discovery.sources {
        if let Some(reason) = src.exclusion {
            let sens = assumed_sensitivity(src);
            trace.record_exclusion(
                safe_candidate_label(src, sens),
                TraceReason::from_exclusion(reason),
                sens,
            );
            continue;
        }
        match materialize_discovered(src) {
            Ok(item) => candidates.push(Candidate {
                item,
                excluded: None,
            }),
            Err(reason) => {
                let sens = assumed_sensitivity(src);
                trace.record_exclusion(
                    safe_candidate_label(src, sens),
                    TraceReason::from_exclusion(reason),
                    sens,
                );
            }
        }
    }
    for fixture in &request.fixture_items {
        candidates.push(Candidate {
            item: fixture.clone(),
            excluded: None,
        });
    }

    // Mark pins as exact-task authority when present.
    for pin in &request.pins {
        for cand in &mut candidates {
            if cand.item.provenance.relative_path == *pin {
                cand.item.authority = AuthorityClass::ExactTaskPin;
            }
        }
    }

    // Mark required sources mandatory; detect missing/ineligible required (§23.39).
    let mut required_failed = false;
    for req in &request.required {
        let found_eligible = candidates
            .iter()
            .any(|c| c.excluded.is_none() && c.item.provenance.relative_path == req.relative_path);
        if found_eligible {
            for cand in &mut candidates {
                if cand.item.provenance.relative_path == req.relative_path {
                    cand.item.mandatory = true;
                }
            }
        } else {
            required_failed = true;
            // Check if discovered but excluded → same outcome as missing.
            let sens = SensitivityClass::Internal;
            trace.record_exclusion(
                format!("required:{}", req.relative_path.display()),
                TraceReason::RequiredContextMissing,
                sens,
            );
        }
    }

    // Deterministic pre-semantic order (authority, then identity) — §23.1.
    candidates.sort_by(|a, b| {
        (
            a.item.authority as u8,
            a.item.provenance.repository_id.0.as_str(),
            a.item.provenance.worktree_id.0.as_str(),
            a.item.provenance.relative_path.as_os_str(),
            a.item.id.0.as_str(),
        )
            .cmp(&(
                b.item.authority as u8,
                b.item.provenance.repository_id.0.as_str(),
                b.item.provenance.worktree_id.0.as_str(),
                b.item.provenance.relative_path.as_os_str(),
                b.item.id.0.as_str(),
            ))
    });

    let pre_semantic_order: Vec<_> = candidates.iter().map(|c| c.item.id.clone()).collect();

    // Dedup / coalesce (§23.20–22).
    let (mut eligible, conflicts) = coalesce_compatible(&candidates, &mut trace);

    // Optional semantic enhancement — cannot reintroduce excluded (§23.31–32).
    if let Some(enhancer) = &request.semantic {
        let before_ids: HashSet<_> = eligible.iter().map(|i| i.id.clone()).collect();
        let outcome = enhancer.enhance(&eligible);
        let attempted_reintroduce = matches!(
            &outcome,
            crate::semantic::SemanticOutcome::Reordered(order)
                if order.iter().any(|id| !before_ids.contains(id))
        );
        let (reordered, fell_back) = crate::semantic::apply_semantic_outcome(outcome, &eligible);
        if attempted_reintroduce {
            trace.push(crate::trace::TraceEntry {
                candidate_id: "semantic".into(),
                included: false,
                reason: TraceReason::SemanticRejectedReintroduce,
                authority: None,
                sensitivity: SensitivityClass::Public,
                detail: None,
            });
        }
        if fell_back {
            trace.push(crate::trace::TraceEntry {
                candidate_id: "semantic".into(),
                included: false,
                reason: TraceReason::SemanticFallback,
                authority: None,
                sensitivity: SensitivityClass::Public,
                detail: Some("deterministic-fallback".into()),
            });
        } else {
            eligible = reordered;
        }
    }

    // Surface conflicts in trace (§23.22).
    for conflict in &conflicts {
        trace.push(crate::trace::TraceEntry {
            candidate_id: conflict.clone(),
            included: false,
            reason: TraceReason::ConflictSurfaced,
            authority: None,
            sensitivity: SensitivityClass::Internal,
            detail: Some("conflict".into()),
        });
    }

    // Token-budget partitioning (§15 / §23.23–25).
    let (selected, status) = admit_with_budget(&eligible, request.budget.max_tokens, &mut trace);

    let status = if required_failed {
        BundleStatus::UnableToBuild
    } else {
        status
    };
    if status == BundleStatus::UnableToBuild {
        trace.push(crate::trace::TraceEntry {
            candidate_id: "mandatory".into(),
            included: false,
            reason: TraceReason::UnableToBuildMandatoryOverflow,
            authority: None,
            sensitivity: SensitivityClass::Public,
            detail: Some("unable-to-build".into()),
        });
    }

    for item in &selected {
        let reason = if item.mandatory {
            TraceReason::IncludedMandatoryAuthority
        } else if item.authority == AuthorityClass::ExactTaskPin {
            TraceReason::IncludedExactTaskPin
        } else {
            TraceReason::IncludedRelevantSource
        };
        trace.record_inclusion(&item.id, reason, item.authority, item.sensitivity);
    }

    let mut bundle =
        ContextBundle::from_selection(&request.scope, selected, status, catalog, &trace);
    trace.bundle_id = Some(bundle.id.0.clone());
    // Re-bind trace id into bundle (already set from pre-bundle trace id).
    bundle.trace_id = trace.id.clone();

    // Ensure undispatchable bundles do not retain payload archive (§23.28).
    if !bundle.is_dispatchable() {
        bundle.retains_payload = false;
        for item in &mut bundle.items {
            item.payload.clear();
        }
    }

    let _ = conflicts;
    Ok(BundleBuildOutcome {
        bundle,
        trace,
        discovery,
        pre_semantic_order,
    })
}

struct Candidate {
    item: ContextItem,
    excluded: Option<ExclusionReason>,
}

fn materialize_discovered(src: &DiscoveredSource) -> Result<ContextItem, ExclusionReason> {
    let path = &src.provenance.absolute_path;
    let payload = if src.source_class == SourceClass::GitState || path.as_os_str().is_empty() {
        Vec::new()
    } else if path.is_file() {
        std::fs::read(path).map_err(|_| ExclusionReason::SourceUnavailable)?
    } else {
        Vec::new()
    };
    let fingerprint = src
        .provenance
        .content_fingerprint
        .unwrap_or_else(|| digest_bytes(&payload));
    let sensitivity = assumed_sensitivity(src);
    let mut authority = authority_for_source(src.source_class);
    // Normative paths keep normative authority.
    if src.source_class == SourceClass::NormativeInstruction {
        authority = AuthorityClass::NormativeInstruction;
    }
    let id = ContextItemId::from_parts(
        &src.provenance.repository_id.0,
        &src.provenance.worktree_id.0,
        &src.provenance.relative_path.to_string_lossy(),
        &fingerprint.hex(),
        None,
    );
    Ok(ContextItem {
        id,
        source_class: src.source_class,
        authority,
        sensitivity,
        provenance: src.provenance.clone(),
        content_fingerprint: fingerprint,
        estimated_tokens: estimate_tokens(&payload),
        payload,
        range: None,
        object_identity: src.provenance.object_identity,
        mandatory: false,
        builder_version: BUNDLE_BUILDER_VERSION.to_string(),
    })
}

fn safe_candidate_label(src: &DiscoveredSource, sens: SensitivityClass) -> String {
    if sens >= SensitivityClass::Secret {
        format!(
            "redacted:{}",
            digest_bytes(src.provenance.relative_path.to_string_lossy().as_bytes()).hex()
        )
    } else {
        src.provenance.relative_path.display().to_string()
    }
}

/// Coalesce byte-identical eligible items only when policy/sensitivity compatible.
fn coalesce_compatible(
    candidates: &[Candidate],
    trace: &mut SelectionTrace,
) -> (Vec<ContextItem>, Vec<String>) {
    let mut by_fp: BTreeMap<String, Vec<&ContextItem>> = BTreeMap::new();
    for c in candidates {
        if c.excluded.is_some() {
            continue;
        }
        // Skip empty git-state placeholders from crowding coalescing.
        if c.item.source_class == SourceClass::GitState && c.item.payload.is_empty() {
            continue;
        }
        by_fp
            .entry(c.item.content_fingerprint.hex())
            .or_default()
            .push(&c.item);
    }

    let mut selected = Vec::new();
    let mut conflicts = Vec::new();
    let mut consumed = HashSet::new();

    for group in by_fp.values() {
        if group.len() == 1 {
            let item = group[0];
            if consumed.insert(item.id.clone()) {
                selected.push((*item).clone());
            }
            continue;
        }
        // Partition by sensitivity/authority compatibility.
        let mut public_like: Vec<&ContextItem> = Vec::new();
        let mut secret_like: Vec<&ContextItem> = Vec::new();
        for item in group {
            if item.sensitivity >= SensitivityClass::Secret {
                secret_like.push(*item);
            } else {
                public_like.push(*item);
            }
        }
        // Public and secret identical bytes must NOT coalesce (§23.21).
        for _item in public_like.iter().chain(secret_like.iter()) {
            // Within the same sensitivity class, coalesce only when authority/scope compatible.
        }
        // Keep distinct when sensitivity differs.
        if !public_like.is_empty() && !secret_like.is_empty() {
            for item in &secret_like {
                trace.record_exclusion(
                    format!("coalesce-blocked:{}", redact_short(&item.id.0)),
                    TraceReason::ExcludedDuplicateCoalesced,
                    SensitivityClass::Secret,
                );
                // Do not select secret sibling into public coalesced item.
            }
            // Select highest-authority public representative.
            if let Some(winner) = public_like.iter().min_by_key(|i| i.authority as u8) {
                if consumed.insert(winner.id.clone()) {
                    selected.push((*winner).clone());
                }
                for other in &public_like {
                    if other.id != winner.id {
                        trace.record_exclusion(
                            other.id.0.clone(),
                            TraceReason::ExcludedDuplicateCoalesced,
                            other.sensitivity,
                        );
                    }
                }
            }
            // Secret remains distinct (typically excluded from selection by sensitivity
            // ceiling earlier); if present as eligible fixture, keep separate.
            for item in &secret_like {
                if consumed.insert(item.id.clone()) {
                    selected.push((*item).clone());
                }
            }
            continue;
        }

        // Same sensitivity: coalesce only when authority/scope/version compatible.
        let compatible = group.windows(2).all(|w| {
            w[0].authority == w[1].authority
                && w[0].sensitivity == w[1].sensitivity
                && w[0].provenance.repository_id == w[1].provenance.repository_id
                && w[0].provenance.worktree_id == w[1].provenance.worktree_id
        });
        if compatible {
            let winner = group.iter().min_by_key(|i| i.id.0.as_str()).unwrap();
            if consumed.insert(winner.id.clone()) {
                selected.push((*winner).clone());
            }
            for other in group {
                if other.id != winner.id {
                    trace.record_exclusion(
                        other.id.0.clone(),
                        TraceReason::ExcludedDuplicateCoalesced,
                        other.sensitivity,
                    );
                }
            }
        } else {
            // Conflicting authority for same bytes — keep both explainable (§23.22).
            for item in group {
                conflicts.push(item.id.0.clone());
                if consumed.insert(item.id.clone()) {
                    selected.push((*item).clone());
                }
            }
        }
    }

    // Preserve deterministic order by authority then id.
    selected.sort_by(|a, b| {
        (a.authority as u8, a.id.0.as_str()).cmp(&(b.authority as u8, b.id.0.as_str()))
    });
    (selected, conflicts)
}

fn redact_short(raw: &str) -> String {
    digest_bytes(raw.as_bytes()).hex()[..8].to_string()
}

fn admit_with_budget(
    eligible: &[ContextItem],
    max_tokens: u64,
    trace: &mut SelectionTrace,
) -> (Vec<ContextItem>, BundleStatus) {
    let mut mandatory: Vec<&ContextItem> = eligible.iter().filter(|i| i.mandatory).collect();
    let mut optional: Vec<&ContextItem> = eligible.iter().filter(|i| !i.mandatory).collect();
    mandatory.sort_by_key(|i| (i.authority as u8, i.id.0.clone()));
    optional.sort_by_key(|i| (i.authority as u8, i.id.0.clone()));

    let mandatory_cost: u64 = mandatory.iter().map(|i| i.estimated_tokens).sum();
    if mandatory_cost > max_tokens {
        // Never silently truncate mandatory (§23.23).
        return (Vec::new(), BundleStatus::UnableToBuild);
    }

    let mut selected: Vec<ContextItem> = mandatory.iter().map(|i| (*i).clone()).collect();
    let mut used = mandatory_cost;

    for item in optional {
        if used + item.estimated_tokens <= max_tokens {
            used += item.estimated_tokens;
            selected.push((*item).clone());
        } else if item.estimated_tokens > 0 && max_tokens > used {
            // Optional chunking when range contract permits (§23.24–25).
            let remaining = max_tokens - used;
            let byte_budget = remaining.saturating_mul(4);
            if byte_budget > 0 && byte_budget < item.payload.len() as u64 {
                let range = ContentRange {
                    start_byte: 0,
                    end_byte: byte_budget,
                };
                let chunked = (*item).clone().with_range(range);
                used += chunked.estimated_tokens;
                selected.push(chunked);
                trace.record_exclusion(
                    format!("budget-chunk:{}", item.id.0),
                    TraceReason::ExcludedBudget,
                    item.sensitivity,
                );
            } else {
                trace.record_exclusion(
                    item.id.0.clone(),
                    TraceReason::ExcludedBudget,
                    item.sensitivity,
                );
            }
        } else {
            trace.record_exclusion(
                item.id.0.clone(),
                TraceReason::ExcludedBudget,
                item.sensitivity,
            );
        }
    }

    (selected, BundleStatus::Dispatchable)
}
