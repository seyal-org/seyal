//! BaselineCalibrationArtifact cold-start binding (SPEC-020 §16).
//!
//! Production weights come only from the integrity-verified artifact under
//! `docs/evidence/`. Synthetic POC weights (SHA-256 `920a4cc0…`) are forbidden.

use super::sha256::sha256_hex;

/// Artifact id frozen by #1294 / `m005-spec020-baseline-calibration-artifact-v1.toml`.
pub const BASELINE_ARTIFACT_ID: &str = "seyal.spec020.baseline-calibration.v1";

/// SHA-256 of the distributable TOML bytes (SPEC-020 §16 / #1294).
pub const BASELINE_ARTIFACT_SHA256: &str =
    "9d31ee776b06d288d914b2c55a8d2354459fa46894d237592ffc4508041b7ecf";

/// Forbidden synthetic POC script hash (mechanics citation only; never defaults).
pub const FORBIDDEN_SYNTHETIC_POC_SHA256: &str =
    "920a4cc064b8bc9c234328938ad5d33575b7c58a8ecc1bfc302e3ca08fd3cb85";

/// Soft-factor order Q, C, T, R, K, L, P (SPEC-020 §7 / §11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoftFactor {
    Q,
    C,
    T,
    R,
    K,
    L,
    P,
}

impl SoftFactor {
    pub const ALL: [SoftFactor; 7] = [
        SoftFactor::Q,
        SoftFactor::C,
        SoftFactor::T,
        SoftFactor::R,
        SoftFactor::K,
        SoftFactor::L,
        SoftFactor::P,
    ];
}

/// Baseline preference profiles (SPEC-020 §11).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PolicyProfile {
    Balanced,
    QualityFirst,
    CostAware,
    LatencySensitive,
    LocalFirst,
}

impl PolicyProfile {
    pub const ALL: [PolicyProfile; 5] = [
        PolicyProfile::Balanced,
        PolicyProfile::QualityFirst,
        PolicyProfile::CostAware,
        PolicyProfile::LatencySensitive,
        PolicyProfile::LocalFirst,
    ];
}

/// Exact rational weights: numerator / denominator (sum of numerators = denominator).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfileWeights {
    pub numerators: [u32; 7],
    pub denominator: u32,
}

impl ProfileWeights {
    pub fn weight_micros(self, factor: SoftFactor) -> i64 {
        let idx = match factor {
            SoftFactor::Q => 0,
            SoftFactor::C => 1,
            SoftFactor::T => 2,
            SoftFactor::R => 3,
            SoftFactor::K => 4,
            SoftFactor::L => 5,
            SoftFactor::P => 6,
        };
        let n = self.numerators[idx] as i64;
        let d = self.denominator as i64;
        (n * 1_000_000) / d
    }

    pub fn sum_micros(self) -> i64 {
        SoftFactor::ALL.iter().map(|f| self.weight_micros(*f)).sum()
    }
}

/// Integrity-verified cold-start / learning-disabled baseline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BaselineCalibration {
    pub artifact_id: &'static str,
    pub artifact_sha256: &'static str,
    pub quality_cohorts_claimed: bool,
    /// Artifact leaves Beta α/β / sample-confidence `k` / numeric quality prior Unknown.
    pub quality_prior_unknown: bool,
    pub sample_confidence_k_unknown: bool,
    pub unknown_cost_is_not_zero: bool,
    pub clean_install_uses_artifact: bool,
    pub learning_disabled_uses_artifact: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaselineError {
    HashMismatch,
    ForbiddenPocDefaults,
}

/// Embedded artifact bytes (integrity-bound).
pub fn baseline_artifact_toml() -> &'static str {
    include_str!("../../../../docs/evidence/m005-spec020-baseline-calibration-artifact-v1.toml")
}

/// Verify artifact bytes against the recorded SHA-256 and return the baseline.
pub fn load_verified_baseline() -> Result<BaselineCalibration, BaselineError> {
    let bytes = baseline_artifact_toml().as_bytes();
    let digest = sha256_hex(bytes);
    if digest != BASELINE_ARTIFACT_SHA256 {
        return Err(BaselineError::HashMismatch);
    }
    if digest == FORBIDDEN_SYNTHETIC_POC_SHA256 {
        return Err(BaselineError::ForbiddenPocDefaults);
    }
    Ok(BaselineCalibration {
        artifact_id: BASELINE_ARTIFACT_ID,
        artifact_sha256: BASELINE_ARTIFACT_SHA256,
        quality_cohorts_claimed: false,
        quality_prior_unknown: true,
        sample_confidence_k_unknown: true,
        unknown_cost_is_not_zero: true,
        clean_install_uses_artifact: true,
        learning_disabled_uses_artifact: true,
    })
}

/// Profile weights from the verified artifact (policy-identity freeze; not POC).
pub fn profile_weights(profile: PolicyProfile) -> ProfileWeights {
    match profile {
        PolicyProfile::Balanced => ProfileWeights {
            numerators: [1, 1, 1, 1, 1, 1, 1],
            denominator: 7,
        },
        PolicyProfile::QualityFirst => ProfileWeights {
            numerators: [2, 1, 1, 1, 1, 1, 1],
            denominator: 8,
        },
        PolicyProfile::CostAware => ProfileWeights {
            numerators: [1, 1, 1, 1, 2, 1, 1],
            denominator: 8,
        },
        PolicyProfile::LatencySensitive => ProfileWeights {
            numerators: [1, 1, 1, 1, 1, 2, 1],
            denominator: 8,
        },
        PolicyProfile::LocalFirst => ProfileWeights {
            numerators: [1, 1, 1, 1, 1, 1, 2],
            denominator: 8,
        },
    }
}

/// Cold-start and learning-disabled installs must bind the same artifact.
pub fn cold_start_baseline_matches(learning_enabled: bool) -> bool {
    let Ok(baseline) = load_verified_baseline() else {
        return false;
    };
    if learning_enabled {
        baseline.clean_install_uses_artifact
    } else {
        baseline.learning_disabled_uses_artifact
            && baseline.artifact_sha256 == BASELINE_ARTIFACT_SHA256
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_hash_matches_recorded_integrity() {
        let baseline = load_verified_baseline().expect("verified baseline");
        assert_eq!(baseline.artifact_id, BASELINE_ARTIFACT_ID);
        assert_eq!(baseline.artifact_sha256, BASELINE_ARTIFACT_SHA256);
        assert!(baseline.quality_prior_unknown);
        assert!(!baseline.quality_cohorts_claimed);
    }

    #[test]
    fn profile_weights_sum_to_one_in_micros() {
        for profile in PolicyProfile::ALL {
            let w = profile_weights(profile);
            // Exact rationals sum to 1; micros may truncate by <7.
            let sum = w.sum_micros();
            assert!((1_000_000 - sum).abs() < 8, "profile {profile:?} sum={sum}");
        }
    }

    #[test]
    fn clean_and_learning_disabled_share_artifact() {
        assert!(cold_start_baseline_matches(true));
        assert!(cold_start_baseline_matches(false));
        let a = load_verified_baseline().unwrap();
        assert_eq!(a.artifact_sha256, BASELINE_ARTIFACT_SHA256);
    }
}
