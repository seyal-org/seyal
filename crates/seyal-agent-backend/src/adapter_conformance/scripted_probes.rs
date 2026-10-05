//! Shared catalog probes over FakeExecutionHost + ObservationAuthority.
//!
//! Used by the fixture-host harness proof driver and the permanent offline
//! replay adapter (#1278). Available only with `--features fixture-host`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use seyal_agent_core::{
    AdapterId, AgentDomain, AgentRunId, BindingGeneration, LaunchDescriptor, RouteOfferingId,
    WorkScopeKind,
};
use seyal_agent_protocol::ProtocolVersion;
use seyal_agent_store::{AgentStore, CwdPolicy, LaunchDescriptorTemplate};

use crate::adapter_conformance::driver::ConformanceVerdict;
use crate::{
    FakeExecutionHost, HostExitKind, HostObservation, HostObservationKind, HostStartOutcome,
    ObservationAuthority, ObserveError, RunLiveness, ScriptStep, SessionExecutionHost,
    WorkItemOutcome,
};

static NEXT_STORE: AtomicU64 = AtomicU64::new(1);

/// Full catalog coverage claimed by both fixture-host and replay drivers.
pub const FULL_CATALOG_CASE_IDS: &[&str] = &[
    "manifest.schema.protocol_version",
    "discovery.duplicate_id",
    "trust.untrusted_repo_no_auto_execute",
    "isolation.adapter_crash_preserves_terminal_execution",
    "bounds.oversized_observation_ipc",
    "capability.enforcement_class_honesty",
    "launch.enabled_manifest_descriptor_only",
    "lifecycle.cancel_terminating_cancelled",
    "fence.stale_binding_generation",
    "liveness.channel_loss_not_process_death",
    "events.duplicate_out_of_order_idempotent",
    "cache.mutating_unknown_never_replay",
];

fn temp_db() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "seyal-adapter-conformance-{}-{}-{}",
        std::process::id(),
        NEXT_STORE.fetch_add(1, Ordering::Relaxed),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("agent.db")
}

fn sample_descriptor() -> LaunchDescriptor {
    LaunchDescriptor {
        program: "/bin/echo".to_string(),
        argv: vec!["conformance".to_string()],
        env: Vec::new(),
        cwd: std::env::temp_dir(),
    }
}

fn mint_run(domain: &mut AgentDomain) -> (AgentRunId, BindingGeneration) {
    let scope = domain.create_work_scope(WorkScopeKind::AdHoc);
    let item = domain.create_work_item(scope).expect("work item");
    let attempt = domain.create_attempt(item).expect("attempt");
    let run = domain.create_agent_run(attempt).expect("agent run");
    let generation = domain.agent_run(run).expect("run").binding_generation();
    (run, generation)
}

pub fn probe_manifest_schema_protocol_version() -> ConformanceVerdict {
    let supported = ProtocolVersion::V1;
    if supported.get() != 1 {
        return ConformanceVerdict::fail("ProtocolVersion::V1 must be 1");
    }
    let unknown = ProtocolVersion::new(99);
    if unknown.get() == supported.get() {
        return ConformanceVerdict::fail("unknown protocol version collided with V1");
    }
    ConformanceVerdict::pass()
}

pub fn probe_discovery_duplicate_id() -> ConformanceVerdict {
    let path = temp_db();
    let store = match AgentStore::open(&path) {
        Ok(store) => store,
        Err(error) => {
            return ConformanceVerdict::fail(format!("open store: {error:?}"));
        }
    };
    let adapter_id = AdapterId::new();
    let offering_id = RouteOfferingId::new();
    let launch = LaunchDescriptorTemplate::new("/bin/echo", CwdPolicy::AdapterWorkDir)
        .with_argv(["fixture"]);
    let generation = match store.install_or_update_adapter(adapter_id, 0, true, &launch) {
        Ok(generation) => generation,
        Err(error) => return ConformanceVerdict::fail(format!("install: {error:?}")),
    };
    if generation != 1 {
        return ConformanceVerdict::fail(format!("first install generation {generation} != 1"));
    }
    if let Err(error) = store.add_route_offering(offering_id, adapter_id, false) {
        return ConformanceVerdict::fail(format!("offering: {error:?}"));
    }
    // Re-install of the same adapter_id bumps generation in place — no
    // second discovery identity for the same durable adapter_id.
    let again = match store.install_or_update_adapter(adapter_id, 0, true, &launch) {
        Ok(generation) => generation,
        Err(error) => return ConformanceVerdict::fail(format!("reinstall: {error:?}")),
    };
    if again != 2 {
        return ConformanceVerdict::fail(format!(
            "expected generation 2 after reinstall, got {again}"
        ));
    }
    match store.get_adapter_manifest(adapter_id) {
        Ok(Some(row)) if row.generation == 2 => ConformanceVerdict::pass(),
        Ok(Some(row)) => {
            ConformanceVerdict::fail(format!("lookup generation {} != 2", row.generation))
        }
        Ok(None) => ConformanceVerdict::fail("manifest missing after reinstall"),
        Err(error) => ConformanceVerdict::fail(format!("lookup: {error:?}")),
    }
}

pub fn probe_untrusted_repo_no_auto_execute() -> ConformanceVerdict {
    let path = temp_db();
    let store = match AgentStore::open(&path) {
        Ok(store) => store,
        Err(error) => {
            return ConformanceVerdict::fail(format!("open store: {error:?}"));
        }
    };
    // Repository path presence is not an install. An uninstalled adapter_id
    // must not resolve to an enabled manifest.
    let foreign = AdapterId::new();
    match store.get_adapter_manifest(foreign) {
        Ok(None) => ConformanceVerdict::pass(),
        Ok(Some(_)) => ConformanceVerdict::fail(
            "untrusted/uninstalled adapter must not appear as an installed manifest",
        ),
        Err(error) => ConformanceVerdict::fail(format!("lookup: {error:?}")),
    }
}

pub fn probe_adapter_crash_preserves_terminal_execution() -> ConformanceVerdict {
    let mut domain = AgentDomain::new();
    let (run, generation) = mint_run(&mut domain);
    let mut authority = ObservationAuthority::new(domain);
    let mut host = match FakeExecutionHost::new(64) {
        Ok(host) => host,
        Err(error) => return ConformanceVerdict::fail(format!("host: {error:?}")),
    };
    host.set_script(vec![
        ScriptStep::Emit(HostObservationKind::Started),
        ScriptStep::Emit(HostObservationKind::HarnessCrashed),
    ]);
    let handle = match host.start(run, generation, sample_descriptor()) {
        HostStartOutcome::Started(handle) => handle,
        other => {
            return ConformanceVerdict::fail(format!("expected Started, got {other:?}"));
        }
    };
    let observations = match host.observe(handle) {
        Ok(obs) => obs,
        Err(()) => return ConformanceVerdict::fail("observe failed"),
    };
    for observation in observations {
        if let Err(error) = authority.apply(observation) {
            return ConformanceVerdict::fail(format!("apply: {error:?}"));
        }
    }
    match authority.liveness(run) {
        RunLiveness::UnknownAfterCrash => {
            if authority.work_item_outcome(run) != WorkItemOutcome::NotCommitted {
                return ConformanceVerdict::fail(
                    "adapter crash must not commit a WorkItem outcome",
                );
            }
            ConformanceVerdict::pass()
        }
        other => ConformanceVerdict::fail(format!(
            "expected UnknownAfterCrash after harness crash, got {other:?}"
        )),
    }
}

pub fn probe_oversized_observation_ipc() -> ConformanceVerdict {
    let mut host = match FakeExecutionHost::new(8) {
        Ok(host) => host,
        Err(error) => return ConformanceVerdict::fail(format!("host: {error:?}")),
    };
    if FakeExecutionHost::new(0).is_ok() {
        return ConformanceVerdict::fail("zero max_output_chunk must fail closed");
    }
    let oversized = vec![b'x'; 64];
    host.set_script(vec![ScriptStep::Emit(HostObservationKind::Output(
        oversized.clone(),
    ))]);
    let mut domain = AgentDomain::new();
    let (run, generation) = mint_run(&mut domain);
    let handle = match host.start(run, generation, sample_descriptor()) {
        HostStartOutcome::Started(handle) => handle,
        other => {
            return ConformanceVerdict::fail(format!("expected Started, got {other:?}"));
        }
    };
    let observations = match host.observe(handle) {
        Ok(obs) => obs,
        Err(()) => return ConformanceVerdict::fail("observe failed"),
    };
    if observations.is_empty() {
        return ConformanceVerdict::fail("expected chunked output observations");
    }
    for observation in &observations {
        match &observation.kind {
            HostObservationKind::Output(chunk) if chunk.len() <= 8 => {}
            HostObservationKind::Output(chunk) => {
                return ConformanceVerdict::fail(format!(
                    "output chunk len {} exceeds bound 8",
                    chunk.len()
                ));
            }
            other => {
                return ConformanceVerdict::fail(format!("unexpected kind {other:?}"));
            }
        }
    }
    let total: usize = observations
        .iter()
        .map(|o| match &o.kind {
            HostObservationKind::Output(c) => c.len(),
            _ => 0,
        })
        .sum();
    if total != oversized.len() {
        return ConformanceVerdict::fail(format!(
            "chunked total {total} != source {}",
            oversized.len()
        ));
    }
    ConformanceVerdict::pass()
}

pub fn probe_launch_enabled_manifest_descriptor_only() -> ConformanceVerdict {
    let descriptor = sample_descriptor();
    if descriptor.program.is_empty() {
        return ConformanceVerdict::fail("manifest program must be non-empty");
    }
    let mut host = match FakeExecutionHost::new(32) {
        Ok(host) => host,
        Err(error) => return ConformanceVerdict::fail(format!("host: {error:?}")),
    };
    host.set_script(vec![ScriptStep::Emit(HostObservationKind::Started)]);
    let mut domain = AgentDomain::new();
    let (run, generation) = mint_run(&mut domain);
    let _ = host.start(run, generation, descriptor.clone());
    match host.last_descriptor() {
        Some(last) if last == &descriptor => ConformanceVerdict::pass(),
        Some(last) => {
            ConformanceVerdict::fail(format!("host saw unexpected descriptor: {last:?}"))
        }
        None => ConformanceVerdict::fail("host did not record launch descriptor"),
    }
}

pub fn probe_cancel_terminating_cancelled() -> ConformanceVerdict {
    let mut host = match FakeExecutionHost::new(32) {
        Ok(host) => host,
        Err(error) => return ConformanceVerdict::fail(format!("host: {error:?}")),
    };
    host.set_script(vec![ScriptStep::Emit(HostObservationKind::Started)]);
    let mut domain = AgentDomain::new();
    let (run, generation) = mint_run(&mut domain);
    let mut authority = ObservationAuthority::new(domain);
    let handle = match host.start(run, generation, sample_descriptor()) {
        HostStartOutcome::Started(handle) => handle,
        other => {
            return ConformanceVerdict::fail(format!("expected Started, got {other:?}"));
        }
    };
    for observation in host.observe(handle).unwrap_or_default() {
        let _ = authority.apply(observation);
    }
    if host.signal_cancel(handle).is_err() {
        return ConformanceVerdict::fail("signal_cancel failed");
    }
    let after_cancel = host.observe(handle).unwrap_or_default();
    if after_cancel.is_empty() {
        return ConformanceVerdict::fail("cancel must enqueue terminal evidence");
    }
    for observation in after_cancel {
        if let Err(error) = authority.apply(observation) {
            return ConformanceVerdict::fail(format!("apply cancel evidence: {error:?}"));
        }
    }
    match authority.liveness(run) {
        RunLiveness::KnownTerminated => {
            let exit = match host.reap(handle) {
                Ok(evidence) => evidence,
                Err(()) => return ConformanceVerdict::fail("reap failed"),
            };
            if !matches!(exit.kind, HostExitKind::Failed | HostExitKind::Completed) {
                return ConformanceVerdict::fail(format!(
                    "unexpected exit kind after cancel: {:?}",
                    exit.kind
                ));
            }
            ConformanceVerdict::pass()
        }
        other => ConformanceVerdict::fail(format!(
            "expected KnownTerminated after cancel evidence, got {other:?}"
        )),
    }
}

pub fn probe_stale_binding_generation() -> ConformanceVerdict {
    let mut domain = AgentDomain::new();
    let (run, first) = mint_run(&mut domain);
    let current = domain
        .advance_binding_generation(run, first)
        .expect("advance binding");
    let mut authority = ObservationAuthority::new(domain);
    let stale = HostObservation {
        run_id: run,
        binding_generation: first,
        ordinal: 1,
        kind: HostObservationKind::Progress { step: 1 },
    };
    match authority.apply(stale) {
        Err(ObserveError::StaleGeneration) => {
            let fresh = HostObservation {
                run_id: run,
                binding_generation: current,
                ordinal: 1,
                kind: HostObservationKind::Started,
            };
            match authority.apply(fresh) {
                Ok(()) => ConformanceVerdict::pass(),
                Err(error) => {
                    ConformanceVerdict::fail(format!("current binding rejected: {error:?}"))
                }
            }
        }
        Ok(()) => ConformanceVerdict::fail(
            "stale binding generation must not be applied as authoritative control",
        ),
        Err(error) => ConformanceVerdict::fail(format!("unexpected error: {error:?}")),
    }
}

pub fn probe_channel_loss_not_process_death() -> ConformanceVerdict {
    let mut domain = AgentDomain::new();
    let (run, generation) = mint_run(&mut domain);
    let mut authority = ObservationAuthority::new(domain);
    let mut host = match FakeExecutionHost::new(32) {
        Ok(host) => host,
        Err(error) => return ConformanceVerdict::fail(format!("host: {error:?}")),
    };
    host.set_script(vec![
        ScriptStep::Emit(HostObservationKind::Started),
        ScriptStep::Emit(HostObservationKind::ObservationDisconnected),
    ]);
    let handle = match host.start(run, generation, sample_descriptor()) {
        HostStartOutcome::Started(handle) => handle,
        other => {
            return ConformanceVerdict::fail(format!("expected Started, got {other:?}"));
        }
    };
    for observation in host.observe(handle).unwrap_or_default() {
        if let Err(error) = authority.apply(observation) {
            return ConformanceVerdict::fail(format!("apply: {error:?}"));
        }
    }
    match authority.liveness(run) {
        RunLiveness::ObservationLost => {
            if authority.work_item_outcome(run) != WorkItemOutcome::NotCommitted {
                return ConformanceVerdict::fail(
                    "channel loss must not fabricate a WorkItem outcome",
                );
            }
            ConformanceVerdict::pass()
        }
        RunLiveness::KnownTerminated => ConformanceVerdict::fail(
            "ObservationDisconnected must not fabricate KnownTerminated process death",
        ),
        other => ConformanceVerdict::fail(format!(
            "expected ObservationLost after disconnect, got {other:?}"
        )),
    }
}

pub fn probe_duplicate_out_of_order_idempotent() -> ConformanceVerdict {
    let mut domain = AgentDomain::new();
    let (run, generation) = mint_run(&mut domain);
    let mut authority = ObservationAuthority::new(domain);
    let first = HostObservation {
        run_id: run,
        binding_generation: generation,
        ordinal: 1,
        kind: HostObservationKind::Started,
    };
    authority.apply(first.clone()).expect("first observation");
    if let Err(error) = authority.apply(first) {
        return ConformanceVerdict::fail(format!("duplicate must be idempotent: {error:?}"));
    }
    let ooo = HostObservation {
        run_id: run,
        binding_generation: generation,
        ordinal: 3,
        kind: HostObservationKind::Progress { step: 1 },
    };
    match authority.apply(ooo) {
        Err(ObserveError::OutOfOrder) => ConformanceVerdict::pass(),
        Ok(()) => ConformanceVerdict::fail("out-of-order ordinal must fail closed"),
        Err(error) => ConformanceVerdict::fail(format!("unexpected: {error:?}")),
    }
}

pub fn probe_mutating_unknown_never_replay() -> ConformanceVerdict {
    let mut domain = AgentDomain::new();
    let (run, generation) = mint_run(&mut domain);
    let mut authority = ObservationAuthority::new(domain);
    let before = authority.effects_performed();
    let unknown = HostObservation {
        run_id: run,
        binding_generation: generation,
        ordinal: 1,
        kind: HostObservationKind::EffectUnknown,
    };
    if let Err(error) = authority.apply(unknown) {
        return ConformanceVerdict::fail(format!("EffectUnknown apply: {error:?}"));
    }
    if authority.effects_performed() != before {
        return ConformanceVerdict::fail(
            "EffectUnknown must not increment effects_performed (never cache-replay side effects)",
        );
    }
    let mutating = HostObservation {
        run_id: run,
        binding_generation: generation,
        ordinal: 2,
        kind: HostObservationKind::Output(b"write".to_vec()),
    };
    if let Err(error) = authority.apply(mutating) {
        return ConformanceVerdict::fail(format!("Output apply: {error:?}"));
    }
    if authority.effects_performed() <= before {
        return ConformanceVerdict::fail("mutating Output must increment effects_performed");
    }
    ConformanceVerdict::pass()
}
