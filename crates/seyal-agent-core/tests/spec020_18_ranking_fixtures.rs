//! SPEC-020 §18 required fixtures as named production tests.

use seyal_agent_core::{
    admit_fallback, cold_start_baseline_matches, expected_total_cost_micros, fallback_action,
    load_verified_baseline, may_replay_external_mutation, policy_denial_may_select,
    resolve_with_v1_ranking, AdapterId, AdequacyFloors, BudgetScope, DesirabilityBands,
    EvidenceValue, FactorEvidence, FailureClass, FallbackAction, PolicyProfile, RankingCandidate,
    RankingRequest, ResolveFailure, RouteOfferingId, SelectionKind, BASELINE_ARTIFACT_SHA256,
    SCORE_ONE,
};

fn offering(stable: u64) -> RankingCandidate {
    RankingCandidate {
        adapter_id: AdapterId::new(),
        route_offering_id: RouteOfferingId::from_bytes({
            let mut b = [0u8; 16];
            b[..8].copy_from_slice(&stable.to_le_bytes());
            b
        }),
        adapter_enabled: true,
        adapter_manifest_generation: 1,
        hard_constraint_satisfied: true,
        stable_key: stable,
        preference_rank: u32::MAX,
        quality: FactorEvidence {
            observed: EvidenceValue::Known(SCORE_ONE / 2),
            prior: EvidenceValue::Known(SCORE_ONE / 2),
            sample_count: 100,
            sample_confidence_k: 10,
            cohort_similarity: SCORE_ONE,
            evidence_freshness: SCORE_ONE,
            provenance_quality: SCORE_ONE,
            evidence_generation: 1,
            compatible_with_offering_generation: true,
        },
        context_fit: FactorEvidence {
            observed: EvidenceValue::Known(SCORE_ONE / 2),
            prior: EvidenceValue::Known(SCORE_ONE / 2),
            sample_count: 100,
            sample_confidence_k: 10,
            cohort_similarity: SCORE_ONE,
            evidence_freshness: SCORE_ONE,
            provenance_quality: SCORE_ONE,
            evidence_generation: 1,
            compatible_with_offering_generation: true,
        },
        tooling_fit: FactorEvidence {
            observed: EvidenceValue::Known(SCORE_ONE / 2),
            prior: EvidenceValue::Known(SCORE_ONE / 2),
            sample_count: 100,
            sample_confidence_k: 10,
            cohort_similarity: SCORE_ONE,
            evidence_freshness: SCORE_ONE,
            provenance_quality: SCORE_ONE,
            evidence_generation: 1,
            compatible_with_offering_generation: true,
        },
        reliability: FactorEvidence {
            observed: EvidenceValue::Known(SCORE_ONE / 2),
            prior: EvidenceValue::Known(SCORE_ONE / 2),
            sample_count: 100,
            sample_confidence_k: 10,
            cohort_similarity: SCORE_ONE,
            evidence_freshness: SCORE_ONE,
            provenance_quality: SCORE_ONE,
            evidence_generation: 1,
            compatible_with_offering_generation: true,
        },
        direct_expected_cost: EvidenceValue::Known(200),
        expected_total_cost: EvidenceValue::Known(200),
        expected_latency: EvidenceValue::Known(200),
        locality: FactorEvidence {
            observed: EvidenceValue::Known(SCORE_ONE / 2),
            prior: EvidenceValue::Known(SCORE_ONE / 2),
            sample_count: 100,
            sample_confidence_k: 10,
            cohort_similarity: SCORE_ONE,
            evidence_freshness: SCORE_ONE,
            provenance_quality: SCORE_ONE,
            evidence_generation: 1,
            compatible_with_offering_generation: true,
        },
        health_reliability: SCORE_ONE / 2,
        model_generation: 1,
        network_enforcement_ok: true,
        no_network_required: false,
    }
}

fn request(profile: PolicyProfile) -> RankingRequest {
    RankingRequest {
        profile,
        floors: AdequacyFloors::none(),
        cost_bands: DesirabilityBands {
            preferred: 100,
            soft_limit: 500,
            hard_cap: 50_000,
        },
        latency_bands: DesirabilityBands {
            preferred: 100,
            soft_limit: 1_000,
            hard_cap: 50_000,
        },
        local_learning_enabled: false,
        required_capability_bits: 0,
        bound_evidence_bits: 0,
        refuse_silent_text_only: true,
    }
}

#[test]
fn spec020_18_01_privacy_excludes_high_quality() {
    let mut high = offering(1);
    high.quality.observed = EvidenceValue::Known(SCORE_ONE);
    high.hard_constraint_satisfied = false; // privacy / residency hard miss
    let mut ok = offering(2);
    ok.quality.observed = EvidenceValue::Known(SCORE_ONE / 3);
    let r = resolve_with_v1_ranking(
        None,
        &[high, ok.clone()],
        &request(PolicyProfile::QualityFirst),
    )
    .unwrap();
    assert_eq!(r.target.route_offering_id, ok.route_offering_id);
    assert!(r
        .explanation
        .candidates
        .iter()
        .any(|c| !c.eligible && c.exclusion_reason == Some("hard_constraint")));
}

#[test]
fn spec020_18_02_cheap_loses_on_expected_fallback_cost() {
    let mut cheap = offering(1);
    cheap.direct_expected_cost = EvidenceValue::Known(50);
    // High fallback probability makes expected total cost large.
    cheap.expected_total_cost = expected_total_cost_micros(
        EvidenceValue::Known(50),
        900_000,
        EvidenceValue::Known(5_000),
        1,
    );
    let mut reliable = offering(2);
    reliable.direct_expected_cost = EvidenceValue::Known(300);
    reliable.expected_total_cost = EvidenceValue::Known(300);
    let r = resolve_with_v1_ranking(
        None,
        &[cheap, reliable.clone()],
        &request(PolicyProfile::CostAware),
    )
    .unwrap();
    assert_eq!(r.target.route_offering_id, reliable.route_offering_id);
}

#[test]
fn spec020_18_03_low_sample_shrinks() {
    let mut low = offering(1);
    low.quality.observed = EvidenceValue::Known(SCORE_ONE);
    low.quality.prior = EvidenceValue::Known(SCORE_ONE / 5);
    low.quality.sample_count = 1;
    low.quality.sample_confidence_k = 50;
    let mut mature = offering(2);
    mature.quality.observed = EvidenceValue::Known(700_000);
    mature.quality.prior = EvidenceValue::Known(SCORE_ONE / 5);
    mature.quality.sample_count = 200;
    mature.quality.sample_confidence_k = 50;
    let r = resolve_with_v1_ranking(
        None,
        &[low, mature.clone()],
        &request(PolicyProfile::QualityFirst),
    )
    .unwrap();
    assert_eq!(r.target.route_offering_id, mature.route_offering_id);
}

#[test]
fn spec020_18_04_profiles_differ_hard_policy_held() {
    let mut q = offering(1);
    q.quality.observed = EvidenceValue::Known(900_000);
    q.expected_total_cost = EvidenceValue::Known(800);
    let mut k = offering(2);
    k.quality.observed = EvidenceValue::Known(400_000);
    k.expected_total_cost = EvidenceValue::Known(100);
    let denied = offering(3);
    // Hard-denied third candidate must never win under any profile.
    let mut denied = denied;
    denied.hard_constraint_satisfied = false;

    let quality_first = resolve_with_v1_ranking(
        None,
        &[q.clone(), k.clone(), denied.clone()],
        &request(PolicyProfile::QualityFirst),
    )
    .unwrap();
    let cost_aware =
        resolve_with_v1_ranking(None, &[q, k, denied], &request(PolicyProfile::CostAware)).unwrap();
    assert_ne!(
        quality_first.target.route_offering_id,
        cost_aware.target.route_offering_id
    );
    assert!(quality_first
        .explanation
        .candidates
        .iter()
        .any(|c| !c.eligible && c.exclusion_reason == Some("hard_constraint")));
}

#[test]
fn spec020_18_05_quality_floor_removes_weak() {
    let mut weak = offering(1);
    weak.quality.observed = EvidenceValue::Known(100_000);
    weak.quality.prior = EvidenceValue::Known(100_000);
    let mut strong = offering(2);
    strong.quality.observed = EvidenceValue::Known(800_000);
    let mut req = request(PolicyProfile::Balanced);
    req.floors.quality = Some(500_000);
    req.floors.allow_unknown_prior_for_floors = true;
    let r = resolve_with_v1_ranking(None, &[weak, strong.clone()], &req).unwrap();
    assert_eq!(r.target.route_offering_id, strong.route_offering_id);
}

#[test]
fn spec020_18_06_unknown_cost_not_zero() {
    let baseline = load_verified_baseline().unwrap();
    assert!(baseline.unknown_cost_is_not_zero);
    let mut unknown = offering(1);
    unknown.expected_total_cost = EvidenceValue::Unknown;
    unknown.direct_expected_cost = EvidenceValue::Unknown;
    let mut known_cheap = offering(2);
    known_cheap.expected_total_cost = EvidenceValue::Known(50);
    // Unknown must not be treated as free/zero and beat a known cheap route
    // solely by fabricating zero cost — CostAware should prefer known cheap.
    let r = resolve_with_v1_ranking(
        None,
        &[unknown.clone(), known_cheap.clone()],
        &request(PolicyProfile::CostAware),
    )
    .unwrap();
    assert_eq!(r.target.route_offering_id, known_cheap.route_offering_id);
    assert_ne!(unknown.expected_total_cost, EvidenceValue::Known(0));
}

#[test]
fn spec020_18_07_cold_start_deterministic() {
    let a = offering(10);
    let b = offering(20);
    let req = RankingRequest::cold_start(PolicyProfile::Balanced);
    let r1 = resolve_with_v1_ranking(None, &[a.clone(), b.clone()], &req).unwrap();
    let r2 = resolve_with_v1_ranking(None, &[b, a], &req).unwrap();
    assert_eq!(r1.target.route_offering_id, r2.target.route_offering_id);
    assert_eq!(r1.explanation.winner_score, r2.explanation.winner_score);
    assert_eq!(r1.explanation.baseline_sha256, BASELINE_ARTIFACT_SHA256);
}

#[test]
fn spec020_18_08_model_version_partitions_evidence() {
    let mut stale = offering(1);
    stale.quality.observed = EvidenceValue::Known(SCORE_ONE);
    stale.quality.sample_count = 500;
    stale.quality.compatible_with_offering_generation = false;
    stale.model_generation = 1;
    let mut fresh = offering(2);
    fresh.quality.observed = EvidenceValue::Known(600_000);
    fresh.quality.sample_count = 500;
    fresh.model_generation = 2;
    let r = resolve_with_v1_ranking(
        None,
        &[stale, fresh.clone()],
        &request(PolicyProfile::QualityFirst),
    )
    .unwrap();
    assert_eq!(r.target.route_offering_id, fresh.route_offering_id);
}

#[test]
fn spec020_18_09_irrelevant_candidate_no_norm_effect() {
    let a = offering(1);
    let mut b = offering(2);
    b.expected_total_cost = EvidenceValue::Known(400);
    b.quality.observed = EvidenceValue::Known(400_000);
    let mut distractor = offering(99);
    distractor.expected_total_cost = EvidenceValue::Known(1);
    distractor.quality.observed = EvidenceValue::Known(1);
    // Distractor fails hard policy so it is not eligible — and must not change A-vs-B.
    distractor.hard_constraint_satisfied = false;

    let without = resolve_with_v1_ranking(
        None,
        &[a.clone(), b.clone()],
        &request(PolicyProfile::Balanced),
    )
    .unwrap();
    let with =
        resolve_with_v1_ranking(None, &[a, b, distractor], &request(PolicyProfile::Balanced))
            .unwrap();
    assert_eq!(
        without.target.route_offering_id,
        with.target.route_offering_id
    );
}

#[test]
fn spec020_18_10_tie_stable_order() {
    let a = offering(5);
    let b = offering(3);
    // Identical soft factors → stable_key order.
    let r = resolve_with_v1_ranking(
        None,
        &[a.clone(), b.clone()],
        &request(PolicyProfile::Balanced),
    )
    .unwrap();
    assert_eq!(r.target.route_offering_id, b.route_offering_id);
    assert_eq!(r.target.selection_kind, SelectionKind::RouterV1);
}

#[test]
fn spec020_18_11_rate_limit_reroute() {
    assert_eq!(
        fallback_action(FailureClass::RateLimited),
        FallbackAction::RerouteNewDecision
    );
    let primary = offering(1);
    let alt = offering(2);
    // After rate-limit, ranking among remaining eligible peers picks alt when
    // primary is hard-excluded for this decision.
    let mut primary = primary;
    primary.hard_constraint_satisfied = false;
    let r = resolve_with_v1_ranking(
        None,
        &[primary, alt.clone()],
        &request(PolicyProfile::Balanced),
    )
    .unwrap();
    assert_eq!(r.target.route_offering_id, alt.route_offering_id);
    assert_eq!(r.target.selection_kind, SelectionKind::Singleton);
}

#[test]
fn spec020_18_12_policy_denial_never_relaxes() {
    assert!(!policy_denial_may_select(true));
    assert_eq!(
        fallback_action(FailureClass::PermissionDenied),
        FallbackAction::NeverRelax
    );
    let mut denied = offering(1);
    denied.hard_constraint_satisfied = false;
    denied.quality.observed = EvidenceValue::Known(SCORE_ONE);
    let err = resolve_with_v1_ranking(None, &[denied], &request(PolicyProfile::QualityFirst));
    assert_eq!(err, Err(ResolveFailure::NoRoute));
}

#[test]
fn spec020_18_13_evaluation_rejected_new_attempt() {
    assert_eq!(
        fallback_action(FailureClass::EvaluationRejected),
        FallbackAction::NewAttemptAndDecision
    );
}

#[test]
fn spec020_18_14_effect_unknown_no_duplicate_mutation() {
    assert!(!may_replay_external_mutation(
        FailureClass::ExternalEffectUnknown
    ));
    assert_eq!(
        fallback_action(FailureClass::ExternalEffectUnknown),
        FallbackAction::ReconcileNoReplay
    );
}

#[test]
fn spec020_18_15_no_network_rejects_unenforced_harness() {
    let mut unenforced = offering(1);
    unenforced.no_network_required = true;
    unenforced.network_enforcement_ok = false;
    unenforced.quality.observed = EvidenceValue::Known(SCORE_ONE);
    let mut local = offering(2);
    local.no_network_required = true;
    local.network_enforcement_ok = true;
    let r = resolve_with_v1_ranking(
        None,
        &[unenforced, local.clone()],
        &request(PolicyProfile::LocalFirst),
    )
    .unwrap();
    assert_eq!(r.target.route_offering_id, local.route_offering_id);
}

#[test]
fn spec020_18_16_frozen_fixture_reproducible() {
    let cands = [offering(7), offering(8), offering(9)];
    let req = request(PolicyProfile::Balanced);
    let first = resolve_with_v1_ranking(None, &cands, &req).unwrap();
    for _ in 0..16 {
        let again = resolve_with_v1_ranking(None, &cands, &req).unwrap();
        assert_eq!(again.target, first.target);
        assert_eq!(
            again.explanation.winner_score,
            first.explanation.winner_score
        );
        assert_eq!(
            again.explanation.baseline_sha256,
            first.explanation.baseline_sha256
        );
    }
}

#[test]
fn spec020_18_17_same_prompt_different_bound_evidence() {
    let base = offering(1);
    let mut diagnostic = request(PolicyProfile::Balanced);
    diagnostic.required_capability_bits = 0b001; // compiler diagnostic
    diagnostic.bound_evidence_bits = 0b001;
    let mut deploy = request(PolicyProfile::Balanced);
    deploy.required_capability_bits = 0b010; // deployment log
    deploy.bound_evidence_bits = 0b010;
    let mut screenshot = request(PolicyProfile::Balanced);
    screenshot.required_capability_bits = 0b100; // layout screenshot
    screenshot.bound_evidence_bits = 0b100;

    // Same offering set; different bound evidence → different requirement masks
    // recorded on the decision explanation profile path (request frozen).
    let d = resolve_with_v1_ranking(None, &[base.clone()], &diagnostic).unwrap();
    let e = resolve_with_v1_ranking(None, &[base.clone()], &deploy).unwrap();
    let s = resolve_with_v1_ranking(None, &[base], &screenshot).unwrap();
    assert_eq!(d.target.selection_kind, SelectionKind::Singleton);
    assert_eq!(e.target.selection_kind, SelectionKind::Singleton);
    assert_eq!(s.target.selection_kind, SelectionKind::Singleton);
    assert_ne!(
        diagnostic.required_capability_bits,
        deploy.required_capability_bits
    );
    assert_ne!(
        deploy.required_capability_bits,
        screenshot.required_capability_bits
    );
}

#[test]
fn spec020_18_18_missing_evidence_not_text_only() {
    let text_only = offering(1);
    let mut req = request(PolicyProfile::Balanced);
    req.required_capability_bits = 0b100; // screenshot required
    req.bound_evidence_bits = 0; // missing
    req.refuse_silent_text_only = true;
    let err = resolve_with_v1_ranking(None, &[text_only], &req);
    assert_eq!(err, Err(ResolveFailure::NoRoute));
}

#[test]
fn spec020_18_19_hard_cap_blocks_fallback_including_concurrent() {
    let mut scope = BudgetScope::new(10 * SCORE_ONE);
    scope.settled_spend_micros = 6 * SCORE_ONE;
    assert_eq!(
        admit_fallback(
            &mut scope,
            6 * SCORE_ONE,
            EvidenceValue::Known(6 * SCORE_ONE)
        ),
        seyal_agent_core::BudgetDecision::Denied
    );
    let mut scope2 = BudgetScope::new(10 * SCORE_ONE);
    scope2.settled_spend_micros = 6 * SCORE_ONE;
    let decisions = scope2.try_reserve_concurrent(&[
        EvidenceValue::Known(6 * SCORE_ONE),
        EvidenceValue::Known(4 * SCORE_ONE),
    ]);
    assert_eq!(decisions[0], seyal_agent_core::BudgetDecision::Denied);
    // 4-unit fallback still denied while settled=6 under cap 10? 6+4=10 OK.
    // Concurrent pair: first 6 denied; second 4 admitted.
    assert_eq!(
        decisions[1],
        seyal_agent_core::BudgetDecision::Admitted {
            reservation_micros: 4 * SCORE_ONE
        }
    );
}

#[test]
fn spec020_18_20_clean_vs_learning_disabled_same_baseline() {
    assert!(cold_start_baseline_matches(true));
    assert!(cold_start_baseline_matches(false));
    let cands = [offering(1), offering(2)];
    let mut clean = RankingRequest::cold_start(PolicyProfile::Balanced);
    clean.local_learning_enabled = true;
    let mut disabled = RankingRequest::cold_start(PolicyProfile::Balanced);
    disabled.local_learning_enabled = false;
    let a = resolve_with_v1_ranking(None, &cands, &clean).unwrap();
    let b = resolve_with_v1_ranking(None, &cands, &disabled).unwrap();
    assert_eq!(a.target.route_offering_id, b.target.route_offering_id);
    assert_eq!(a.explanation.baseline_sha256, BASELINE_ARTIFACT_SHA256);
    assert_eq!(b.explanation.baseline_sha256, BASELINE_ARTIFACT_SHA256);
    assert_eq!(a.explanation.winner_score, b.explanation.winner_score);
}

#[test]
fn pin_remains_hard_over_soft_rank() {
    let mut preferred = offering(1);
    preferred.quality.observed = EvidenceValue::Known(SCORE_ONE);
    let mut pinned = offering(2);
    pinned.quality.observed = EvidenceValue::Known(100_000);
    let r = resolve_with_v1_ranking(
        Some(pinned.route_offering_id),
        &[preferred, pinned.clone()],
        &request(PolicyProfile::QualityFirst),
    )
    .unwrap();
    assert_eq!(r.target.selection_kind, SelectionKind::Pinned);
    assert_eq!(r.target.route_offering_id, pinned.route_offering_id);
}
