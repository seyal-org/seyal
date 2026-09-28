//! SPEC-009 §8.2.1 / ADR-017 §6.4 reconnect and fresh-GUI resolution.

use seyal_core::ExecutionId;

/// Fresh client process with no recorded Pane→execution bindings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreshSessionPlan {
    /// Zero survivors → provision exactly one new execution for the initial Pane.
    ProvisionNew,
    /// Exactly one survivor → adopt it (Pass 9 continuity).
    Adopt(ExecutionId),
    /// Two or more → adopt none; provision new; leave survivors unreferenced.
    ProvisionNewLeaveSurvivors,
}

/// Within one live client session, reconnect binds by recorded ExecutionId.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconnectPlan {
    /// Recorded id is still listed → attach that exact execution.
    BindRecorded(ExecutionId),
    /// Recorded id missing from the list → no guess by list order.
    RecordedMissing,
    /// No recorded binding for this Pane.
    NoRecord,
}

pub(super) fn resolve_fresh_session(survivors: &[ExecutionId]) -> FreshSessionPlan {
    match survivors {
        [] => FreshSessionPlan::ProvisionNew,
        [only] => FreshSessionPlan::Adopt(*only),
        _ => FreshSessionPlan::ProvisionNewLeaveSurvivors,
    }
}

pub(super) fn resolve_reconnect(
    recorded: Option<ExecutionId>,
    listed: &[ExecutionId],
) -> ReconnectPlan {
    let Some(recorded) = recorded else {
        return ReconnectPlan::NoRecord;
    };
    if listed.iter().any(|id| *id == recorded) {
        ReconnectPlan::BindRecorded(recorded)
    } else {
        ReconnectPlan::RecordedMissing
    }
}
