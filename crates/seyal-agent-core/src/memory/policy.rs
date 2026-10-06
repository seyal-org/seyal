//! Composite policy generation and caller scope binding (SPEC-012 §3 / §10).

use crate::{RevocationGeneration, ScopePolicyGeneration};

use super::types::{MemoryMode, ScopeKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ScopeIdentity {
    pub kind: ScopeKind,
    pub id: [u8; 16],
}

impl ScopeIdentity {
    pub fn new(kind: ScopeKind, id: [u8; 16]) -> Self {
        Self { kind, id }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PolicyScopeMember {
    pub scope: ScopeIdentity,
    pub policy_generation: ScopePolicyGeneration,
    pub revocation_generation: RevocationGeneration,
    pub mode: MemoryMode,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PolicyGeneration {
    members: Vec<PolicyScopeMember>,
}

impl PolicyGeneration {
    pub fn new(mut members: Vec<PolicyScopeMember>) -> Result<Self, PolicyError> {
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

    pub fn members(&self) -> &[PolicyScopeMember] {
        &self.members
    }

    pub fn effective_mode(&self) -> MemoryMode {
        MemoryMode::compose(self.members.iter().map(|m| m.mode))
    }

    pub fn owning_scope(&self) -> ScopeIdentity {
        self.members[0].scope
    }

    pub fn matches(&self, other: &Self) -> bool {
        self.members == other.members
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(self.members.len() as u32).to_le_bytes());
        for m in &self.members {
            out.push(m.scope.kind.code());
            out.extend_from_slice(&m.scope.id);
            out.extend_from_slice(&m.policy_generation.get().to_le_bytes());
            out.extend_from_slice(&m.revocation_generation.get().to_le_bytes());
            out.push(m.mode.code());
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
            if bytes.len() < offset + 1 + 16 + 8 + 8 + 1 {
                return Err(PolicyError::Malformed);
            }
            let kind = ScopeKind::from_code(bytes[offset]).ok_or(PolicyError::UnknownDomain)?;
            offset += 1;
            let mut id = [0u8; 16];
            id.copy_from_slice(&bytes[offset..offset + 16]);
            offset += 16;
            let policy_generation = ScopePolicyGeneration::from_raw(u64::from_le_bytes(
                bytes[offset..offset + 8].try_into().unwrap(),
            ))
            .ok_or(PolicyError::Malformed)?;
            offset += 8;
            let revocation_generation = RevocationGeneration::from_raw(u64::from_le_bytes(
                bytes[offset..offset + 8].try_into().unwrap(),
            ))
            .ok_or(PolicyError::Malformed)?;
            offset += 8;
            let mode = MemoryMode::from_code(bytes[offset]).ok_or(PolicyError::UnknownDomain)?;
            offset += 1;
            members.push(PolicyScopeMember {
                scope: ScopeIdentity::new(kind, id),
                policy_generation,
                revocation_generation,
                mode,
            });
        }
        Self::new(members)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallerScopeContext {
    pub authorized_scopes: Vec<ScopeIdentity>,
    pub principal_id: [u8; 16],
}

impl CallerScopeContext {
    pub fn authorize_owning_scope(
        &self,
        target: ScopeIdentity,
    ) -> Result<ScopeIdentity, PolicyError> {
        if self.authorized_scopes.contains(&target) {
            Ok(target)
        } else {
            Err(PolicyError::ScopeNotAuthorized)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyError {
    NoResolvableScope,
    DuplicateScope,
    Malformed,
    UnknownDomain,
    ScopeNotAuthorized,
    ModeForbidden,
    StalePolicyGeneration,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RevocationGeneration, ScopePolicyGeneration};

    #[test]
    fn compose_fails_closed_to_disabled_when_any_member_is_disabled() {
        let members = vec![
            PolicyScopeMember {
                scope: ScopeIdentity::new(ScopeKind::Workspace, [1; 16]),
                policy_generation: ScopePolicyGeneration::FIRST,
                revocation_generation: RevocationGeneration::FIRST,
                mode: MemoryMode::Assisted,
            },
            PolicyScopeMember {
                scope: ScopeIdentity::new(ScopeKind::Worktree, [2; 16]),
                policy_generation: ScopePolicyGeneration::FIRST,
                revocation_generation: RevocationGeneration::FIRST,
                mode: MemoryMode::Disabled,
            },
        ];
        let pg = PolicyGeneration::new(members).unwrap();
        assert_eq!(pg.effective_mode(), MemoryMode::Disabled);
    }
}
