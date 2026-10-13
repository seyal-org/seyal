//! Privacy revocation fence and local forgetting state machine (SPEC-015).

use crate::RevocationGeneration;

use super::policy::{PolicyError, ScopeIdentity};
use super::types::ScopeKind;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RevocationFenceMember {
    pub scope: ScopeIdentity,
    pub generation: RevocationGeneration,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RevocationFence {
    members: Vec<RevocationFenceMember>,
}

impl RevocationFence {
    pub fn new(mut members: Vec<RevocationFenceMember>) -> Result<Self, PolicyError> {
        if members.is_empty() {
            return Err(PolicyError::NoResolvableScope);
        }
        members.sort_by(|a, b| {
            (a.scope.kind.code(), a.scope.id).cmp(&(b.scope.kind.code(), b.scope.id))
        });
        for window in members.windows(2) {
            if window[0].scope == window[1].scope {
                return Err(PolicyError::DuplicateScope);
            }
        }
        Ok(Self { members })
    }

    pub fn members(&self) -> &[RevocationFenceMember] {
        &self.members
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(self.members.len() as u32).to_le_bytes());
        for m in &self.members {
            out.push(m.scope.kind.code());
            out.extend_from_slice(&m.scope.id);
            out.extend_from_slice(&m.generation.get().to_le_bytes());
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, PolicyError> {
        if bytes.len() < 4 {
            return Err(PolicyError::Malformed);
        }
        let count = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
        let mut offset = 4;
        let mut members = Vec::with_capacity(count);
        for _ in 0..count {
            if bytes.len() < offset + 1 + 16 + 8 {
                return Err(PolicyError::Malformed);
            }
            let kind = ScopeKind::from_code(bytes[offset]).ok_or(PolicyError::UnknownDomain)?;
            offset += 1;
            let mut id = [0u8; 16];
            id.copy_from_slice(&bytes[offset..offset + 16]);
            offset += 16;
            let generation = RevocationGeneration::from_raw(u64::from_le_bytes(
                bytes[offset..offset + 8].try_into().unwrap(),
            ))
            .ok_or(PolicyError::Malformed)?;
            offset += 8;
            members.push(RevocationFenceMember {
                scope: ScopeIdentity::new(kind, id),
                generation,
            });
        }
        Self::new(members)
    }

    /// True when `other` is a strict advance of any overlapping domain or adds a domain.
    pub fn is_dominated_by(&self, other: &Self) -> bool {
        for mine in &self.members {
            match other.members.iter().find(|o| o.scope == mine.scope) {
                None => return true, // other omitted a previously applicable domain → incomplete
                Some(o) if o.generation.get() > mine.generation.get() => return true,
                Some(_) => {}
            }
        }
        for theirs in &other.members {
            if !self.members.iter().any(|m| m.scope == theirs.scope) {
                return true; // newly applicable domain
            }
        }
        false
    }

    pub fn is_current_for(&self, current: &Self) -> bool {
        self.members == current.members
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ForgettingState {
    RevocationRequested,
    RevocationRequestDegraded,
    RevocationCommitUnknown,
    RevocationCommitted,
    CleanupPending,
    CleanupDegraded,
    LocalForgotten,
}

impl ForgettingState {
    pub const fn code(self) -> u8 {
        match self {
            Self::RevocationRequested => 1,
            Self::RevocationRequestDegraded => 2,
            Self::RevocationCommitUnknown => 3,
            Self::RevocationCommitted => 4,
            Self::CleanupPending => 5,
            Self::CleanupDegraded => 6,
            Self::LocalForgotten => 7,
        }
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::RevocationRequested),
            2 => Some(Self::RevocationRequestDegraded),
            3 => Some(Self::RevocationCommitUnknown),
            4 => Some(Self::RevocationCommitted),
            5 => Some(Self::CleanupPending),
            6 => Some(Self::CleanupDegraded),
            7 => Some(Self::LocalForgotten),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForgettingTransitionError {
    UnsupportedEdge,
    NoFalseLocalForgotten,
}

pub fn forgetting_transition(
    from: ForgettingState,
    to: ForgettingState,
) -> Result<(), ForgettingTransitionError> {
    use ForgettingState::*;
    let ok = matches!(
        (from, to),
        (RevocationRequested, RevocationRequestDegraded)
            | (RevocationRequested, RevocationCommitUnknown)
            | (RevocationRequested, RevocationCommitted)
            | (RevocationCommitUnknown, RevocationCommitted)
            | (RevocationCommitUnknown, RevocationRequested)
            | (RevocationCommitUnknown, RevocationRequestDegraded)
            | (RevocationCommitted, CleanupPending)
            | (CleanupPending, LocalForgotten)
            | (CleanupPending, CleanupDegraded)
            | (CleanupDegraded, LocalForgotten)
    );
    if matches!((from, to), (LocalForgotten, CleanupDegraded)) {
        return Err(ForgettingTransitionError::NoFalseLocalForgotten);
    }
    if ok {
        Ok(())
    } else {
        Err(ForgettingTransitionError::UnsupportedEdge)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProviderDeletionTruth {
    NotAttempted,
    Unsupported,
    Requested,
    Confirmed,
    Failed,
}

/// One-shot provider handoff fence token (SPEC-015 §8). Adapter without enforceable fence fails closed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoffFence {
    pub agent_run_id: [u8; 16],
    pub payload_digest: [u8; 32],
    pub adapter_id: [u8; 16],
    pub fence_vector: Vec<u8>,
    pub expires_at_unix_ms: u64,
    pub consumed: bool,
    pub enforceable: bool,
}

impl HandoffFence {
    pub fn acquire(
        agent_run_id: [u8; 16],
        payload_digest: [u8; 32],
        adapter_id: [u8; 16],
        fence: &RevocationFence,
        expires_at_unix_ms: u64,
        enforceable: bool,
    ) -> Result<Self, HandoffError> {
        if !enforceable {
            return Err(HandoffError::FenceNotEnforceable);
        }
        Ok(Self {
            agent_run_id,
            payload_digest,
            adapter_id,
            fence_vector: fence.encode(),
            expires_at_unix_ms,
            consumed: false,
            enforceable: true,
        })
    }

    pub fn use_for_send(
        &mut self,
        now_unix_ms: u64,
        current: &RevocationFence,
        agent_run_id: [u8; 16],
        payload_digest: [u8; 32],
    ) -> Result<(), HandoffError> {
        if self.consumed {
            return Err(HandoffError::AlreadyConsumed);
        }
        if now_unix_ms >= self.expires_at_unix_ms {
            return Err(HandoffError::Expired);
        }
        if self.agent_run_id != agent_run_id || self.payload_digest != payload_digest {
            return Err(HandoffError::BindingMismatch);
        }
        let bound =
            RevocationFence::decode(&self.fence_vector).map_err(|_| HandoffError::InvalidVector)?;
        if !bound.is_current_for(current) || bound.is_dominated_by(current) {
            // if current advanced, fail
            if !bound.is_current_for(current) {
                return Err(HandoffError::RevocationWon);
            }
        }
        // Dominated check: if current is ahead of bound, revocation won.
        if bound.members() != current.members() {
            return Err(HandoffError::RevocationWon);
        }
        self.consumed = true;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoffError {
    FenceNotEnforceable,
    AlreadyConsumed,
    Expired,
    BindingMismatch,
    InvalidVector,
    RevocationWon,
}

/// Opaque suppression token material (matching key excludes generation/kind).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SuppressionIdentity {
    pub scope: ScopeIdentity,
    pub semantic_token: [u8; 32],
    pub applicability_token: [u8; 32],
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RevocationGeneration;

    #[test]
    fn local_forgotten_cannot_degrade() {
        assert_eq!(
            forgetting_transition(
                ForgettingState::LocalForgotten,
                ForgettingState::CleanupDegraded
            ),
            Err(ForgettingTransitionError::NoFalseLocalForgotten)
        );
    }

    #[test]
    fn incomplete_vector_is_dominated() {
        let scope_a = ScopeIdentity::new(ScopeKind::Workspace, [1; 16]);
        let scope_b = ScopeIdentity::new(ScopeKind::Worktree, [2; 16]);
        let old = RevocationFence::new(vec![RevocationFenceMember {
            scope: scope_a,
            generation: RevocationGeneration::FIRST,
        }])
        .unwrap();
        let new = RevocationFence::new(vec![
            RevocationFenceMember {
                scope: scope_a,
                generation: RevocationGeneration::FIRST,
            },
            RevocationFenceMember {
                scope: scope_b,
                generation: RevocationGeneration::FIRST,
            },
        ])
        .unwrap();
        assert!(old.is_dominated_by(&new));
    }
}
