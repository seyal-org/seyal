//! Shared SPEC-019 enumerations and wire codes.

/// Evaluator class (SPEC-019 §3). No universal trust ranking.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EvaluatorClass {
    DeterministicTest,
    Build,
    Lint,
    StaticAnalysis,
    SecurityScanner,
    ArchitecturePolicy,
    RuntimeVerification,
    ScmCiStatus,
    HumanDecision,
    IndependentModelReview,
    HarnessReport,
    ProviderReport,
    ProjectSpecificEvaluator,
}

impl EvaluatorClass {
    pub const fn code(self) -> u8 {
        match self {
            Self::DeterministicTest => 1,
            Self::Build => 2,
            Self::Lint => 3,
            Self::StaticAnalysis => 4,
            Self::SecurityScanner => 5,
            Self::ArchitecturePolicy => 6,
            Self::RuntimeVerification => 7,
            Self::ScmCiStatus => 8,
            Self::HumanDecision => 9,
            Self::IndependentModelReview => 10,
            Self::HarnessReport => 11,
            Self::ProviderReport => 12,
            Self::ProjectSpecificEvaluator => 13,
        }
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::DeterministicTest),
            2 => Some(Self::Build),
            3 => Some(Self::Lint),
            4 => Some(Self::StaticAnalysis),
            5 => Some(Self::SecurityScanner),
            6 => Some(Self::ArchitecturePolicy),
            7 => Some(Self::RuntimeVerification),
            8 => Some(Self::ScmCiStatus),
            9 => Some(Self::HumanDecision),
            10 => Some(Self::IndependentModelReview),
            11 => Some(Self::HarnessReport),
            12 => Some(Self::ProviderReport),
            13 => Some(Self::ProjectSpecificEvaluator),
            _ => None,
        }
    }

    /// Self-report classes are advisory unless an explicit low-risk criterion allows them.
    pub const fn is_self_report(self) -> bool {
        matches!(
            self,
            Self::HarnessReport | Self::ProviderReport | Self::ProjectSpecificEvaluator
        )
    }
}

/// Criterion result (SPEC-019 §5). Unknown/Inconclusive never means pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CriterionResult {
    Satisfied,
    NotSatisfied,
    Inconclusive,
    Error,
    NotRun,
    Stale,
}

impl CriterionResult {
    pub const fn code(self) -> u8 {
        match self {
            Self::Satisfied => 1,
            Self::NotSatisfied => 2,
            Self::Inconclusive => 3,
            Self::Error => 4,
            Self::NotRun => 5,
            Self::Stale => 6,
        }
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Satisfied),
            2 => Some(Self::NotSatisfied),
            3 => Some(Self::Inconclusive),
            4 => Some(Self::Error),
            5 => Some(Self::NotRun),
            6 => Some(Self::Stale),
            _ => None,
        }
    }

    /// Never treat Unknown/Inconclusive/Error/NotRun/Stale as pass.
    pub const fn is_pass(self) -> bool {
        matches!(self, Self::Satisfied)
    }

    pub const fn blocks_policy_final(self) -> bool {
        !matches!(self, Self::Satisfied)
    }
}

/// Test integrity class (SPEC-019 §11).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TestIntegrityClass {
    TrustedExisting,
    TrustedProjectGenerated,
    AgentGeneratedUnreviewed,
    ExternalVerified,
}

impl TestIntegrityClass {
    pub const fn code(self) -> u8 {
        match self {
            Self::TrustedExisting => 1,
            Self::TrustedProjectGenerated => 2,
            Self::AgentGeneratedUnreviewed => 3,
            Self::ExternalVerified => 4,
        }
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::TrustedExisting),
            2 => Some(Self::TrustedProjectGenerated),
            3 => Some(Self::AgentGeneratedUnreviewed),
            4 => Some(Self::ExternalVerified),
            _ => None,
        }
    }

    pub const fn is_trusted_for_acceptance(self) -> bool {
        matches!(
            self,
            Self::TrustedExisting | Self::TrustedProjectGenerated | Self::ExternalVerified
        )
    }
}

/// Purpose eligibility for retained routing-quality evidence (SPEC-019 §15).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PurposeEligibility {
    pub operational_audit: bool,
    pub local_adaptation: bool,
    pub export_training: bool,
}

impl PurposeEligibility {
    pub const fn audit_only() -> Self {
        Self {
            operational_audit: true,
            local_adaptation: false,
            export_training: false,
        }
    }

    pub const fn none() -> Self {
        Self {
            operational_audit: false,
            local_adaptation: false,
            export_training: false,
        }
    }
}

/// How a route was selected for comparative evidence (SPEC-019 §15 / fixture 17).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SelectionSupport {
    /// Randomized experiment with recorded propensity in (0, 1].
    Randomized { propensity_millis: u32 },
    /// Deterministic selection; cannot fabricate counterfactual support.
    DeterministicZeroSupport,
}

impl SelectionSupport {
    pub const fn can_support_counterfactual(self) -> bool {
        matches!(self, Self::Randomized { propensity_millis } if propensity_millis > 0)
    }
}

/// Attribution class for quality changes (SPEC-019 §15 / fixture 16).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QualityAttribution {
    ContextCompiler,
    ModelOrRoute,
    ToolAvailability,
    HumanRepair,
    EvaluatorContract,
    MixedOrUnknown,
}

/// Fallback / difficulty bias class for comparative claims (SPEC-019 §15 / fixture 15).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RouteComparisonBias {
    /// Easy tasks on first route vs hard fallback — not an unbiased comparison.
    EasyFirstVsHardFallback,
    MatchedIsolatedStart,
    RandomizedSupported,
    AcceptedOffPolicy,
    ExplicitAbstain,
}

impl RouteComparisonBias {
    pub const fn allows_unbiased_claim(self) -> bool {
        matches!(
            self,
            Self::MatchedIsolatedStart | Self::RandomizedSupported | Self::AcceptedOffPolicy
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn criterion_result_unknown_never_pass() {
        for result in [
            CriterionResult::Inconclusive,
            CriterionResult::Error,
            CriterionResult::NotRun,
            CriterionResult::Stale,
            CriterionResult::NotSatisfied,
        ] {
            assert!(!result.is_pass());
            assert!(result.blocks_policy_final());
        }
        assert!(CriterionResult::Satisfied.is_pass());
    }

    #[test]
    fn malformed_evaluator_class_code_rejected() {
        assert!(EvaluatorClass::from_code(0).is_none());
        assert!(EvaluatorClass::from_code(255).is_none());
        assert_eq!(
            EvaluatorClass::from_code(1),
            Some(EvaluatorClass::DeterministicTest)
        );
    }
}
