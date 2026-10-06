//! Fixture enforcement-class labels for conformance honesty probes.
//!
//! Production presence types land under #1276. Until then this fixture enum is
//! the sole catalog vocabulary for ADR-012 §12 honesty cases — it must not
//! become a second enforcement authority outside the harness.

/// ADR-012 §12 enforcement class (fixture vocabulary until #1276).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FixtureEnforcementClass {
    /// Backend can observe/report only.
    Observed,
    /// Backend can request upstream behavior but cannot claim local enforcement.
    UpstreamRequestable,
    /// Operation passes through a backend-owned typed authority boundary.
    BackendEnforced,
}

/// Result of an enforcement-class honesty claim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnforcementClaimOutcome {
    /// Claim accepted because the typed boundary evidence matches the class.
    Accepted,
    /// Claim rejected: BackendEnforced without a typed backend boundary.
    RejectedDishonest,
    /// Weaker classes always accepted when evidence matches observation-only.
    AcceptedObserved,
}

/// Honesty gate: `BackendEnforced` requires an explicit typed boundary flag.
///
/// This is the catalog probe contract — adapters must route real claims through
/// the same rule. Soft presence (#1276) may replace the fixture enum later
/// without inventing a parallel honesty engine.
pub fn evaluate_enforcement_claim(
    claimed: FixtureEnforcementClass,
    has_typed_backend_boundary: bool,
) -> EnforcementClaimOutcome {
    match claimed {
        FixtureEnforcementClass::BackendEnforced if !has_typed_backend_boundary => {
            EnforcementClaimOutcome::RejectedDishonest
        }
        FixtureEnforcementClass::BackendEnforced => EnforcementClaimOutcome::Accepted,
        FixtureEnforcementClass::Observed | FixtureEnforcementClass::UpstreamRequestable => {
            EnforcementClaimOutcome::AcceptedObserved
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_conformance_enforcement_class_honesty() {
        assert_eq!(
            evaluate_enforcement_claim(FixtureEnforcementClass::BackendEnforced, false),
            EnforcementClaimOutcome::RejectedDishonest
        );
        assert_eq!(
            evaluate_enforcement_claim(FixtureEnforcementClass::BackendEnforced, true),
            EnforcementClaimOutcome::Accepted
        );
        assert_eq!(
            evaluate_enforcement_claim(FixtureEnforcementClass::Observed, false),
            EnforcementClaimOutcome::AcceptedObserved
        );
        assert_eq!(
            evaluate_enforcement_claim(FixtureEnforcementClass::UpstreamRequestable, false),
            EnforcementClaimOutcome::AcceptedObserved
        );
    }
}
