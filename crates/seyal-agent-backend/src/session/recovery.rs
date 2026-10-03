//! Restore persisted identities into the in-memory authority.

use seyal_agent_core::{BindingGeneration, ControlGeneration, DomainError, WorkScopeKind};
use seyal_agent_store::{AgentStore, AggregateId};

use crate::{AuthorizationRepository, ObservationAuthority, RunLiveness};

use super::wire::{liveness_code, snapshot_payload};
use super::ServiceError;

/// Outbox marker appended when a recovered run fences generations and
/// rewrites its snapshot so GetSnapshot cannot report pre-crash *live*
/// liveness. Committed terminal observations remain KnownTerminated.
const EVENT_RECOVERY_FENCE: u16 = 3;

pub(super) fn restore_identities(
    store: &AgentStore,
    authority: &mut ObservationAuthority,
    auth: &mut AuthorizationRepository,
) -> Result<(), ServiceError> {
    for (id, kind) in store.work_scopes().map_err(|_| ServiceError::Failed)? {
        let kind = WorkScopeKind::from_code(kind).ok_or(ServiceError::Failed)?;
        authority
            .restore_work_scope(id, kind)
            .map_err(|_| ServiceError::Failed)?;
    }
    for (id, scope) in store.work_items().map_err(|_| ServiceError::Failed)? {
        authority
            .restore_work_item(id, scope)
            .map_err(|_| ServiceError::Failed)?;
    }
    for (id, item) in store.attempts().map_err(|_| ServiceError::Failed)? {
        authority
            .restore_attempt(id, item)
            .map_err(|_| ServiceError::Failed)?;
    }
    for (id, run) in store.agent_runs().map_err(|_| ServiceError::Failed)? {
        let binding =
            BindingGeneration::from_raw(run.binding_generation).ok_or(ServiceError::Failed)?;
        let control =
            ControlGeneration::from_raw(run.control_generation).ok_or(ServiceError::Failed)?;
        match authority.restore_agent_run(id, run.attempt_id, binding, control) {
            Ok(()) => {}
            Err(DomainError::UnknownAttempt(_)) => {
                // Migrated/crash-recovered runs may lack WorkScope/WorkItem/
                // Attempt parents. Quarantine the run as recoverable unknown
                // liveness instead of refusing daemon startup.
                authority
                    .restore_orphaned_agent_run(id, run.attempt_id, binding, control)
                    .map_err(|_| ServiceError::Failed)?;
            }
            Err(_) => return Err(ServiceError::Failed),
        }
        restore_committed_terminal_liveness(store, authority, id)?;
        authority.mark_recovered(id);
        fence_recovered_run(store, authority, id)?;
        auth.allow_run_for_observers(id);
    }
    Ok(())
}

/// Honor durable terminal observations before classifying recovery liveness.
///
/// Snapshot code 2 (KnownTerminated) is a fast path; the store also proves
/// terminal observation payloads when the snapshot is missing or stale.
fn restore_committed_terminal_liveness(
    store: &AgentStore,
    authority: &mut ObservationAuthority,
    run_id: seyal_agent_core::AgentRunId,
) -> Result<(), ServiceError> {
    if let Some((_, payload)) = store
        .get_snapshot(AggregateId::AgentRun(run_id))
        .map_err(|_| ServiceError::Failed)?
        && payload.first().copied() == Some(liveness_code(RunLiveness::KnownTerminated))
    {
        authority.note_committed_terminal(run_id);
        return Ok(());
    }
    if store
        .has_committed_terminal_observation(run_id)
        .map_err(|_| ServiceError::Failed)?
    {
        authority.note_committed_terminal(run_id);
    }
    Ok(())
}

/// After crash recovery, ADVANCES binding and control generations, persists
/// them, and rewrites the run snapshot so snapshot+replay cannot conclude
/// ScriptedLive from a stale stored payload (SPEC-017 §11 / §14).
fn fence_recovered_run(
    store: &AgentStore,
    authority: &mut ObservationAuthority,
    run_id: seyal_agent_core::AgentRunId,
) -> Result<(), ServiceError> {
    let run = *authority
        .domain()
        .agent_run(run_id)
        .ok_or(ServiceError::Failed)?;
    let next_binding = authority
        .advance_binding_generation(run_id, run.binding_generation())
        .map_err(|_| ServiceError::Failed)?;
    let next_control = authority
        .advance_control_generation(run_id, run.control_generation())
        .map_err(|_| ServiceError::Failed)?;
    let sequence = store
        .mutate_agent_run_and_append(
            run_id,
            run.attempt_id(),
            next_binding.get(),
            next_control.get(),
            EVENT_RECOVERY_FENCE,
            &[],
        )
        .map_err(|_| ServiceError::Failed)?;
    let payload = snapshot_payload(authority, run_id);
    store
        .snapshot(AggregateId::AgentRun(run_id), sequence, &payload)
        .map_err(|_| ServiceError::Failed)?;
    Ok(())
}
