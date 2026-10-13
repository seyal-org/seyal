//! Typed failure-class fallback rules (SPEC-020 §15).
//!
//! Every material reroute creates a new immutable RoutingDecision. This module
//! is the replaceable policy stage for failure classification — not a second
//! router authority.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureClass {
    TransientTransport,
    RateLimited,
    ProviderUnavailable,
    AuthenticationRequired,
    ContextTooLarge,
    CapabilityMismatch,
    PolicyChanged,
    PermissionDenied,
    EvaluationRejected,
    ExternalEffectUnknown,
    UnknownFailure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FallbackAction {
    /// Bounded same-invocation retry.
    SameInvocationRetry,
    /// Choose another eligible route; mint a new RoutingDecision.
    RerouteNewDecision,
    /// Require reauth or another authorized connection.
    ReauthOrOtherConnection,
    /// Rebuild/reduce context or choose a compatible larger route.
    RebuildContextOrLargerRoute,
    /// Invalidate stale capability evidence.
    InvalidateCapabilityEvidence,
    /// Never silently relax; explicit NoRoute / Attention.
    NeverRelax,
    /// Create a new Attempt + RoutingDecision when budget/policy allows.
    NewAttemptAndDecision,
    /// Reconciliation required; never another agent replay / duplicate mutation.
    ReconcileNoReplay,
    /// Conservative non-blind handling.
    Conservative,
}

pub fn fallback_action(class: FailureClass) -> FallbackAction {
    match class {
        FailureClass::TransientTransport => FallbackAction::SameInvocationRetry,
        FailureClass::RateLimited | FailureClass::ProviderUnavailable => {
            FallbackAction::RerouteNewDecision
        }
        FailureClass::AuthenticationRequired => FallbackAction::ReauthOrOtherConnection,
        FailureClass::ContextTooLarge => FallbackAction::RebuildContextOrLargerRoute,
        FailureClass::CapabilityMismatch => FallbackAction::InvalidateCapabilityEvidence,
        FailureClass::PolicyChanged | FailureClass::PermissionDenied => FallbackAction::NeverRelax,
        FailureClass::EvaluationRejected => FallbackAction::NewAttemptAndDecision,
        FailureClass::ExternalEffectUnknown => FallbackAction::ReconcileNoReplay,
        FailureClass::UnknownFailure => FallbackAction::Conservative,
    }
}

/// Policy denial never relaxes — even when a cheaper/higher-scoring fallback exists.
pub fn policy_denial_may_select(fallback_offering_denied: bool) -> bool {
    !fallback_offering_denied
}

/// EffectUnknown must not authorize a duplicate external mutation.
pub fn may_replay_external_mutation(class: FailureClass) -> bool {
    !matches!(class, FailureClass::ExternalEffectUnknown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_reroutes() {
        assert_eq!(
            fallback_action(FailureClass::RateLimited),
            FallbackAction::RerouteNewDecision
        );
    }

    #[test]
    fn policy_denial_never_relaxes() {
        assert!(!policy_denial_may_select(true));
        assert_eq!(
            fallback_action(FailureClass::PermissionDenied),
            FallbackAction::NeverRelax
        );
    }

    #[test]
    fn evaluation_rejected_new_attempt() {
        assert_eq!(
            fallback_action(FailureClass::EvaluationRejected),
            FallbackAction::NewAttemptAndDecision
        );
    }

    #[test]
    fn effect_unknown_no_duplicate_mutation() {
        assert!(!may_replay_external_mutation(
            FailureClass::ExternalEffectUnknown
        ));
        assert_eq!(
            fallback_action(FailureClass::ExternalEffectUnknown),
            FallbackAction::ReconcileNoReplay
        );
    }
}
