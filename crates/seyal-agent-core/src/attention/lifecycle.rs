//! AttentionItem lifecycle edges (SPEC-028 §4.2).

use super::types::AttentionState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttentionTransitionError {
    UnsupportedEdge,
    AlreadyTerminal,
}

/// Complete permitted edge set from SPEC-028 §4.2. Unlisted edges are rejected.
pub fn allowed_attention_transition(
    from: AttentionState,
    to: AttentionState,
) -> Result<(), AttentionTransitionError> {
    match (from, to) {
        (AttentionState::Open, AttentionState::Acknowledged)
        | (AttentionState::Open, AttentionState::Resolved)
        | (AttentionState::Open, AttentionState::Dismissed)
        | (AttentionState::Open, AttentionState::Expired)
        | (AttentionState::Acknowledged, AttentionState::Resolved)
        | (AttentionState::Acknowledged, AttentionState::Dismissed)
        | (AttentionState::Acknowledged, AttentionState::Expired) => Ok(()),
        (from, _) if from.is_terminal() => Err(AttentionTransitionError::AlreadyTerminal),
        _ => Err(AttentionTransitionError::UnsupportedEdge),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlisted_edges_rejected() {
        assert!(allowed_attention_transition(
            AttentionState::Resolved,
            AttentionState::Open
        )
        .is_err());
        assert!(allowed_attention_transition(
            AttentionState::Dismissed,
            AttentionState::Acknowledged
        )
        .is_err());
        assert!(allowed_attention_transition(
            AttentionState::Acknowledged,
            AttentionState::Open
        )
        .is_err());
    }

    #[test]
    fn open_to_ack_resolve_dismiss_ok() {
        assert!(allowed_attention_transition(
            AttentionState::Open,
            AttentionState::Acknowledged
        )
        .is_ok());
        assert!(allowed_attention_transition(
            AttentionState::Open,
            AttentionState::Resolved
        )
        .is_ok());
        assert!(allowed_attention_transition(
            AttentionState::Open,
            AttentionState::Dismissed
        )
        .is_ok());
    }
}
