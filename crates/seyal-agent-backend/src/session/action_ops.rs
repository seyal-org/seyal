//! SPEC-016 ActionIntent prepare, dispatch fencing, recovery, persist-pause.
//!
//! Control-plane only: these methods do not start a host or wait on
//! TerminalExecution.

use seyal_agent_core::{
    ActionId, ActionIntent, ClientPrincipalId, ClientSessionId, CrashBoundary, PersistHealth,
    RecoveryDecision, RecoveryEvidence,
};
use seyal_agent_store::{
    ActionError, DispatchInput, DispatchOutcome, PersistResume, PrepareOutcome,
};

use crate::auth::ClientScope;

use super::IntegrationService;

impl IntegrationService {
    /// Persist immutable ActionIntent before Seyal-controlled dispatch
    /// (SPEC-016 §3). In-process seam; does not start a host.
    pub fn prepare_action(&self, intent: &ActionIntent) -> Result<PrepareOutcome, ActionError> {
        self.store.actions().prepare(intent)
    }

    /// RPC/harness prepare: live session + principal must already own the
    /// AgentRun. Does not trust caller-supplied session flags.
    pub fn prepare_action_for_run(
        &self,
        principal_id: ClientPrincipalId,
        session_id: ClientSessionId,
        intent: &ActionIntent,
        expected_binding_generation: u64,
    ) -> Result<PrepareOutcome, ActionError> {
        if self
            .authorized_run(
                principal_id,
                session_id,
                ClientScope::RunsCreate,
                intent.agent_run_id(),
            )
            .is_err()
        {
            return Err(ActionError::CallerRunDenied);
        }
        self.store
            .actions()
            .prepare_bound(intent, Some(expected_binding_generation))
    }

    /// Atomic approval consumption + `Dispatching`. Does not start a host.
    pub fn dispatch_action(
        &self,
        action_id: ActionId,
        input: DispatchInput,
    ) -> Result<DispatchOutcome, ActionError> {
        self.store.actions().dispatch(action_id, input)
    }

    pub fn authorize_action(&self, action_id: ActionId, now_ms: u64) -> Result<(), ActionError> {
        self.store.actions().authorize(action_id, now_ms)
    }

    /// SPEC-016 EffectUnknown recovery. Control-plane only: does not wait on
    /// TerminalExecution or enter the PTY/VT/Metal hot path.
    pub fn recover_action(
        &self,
        action_id: ActionId,
        boundary: CrashBoundary,
        evidence: &RecoveryEvidence,
    ) -> Result<RecoveryDecision, ActionError> {
        self.store.actions().recover(action_id, boundary, evidence)
    }

    pub fn reconcile_action(
        &self,
        action_id: ActionId,
        evidence: &RecoveryEvidence,
    ) -> Result<RecoveryDecision, ActionError> {
        self.store.actions().reconcile(action_id, evidence)
    }

    pub fn cancel_action(&self, action_id: ActionId) -> Result<RecoveryDecision, ActionError> {
        self.store.actions().cancel(action_id)
    }

    pub fn action_persist_health(&self) -> PersistHealth {
        self.store.action_persist_health()
    }

    /// Resume Action persist after the store is healthy. Revalidates fences
    /// before recovery. Control-plane only. Resume is not authorization.
    pub fn resume_action_persist(&self, action_id: ActionId) -> Result<PersistResume, ActionError> {
        self.store.actions().resume_after_persist_health(action_id)
    }
}
