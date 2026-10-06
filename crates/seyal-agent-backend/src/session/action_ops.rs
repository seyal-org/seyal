//! SPEC-016 ActionIntent prepare, EffectUnknown recovery, and persistence-failure pause.
//!
//! Control-plane only: these methods do not start a host or wait on
//! TerminalExecution.

use seyal_agent_core::{
    ActionId, ActionIntent, CrashBoundary, PersistHealth, RecoveryDecision, RecoveryEvidence,
};
use seyal_agent_store::{ActionError, PersistResume, PrepareOutcome};

use super::IntegrationService;

impl IntegrationService {
    /// Persist immutable ActionIntent before Seyal-controlled dispatch
    /// (SPEC-016 §3). Control-plane only: does not start a host or wait on
    /// TerminalExecution.
    pub fn prepare_action(&self, intent: &ActionIntent) -> Result<PrepareOutcome, ActionError> {
        self.store.actions().prepare(intent)
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
    /// before recovery. Control-plane only.
    pub fn resume_action_persist(&self, action_id: ActionId) -> Result<PersistResume, ActionError> {
        self.store.actions().resume_after_persist_health(action_id)
    }
}
