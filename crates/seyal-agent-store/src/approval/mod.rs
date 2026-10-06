//! Durable ApprovalAuthority (SPEC-028 §5 / SPEC-016 §5 recording + consume seam).
//!
//! Dispatch fencing stays #1310. This module records exact requests/decisions
//! and exposes single-use consume for the SPEC-016 fixture consumer.

mod ops;
pub(crate) use ops::{consume_in_tx, load_decision, load_request};
pub(crate) mod schema_v14;

#[cfg(test)]
mod tests;

pub use ops::{ApprovalAuthority, ApprovalStoreError, DecideInput, RecordedApproval};

use crate::sqlite::AgentStore;

impl AgentStore {
    pub fn approvals(&self) -> ApprovalAuthority<'_> {
        ApprovalAuthority::new(self)
    }
}
