//! Encode and map session results without owning the session.

use seyal_agent_core::{AgentRunId, DomainError};
use seyal_agent_protocol::{
    AggregateRef, CommandError, ReplayEvent, REPLAY_EVENT_OVERHEAD, REPLAY_RESULT_OVERHEAD,
};
use seyal_agent_store::{AggregateEventEnvelopeV1, AggregateId};

use crate::{
    AuthorizationError, HostObservation, HostObservationKind, ObservationAuthority, RunLiveness,
};

pub(super) enum ReplayPage {
    Events(Vec<ReplayEvent>),
    /// The next event alone exceeds the negotiated frame size.
    NextEventTooLarge,
}

/// Fill a Subscribe window only while the encoded Result frame fits.
///
/// Byte accounting is incremental. The last included sequence is the
/// continuation cursor. An empty [`ReplayPage::Events`] means no events remain.
pub(super) fn fit_replay_events(
    events: Vec<AggregateEventEnvelopeV1>,
    window: usize,
    max_frame_size: u32,
) -> ReplayPage {
    let mut selected = Vec::new();
    let mut used = REPLAY_RESULT_OVERHEAD;
    let budget = max_frame_size as usize;
    for event in events.into_iter().take(window) {
        let need = REPLAY_EVENT_OVERHEAD.saturating_add(event.payload.len());
        if used.saturating_add(need) > budget {
            if selected.is_empty() {
                return ReplayPage::NextEventTooLarge;
            }
            break;
        }
        used += need;
        selected.push(ReplayEvent {
            sequence: event.sequence.get(),
            kind: event.kind,
            payload: event.payload,
        });
    }
    ReplayPage::Events(selected)
}

pub(super) fn to_aggregate(aggregate: AggregateRef) -> AggregateId {
    match aggregate {
        AggregateRef::WorkScope(id) => AggregateId::WorkScope(id),
        AggregateRef::WorkItem(id) => AggregateId::WorkItem(id),
        AggregateRef::Attempt(id) => AggregateId::Attempt(id),
        AggregateRef::AgentRun(id) => AggregateId::AgentRun(id),
    }
}

pub(super) fn snapshot_payload(authority: &ObservationAuthority, run_id: AgentRunId) -> Vec<u8> {
    let run = authority.domain().agent_run(run_id);
    let mut payload = vec![liveness_code(authority.liveness(run_id))];
    if let Some(run) = run {
        payload.extend_from_slice(&run.binding_generation().get().to_le_bytes());
        payload.extend_from_slice(&run.control_generation().get().to_le_bytes());
    }
    payload
}

pub(super) fn liveness_code(liveness: RunLiveness) -> u8 {
    match liveness {
        RunLiveness::ScriptedLive => 1,
        RunLiveness::KnownTerminated => 2,
        RunLiveness::UnknownAfterCrash => 3,
        RunLiveness::ObservationLost => 4,
    }
}

pub(super) fn observation_payload(observation: &HostObservation) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&observation.ordinal.to_le_bytes());
    match &observation.kind {
        HostObservationKind::Started => payload.push(1),
        HostObservationKind::Progress { step } => {
            payload.push(2);
            payload.extend_from_slice(&step.to_le_bytes());
        }
        HostObservationKind::KnownSuccess => payload.push(3),
        HostObservationKind::KnownFailure => payload.push(4),
        HostObservationKind::HarnessCrashed | HostObservationKind::UnknownLiveness => {
            payload.push(5)
        }
        HostObservationKind::ObservationDisconnected => payload.push(6),
        HostObservationKind::ObservationReconnected => payload.push(7),
        // High-volume Output is persisted only through AgentStore::append_output_event.
        HostObservationKind::Output(_) => {
            unreachable!("output observations use append_output_event, not observation_payload")
        }
        HostObservationKind::Result(bytes) => {
            payload.push(9);
            payload.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            payload.extend_from_slice(bytes);
        }
        HostObservationKind::EffectUnknown => payload.push(10),
        HostObservationKind::Delayed { ticks } => {
            payload.push(11);
            payload.extend_from_slice(&ticks.to_le_bytes());
        }
    }
    payload
}

pub(super) fn map_auth(error: AuthorizationError) -> CommandError {
    match error {
        AuthorizationError::UnknownSession
        | AuthorizationError::StaleBackendInstance
        | AuthorizationError::SessionPrincipalMismatch => CommandError::RejectedSession,
        AuthorizationError::Malformed | AuthorizationError::ReplayedRequest => {
            CommandError::Malformed
        }
        _ => CommandError::Denied,
    }
}

pub(super) fn map_domain(error: DomainError) -> CommandError {
    match error {
        DomainError::StaleBindingGeneration { .. } => CommandError::StaleBinding,
        DomainError::StaleControlGeneration { .. } => CommandError::StaleControl,
        DomainError::UnknownWorkScope(_)
        | DomainError::UnknownWorkItem(_)
        | DomainError::UnknownAttempt(_)
        | DomainError::UnknownAgentRun(_) => CommandError::NotFound,
        DomainError::GenerationExhausted | DomainError::Conflict => CommandError::Failed,
    }
}
