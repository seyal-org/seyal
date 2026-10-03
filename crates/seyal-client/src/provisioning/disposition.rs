//! ADR-017 §6.3 disposition plans for never-bound executions.

use seyal_core::{AttachmentId, ExecutionId};

/// Which §6.3 row applies when an intent dies or bind/attach fails after Created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispositionKind {
    /// Never bound, never attached → attach Controller solely to dispose, one terminate.
    AttachThenTerminate,
    /// Attached as Controller, never bound → one terminate on existing attachment, then detach.
    TerminateThenDetach,
    /// Bound at any time → detach only; becomes unreferenced live execution.
    DetachOnly,
    /// Client died before the result → Runtime owns the outcome; survivor is unreferenced.
    RuntimeOwnedUnreferenced,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DispositionPlan {
    pub kind: DispositionKind,
    pub execution: ExecutionId,
    pub attachment: Option<AttachmentId>,
}

impl DispositionPlan {
    pub fn for_intent_death(
        bound: bool,
        attached: bool,
        execution: Option<ExecutionId>,
        attachment: Option<AttachmentId>,
    ) -> Option<Self> {
        let execution = execution?;
        if bound {
            return Some(Self {
                kind: DispositionKind::DetachOnly,
                execution,
                attachment,
            });
        }
        if attached {
            return Some(Self {
                kind: DispositionKind::TerminateThenDetach,
                execution,
                attachment,
            });
        }
        Some(Self {
            kind: DispositionKind::AttachThenTerminate,
            execution,
            attachment: None,
        })
    }
}
