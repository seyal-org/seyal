//! Single-owner dispatch fencing (SPEC-016 §§4–6, 20).
//!
//! Pure control-plane checks. Durable consume + `Dispatching` is one store
//! transaction in `seyal-agent-store`. Degraded/Paused persist health never
//! mints a new effect.

use super::intent::{ActionIntent, ActionLifecycle, AuthorizationClass};
use super::persist_pause::PersistHealth;
use super::recovery::ActionRuntime;

/// Frozen canonical ActionIntent identity layout. `encode()` always writes
/// `Prepared`; mutable lifecycle lives on the store column so the digest cannot
/// drift. Do not append identity fields without a versioned layout prefix.
pub const CANONICAL_INTENT_LAYOUT: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchError {
    NotAuthorized,
    AlreadyDispatching,
    Cancelled,
    PersistHealthNotMinting,
    Expired,
    StaleFence,
    StalePolicy,
    StaleBinding,
    IncompleteFence,
    ApprovalRequired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DispatchEval<'a> {
    pub intent: &'a ActionIntent,
    pub runtime: &'a ActionRuntime,
    pub persist_health: PersistHealth,
    pub now_ms: u64,
    pub current_policy_generation: u64,
    pub current_fence: &'a crate::memory::RevocationFence,
    pub current_binding_generation: u64,
    pub expected_binding_generation: u64,
}

pub fn next_dispatch_generation(runtime: &ActionRuntime) -> u64 {
    runtime.dispatch_generation.unwrap_or(0).saturating_add(1)
}

/// SPEC-016 §6 preconditions for the atomic `Authorized -> Dispatching` edge.
pub fn evaluate_dispatch(eval: DispatchEval<'_>) -> Result<u64, DispatchError> {
    if eval.persist_health != PersistHealth::Healthy || !eval.persist_health.allows_new_effect() {
        return Err(DispatchError::PersistHealthNotMinting);
    }
    match eval.runtime.lifecycle {
        ActionLifecycle::Dispatching => return Err(DispatchError::AlreadyDispatching),
        ActionLifecycle::Authorized => {}
        _ => return Err(DispatchError::NotAuthorized),
    }
    if eval.runtime.cancel_requested {
        return Err(DispatchError::Cancelled);
    }
    if eval.runtime.authorization_invalidated {
        return Err(DispatchError::NotAuthorized);
    }
    if eval
        .intent
        .expires_at_ms()
        .is_some_and(|expiry| eval.now_ms >= expiry)
    {
        return Err(DispatchError::Expired);
    }
    if eval.current_fence.members().is_empty() {
        return Err(DispatchError::IncompleteFence);
    }
    if eval.current_fence != eval.intent.revocation_fence() {
        return Err(DispatchError::StaleFence);
    }
    if eval.current_policy_generation != eval.intent.policy_generation() {
        return Err(DispatchError::StalePolicy);
    }
    if eval.current_binding_generation != eval.expected_binding_generation {
        return Err(DispatchError::StaleBinding);
    }
    let _ = CANONICAL_INTENT_LAYOUT;
    Ok(next_dispatch_generation(eval.runtime))
}

pub fn human_approval_required(intent: &ActionIntent) -> bool {
    intent.authorization_class() == AuthorizationClass::HumanApproval
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{
        ArgumentFingerprint, CapabilityRef, EffectClass, PrivacyDependencyId, RequestProvenance,
        ResourceIdentity,
    };
    use crate::memory::{RevocationFence, RevocationFenceMember, ScopeIdentity, ScopeKind};
    use crate::{ActionId, AgentRunId, RevocationGeneration};

    fn fence_at(generation: u64) -> RevocationFence {
        RevocationFence::new(vec![RevocationFenceMember {
            scope: ScopeIdentity::new(ScopeKind::Workspace, [1; 16]),
            generation: RevocationGeneration::from_raw(generation).unwrap(),
        }])
        .unwrap()
    }

    fn intent() -> ActionIntent {
        ActionIntent::prepare(
            ActionId::new(),
            AgentRunId::new(),
            CapabilityRef::new(b"fs.write").unwrap(),
            ResourceIdentity::new(b"file", [1; 16], [2; 16], [3; 32]).unwrap(),
            ArgumentFingerprint::of(b"a"),
            EffectClass::NonReplayable,
            1,
            PrivacyDependencyId([8; 16]),
            fence_at(1),
            RequestProvenance::AgentBackend,
            AuthorizationClass::HumanApproval,
            1_000,
            Some(5_000),
            None,
        )
        .unwrap()
    }

    fn eval<'a>(
        intent: &'a ActionIntent,
        runtime: &'a ActionRuntime,
        health: PersistHealth,
        policy: u64,
        fence: &'a RevocationFence,
        binding: u64,
    ) -> DispatchEval<'a> {
        DispatchEval {
            intent,
            runtime,
            persist_health: health,
            now_ms: 2_000,
            current_policy_generation: policy,
            current_fence: fence,
            current_binding_generation: binding,
            expected_binding_generation: binding,
        }
    }

    #[test]
    fn prepared_cannot_enter_dispatching() {
        let intent = intent();
        let runtime = ActionRuntime::prepared();
        let err = evaluate_dispatch(eval(
            &intent,
            &runtime,
            PersistHealth::Healthy,
            1,
            intent.revocation_fence(),
            1,
        ))
        .unwrap_err();
        assert_eq!(err, DispatchError::NotAuthorized);
    }

    #[test]
    fn degraded_persist_health_does_not_mint() {
        let intent = intent();
        let mut runtime = ActionRuntime::prepared();
        runtime.lifecycle = ActionLifecycle::Authorized;
        assert_eq!(
            evaluate_dispatch(eval(
                &intent,
                &runtime,
                PersistHealth::Degraded,
                1,
                intent.revocation_fence(),
                1,
            ))
            .unwrap_err(),
            DispatchError::PersistHealthNotMinting
        );
    }

    #[test]
    fn stale_fence_or_policy_fails_closed() {
        let intent = intent();
        let mut runtime = ActionRuntime::prepared();
        runtime.lifecycle = ActionLifecycle::Authorized;
        let stale = fence_at(2);
        assert_eq!(
            evaluate_dispatch(eval(
                &intent,
                &runtime,
                PersistHealth::Healthy,
                1,
                &stale,
                1,
            ))
            .unwrap_err(),
            DispatchError::StaleFence
        );
        assert_eq!(
            evaluate_dispatch(eval(
                &intent,
                &runtime,
                PersistHealth::Healthy,
                2,
                intent.revocation_fence(),
                1,
            ))
            .unwrap_err(),
            DispatchError::StalePolicy
        );
    }

    #[test]
    fn authorized_healthy_path_advances_generation() {
        let intent = intent();
        let mut runtime = ActionRuntime::prepared();
        runtime.lifecycle = ActionLifecycle::Authorized;
        runtime.dispatch_generation = Some(3);
        let mut args = eval(
            &intent,
            &runtime,
            PersistHealth::Healthy,
            1,
            intent.revocation_fence(),
            4,
        );
        args.expected_binding_generation = 4;
        args.current_binding_generation = 4;
        assert_eq!(evaluate_dispatch(args).unwrap(), 4);
    }
}
