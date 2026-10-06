//! SPEC-028 / SPEC-016 approval recording seam.
//!
//! Control-plane only: does not start a host or wait on TerminalExecution.

use seyal_agent_core::{
    ApprovalDecision, ApprovalId, ApprovalRequest, ApprovalRequestSpec, ConsumptionWitness,
};
use seyal_agent_store::{ApprovalStoreError, DecideInput};

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

    /// Record Approved|Rejected under `approval.decide`. Resolves Attention.
    pub fn decide_approval(
        &self,
        input: DecideInput,
    ) -> Result<ApprovalDecision, ApprovalStoreError> {
        self.store.approvals().decide(input)
    }

    /// SPEC-016 fixture consumer. Does not enter Dispatching (#1310).
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
