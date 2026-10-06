//! MemoryRecord lifecycle transitions (SPEC-012 §§5–6).

use super::types::{MemoryState, TransitionReason};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionError {
    UnsupportedEdge,
    QualityRejectNotRevocation,
    MissingForgetReason,
}

/// Complete permitted edge set from SPEC-012 §6. Unlisted edges are rejected.
pub fn allowed_transition(
    from: MemoryState,
    to: MemoryState,
    reason: TransitionReason,
) -> Result<(), TransitionError> {
    match (from, to) {
        (MemoryState::Proposed, MemoryState::Accepted) => {
            if reason == TransitionReason::Accept {
                Ok(())
            } else {
                Err(TransitionError::UnsupportedEdge)
            }
        }
        (MemoryState::Proposed, MemoryState::Revoked) => {
            if reason.is_forget_or_privacy() {
                Ok(())
            } else if reason == TransitionReason::QualityReject {
                Err(TransitionError::QualityRejectNotRevocation)
            } else {
                Err(TransitionError::MissingForgetReason)
            }
        }
        (MemoryState::Proposed, MemoryState::Expired) => {
            if matches!(
                reason,
                TransitionReason::ProposalTtl | TransitionReason::Expiry
            ) {
                Ok(())
            } else {
                Err(TransitionError::UnsupportedEdge)
            }
        }
        (MemoryState::Accepted, MemoryState::Accepted) => {
            if matches!(
                reason,
                TransitionReason::RevalidateRefresh | TransitionReason::Accept
            ) {
                Ok(())
            } else {
                Err(TransitionError::UnsupportedEdge)
            }
        }
        (MemoryState::Accepted, MemoryState::Superseded) => {
            if reason == TransitionReason::Supersede {
                Ok(())
            } else {
                Err(TransitionError::UnsupportedEdge)
            }
        }
        (MemoryState::Accepted, MemoryState::Revoked)
        | (MemoryState::Superseded, MemoryState::Revoked)
        | (MemoryState::Expired, MemoryState::Revoked) => {
            if reason.is_forget_or_privacy() {
                Ok(())
            } else {
                Err(TransitionError::MissingForgetReason)
            }
        }
        (MemoryState::Accepted, MemoryState::Expired) => {
            if matches!(
                reason,
                TransitionReason::Expiry | TransitionReason::IntegrityQuarantine
            ) {
                Ok(())
            } else {
                Err(TransitionError::UnsupportedEdge)
            }
        }
        // Explicit denials from the table.
        (MemoryState::Superseded, MemoryState::Accepted)
        | (MemoryState::Revoked, MemoryState::Accepted)
        | (MemoryState::Expired, MemoryState::Accepted)
        | (MemoryState::Revoked, _) => Err(TransitionError::UnsupportedEdge),
        _ => Err(TransitionError::UnsupportedEdge),
    }
}

/// Safety maintenance (expiry / privacy revocation) remains available under Disabled/ReadOnly.
pub fn is_safety_maintenance(to: MemoryState, reason: TransitionReason) -> bool {
    matches!(
        (to, reason),
        (
            MemoryState::Expired,
            TransitionReason::Expiry
                | TransitionReason::ProposalTtl
                | TransitionReason::IntegrityQuarantine
        ) | (
            MemoryState::Revoked,
            TransitionReason::Forget
                | TransitionReason::DoNotRemember
                | TransitionReason::PrivacyRevocation
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlisted_edges_are_rejected() {
        assert!(allowed_transition(
            MemoryState::Revoked,
            MemoryState::Accepted,
            TransitionReason::Accept
        )
        .is_err());
        assert!(allowed_transition(
            MemoryState::Expired,
            MemoryState::Accepted,
            TransitionReason::Accept
        )
        .is_err());
        assert!(allowed_transition(
            MemoryState::Superseded,
            MemoryState::Accepted,
            TransitionReason::Accept
        )
        .is_err());
    }

    #[test]
    fn quality_reject_is_not_revocation() {
        assert_eq!(
            allowed_transition(
                MemoryState::Proposed,
                MemoryState::Revoked,
                TransitionReason::QualityReject
            ),
            Err(TransitionError::QualityRejectNotRevocation)
        );
    }
}
