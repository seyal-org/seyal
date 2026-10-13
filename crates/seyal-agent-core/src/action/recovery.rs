//! EffectUnknown recovery and reconciliation (SPEC-016 §§4, 9–16, 21–22).
//!
//! This module is pure Agent-domain control-plane logic. It has no Terminal
//! Runtime, PTY, VT, TerminalState, or Metal dependency (SPEC-016 §24).

use crate::identity::{ActionId, AgentRunId};

use super::intent::{ActionLifecycle, EffectClass};

/// Finite automatic reconciliation attempts before Attention is required
/// (SPEC-016 §21.3). Concrete product budgets may tighten this later.
pub const DEFAULT_AUTOMATIC_RECONCILIATION_BUDGET: u32 = 3;

/// Durable ordering boundaries used for crash/fault recovery (SPEC-016 §22).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CrashBoundary {
    BeforeIntentPersist = 1,
    AfterPrepared = 2,
    AuthorizedBeforeDispatchCommit = 3,
    DispatchingBeforeInvocation = 4,
    DuringExecutor = 5,
    AfterEffectBeforeResultPersist = 6,
    AfterResultPersist = 7,
}

/// Causal marker that can bind observed state to this Action (SPEC-016 §14).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CausalMarkerKind {
    OperationId = 1,
    CasWitness = 2,
    TransactionId = 3,
    PrivacyNoInvocation = 4,
}

impl CausalMarkerKind {
    pub const fn code(self) -> u8 {
        self as u8
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::OperationId),
            2 => Some(Self::CasWitness),
            3 => Some(Self::TransactionId),
            4 => Some(Self::PrivacyNoInvocation),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CausalMarker {
    pub kind: CausalMarkerKind,
    pub bytes: [u8; 16],
}

/// Evidence presented to recovery/reconciliation. Untrusted sources cannot
/// mint known outcomes (SPEC-016 §§11, 13, 23.6–7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum EvidenceKind {
    None = 0,
    KnownNotDispatched = 1,
    CausalSuccess = 2,
    CausalFailedKnown = 3,
    ObservedStateNoCausalMarker = 4,
    UntrustedNarration = 5,
    UntrustedIdempotencyClaim = 6,
    ValidatedIdempotencyContract = 7,
    OperatorAckWithoutCausalEvidence = 8,
}

impl EvidenceKind {
    pub const fn code(self) -> u8 {
        self as u8
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::None),
            1 => Some(Self::KnownNotDispatched),
            2 => Some(Self::CausalSuccess),
            3 => Some(Self::CausalFailedKnown),
            4 => Some(Self::ObservedStateNoCausalMarker),
            5 => Some(Self::UntrustedNarration),
            6 => Some(Self::UntrustedIdempotencyClaim),
            7 => Some(Self::ValidatedIdempotencyContract),
            8 => Some(Self::OperatorAckWithoutCausalEvidence),
            _ => None,
        }
    }

    pub const fn is_authoritative(self) -> bool {
        matches!(
            self,
            Self::KnownNotDispatched
                | Self::CausalSuccess
                | Self::CausalFailedKnown
                | Self::ValidatedIdempotencyContract
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RecoveryEvidence {
    pub kind: EvidenceKind,
    pub dispatch_generation: Option<u64>,
    pub causal: Option<CausalMarker>,
    pub executor_authenticated: bool,
}

impl RecoveryEvidence {
    pub fn none() -> Self {
        Self {
            kind: EvidenceKind::None,
            dispatch_generation: None,
            causal: None,
            executor_authenticated: false,
        }
    }

    pub fn known_not_dispatched(dispatch_generation: u64) -> Self {
        Self {
            kind: EvidenceKind::KnownNotDispatched,
            dispatch_generation: Some(dispatch_generation),
            causal: Some(CausalMarker {
                kind: CausalMarkerKind::PrivacyNoInvocation,
                bytes: [0; 16],
            }),
            executor_authenticated: true,
        }
    }

    pub fn causal_success(dispatch_generation: u64, marker: CausalMarker) -> Self {
        Self {
            kind: EvidenceKind::CausalSuccess,
            dispatch_generation: Some(dispatch_generation),
            causal: Some(marker),
            executor_authenticated: true,
        }
    }

    pub fn causal_failed_known(dispatch_generation: u64, marker: CausalMarker) -> Self {
        Self {
            kind: EvidenceKind::CausalFailedKnown,
            dispatch_generation: Some(dispatch_generation),
            causal: Some(marker),
            executor_authenticated: true,
        }
    }

    pub fn observed_state_without_causal_marker() -> Self {
        Self {
            kind: EvidenceKind::ObservedStateNoCausalMarker,
            dispatch_generation: None,
            causal: None,
            executor_authenticated: false,
        }
    }

    pub fn untrusted_narration() -> Self {
        Self {
            kind: EvidenceKind::UntrustedNarration,
            dispatch_generation: None,
            causal: None,
            executor_authenticated: false,
        }
    }

    pub fn untrusted_idempotency_claim() -> Self {
        Self {
            kind: EvidenceKind::UntrustedIdempotencyClaim,
            dispatch_generation: None,
            causal: None,
            executor_authenticated: false,
        }
    }

    pub fn validated_idempotency_contract(dispatch_generation: u64) -> Self {
        Self {
            kind: EvidenceKind::ValidatedIdempotencyContract,
            dispatch_generation: Some(dispatch_generation),
            causal: Some(CausalMarker {
                kind: CausalMarkerKind::OperationId,
                bytes: [1; 16],
            }),
            executor_authenticated: true,
        }
    }

    pub fn operator_ack_without_causal_evidence() -> Self {
        Self {
            kind: EvidenceKind::OperatorAckWithoutCausalEvidence,
            dispatch_generation: None,
            causal: None,
            executor_authenticated: false,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(29);
        out.push(self.kind.code());
        match self.dispatch_generation {
            Some(generation) => {
                out.push(1);
                out.extend_from_slice(&generation.to_le_bytes());
            }
            None => out.push(0),
        }
        match self.causal {
            Some(marker) => {
                out.push(1);
                out.push(marker.kind.code());
                out.extend_from_slice(&marker.bytes);
            }
            None => out.push(0),
        }
        out.push(u8::from(self.executor_authenticated));
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.is_empty() {
            return None;
        }
        let mut offset = 0;
        let kind = EvidenceKind::from_code(*bytes.get(offset)?)?;
        offset += 1;
        let dispatch_generation = match *bytes.get(offset)? {
            0 => {
                offset += 1;
                None
            }
            1 => {
                offset += 1;
                let mut generation = [0u8; 8];
                generation.copy_from_slice(bytes.get(offset..offset + 8)?);
                offset += 8;
                Some(u64::from_le_bytes(generation))
            }
            _ => return None,
        };
        let causal = match *bytes.get(offset)? {
            0 => {
                offset += 1;
                None
            }
            1 => {
                offset += 1;
                let marker_kind = CausalMarkerKind::from_code(*bytes.get(offset)?)?;
                offset += 1;
                let mut marker = [0u8; 16];
                marker.copy_from_slice(bytes.get(offset..offset + 16)?);
                offset += 16;
                Some(CausalMarker {
                    kind: marker_kind,
                    bytes: marker,
                })
            }
            _ => return None,
        };
        let executor_authenticated = match *bytes.get(offset)? {
            0 => false,
            1 => true,
            _ => return None,
        };
        offset += 1;
        if offset != bytes.len() {
            return None;
        }
        Some(Self {
            kind,
            dispatch_generation,
            causal,
            executor_authenticated,
        })
    }
}

/// Mutable runtime facts stored beside immutable ActionIntent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionRuntime {
    pub lifecycle: ActionLifecycle,
    pub dispatch_generation: Option<u64>,
    pub authorization_invalidated: bool,
    pub cancel_requested: bool,
    pub reconciliation_attempts: u32,
    pub automatic_budget: u32,
}

impl ActionRuntime {
    pub fn prepared() -> Self {
        Self {
            lifecycle: ActionLifecycle::Prepared,
            dispatch_generation: None,
            authorization_invalidated: false,
            cancel_requested: false,
            reconciliation_attempts: 0,
            automatic_budget: DEFAULT_AUTOMATIC_RECONCILIATION_BUDGET,
        }
    }
}

/// Attention hook only — #680 owns presentation (SPEC-028 ReconciliationRequired).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ReconciliationRequiredHook {
    pub action_id: ActionId,
    pub agent_run_id: AgentRunId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryDecision {
    pub runtime: ActionRuntime,
    pub may_retry_effect: bool,
    pub may_query_status: bool,
    pub may_reconsider_intent: bool,
    pub claims_rollback: bool,
    pub fresh_authorization_required: bool,
    pub automatic_reschedule_stopped: bool,
    pub attention: Option<ReconciliationRequiredHook>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryError {
    StaleDispatchGeneration,
    MissingCausalMarker,
    UnauthenticatedAuthoritativeClaim,
    IllegalCrashBoundary,
    IllegalReconcile,
}

pub fn recover(
    action_id: ActionId,
    agent_run_id: AgentRunId,
    effect_class: EffectClass,
    runtime: &ActionRuntime,
    boundary: CrashBoundary,
    evidence: &RecoveryEvidence,
) -> Result<RecoveryDecision, RecoveryError> {
    if runtime.lifecycle.is_known_terminal()
        && !matches!(runtime.lifecycle, ActionLifecycle::CancelledBeforeDispatch)
    {
        return Ok(terminal_decision(runtime.clone(), false));
    }
    if runtime.lifecycle == ActionLifecycle::Succeeded
        || runtime.lifecycle == ActionLifecycle::FailedKnown
    {
        return Ok(terminal_decision(runtime.clone(), false));
    }

    match boundary {
        CrashBoundary::BeforeIntentPersist => {
            // Store recover already loaded a row. Only the in-memory
            // Prepared stand-in (no generation) may claim "nothing persisted".
            if runtime.lifecycle == ActionLifecycle::Prepared
                && runtime.dispatch_generation.is_none()
            {
                return Ok(RecoveryDecision {
                    runtime: runtime.clone(),
                    may_retry_effect: false,
                    may_query_status: false,
                    may_reconsider_intent: false,
                    claims_rollback: false,
                    fresh_authorization_required: false,
                    automatic_reschedule_stopped: true,
                    attention: None,
                });
            }
            Err(RecoveryError::IllegalCrashBoundary)
        }
        CrashBoundary::AfterPrepared => match runtime.lifecycle {
            ActionLifecycle::Prepared => Ok(RecoveryDecision {
                runtime: runtime.clone(),
                may_retry_effect: false,
                may_query_status: false,
                may_reconsider_intent: true,
                claims_rollback: false,
                fresh_authorization_required: false,
                automatic_reschedule_stopped: false,
                attention: None,
            }),
            ActionLifecycle::Authorized => Ok(RecoveryDecision {
                runtime: runtime.clone(),
                may_retry_effect: false,
                may_query_status: false,
                may_reconsider_intent: false,
                claims_rollback: false,
                fresh_authorization_required: false,
                automatic_reschedule_stopped: false,
                attention: None,
            }),
            ActionLifecycle::Dispatching | ActionLifecycle::EffectUnknown => {
                recover_after_dispatch(
                    action_id,
                    agent_run_id,
                    effect_class,
                    runtime,
                    &RecoveryEvidence::none(),
                )
            }
            ActionLifecycle::CancelledAfterDispatch => Ok(stay_cancelled_after_dispatch(runtime)),
            _ => Err(RecoveryError::IllegalCrashBoundary),
        },
        CrashBoundary::AuthorizedBeforeDispatchCommit => match runtime.lifecycle {
            ActionLifecycle::Authorized => {
                let mut next = runtime.clone();
                next.lifecycle = ActionLifecycle::Prepared;
                next.authorization_invalidated = true;
                next.dispatch_generation = None;
                Ok(RecoveryDecision {
                    runtime: next,
                    may_retry_effect: false,
                    may_query_status: false,
                    may_reconsider_intent: true,
                    claims_rollback: false,
                    fresh_authorization_required: true,
                    automatic_reschedule_stopped: false,
                    attention: None,
                })
            }
            ActionLifecycle::Dispatching | ActionLifecycle::EffectUnknown => {
                recover_after_dispatch(
                    action_id,
                    agent_run_id,
                    effect_class,
                    runtime,
                    &RecoveryEvidence::none(),
                )
            }
            ActionLifecycle::CancelledAfterDispatch => Ok(stay_cancelled_after_dispatch(runtime)),
            _ => Err(RecoveryError::IllegalCrashBoundary),
        },
        CrashBoundary::DispatchingBeforeInvocation
        | CrashBoundary::DuringExecutor
        | CrashBoundary::AfterEffectBeforeResultPersist
        | CrashBoundary::AfterResultPersist => match runtime.lifecycle {
            ActionLifecycle::Dispatching
            | ActionLifecycle::EffectUnknown
            | ActionLifecycle::CancelledAfterDispatch => {
                recover_after_dispatch(action_id, agent_run_id, effect_class, runtime, evidence)
            }
            _ => Err(RecoveryError::IllegalCrashBoundary),
        },
    }
}

pub fn reconcile(
    action_id: ActionId,
    agent_run_id: AgentRunId,
    effect_class: EffectClass,
    runtime: &ActionRuntime,
    evidence: &RecoveryEvidence,
) -> Result<RecoveryDecision, RecoveryError> {
    match runtime.lifecycle {
        ActionLifecycle::Dispatching
        | ActionLifecycle::EffectUnknown
        | ActionLifecycle::CancelledAfterDispatch => {
            recover_after_dispatch(action_id, agent_run_id, effect_class, runtime, evidence)
        }
        _ => Err(RecoveryError::IllegalReconcile),
    }
}

/// SPEC-016 §16: cancellation linearizes against durable Dispatching.
pub fn linearize_cancel(runtime: &ActionRuntime) -> RecoveryDecision {
    let mut next = runtime.clone();
    next.cancel_requested = true;
    match runtime.lifecycle {
        ActionLifecycle::Prepared | ActionLifecycle::Authorized => {
            next.lifecycle = ActionLifecycle::CancelledBeforeDispatch;
            next.dispatch_generation = None;
            RecoveryDecision {
                runtime: next,
                may_retry_effect: false,
                may_query_status: false,
                may_reconsider_intent: false,
                claims_rollback: false,
                fresh_authorization_required: false,
                automatic_reschedule_stopped: true,
                attention: None,
            }
        }
        ActionLifecycle::Dispatching
        | ActionLifecycle::EffectUnknown
        | ActionLifecycle::CancelledAfterDispatch => {
            if runtime.lifecycle != ActionLifecycle::CancelledAfterDispatch {
                next.lifecycle = ActionLifecycle::CancelledAfterDispatch;
            }
            RecoveryDecision {
                runtime: next,
                may_retry_effect: false,
                may_query_status: true,
                may_reconsider_intent: false,
                claims_rollback: false,
                fresh_authorization_required: false,
                automatic_reschedule_stopped: true,
                attention: None,
            }
        }
        ActionLifecycle::Succeeded | ActionLifecycle::FailedKnown => {
            terminal_decision(runtime.clone(), false)
        }
        ActionLifecycle::CancelledBeforeDispatch => terminal_decision(runtime.clone(), false),
    }
}

fn recover_after_dispatch(
    action_id: ActionId,
    agent_run_id: AgentRunId,
    effect_class: EffectClass,
    runtime: &ActionRuntime,
    evidence: &RecoveryEvidence,
) -> Result<RecoveryDecision, RecoveryError> {
    if evidence.kind.is_authoritative() {
        if !evidence.executor_authenticated {
            return Err(RecoveryError::UnauthenticatedAuthoritativeClaim);
        }
        if let Some(claimed) = evidence.dispatch_generation
            && runtime.dispatch_generation.is_some()
            && runtime.dispatch_generation != Some(claimed)
        {
            return Err(RecoveryError::StaleDispatchGeneration);
        }
        if matches!(
            evidence.kind,
            EvidenceKind::CausalSuccess | EvidenceKind::CausalFailedKnown
        ) && evidence.causal.is_none()
        {
            return Err(RecoveryError::MissingCausalMarker);
        }
    }

    if runtime.lifecycle == ActionLifecycle::CancelledBeforeDispatch {
        return Ok(terminal_decision(runtime.clone(), false));
    }

    let mut next = runtime.clone();
    let mut may_query_status = false;
    let mut fresh_authorization_required = false;
    let mut attention = None;
    let mut automatic_reschedule_stopped = false;

    match evidence.kind {
        EvidenceKind::KnownNotDispatched => {
            if !matches!(
                runtime.lifecycle,
                ActionLifecycle::Dispatching | ActionLifecycle::EffectUnknown
            ) {
                return Err(RecoveryError::IllegalReconcile);
            }
            match (runtime.dispatch_generation, evidence.dispatch_generation) {
                (Some(stored), Some(claimed)) if stored == claimed => {}
                _ => return Err(RecoveryError::StaleDispatchGeneration),
            }
            next.lifecycle = ActionLifecycle::Prepared;
            next.dispatch_generation = None;
            next.authorization_invalidated = true;
            fresh_authorization_required = true;
        }
        EvidenceKind::CausalSuccess => {
            next.lifecycle = ActionLifecycle::Succeeded;
        }
        EvidenceKind::CausalFailedKnown => {
            next.lifecycle = ActionLifecycle::FailedKnown;
        }
        EvidenceKind::ValidatedIdempotencyContract
            if effect_class == EffectClass::IdempotentExternal
                && matches!(
                    runtime.lifecycle,
                    ActionLifecycle::Dispatching | ActionLifecycle::EffectUnknown
                ) =>
        {
            next.lifecycle = ActionLifecycle::Dispatching;
            may_query_status = true;
        }
        EvidenceKind::None
        | EvidenceKind::ObservedStateNoCausalMarker
        | EvidenceKind::UntrustedNarration
        | EvidenceKind::UntrustedIdempotencyClaim
        | EvidenceKind::OperatorAckWithoutCausalEvidence
        | EvidenceKind::ValidatedIdempotencyContract => {
            if matches!(runtime.lifecycle, ActionLifecycle::CancelledAfterDispatch) {
                next.lifecycle = ActionLifecycle::CancelledAfterDispatch;
            } else if runtime.lifecycle != ActionLifecycle::Succeeded
                && runtime.lifecycle != ActionLifecycle::FailedKnown
            {
                next.lifecycle = ActionLifecycle::EffectUnknown;
            }
            if next.lifecycle == ActionLifecycle::EffectUnknown
                || next.lifecycle == ActionLifecycle::CancelledAfterDispatch
            {
                next.reconciliation_attempts = next.reconciliation_attempts.saturating_add(1);
                if next.reconciliation_attempts >= next.automatic_budget {
                    automatic_reschedule_stopped = true;
                    attention = Some(ReconciliationRequiredHook {
                        action_id,
                        agent_run_id,
                    });
                } else {
                    attention = Some(ReconciliationRequiredHook {
                        action_id,
                        agent_run_id,
                    });
                }
            }
        }
    }

    if next.lifecycle == ActionLifecycle::EffectUnknown
        || next.lifecycle == ActionLifecycle::CancelledAfterDispatch
    {
        attention = Some(ReconciliationRequiredHook {
            action_id,
            agent_run_id,
        });
        if next.reconciliation_attempts >= next.automatic_budget {
            automatic_reschedule_stopped = true;
        }
    }

    Ok(RecoveryDecision {
        runtime: next,
        may_retry_effect: false,
        may_query_status,
        may_reconsider_intent: false,
        claims_rollback: false,
        fresh_authorization_required,
        automatic_reschedule_stopped,
        attention,
    })
}

fn stay_cancelled_after_dispatch(runtime: &ActionRuntime) -> RecoveryDecision {
    let mut next = runtime.clone();
    next.lifecycle = ActionLifecycle::CancelledAfterDispatch;
    RecoveryDecision {
        runtime: next,
        may_retry_effect: false,
        may_query_status: true,
        may_reconsider_intent: false,
        claims_rollback: false,
        fresh_authorization_required: false,
        automatic_reschedule_stopped: true,
        attention: None,
    }
}

fn terminal_decision(runtime: ActionRuntime, claims_rollback: bool) -> RecoveryDecision {
    RecoveryDecision {
        runtime,
        may_retry_effect: false,
        may_query_status: false,
        may_reconsider_intent: false,
        claims_rollback,
        fresh_authorization_required: false,
        automatic_reschedule_stopped: true,
        attention: None,
    }
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;
