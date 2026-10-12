//! MemoryRecord value type and use-time eligibility (SPEC-012 §§3, 5.2, 12).

use crate::{MemoryId, RecordGeneration};

use super::policy::PolicyGeneration;
use super::semantic::{ApplicabilityIdentity, SemanticIdentity};
use super::types::{AuthorityClass, MemoryKind, MemoryState, Sensitivity};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvidenceRef {
    pub kind: u16,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryRecord {
    pub id: MemoryId,
    pub record_generation: RecordGeneration,
    pub schema_version: u16,
    pub kind: MemoryKind,
    pub payload: Vec<u8>,
    pub payload_schema_version: u16,
    pub semantic: SemanticIdentity,
    pub applicability: ApplicabilityIdentity,
    pub evidence_refs: Vec<EvidenceRef>,
    pub authority_class: AuthorityClass,
    pub sensitivity: Sensitivity,
    pub state: MemoryState,
    pub created_at_unix_ms: u64,
    pub accepted_at_unix_ms: Option<u64>,
    pub last_validated_at_unix_ms: Option<u64>,
    pub revalidate_after_unix_ms: Option<u64>,
    pub expires_at_unix_ms: Option<u64>,
    pub supersedes: Vec<MemoryId>,
    pub superseded_by: Vec<MemoryId>,
    pub conflicts_with: Vec<MemoryId>,
    pub source_fingerprints: Vec<Vec<u8>>,
    pub policy_generation: PolicyGeneration,
    pub revocation_generation: Option<u64>,
    pub quarantine_reason: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eligibility {
    Eligible,
    DeniedDisabledMode,
    DeniedNotAccepted,
    DeniedPendingRevalidation,
    DeniedExpired,
    DeniedRevoked,
    DeniedSuperseded,
    DeniedQuarantined,
    DeniedScope,
    DeniedSensitivity,
}

impl MemoryRecord {
    pub fn encoded_control_bytes(&self) -> usize {
        // Approximate control metadata footprint for admission (payload counted separately).
        self.semantic.canonical.len()
            + self.applicability.canonical.len()
            + self
                .evidence_refs
                .iter()
                .map(|e| e.bytes.len() + 2)
                .sum::<usize>()
            + self
                .source_fingerprints
                .iter()
                .map(|f| f.len())
                .sum::<usize>()
            + self.policy_generation.encode().len()
            + 256
    }

    pub fn encoded_total_bytes(&self) -> usize {
        self.payload.len() + self.encoded_control_bytes()
    }

    /// Use-time eligibility is separate from durable lifecycle state (SPEC-012 §5.2 / §12).
    pub fn use_time_eligibility(
        &self,
        now_unix_ms: u64,
        effective_mode_allows_read: bool,
        current_policy: &PolicyGeneration,
    ) -> Eligibility {
        if self.quarantine_reason.is_some() {
            return Eligibility::DeniedQuarantined;
        }
        if !effective_mode_allows_read {
            return Eligibility::DeniedDisabledMode;
        }
        // SPEC-012 §5.2 / SPEC-015: use-time must check current policy mode *and*
        // privacy/revocation generations. Lifecycle may remain Accepted while the
        // fence advances; mismatch fails closed (never silent Eligible).
        if !self.policy_generation.matches(current_policy) {
            if self.policy_generation.owning_scope() != current_policy.owning_scope() {
                return Eligibility::DeniedScope;
            }
            return Eligibility::DeniedPendingRevalidation;
        }
        match self.state {
            MemoryState::Revoked => return Eligibility::DeniedRevoked,
            MemoryState::Superseded => return Eligibility::DeniedSuperseded,
            MemoryState::Expired => return Eligibility::DeniedExpired,
            MemoryState::Proposed => return Eligibility::DeniedNotAccepted,
            MemoryState::Accepted => {}
        }
        if let Some(expires) = self.expires_at_unix_ms
            && now_unix_ms >= expires
        {
            return Eligibility::DeniedExpired;
        }
        if let Some(revalidate_after) = self.revalidate_after_unix_ms
            && now_unix_ms >= revalidate_after
        {
            // Equal revalidate/expires boundary: expiry wins once expires_at is reached.
            if let Some(expires) = self.expires_at_unix_ms
                && now_unix_ms >= expires
            {
                return Eligibility::DeniedExpired;
            }
            return Eligibility::DeniedPendingRevalidation;
        }
        Eligibility::Eligible
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::policy::{PolicyGeneration, PolicyScopeMember, ScopeIdentity};
    use crate::memory::semantic::{ApplicabilityIdentity, CanonicalBytes, SemanticIdentity};
    use crate::memory::types::{MemoryMode, ScopeKind};
    use crate::{RevocationGeneration, ScopePolicyGeneration};

    fn accepted_record(policy: PolicyGeneration) -> MemoryRecord {
        MemoryRecord {
            id: MemoryId::new(),
            record_generation: RecordGeneration::FIRST,
            schema_version: 1,
            kind: MemoryKind::EngineeringFact,
            payload: b"fact".to_vec(),
            payload_schema_version: 1,
            semantic: SemanticIdentity {
                version: 1,
                canonical: CanonicalBytes(b"fact".to_vec()),
            },
            applicability: ApplicabilityIdentity::empty_v1(),
            evidence_refs: Vec::new(),
            authority_class: AuthorityClass::A2TypedObservation,
            sensitivity: Sensitivity::Internal,
            state: MemoryState::Accepted,
            created_at_unix_ms: 0,
            accepted_at_unix_ms: Some(0),
            last_validated_at_unix_ms: Some(0),
            revalidate_after_unix_ms: None,
            expires_at_unix_ms: None,
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            conflicts_with: Vec::new(),
            source_fingerprints: Vec::new(),
            policy_generation: policy,
            revocation_generation: None,
            quarantine_reason: None,
        }
    }

    #[test]
    fn privacy_generation_mismatch_denies_pending_revalidation() {
        let scope = ScopeIdentity::new(ScopeKind::Workspace, [1; 16]);
        let recorded = PolicyGeneration::new(vec![PolicyScopeMember {
            scope,
            policy_generation: ScopePolicyGeneration::FIRST,
            revocation_generation: RevocationGeneration::FIRST,
            mode: MemoryMode::Curated,
        }])
        .unwrap();
        let advanced = PolicyGeneration::new(vec![PolicyScopeMember {
            scope,
            policy_generation: ScopePolicyGeneration::FIRST,
            revocation_generation: RevocationGeneration::from_raw(2).unwrap(),
            mode: MemoryMode::Curated,
        }])
        .unwrap();
        let record = accepted_record(recorded);
        assert_eq!(
            record.use_time_eligibility(0, true, &advanced),
            Eligibility::DeniedPendingRevalidation
        );
    }

    #[test]
    fn owning_scope_mismatch_denies_scope() {
        let recorded = PolicyGeneration::new(vec![PolicyScopeMember {
            scope: ScopeIdentity::new(ScopeKind::Workspace, [1; 16]),
            policy_generation: ScopePolicyGeneration::FIRST,
            revocation_generation: RevocationGeneration::FIRST,
            mode: MemoryMode::Curated,
        }])
        .unwrap();
        let other = PolicyGeneration::new(vec![PolicyScopeMember {
            scope: ScopeIdentity::new(ScopeKind::Workspace, [2; 16]),
            policy_generation: ScopePolicyGeneration::FIRST,
            revocation_generation: RevocationGeneration::FIRST,
            mode: MemoryMode::Curated,
        }])
        .unwrap();
        let record = accepted_record(recorded);
        assert_eq!(
            record.use_time_eligibility(0, true, &other),
            Eligibility::DeniedScope
        );
    }
}
