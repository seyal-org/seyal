//! SPEC-028 / SPEC-016 approval recording seam.
//!
//! Control-plane only: does not start a host or wait on TerminalExecution.
//! Production dispatch must consume inside `dispatch_action`, not via
//! `consume_approval_exact`.

use seyal_agent_core::{
    ApprovalDecision, ApprovalError, ApprovalId, ApprovalRequest, ApprovalRequestSpec,
    ClientPrincipalId, ClientSessionId, ConsumptionWitness,
};
use seyal_agent_store::{ApprovalStoreError, DecideInput};

use crate::auth::ClientScope;

use super::IntegrationService;

impl IntegrationService {
    /// Record ApprovalRequestV1 + linked ApprovalRequired Attention.
    pub fn record_approval_request(
        &self,
        spec: ApprovalRequestSpec,
        summary: impl Into<String>,
    ) -> Result<ApprovalRequest, ApprovalStoreError> {
        self.store.approvals().record_request(spec, summary)
    }

    /// Record Approved|Rejected. Hostless/in-process tests may pass
    /// `require_session: false`. Production HITL uses
    /// [`Self::decide_approval_for_session`].
    pub fn decide_approval(
        &self,
        input: DecideInput,
    ) -> Result<ApprovalDecision, ApprovalStoreError> {
        self.store.approvals().decide(input)
    }

    /// Resolve the live ClientSession and `RunsControl` scope. Never copies
    /// client-supplied `session_valid` / `has_approval_decide_scope` flags.
    pub fn decide_approval_for_session(
        &self,
        principal_id: ClientPrincipalId,
        session_id: ClientSessionId,
        mut input: DecideInput,
    ) -> Result<ApprovalDecision, ApprovalStoreError> {
        if self
            .authorized_session(principal_id, session_id, ClientScope::RunsControl)
            .is_err()
        {
            return Err(ApprovalError::StaleSession.into());
        }
        input.require_session = true;
        input.client_session = Some(session_id);
        input.session_valid = true;
        input.has_approval_decide_scope = true;
        self.store.approvals().decide(input)
    }

    /// SPEC-016 fixture consumer. Does not enter Dispatching.
    pub fn consume_approval_exact(
        &self,
        approval_id: ApprovalId,
        witness: &ConsumptionWitness,
        now_unix_ms: u64,
    ) -> Result<ApprovalDecision, ApprovalStoreError> {
        self.store
            .approvals()
            .consume_exact(approval_id, witness, now_unix_ms)
    }
}
