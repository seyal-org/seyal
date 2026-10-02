//! Restore persisted identities into the in-memory authority.

use seyal_agent_core::{BindingGeneration, ControlGeneration, DomainError, WorkScopeKind};
use seyal_agent_store::{AggregateId, AgentStore};

use crate::{AuthorizationRepository, ObservationAuthority};

use super::wire::snapshot_payload;
use super::ServiceError;

/// Outbox marker appended when a recovered run fences generations and
/// rewrites its snapshot so GetSnapshot cannot report pre-crash liveness.
const EVENT_RECOVERY_FENCE: u16 = 3;

pub(super) fn restore_identities(
    store: &AgentStore,
    authority: &mut ObservationAuthority,
    auth: &mut AuthorizationRepository,
    principal_id: seyal_agent_core::ClientPrincipalId,
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
        authority.mark_recovered(id);
        fence_recovered_run(store, authority, id)?;
        auth.allow_run(principal_id, id)
            .map_err(|_| ServiceError::Failed)?;
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
