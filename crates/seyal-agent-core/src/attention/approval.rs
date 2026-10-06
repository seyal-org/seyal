//! ApprovalRequestV1 / ApprovalDecisionV1 (SPEC-028 §5 / SPEC-016 §5).
//!
//! Recording lives here. Atomic Action dispatch/fencing remains #1310.
//! Privileged approvals are never minted from OSC or raw terminal text.

use crate::action::{
    ActionIntentError, ArgumentFingerprint, CapabilityRef, EffectClass, ResourceIdentity,
};
use crate::memory::{PolicyError, RevocationFence};
use crate::presence::terminal_text_authorizes_approval;
use crate::{ActionId, AgentRunId, ApprovalId, AttentionId, ClientPrincipalId};

/// Privileged path only. ExternalObserved must not mint dispatch-usable requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ControlMode {
    SeyalControlled = 1,
    ExternalObserved = 2,
}

impl ControlMode {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::SeyalControlled),
            2 => Some(Self::ExternalObserved),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ApprovalVerdict {
    Approved = 1,
    Rejected = 2,
}

impl ApprovalVerdict {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Approved),
            2 => Some(Self::Rejected),
            _ => None,
        }
    }
}

/// SPEC-028 `User | Policy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum DecisionAuthority {
    User = 1,
    Policy = 2,
}

impl DecisionAuthority {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::User),
            2 => Some(Self::Policy),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalError {
    IncompleteBinding,
    ExternalObservedForbidden,
    UntrustedSource,
    BindingMismatch,
    Expired,
    AlreadyConsumed,
    AlreadyDecided,
    UnknownApproval,
    StaleSession,
    MissingDecideScope,
    ReconciliationDoesNotAuthorize,
    RejectedDoesNotAuthorize,
    Intent(ActionIntentError),
    Fence(PolicyError),
}

impl From<ActionIntentError> for ApprovalError {
    fn from(value: ActionIntentError) -> Self {
        Self::Intent(value)
    }
}

impl From<PolicyError> for ApprovalError {
    fn from(value: PolicyError) -> Self {
        Self::Fence(value)
    }
}

/// Inputs for §5.1 construction. Optional required fields fail closed.
#[derive(Clone, Debug)]
pub struct ApprovalRequestSpec {
    pub approval_id: ApprovalId,
    pub action_id: Option<ActionId>,
    pub action_intent_digest: Option<[u8; 32]>,
    pub agent_run_id: Option<AgentRunId>,
    pub capability: Option<CapabilityRef>,
    pub resource: Option<ResourceIdentity>,
    pub argument_fingerprint: Option<ArgumentFingerprint>,
    pub effect_class: Option<EffectClass>,
    pub policy_generation: Option<u64>,
    pub revocation_fence: Option<RevocationFence>,
    pub expires_at_unix_ms: Option<u64>,
    pub requested_at_unix_ms: u64,
    pub attention_id: Option<AttentionId>,
    pub control_mode: ControlMode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalRequest {
    pub approval_id: ApprovalId,
    pub action_id: ActionId,
    pub action_intent_digest: [u8; 32],
    pub agent_run_id: AgentRunId,
    pub capability: CapabilityRef,
    pub resource: ResourceIdentity,
    pub argument_fingerprint: ArgumentFingerprint,
    pub effect_class: EffectClass,
    pub policy_generation: u64,
    pub revocation_fence: RevocationFence,
    pub expires_at_unix_ms: Option<u64>,
    pub requested_at_unix_ms: u64,
    pub attention_id: AttentionId,
    pub control_mode: ControlMode,
}

impl ApprovalRequest {
    pub fn try_build(spec: ApprovalRequestSpec) -> Result<Self, ApprovalError> {
        if spec.control_mode != ControlMode::SeyalControlled {
            return Err(ApprovalError::ExternalObservedForbidden);
        }
        let action_id = spec.action_id.ok_or(ApprovalError::IncompleteBinding)?;
        let action_intent_digest = spec
            .action_intent_digest
            .ok_or(ApprovalError::IncompleteBinding)?;
        if action_intent_digest == [0u8; 32] {
            return Err(ApprovalError::IncompleteBinding);
        }
        let agent_run_id = spec.agent_run_id.ok_or(ApprovalError::IncompleteBinding)?;
        let capability = spec.capability.ok_or(ApprovalError::IncompleteBinding)?;
        let resource = spec.resource.ok_or(ApprovalError::IncompleteBinding)?;
        let argument_fingerprint = spec
            .argument_fingerprint
            .ok_or(ApprovalError::IncompleteBinding)?;
        let effect_class = spec.effect_class.ok_or(ApprovalError::IncompleteBinding)?;
        let policy_generation = spec
            .policy_generation
            .ok_or(ApprovalError::IncompleteBinding)?;
        if policy_generation == 0 {
            return Err(ApprovalError::IncompleteBinding);
        }
        let revocation_fence = spec
            .revocation_fence
            .ok_or(ApprovalError::IncompleteBinding)?;
        if revocation_fence.members().is_empty() {
            return Err(ApprovalError::IncompleteBinding);
        }
        let attention_id = spec.attention_id.ok_or(ApprovalError::IncompleteBinding)?;
        if spec
            .expires_at_unix_ms
            .is_some_and(|expiry| expiry < spec.requested_at_unix_ms)
        {
            return Err(ApprovalError::IncompleteBinding);
        }
        Ok(Self {
            approval_id: spec.approval_id,
            action_id,
            action_intent_digest,
            agent_run_id,
            capability,
            resource,
            argument_fingerprint,
            effect_class,
            policy_generation,
            revocation_fence,
            expires_at_unix_ms: spec.expires_at_unix_ms,
            requested_at_unix_ms: spec.requested_at_unix_ms,
            attention_id,
            control_mode: spec.control_mode,
        })
    }

    pub fn is_expired(&self, now_unix_ms: u64) -> bool {
        self.expires_at_unix_ms
            .is_some_and(|expiry| now_unix_ms >= expiry)
    }

    pub fn matches_witness(&self, witness: &ConsumptionWitness) -> bool {
        self.action_id == witness.action_id
            && self.action_intent_digest == witness.action_intent_digest
            && self.agent_run_id == witness.agent_run_id
            && self.capability == witness.capability
            && self.resource == witness.resource
            && self.argument_fingerprint == witness.argument_fingerprint
            && self.effect_class == witness.effect_class
            && self.policy_generation == witness.policy_generation
            && self.revocation_fence == witness.revocation_fence
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalDecision {
    pub approval_id: ApprovalId,
    pub action_id: ActionId,
    pub agent_run_id: AgentRunId,
    pub decision: ApprovalVerdict,
    pub authority: DecisionAuthority,
    pub decided_at_unix_ms: u64,
    pub decision_policy_generation: u64,
    pub decision_principal_id: Option<ClientPrincipalId>,
    pub consumed: bool,
}

impl ApprovalDecision {
    pub fn is_consumable(&self, request: &ApprovalRequest, now_unix_ms: u64) -> bool {
        self.decision == ApprovalVerdict::Approved
            && !self.consumed
            && self.action_id == request.action_id
            && self.agent_run_id == request.agent_run_id
            && !request.is_expired(now_unix_ms)
    }
}

/// Exact fields SPEC-016 re-checks at consumption (fixture consumer / #1310).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsumptionWitness {
    pub action_id: ActionId,
    pub action_intent_digest: [u8; 32],
    pub agent_run_id: AgentRunId,
    pub capability: CapabilityRef,
    pub resource: ResourceIdentity,
    pub argument_fingerprint: ArgumentFingerprint,
    pub effect_class: EffectClass,
    pub policy_generation: u64,
    pub revocation_fence: RevocationFence,
}

impl ConsumptionWitness {
    pub fn from_request(request: &ApprovalRequest) -> Self {
        Self {
            action_id: request.action_id,
            action_intent_digest: request.action_intent_digest,
            agent_run_id: request.agent_run_id,
            capability: request.capability.clone(),
            resource: request.resource.clone(),
            argument_fingerprint: request.argument_fingerprint,
            effect_class: request.effect_class,
            policy_generation: request.policy_generation,
            revocation_fence: request.revocation_fence.clone(),
        }
    }
}

/// Osc/raw terminal text never creates a privileged ApprovalRequest.
pub fn request_from_untrusted_terminal(text: &str) -> Result<ApprovalRequest, ApprovalError> {
    let _ = terminal_text_authorizes_approval(text);
    Err(ApprovalError::UntrustedSource)
}

/// ReconciliationRequired Attention never auto-approves or auto-retries.
pub fn auto_approve_reconciliation() -> Result<(), ApprovalError> {
    Err(ApprovalError::ReconciliationDoesNotAuthorize)
}

pub fn authorize_decide(
    session_valid: bool,
    has_approval_decide_scope: bool,
) -> Result<(), ApprovalError> {
    if !session_valid {
        return Err(ApprovalError::StaleSession);
    }
    if !has_approval_decide_scope {
        return Err(ApprovalError::MissingDecideScope);
    }
    Ok(())
}

pub fn evaluate_consume(
    request: &ApprovalRequest,
    decision: &ApprovalDecision,
    witness: &ConsumptionWitness,
    now_unix_ms: u64,
) -> Result<(), ApprovalError> {
    if decision.approval_id != request.approval_id {
        return Err(ApprovalError::BindingMismatch);
    }
    if decision.decision == ApprovalVerdict::Rejected {
        return Err(ApprovalError::RejectedDoesNotAuthorize);
    }
    if decision.consumed {
        return Err(ApprovalError::AlreadyConsumed);
    }
    if request.is_expired(now_unix_ms) {
        return Err(ApprovalError::Expired);
    }
    if !request.matches_witness(witness) {
        return Err(ApprovalError::BindingMismatch);
    }
    if !decision.is_consumable(request, now_unix_ms) {
        return Err(ApprovalError::BindingMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{RevocationFenceMember, ScopeIdentity, ScopeKind};
    use crate::{ActionId, AgentRunId, AttentionId, RevocationGeneration};

    fn fence() -> RevocationFence {
        RevocationFence::new(vec![RevocationFenceMember {
            scope: ScopeIdentity::new(ScopeKind::Workspace, [7; 16]),
            generation: RevocationGeneration::FIRST,
        }])
        .unwrap()
    }

    fn resource(version: u8) -> ResourceIdentity {
        ResourceIdentity::new(b"file", [1; 16], [2; 16], [version; 32]).unwrap()
    }

    fn spec() -> ApprovalRequestSpec {
        ApprovalRequestSpec {
            approval_id: ApprovalId::new(),
            action_id: Some(ActionId::new()),
            action_intent_digest: Some([9; 32]),
            agent_run_id: Some(AgentRunId::new()),
            capability: Some(CapabilityRef::new(b"fs.write").unwrap()),
            resource: Some(resource(1)),
            argument_fingerprint: Some(ArgumentFingerprint::of(b"a=1")),
            effect_class: Some(EffectClass::NonReplayable),
            policy_generation: Some(1),
            revocation_fence: Some(fence()),
            expires_at_unix_ms: None,
            requested_at_unix_ms: 1_000,
            attention_id: Some(AttentionId::new()),
            control_mode: ControlMode::SeyalControlled,
        }
    }

    #[test]
    fn spec028_12_04_missing_field_fails_closed() {
        let mut missing = spec();
        missing.action_id = None;
        assert_eq!(
            ApprovalRequest::try_build(missing).unwrap_err(),
            ApprovalError::IncompleteBinding
        );
        let mut digest = spec();
        digest.action_intent_digest = Some([0; 32]);
        assert_eq!(
            ApprovalRequest::try_build(digest).unwrap_err(),
            ApprovalError::IncompleteBinding
        );
    }

    #[test]
    fn spec028_12_13_external_observed_cannot_mint_privileged_request() {
        let mut observed = spec();
        observed.control_mode = ControlMode::ExternalObserved;
        assert_eq!(
            ApprovalRequest::try_build(observed).unwrap_err(),
            ApprovalError::ExternalObservedForbidden
        );
    }

    #[test]
    fn spec028_12_12_terminal_text_cannot_mint_request() {
        assert_eq!(
            request_from_untrusted_terminal("Approve? [y/N]"),
            Err(ApprovalError::UntrustedSource)
        );
    }

    #[test]
    fn spec028_12_24_reconciliation_never_auto_approves() {
        assert_eq!(
            auto_approve_reconciliation(),
            Err(ApprovalError::ReconciliationDoesNotAuthorize)
        );
    }
}
