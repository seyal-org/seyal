//! SPEC-020 V1 soft ranking stage (replaceable under §19).
//!
//! Hard constraints/floors are applied before this module. Candidate-relative
//! min/max normalization is forbidden for winner selection (§9).

use crate::{AdapterId, RouteOfferingId};

use super::baseline::{
    load_verified_baseline, profile_weights, BaselineCalibration, PolicyProfile, SoftFactor,
};
use super::pin::{ResolveFailure, ResolvedTarget};
use crate::lifecycle::SelectionKind;

/// Fixed-point score unit: 1_000_000 = 1.0.
pub type Micros = i64;

pub const SCORE_ONE: Micros = 1_000_000;
/// Ranking-neutral stand-in when the baseline leaves a numeric prior Unknown.
/// Not a claimed provider quality rate (artifact `numeric_quality_prior=Unknown`).
pub const UNKNOWN_SOFT_MID: Micros = SCORE_ONE / 2;
/// Versioned tie epsilon (SPEC-020 §13).
pub const SCORE_EPSILON: Micros = 1_000; // 0.001

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidenceValue {
    /// Known numeric factor in [0, SCORE_ONE].
    Known(Micros),
    /// Explicit Unknown — low confidence, never treated as zero cost/quality claim.
    Unknown,
}

impl EvidenceValue {
    pub fn as_known(self) -> Option<Micros> {
        match self {
            Self::Known(v) => Some(v),
            Self::Unknown => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FactorEvidence {
    pub observed: EvidenceValue,
    pub prior: EvidenceValue,
    pub sample_count: u64,
    /// Fixture-local sample-confidence k. When baseline leaves k Unknown,
    /// callers may supply a fixture-local k without claiming corpus calibration.
    pub sample_confidence_k: u64,
    pub cohort_similarity: Micros,
    pub evidence_freshness: Micros,
    pub provenance_quality: Micros,
    /// Model/harness/provider generation for partition checks (§16).
    pub evidence_generation: u64,
    pub compatible_with_offering_generation: bool,
}

impl FactorEvidence {
    pub fn unknown() -> Self {
        Self {
            observed: EvidenceValue::Unknown,
            prior: EvidenceValue::Unknown,
            sample_count: 0,
            sample_confidence_k: 0,
            cohort_similarity: SCORE_ONE,
            evidence_freshness: SCORE_ONE,
            provenance_quality: SCORE_ONE,
            evidence_generation: 0,
            compatible_with_offering_generation: true,
        }
    }

    pub fn confidence_micros(self) -> Micros {
        if !self.compatible_with_offering_generation {
            return 0;
        }
        let k = self.sample_confidence_k.max(1);
        let n = self.sample_count;
        let sample = ((n as i64) * SCORE_ONE) / ((n + k) as i64);
        let mut conf = sample.saturating_mul(self.cohort_similarity) / SCORE_ONE;
        conf = conf.saturating_mul(self.evidence_freshness) / SCORE_ONE;
        conf = conf.saturating_mul(self.provenance_quality) / SCORE_ONE;
        conf
    }

    pub fn adjusted_micros(self) -> Micros {
        let conf = self.confidence_micros();
        let observed = match self.observed {
            EvidenceValue::Known(v) => v,
            EvidenceValue::Unknown => UNKNOWN_SOFT_MID,
        };
        let prior = match self.prior {
            EvidenceValue::Known(v) => v,
            EvidenceValue::Unknown => UNKNOWN_SOFT_MID,
        };
        // adjusted = confidence * observed + (1-confidence) * prior
        (conf.saturating_mul(observed) / SCORE_ONE)
            + ((SCORE_ONE - conf).saturating_mul(prior) / SCORE_ONE)
    }
}

/// Policy-anchored cost/latency bands (SPEC-020 §9). Not candidate-relative.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DesirabilityBands {
    pub preferred: Micros,
    pub soft_limit: Micros,
    pub hard_cap: Micros,
}

impl DesirabilityBands {
    /// Map raw cost/latency (higher worse) to desirability in [0,1].
    /// Values above hard_cap are ineligible (None).
    pub fn desirability(self, raw: EvidenceValue) -> Option<Micros> {
        let Some(value) = raw.as_known() else {
            // Unknown is not zero; conservative soft-limit desirability.
            return Some(SCORE_ONE / 4);
        };
        if value > self.hard_cap {
            return None;
        }
        if value <= self.preferred {
            return Some(SCORE_ONE);
        }
        if value <= self.soft_limit {
            let span = (self.soft_limit - self.preferred).max(1);
            let over = value - self.preferred;
            return Some(SCORE_ONE - (over * (SCORE_ONE / 2) / span));
        }
        let span = (self.hard_cap - self.soft_limit).max(1);
        let over = value - self.soft_limit;
        Some((SCORE_ONE / 2) - (over * (SCORE_ONE / 2) / span))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdequacyFloors {
    pub quality: Option<Micros>,
    pub reliability: Option<Micros>,
    pub context_fit: Option<Micros>,
    pub tooling_fit: Option<Micros>,
    /// When true, Unknown evidence may satisfy a floor via UNKNOWN_SOFT_MID.
    pub allow_unknown_prior_for_floors: bool,
}

impl AdequacyFloors {
    pub fn none() -> Self {
        Self {
            quality: None,
            reliability: None,
            context_fit: None,
            tooling_fit: None,
            allow_unknown_prior_for_floors: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RankingCandidate {
    pub adapter_id: AdapterId,
    pub route_offering_id: RouteOfferingId,
    pub adapter_enabled: bool,
    pub adapter_manifest_generation: u64,
    pub hard_constraint_satisfied: bool,
    /// Stable lexicographic key for final tie-break (§13.7).
    pub stable_key: u64,
    pub preference_rank: u32,
    pub quality: FactorEvidence,
    pub context_fit: FactorEvidence,
    pub tooling_fit: FactorEvidence,
    pub reliability: FactorEvidence,
    /// Direct expected cost (micros of currency units). Unknown ≠ 0.
    pub direct_expected_cost: EvidenceValue,
    /// Expected total cost including bounded fallback (§10).
    pub expected_total_cost: EvidenceValue,
    pub expected_latency: EvidenceValue,
    pub locality: FactorEvidence,
    pub health_reliability: Micros,
    pub model_generation: u64,
    /// Enforcement class for network/harness hard checks.
    pub network_enforcement_ok: bool,
    pub no_network_required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RankingRequest {
    pub profile: PolicyProfile,
    pub floors: AdequacyFloors,
    pub cost_bands: DesirabilityBands,
    pub latency_bands: DesirabilityBands,
    pub local_learning_enabled: bool,
    /// Required evidence bindings; empty means assessment complete for text-only.
    pub required_capability_bits: u64,
    pub bound_evidence_bits: u64,
    /// When true, missing required evidence must not silently become text-only.
    pub refuse_silent_text_only: bool,
}

impl RankingRequest {
    pub fn cold_start(profile: PolicyProfile) -> Self {
        Self {
            profile,
            floors: AdequacyFloors::none(),
            cost_bands: DesirabilityBands {
                preferred: 100,
                soft_limit: 500,
                hard_cap: 10_000,
            },
            latency_bands: DesirabilityBands {
                preferred: 100,
                soft_limit: 1_000,
                hard_cap: 10_000,
            },
            local_learning_enabled: false,
            required_capability_bits: 0,
            bound_evidence_bits: 0,
            refuse_silent_text_only: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FactorBreakdown {
    pub factor: SoftFactor,
    pub raw_adjusted: Micros,
    pub weight_micros: Micros,
    pub contribution: Micros,
    pub confidence: Micros,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidateExplanation {
    pub route_offering_id: RouteOfferingId,
    pub eligible: bool,
    pub exclusion_reason: Option<&'static str>,
    pub score: Micros,
    pub factors: Vec<FactorBreakdown>,
    pub expected_total_cost: EvidenceValue,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RankingExplanation {
    pub baseline_artifact_id: &'static str,
    pub baseline_sha256: &'static str,
    pub profile: PolicyProfile,
    pub winner_score: Micros,
    pub candidates: Vec<CandidateExplanation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RankedResolution {
    pub target: ResolvedTarget,
    pub explanation: RankingExplanation,
}

fn floor_value(ev: &FactorEvidence, allow_unknown: bool) -> Option<Micros> {
    match ev.observed {
        EvidenceValue::Known(_) | EvidenceValue::Unknown
            if ev.compatible_with_offering_generation =>
        {
            let adj = ev.adjusted_micros();
            if matches!(ev.observed, EvidenceValue::Unknown)
                && matches!(ev.prior, EvidenceValue::Unknown)
                && !allow_unknown
            {
                return None;
            }
            Some(adj)
        }
        _ => None,
    }
}

fn passes_floors(c: &RankingCandidate, floors: &AdequacyFloors) -> Result<(), &'static str> {
    if let Some(min_q) = floors.quality {
        match floor_value(&c.quality, floors.allow_unknown_prior_for_floors) {
            Some(v) if v >= min_q => {}
            _ => return Err("quality_floor"),
        }
    }
    if let Some(min_r) = floors.reliability {
        match floor_value(&c.reliability, floors.allow_unknown_prior_for_floors) {
            Some(v) if v >= min_r => {}
            _ => return Err("reliability_floor"),
        }
    }
    if let Some(min_c) = floors.context_fit {
        match floor_value(&c.context_fit, floors.allow_unknown_prior_for_floors) {
            Some(v) if v >= min_c => {}
            _ => return Err("context_floor"),
        }
    }
    if let Some(min_t) = floors.tooling_fit {
        match floor_value(&c.tooling_fit, floors.allow_unknown_prior_for_floors) {
            Some(v) if v >= min_t => {}
            _ => return Err("tooling_floor"),
        }
    }
    Ok(())
}

fn score_candidate(
    c: &RankingCandidate,
    request: &RankingRequest,
    weights: super::baseline::ProfileWeights,
) -> Result<(Micros, Vec<FactorBreakdown>, Micros), &'static str> {
    if !c.adapter_enabled {
        return Err("adapter_disabled");
    }
    if !c.hard_constraint_satisfied {
        return Err("hard_constraint");
    }
    if request.refuse_silent_text_only
        && request.required_capability_bits != 0
        && (request.bound_evidence_bits & request.required_capability_bits)
            != request.required_capability_bits
    {
        // Missing required evidence: offering may still be scored only if it
        // can deliver bound raw evidence for every known hard requirement.
        // Text-only offerings without the bits are excluded.
        return Err("missing_required_evidence");
    }
    if c.no_network_required && !c.network_enforcement_ok {
        return Err("no_network_unenforced");
    }
    passes_floors(c, &request.floors)?;

    let k_des = request
        .cost_bands
        .desirability(c.expected_total_cost)
        .ok_or("cost_hard_cap")?;
    let l_des = request
        .latency_bands
        .desirability(c.expected_latency)
        .ok_or("latency_hard_cap")?;

    let q = c.quality.adjusted_micros();
    let ctx = c.context_fit.adjusted_micros();
    let t = c.tooling_fit.adjusted_micros();
    let r = c.reliability.adjusted_micros();
    let p = c.locality.adjusted_micros();

    let adjusted = [
        (SoftFactor::Q, q, c.quality.confidence_micros()),
        (SoftFactor::C, ctx, c.context_fit.confidence_micros()),
        (SoftFactor::T, t, c.tooling_fit.confidence_micros()),
        (SoftFactor::R, r, c.reliability.confidence_micros()),
        (SoftFactor::K, k_des, SCORE_ONE),
        (SoftFactor::L, l_des, SCORE_ONE),
        (SoftFactor::P, p, c.locality.confidence_micros()),
    ];

    let mut factors = Vec::with_capacity(7);
    let mut score = 0i64;
    for (factor, adj, conf) in adjusted {
        let w = weights.weight_micros(factor);
        let contribution = w.saturating_mul(adj) / SCORE_ONE;
        score = score.saturating_add(contribution);
        factors.push(FactorBreakdown {
            factor,
            raw_adjusted: adj,
            weight_micros: w,
            contribution,
            confidence: conf,
        });
    }
    Ok((score, factors, q))
}

fn min_factor_confidence(factors: &[FactorBreakdown]) -> Micros {
    factors.iter().map(|f| f.confidence).min().unwrap_or(0)
}

fn quality_confidence(factors: &[FactorBreakdown]) -> Micros {
    factors
        .iter()
        .find(|f| f.factor == SoftFactor::Q)
        .map(|f| f.confidence)
        .unwrap_or(0)
}

fn cost_key(v: EvidenceValue) -> (u8, Micros) {
    match v {
        EvidenceValue::Known(c) => (0, c),
        EvidenceValue::Unknown => (1, Micros::MAX),
    }
}

fn latency_key(v: EvidenceValue) -> (u8, Micros) {
    cost_key(v)
}

/// Deterministic V1 ranking among already hard-filtered candidates.
pub fn rank_v1(
    candidates: &[RankingCandidate],
    request: &RankingRequest,
    baseline: &BaselineCalibration,
) -> Result<RankedResolution, ResolveFailure> {
    if baseline.artifact_sha256 != super::baseline::BASELINE_ARTIFACT_SHA256
        || baseline.artifact_id != super::baseline::BASELINE_ARTIFACT_ID
    {
        return Err(ResolveFailure::BaselineIntegrity);
    }
    // Cold-start / learning-disabled parity: both bind the same artifact.
    if !request.local_learning_enabled && !baseline.learning_disabled_uses_artifact {
        return Err(ResolveFailure::BaselineIntegrity);
    }

    #[derive(Clone, Copy)]
    struct ScoredRow {
        score: Micros,
        min_conf: Micros,
        q_conf: Micros,
        health: Micros,
        cost: (u8, Micros),
        latency: (u8, Micros),
        preference_rank: u32,
        stable_key: u64,
        idx: usize,
    }

    let weights = profile_weights(request.profile);
    let mut explanations = Vec::with_capacity(candidates.len());
    let mut scored: Vec<ScoredRow> = Vec::new();

    for (idx, c) in candidates.iter().enumerate() {
        match score_candidate(c, request, weights) {
            Ok((score, factors, _q)) => {
                let min_conf = min_factor_confidence(&factors);
                let q_conf = quality_confidence(&factors);
                explanations.push(CandidateExplanation {
                    route_offering_id: c.route_offering_id,
                    eligible: true,
                    exclusion_reason: None,
                    score,
                    factors,
                    expected_total_cost: c.expected_total_cost,
                });
                scored.push(ScoredRow {
                    score,
                    min_conf,
                    q_conf,
                    health: c.health_reliability,
                    cost: cost_key(c.expected_total_cost),
                    latency: latency_key(c.expected_latency),
                    preference_rank: c.preference_rank,
                    stable_key: c.stable_key,
                    idx,
                });
            }
            Err(reason) => {
                explanations.push(CandidateExplanation {
                    route_offering_id: c.route_offering_id,
                    eligible: false,
                    exclusion_reason: Some(reason),
                    score: 0,
                    factors: Vec::new(),
                    expected_total_cost: c.expected_total_cost,
                });
            }
        }
    }

    if scored.is_empty() {
        return Err(ResolveFailure::NoRoute);
    }

    // Group against top score within epsilon, then apply §13 tie-breaks.
    let top = scored.iter().map(|s| s.score).max().unwrap();
    scored.retain(|s| top - s.score <= SCORE_EPSILON);
    scored.sort_by(|a, b| {
        a.preference_rank
            .cmp(&b.preference_rank)
            .then_with(|| b.min_conf.cmp(&a.min_conf))
            .then_with(|| b.q_conf.cmp(&a.q_conf))
            .then_with(|| a.cost.cmp(&b.cost))
            .then_with(|| a.latency.cmp(&b.latency))
            .then_with(|| b.health.cmp(&a.health))
            .then_with(|| a.stable_key.cmp(&b.stable_key))
            .then_with(|| a.idx.cmp(&b.idx))
    });

    let winner_idx = scored[0].idx;
    let winner = &candidates[winner_idx];
    let winner_score = scored[0].score;
    Ok(RankedResolution {
        target: ResolvedTarget {
            adapter_id: winner.adapter_id,
            route_offering_id: winner.route_offering_id,
            adapter_manifest_generation: winner.adapter_manifest_generation,
            selection_kind: SelectionKind::RouterV1,
        },
        explanation: RankingExplanation {
            baseline_artifact_id: baseline.artifact_id,
            baseline_sha256: baseline.artifact_sha256,
            profile: request.profile,
            winner_score,
            candidates: explanations,
        },
    })
}

/// Full envelope: hard pin → hard-eligible set → floors/soft rank → decision.
pub fn resolve_with_v1_ranking(
    pin: Option<RouteOfferingId>,
    candidates: &[RankingCandidate],
    request: &RankingRequest,
) -> Result<RankedResolution, ResolveFailure> {
    let baseline = load_verified_baseline().map_err(|_| ResolveFailure::BaselineIntegrity)?;

    if let Some(pinned) = pin {
        let candidate = candidates
            .iter()
            .find(|c| c.route_offering_id == pinned)
            .ok_or(ResolveFailure::TargetUnavailable)?;
        if !candidate.hard_constraint_satisfied {
            return Err(ResolveFailure::TargetUnavailable);
        }
        if !candidate.adapter_enabled {
            return Err(ResolveFailure::AdapterDisabled {
                adapter_id: candidate.adapter_id,
            });
        }
        // Pin wins even if soft ranking would prefer another eligible offering.
        let explanation = rank_v1(candidates, request, &baseline)
            .map(|r| r.explanation)
            .unwrap_or(RankingExplanation {
                baseline_artifact_id: baseline.artifact_id,
                baseline_sha256: baseline.artifact_sha256,
                profile: request.profile,
                winner_score: 0,
                candidates: Vec::new(),
            });
        return Ok(RankedResolution {
            target: ResolvedTarget {
                adapter_id: candidate.adapter_id,
                route_offering_id: candidate.route_offering_id,
                adapter_manifest_generation: candidate.adapter_manifest_generation,
                selection_kind: SelectionKind::Pinned,
            },
            explanation,
        });
    }

    // Rank over the full candidate snapshot so hard exclusions remain visible
    // in explanations; score_candidate rejects hard misses before soft weights.
    let eligible_count = candidates
        .iter()
        .filter(|c| c.adapter_enabled && c.hard_constraint_satisfied)
        .count();
    match eligible_count {
        0 => Err(ResolveFailure::NoRoute),
        1 => rank_v1(candidates, request, &baseline).map(|mut r| {
            r.target.selection_kind = SelectionKind::Singleton;
            r
        }),
        _ => rank_v1(candidates, request, &baseline),
    }
}

/// Expected total cost recursion helper (SPEC-020 §10). Bounded by `depth`.
pub fn expected_total_cost_micros(
    direct: EvidenceValue,
    fallback_prob_micros: Micros,
    fallback_cost: EvidenceValue,
    depth: u32,
) -> EvidenceValue {
    if depth == 0 {
        return direct;
    }
    match (direct, fallback_cost) {
        (EvidenceValue::Unknown, _) | (_, EvidenceValue::Unknown) => EvidenceValue::Unknown,
        (EvidenceValue::Known(d), EvidenceValue::Known(f)) => {
            let add = fallback_prob_micros.saturating_mul(f) / SCORE_ONE;
            EvidenceValue::Known(d.saturating_add(add))
        }
    }
}
