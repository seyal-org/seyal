//! Agent Backend routing envelope (SPEC-020 / SPEC-027 §4).
//!
//! One envelope only:
//!
//! ```text
//! hard policy / pin / allow / deny
//!         ↓
//! eligible RouteOfferings
//!         ↓
//! replaceable V1 soft ranking (§19)
//!         ↓
//! immutable RoutingDecision (Pinned | Singleton | RouterV1)
//! ```
//!
//! There is no second router process or competing decision writer. Soft ranking
//! is a replaceable stage under SPEC-020 §19. Pin/allow/deny stay hard.

mod baseline;
mod budget;
mod failure;
mod pin;
mod score;
pub(crate) mod sha256;

pub use baseline::{
    baseline_artifact_toml, cold_start_baseline_matches, load_verified_baseline, profile_weights,
    BaselineCalibration, BaselineError, PolicyProfile, ProfileWeights, SoftFactor,
    BASELINE_ARTIFACT_ID, BASELINE_ARTIFACT_SHA256, FORBIDDEN_SYNTHETIC_POC_SHA256,
};
pub use budget::{admit_fallback, BudgetDecision, BudgetScope};
pub use failure::{
    fallback_action, may_replay_external_mutation, policy_denial_may_select, FailureClass,
    FallbackAction,
};
pub use pin::{
    resolve_execution_target, resolve_pin_or_singleton, AdapterCandidate, ResolveFailure,
    ResolvedTarget,
};
pub use score::{
    expected_total_cost_micros, rank_v1, resolve_with_v1_ranking, AdequacyFloors,
    CandidateExplanation, DesirabilityBands, EvidenceValue, FactorBreakdown, FactorEvidence,
    Micros, RankedResolution, RankingCandidate, RankingExplanation, RankingRequest, SCORE_EPSILON,
    SCORE_ONE, UNKNOWN_SOFT_MID,
};
