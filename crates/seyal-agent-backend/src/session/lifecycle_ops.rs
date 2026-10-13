//! SPEC-026 §9.1 / SPEC-027 §7 StartAgentRun lifecycle drive through the
//! domain writer.

use seyal_agent_core::{
    codes, resolve_execution_target, resolve_with_v1_ranking, AdapterCandidate, AgentRunId,
    AgentRunLifecycle, AttemptId, BindingGeneration, ClientPrincipalId, ClientSessionId,
    ControlGeneration, EvidenceValue, FactorEvidence, LaunchDescriptorRef, PolicyProfile,
    RankingCandidate, RankingRequest, ResolveFailure, RouteOfferingId, RoutingDecision,
    SelectionKind,
};
use seyal_agent_protocol::{CommandError, CommandResult};
use seyal_agent_store::{AggregateId, AggregateSequence, StoreError};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::{ClientScope, HostStartOutcome};

use super::launch_resolution::resolve_launch_descriptor;
use super::wire::snapshot_payload;
use super::IntegrationService;

const EVENT_RUN_CREATED: u16 = 1;

impl IntegrationService {
    /// Live eligibility candidates built from the durable adapter catalog
    /// (SPEC-027 §4.3). TTY-requiring offerings never satisfy the hard
    /// constraint: no composed host in this codebase owns a shared PTY
    /// (§9.5, §12 non-goals).
    fn build_adapter_candidates(&self) -> Vec<AdapterCandidate> {
        let Ok(offerings) = self.store.list_route_offerings() else {
            return Vec::new();
        };
        let mut candidates = Vec::with_capacity(offerings.len());
        for offering in offerings {
            let Ok(Some(manifest)) = self.store.get_adapter_manifest(offering.adapter_id) else {
                continue;
            };
            candidates.push(AdapterCandidate {
                adapter_id: offering.adapter_id,
                route_offering_id: offering.route_offering_id,
                adapter_enabled: manifest.enabled,
                adapter_manifest_generation: manifest.generation,
                hard_constraint_satisfied: !offering.requires_tty,
            });
        }
        candidates
    }

    /// Cold-start ranking candidates: hard facts from the catalog; soft factors
    /// Unknown and bound to the integrity-verified BaselineCalibrationArtifact
    /// (SPEC-020 §16). Not synthetic POC quality constants.
    fn build_ranking_candidates(&self) -> Vec<RankingCandidate> {
        self.build_adapter_candidates()
            .into_iter()
            .enumerate()
            .map(|(idx, c)| RankingCandidate {
                adapter_id: c.adapter_id,
                route_offering_id: c.route_offering_id,
                adapter_enabled: c.adapter_enabled,
                adapter_manifest_generation: c.adapter_manifest_generation,
                hard_constraint_satisfied: c.hard_constraint_satisfied,
                stable_key: u64::from(idx as u32)
                    ^ c.route_offering_id.to_bytes()[0..8]
                        .try_into()
                        .map(u64::from_le_bytes)
                        .unwrap_or(idx as u64),
                preference_rank: u32::MAX,
                quality: FactorEvidence::unknown(),
                context_fit: FactorEvidence::unknown(),
                tooling_fit: FactorEvidence::unknown(),
                reliability: FactorEvidence::unknown(),
                direct_expected_cost: EvidenceValue::Unknown,
                expected_total_cost: EvidenceValue::Unknown,
                expected_latency: EvidenceValue::Unknown,
                locality: FactorEvidence::unknown(),
                health_reliability: 500_000,
                model_generation: c.adapter_manifest_generation,
                network_enforcement_ok: true,
                no_network_required: false,
            })
            .collect()
    }

    /// SPEC-027 §4.3 / SPEC-020 envelope: pin → singleton → V1 soft rank.
    fn resolve_target(
        &self,
        route_offering_id: Option<RouteOfferingId>,
    ) -> Result<seyal_agent_core::ResolvedTarget, ResolveFailure> {
        let pin_candidates = self.build_adapter_candidates();
        match resolve_execution_target(route_offering_id, &pin_candidates) {
            Ok(resolved) => Ok(resolved),
            Err(ResolveFailure::AmbiguousOrNoTarget) if route_offering_id.is_none() => {
                let ranked = self.build_ranking_candidates();
                let request = RankingRequest::cold_start(PolicyProfile::Balanced);
                resolve_with_v1_ranking(None, &ranked, &request).map(|r| {
                    debug_assert_eq!(r.target.selection_kind, SelectionKind::RouterV1);
                    r.target
                })
            }
            Err(other) => Err(other),
        }
    }

    pub(super) fn start_agent_run(
        &mut self,
        principal_id: ClientPrincipalId,
        session_id: ClientSessionId,
        attempt_id: AttemptId,
        route_offering_id: Option<RouteOfferingId>,
    ) -> CommandResult {
        // SPEC-027 §7 step 1-2.
        let principal =
            match self.authorized_session(principal_id, session_id, ClientScope::RunsCreate) {
                Ok(principal) => principal,
                Err(error) => return CommandResult::Error(error),
            };
        // §7 step 3.
        if self.authority.domain().attempt(attempt_id).is_none() {
            return CommandResult::Error(CommandError::NotFound);
        }
        if self
            .authority
            .domain()
            .run_for_attempt(attempt_id)
            .is_some()
        {
            return CommandResult::Error(CommandError::Failed);
        }

        // §7 step 4 / §4.3 + SPEC-020 V1 ranking stage: resolve before any mint.
        let resolved = match self.resolve_target(route_offering_id) {
            Ok(resolved) => resolved,
            Err(ResolveFailure::AdapterDisabled { .. }) => {
                return CommandResult::Error(CommandError::AdapterNotEnabled)
            }
            Err(
                ResolveFailure::TargetUnavailable
                | ResolveFailure::AmbiguousOrNoTarget
                | ResolveFailure::NoRoute
                | ResolveFailure::BaselineIntegrity,
            ) => return CommandResult::Error(CommandError::ExecutionTargetUnavailable),
        };

        // §7 step 5: adapter.execute is independent of runs.create (D3).
        if self
            .auth
            .authorize_adapter_execute(session_id, self.instance_id, resolved.adapter_id, principal)
            .is_err()
        {
            return CommandResult::Error(CommandError::AdapterExecuteDenied);
        }

        // §7 step 6: a host of the required kind must be composed. Hostless
        // composition (`self.host == None`) fails closed here with the typed
        // code instead of generic `Failed` (§8.1).
        let Some(host_kind) = self.host.as_ref().map(|host| host.kind()) else {
            return CommandResult::Error(CommandError::ExecutionTargetUnavailable);
        };

        // §5/§6: resolve the launch descriptor from the frozen manifest and
        // this run's WorkScope kind before any mint. `run_id`/`binding` here
        // are pure in-memory values (SEYAL_RUN_ID env identity only); the
        // actual mint is the store write + domain restore at step 7 below.
        let run_id = AgentRunId::new();
        let binding = BindingGeneration::FIRST;
        let Some((work_scope_id, work_scope_kind)) = self
            .authority
            .domain()
            .attempt(attempt_id)
            .and_then(|attempt| self.authority.domain().work_item(attempt.work_item_id()))
            .and_then(|item| self.authority.domain().work_scope(item.work_scope_id()))
            .map(|scope| (scope.id(), scope.kind()))
        else {
            return CommandResult::Error(CommandError::Failed);
        };
        let Ok(Some(manifest)) = self.store.get_adapter_manifest_at_generation(
            resolved.adapter_id,
            resolved.adapter_manifest_generation,
        ) else {
            return CommandResult::Error(CommandError::ExecutionTargetUnavailable);
        };
        let bound_root = match self.store.work_scope_bound_root(work_scope_id) {
            Ok(path) => path.map(PathBuf::from),
            Err(_) => return CommandResult::Error(CommandError::Failed),
        };
        let launch_descriptor = match resolve_launch_descriptor(
            &manifest.launch,
            work_scope_kind,
            &self.adapter_work_root,
            resolved.adapter_id,
            run_id,
            binding,
            bound_root.as_deref(),
        ) {
            Ok(descriptor) => descriptor,
            // §6 missing-root, escaped `{work_scope_root}`, unresolved token.
            Err(_) => return CommandResult::Error(CommandError::ExecutionTargetUnavailable),
        };
        #[cfg(test)]
        {
            self.last_resolved_launch = Some(launch_descriptor.clone());
        }

        // §7 step 7: only now mint, persist, Created -> Prepared -> Dispatching.
        let control = ControlGeneration::FIRST;
        if self
            .store
            .mutate_agent_run_and_append(
                run_id,
                attempt_id,
                binding.get(),
                control.get(),
                EVENT_RUN_CREATED,
                &attempt_id.to_bytes(),
            )
            .and_then(|_| {
                self.authority
                    .restore_agent_run(run_id, attempt_id, binding, control)
                    .map_err(|_| StoreError::Corrupt)
            })
            .is_err()
        {
            return CommandResult::Error(CommandError::Failed);
        }

        // Grant auth before further durable writes so a mid-start persistence
        // fault still leaves the run observable (AB-1 fault-injection contract).
        if self.auth.allow_run(principal, run_id).is_err() {
            return CommandResult::Error(CommandError::Failed);
        }
        self.auth.allow_run_for_observers(run_id);

        let routing = RoutingDecision {
            adapter_id: resolved.adapter_id,
            adapter_manifest_generation: resolved.adapter_manifest_generation,
            route_offering_id: resolved.route_offering_id,
            execution_host_kind: host_kind,
            launch_descriptor_ref: LaunchDescriptorRef::new(resolved.adapter_manifest_generation),
            selection_kind: resolved.selection_kind,
        };
        if self
            .authority
            .domain_mut()
            .start_prepare_and_dispatch(run_id, routing)
            .is_err()
        {
            return CommandResult::Error(CommandError::Failed);
        }
        if persist_run_lifecycle(&self.store, &self.authority, run_id).is_err() {
            return CommandResult::Error(CommandError::Failed);
        }

        // SPEC-027 §5.2 / fixture 11: after mint the RoutingDecision has
        // frozen `adapter_manifest_generation`. A concurrent catalog edit
        // (or test injection) can make that generation unreachable before
        // `host.start`. Re-validate the frozen generation here — never
        // substitute the latest manifest, and never fabricate Active/
        // Completed. Align with §9.3 typed not-started: Dispatching → Prepared.
        #[cfg(test)]
        if self.invalidate_frozen_manifest_after_mint {
            self.invalidate_frozen_manifest_after_mint = false;
            let launch = seyal_agent_store::LaunchDescriptorTemplate::new(
                "/bin/false",
                seyal_agent_store::CwdPolicy::AdapterWorkDir,
            );
            let _ = self
                .store
                .install_or_update_adapter(routing.adapter_id, 0, true, &launch);
        }
        if !matches!(
            self.store.get_adapter_manifest_at_generation(
                routing.adapter_id,
                routing.adapter_manifest_generation,
            ),
            Ok(Some(_))
        ) {
            let _ = self
                .authority
                .domain_mut()
                .pre_start_fallback(run_id, routing, true);
            let _ = persist_run_lifecycle(&self.store, &self.authority, run_id);
            return CommandResult::Error(CommandError::ExecutionTargetUnavailable);
        }

        // SPEC-027 §9.2 fixture 12 ("`start` holds no service mutex across
        // child I/O" / "Concurrent ReadRun completes while child live"):
        // `host.start`/`host.observe` below still run while this method's
        // caller (daemon::serve::serve_session_locked) holds the
        // IntegrationService mutex, because `host` is a field behind that
        // same mutex, not an independently-lockable seam. What satisfies the
        // fixture is that the composed hosts no longer block on child I/O
        // internally (`start` spawns a background reader and returns
        // immediately; `observe` only drains an already-filled queue — see
        // `standalone_process_host.rs`): the hold is bounded to one
        // `Command::spawn` syscall plus one non-blocking channel drain, not
        // the child's full lifetime, so a second connection's concurrent
        // `ReadRun` is never stalled for anything close to that lifetime
        // (verified end-to-end against the real production binary and a
        // multi-second real child in
        // `production_concurrent_read_run_completes_promptly_while_the_real_child_is_still_live`,
        // crates/seyal-agent-backend/tests/process_qualification.rs).
        // Further decoupling `host` onto its own mutex so this is literally
        // zero-overlap rather than bounded-overlap is a possible future
        // hardening, not required to satisfy this fixture.
        let host = self.host.as_mut().expect("host checked present above");
        let observations = match host.start(run_id, binding, launch_descriptor) {
            HostStartOutcome::Started(handle) => {
                // Tracked so a later `ReadRun` can drain whatever the host's
                // background reader buffers after this synchronous observe
                // call returns (SPEC-027 §9.1/§9.4) — see
                // `active_hosted_runs` doc comment.
                self.active_hosted_runs.insert(run_id, handle);
                match host.observe(handle) {
                    Ok(observations) => observations,
                    Err(()) => return CommandResult::Error(CommandError::Failed),
                }
            }
            HostStartOutcome::NotStarted(_reason) => {
                // §9.3 typed not-started: Dispatching -> Prepared, same
                // AgentRun, a freshly minted RoutingDecision (content may be
                // identical absent a second eligible target to fall back to).
                let _ = self
                    .authority
                    .domain_mut()
                    .pre_start_fallback(run_id, routing, true);
                let _ = persist_run_lifecycle(&self.store, &self.authority, run_id);
                return CommandResult::Error(CommandError::ExecutionTargetUnavailable);
            }
        };
        if let Err(error) = self.commit_observation_batch(observations) {
            return CommandResult::Error(error);
        }

        // Host confirmation → Active when still Dispatching (scripted start).
        if self
            .authority
            .domain()
            .agent_run(run_id)
            .is_some_and(|run| run.lifecycle() == AgentRunLifecycle::Dispatching)
            && self
                .authority
                .domain_mut()
                .activate_agent_run(run_id)
                .is_ok()
        {
            let _ = persist_run_lifecycle(&self.store, &self.authority, run_id);
        }

        let aggregate = AggregateId::AgentRun(run_id);
        let event_count = match self.store.high_water(aggregate) {
            Ok(count) if count > 0 => count,
            _ => return CommandResult::Error(CommandError::Failed),
        };
        let Some(last) = AggregateSequence::from_raw(event_count) else {
            return CommandResult::Error(CommandError::Failed);
        };
        let payload = snapshot_payload(&self.authority, run_id);
        if self.store.snapshot(aggregate, last, &payload).is_err() {
            return CommandResult::Error(CommandError::Failed);
        }
        CommandResult::Started {
            run_id,
            binding_generation: binding.get(),
            control_generation: control.get(),
            event_count,
        }
    }

    /// SPEC-026 §9.2 / SPEC-027 §9.4 fixture 13: wire `CancelRun` → Terminating
    /// → evidenced `Terminated(Cancelled)` via `signal_cancel` + observe/reap.
    pub(crate) fn cancel_agent_run_command(
        &mut self,
        principal_id: ClientPrincipalId,
        session_id: ClientSessionId,
        run_id: AgentRunId,
        control_generation: u64,
    ) -> CommandResult {
        if let Err(error) =
            self.authorized_run(principal_id, session_id, ClientScope::RunsControl, run_id)
        {
            return CommandResult::Error(error);
        }
        let Some(control) = ControlGeneration::from_raw(control_generation) else {
            return CommandResult::Error(CommandError::Malformed);
        };
        if let Err(error) = self
            .authority
            .domain_mut()
            .cancel_agent_run(run_id, control, None, false)
        {
            return CommandResult::Error(super::wire::map_domain(error));
        }
        if persist_run_lifecycle(&self.store, &self.authority, run_id).is_err() {
            return CommandResult::Error(CommandError::Failed);
        }

        let lifecycle = self
            .authority
            .domain()
            .agent_run(run_id)
            .map(|run| run.lifecycle());
        if lifecycle == Some(AgentRunLifecycle::Terminating) {
            if self.active_hosted_runs.contains_key(&run_id) {
                if let Some(host) = self.host.as_mut()
                    && let Some(&handle) = self.active_hosted_runs.get(&run_id)
                {
                    let _ = host.signal_cancel(handle);
                }
                // Poll observe until terminal evidence lands (reader is async).
                // Do not reap first — reap drops the observation buffer.
                let deadline = Instant::now() + Duration::from_secs(2);
                loop {
                    if let Err(error) = self.drain_host_observations(run_id) {
                        return CommandResult::Error(error);
                    }
                    if matches!(
                        self.authority.liveness(run_id),
                        crate::RunLiveness::KnownTerminated | crate::RunLiveness::UnknownAfterCrash
                    ) {
                        break;
                    }
                    if Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
            if matches!(
                self.authority.liveness(run_id),
                crate::RunLiveness::KnownTerminated | crate::RunLiveness::UnknownAfterCrash
            ) && self
                .authority
                .domain()
                .agent_run(run_id)
                .is_some_and(|run| run.lifecycle() == AgentRunLifecycle::Terminating)
            {
                let _ = self
                    .authority
                    .domain_mut()
                    .confirm_cancel_termination(run_id);
                // Cancelled runs expose KnownTerminated on the wire (fixture 13),
                // even when the host classified signal death as crash before the
                // cancel remap landed.
                self.authority.note_committed_terminal(run_id);
            }
            let _ = persist_run_lifecycle(&self.store, &self.authority, run_id);
        }

        let Some(run) = self.authority.domain().agent_run(run_id) else {
            return CommandResult::Error(CommandError::NotFound);
        };
        CommandResult::Run {
            binding_generation: run.binding_generation().get(),
            control_generation: run.control_generation().get(),
            liveness: super::wire::liveness_code(self.authority.liveness(run_id)),
        }
    }

    /// Daemon shutdown: signal-and-reap every live hosted child (AGENTS.md
    /// termination invariant / SPEC-027 §9.4).
    pub(crate) fn shutdown_hosted_runs(&mut self) {
        let handles: Vec<_> = self
            .active_hosted_runs
            .drain()
            .map(|(_, handle)| handle)
            .collect();
        let Some(host) = self.host.as_mut() else {
            return;
        };
        for handle in handles {
            let _ = host.signal_cancel(handle);
            let _ = host.reap(handle);
        }
        host.shutdown_all();
    }
}

fn persist_run_lifecycle(
    store: &seyal_agent_store::AgentStore,
    authority: &crate::ObservationAuthority,
    run_id: AgentRunId,
) -> Result<(), StoreError> {
    let run = authority
        .domain()
        .agent_run(run_id)
        .ok_or(StoreError::Corrupt)?;
    // Column-only: StartAgentRun already appends the create outbox event and
    // host observations. Lifecycle facts are durable on the identity row; later
    // control transitions append Critical outbox events when they mutate.
    store.set_agent_run_lifecycle_columns(
        run_id,
        codes::agent_run_lifecycle(run.lifecycle()),
        codes::execution_liveness(run.execution_liveness()),
        codes::observation(run.observation()),
        codes::resumability(run.resumability()),
        run.run_revision(),
    )
}
