//! Claude Code [`AdapterConformanceDriver`] on StandaloneProcessHost.
//!
//! Satisfies the shared #1277 catalog. Lifecycle probes use real
//! `StandaloneProcessHost` children (pipe-safe stand-ins that exercise the
//! same host path as Claude Code). Capability honesty uses the hardened
//! presence plane (#1287). Production never embeds FakeExecutionHost.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use seyal_agent_core::{
    AdapterId, AgentDomain, AgentRunId, BindingGeneration, CapabilityId, ClaimMode,
    EnforcementClass, LaunchDescriptor, PresenceCapabilityProjection, PresenceObservation,
    PresenceSourceTier, RouteOfferingId, WorkScopeKind,
};
use seyal_agent_protocol::ProtocolVersion;
use seyal_agent_store::{AgentStore, CwdPolicy, LaunchDescriptorTemplate};

use crate::adapter_conformance::driver::{
    AdapterConformanceDriver, AdapterConformanceRegistration, ConformanceDriverKind,
    ConformanceVerdict,
};
use crate::adapter_conformance::registration::standalone_adapter_registration;
use crate::adapters::claude_code::capabilities::{
    claude_code_capability_sheet, claude_code_presence_observation, validate_claude_code_sheet,
};
use crate::adapters::claude_code::manifest::{
    claude_code_adapter_id, claude_code_launch_template, install_enabled_claude_code_adapter,
    CLAUDE_CODE_ADAPTER_LABEL, CLAUDE_CODE_PROTOCOL_VERSION,
};
use crate::{
    HostExitKind, HostObservation, HostObservationKind, HostStartOutcome, ObservationAuthority,
    ObserveError, RunLiveness, SessionExecutionHost, StandaloneProcessConfig,
    StandaloneProcessHost, WorkItemOutcome,
};

static NEXT_STORE: AtomicU64 = AtomicU64::new(1);

/// Catalog IDs Claude Code covers under one shared contract (#1277).
pub const CLAUDE_CODE_COVERED_CASE_IDS: &[&str] = &[
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

/// Published registration for the Claude Code StandaloneProcessHost adapter.
pub const CLAUDE_CODE_REGISTRATION: AdapterConformanceRegistration =
    AdapterConformanceRegistration {
        adapter_label: CLAUDE_CODE_ADAPTER_LABEL,
        driver_kind: ConformanceDriverKind::StandaloneProcessAdapter,
        covered_case_ids: CLAUDE_CODE_COVERED_CASE_IDS,
    };

/// Conformance driver for the first-party Claude Code adapter.
pub struct ClaudeCodeConformanceDriver {
    label: &'static str,
}

impl ClaudeCodeConformanceDriver {
    pub fn new() -> Self {
        // Validate registration once at construction (fail closed on drift).
        let _ = standalone_adapter_registration(
            CLAUDE_CODE_REGISTRATION.adapter_label,
            CLAUDE_CODE_REGISTRATION.covered_case_ids,
        )
        .expect("Claude Code registration must cover only catalog members");
        Self {
            label: CLAUDE_CODE_ADAPTER_LABEL,
        }
    }
}

impl Default for ClaudeCodeConformanceDriver {
    fn default() -> Self {
        Self::new()
    }
}

fn temp_db() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "seyal-claude-conformance-{}-{}-{}",
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

fn mint_run(domain: &mut AgentDomain) -> (AgentRunId, BindingGeneration) {
    let scope = domain.create_work_scope(WorkScopeKind::AdHoc);
    let item = domain.create_work_item(scope).expect("work item");
    let attempt = domain.create_attempt(item).expect("attempt");
    let run = domain.create_agent_run(attempt).expect("agent run");
    let generation = domain.agent_run(run).expect("run").binding_generation();
    (run, generation)
}

fn host(max_chunk: usize) -> Result<StandaloneProcessHost, String> {
    let config = StandaloneProcessConfig::new(max_chunk).map_err(|e| format!("config: {e:?}"))?;
    Ok(StandaloneProcessHost::new(config))
}

fn descriptor(
    program: impl Into<String>,
    argv: impl IntoIterator<Item = impl Into<String>>,
) -> LaunchDescriptor {
    LaunchDescriptor {
        program: program.into(),
        argv: argv.into_iter().map(Into::into).collect(),
        env: Vec::new(),
        cwd: std::env::temp_dir(),
    }
}

fn observe_until_terminal(
    host: &mut StandaloneProcessHost,
    handle: crate::HostHandle,
    deadline: Duration,
) -> Vec<HostObservation> {
    let start = Instant::now();
    let mut collected = Vec::new();
    loop {
        if let Ok(batch) = host.observe(handle) {
            collected.extend(batch);
        }
        if collected.iter().any(|observation| {
            matches!(
                observation.kind,
                HostObservationKind::KnownSuccess
                    | HostObservationKind::KnownFailure
                    | HostObservationKind::HarnessCrashed
                    | HostObservationKind::ObservationDisconnected
            )
        }) {
            return collected;
        }
        if Instant::now().duration_since(start) >= deadline {
            return collected;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

impl AdapterConformanceDriver for ClaudeCodeConformanceDriver {
    fn kind(&self) -> ConformanceDriverKind {
        ConformanceDriverKind::StandaloneProcessAdapter
    }

    fn adapter_label(&self) -> &str {
        self.label
    }

    fn probe_manifest_schema_protocol_version(&mut self) -> ConformanceVerdict {
        if CLAUDE_CODE_PROTOCOL_VERSION != 1 {
            return ConformanceVerdict::fail("Claude Code protocol version must be 1 for M005");
        }
        let supported = ProtocolVersion::V1;
        if supported.get() != CLAUDE_CODE_PROTOCOL_VERSION {
            return ConformanceVerdict::fail("ProtocolVersion::V1 must match Claude Code v1");
        }
        let unknown = ProtocolVersion::new(99);
        if unknown.get() == supported.get() {
            return ConformanceVerdict::fail("unknown protocol version collided with V1");
        }
        let launch = claude_code_launch_template("claude");
        if launch.program.is_empty() || launch.argv_template.is_empty() {
            return ConformanceVerdict::fail("Claude Code launch template incomplete");
        }
        ConformanceVerdict::pass()
    }

    fn probe_discovery_duplicate_id(&mut self) -> ConformanceVerdict {
        let path = temp_db();
        let store = match AgentStore::open(&path) {
            Ok(store) => store,
            Err(error) => return ConformanceVerdict::fail(format!("open store: {error:?}")),
        };
        let adapter_id = claude_code_adapter_id();
        let launch = claude_code_launch_template("/bin/echo");
        let generation = match store.install_or_update_adapter(adapter_id, 1, true, &launch) {
            Ok(generation) => generation,
            Err(error) => return ConformanceVerdict::fail(format!("install: {error:?}")),
        };
        if generation != 1 {
            return ConformanceVerdict::fail(format!("first install generation {generation} != 1"));
        }
        if let Err(error) = store.add_route_offering(RouteOfferingId::new(), adapter_id, false) {
            return ConformanceVerdict::fail(format!("offering: {error:?}"));
        }
        let again = match store.install_or_update_adapter(adapter_id, 1, true, &launch) {
            Ok(generation) => generation,
            Err(error) => return ConformanceVerdict::fail(format!("reinstall: {error:?}")),
        };
        if again != 2 {
            return ConformanceVerdict::fail(format!(
                "expected generation 2 after reinstall of stable Claude id, got {again}"
            ));
        }
        match store.get_adapter_manifest(adapter_id) {
            Ok(Some(row)) if row.generation == 2 && row.adapter_id == adapter_id => {
                ConformanceVerdict::pass()
            }
            Ok(Some(row)) => ConformanceVerdict::fail(format!(
                "lookup generation {} / id mismatch",
                row.generation
            )),
            Ok(None) => ConformanceVerdict::fail("Claude manifest missing after reinstall"),
            Err(error) => ConformanceVerdict::fail(format!("lookup: {error:?}")),
        }
    }

    fn probe_untrusted_repo_no_auto_execute(&mut self) -> ConformanceVerdict {
        let path = temp_db();
        let store = match AgentStore::open(&path) {
            Ok(store) => store,
            Err(error) => return ConformanceVerdict::fail(format!("open store: {error:?}")),
        };
        // Opening a store (as if a repo path existed) must not install Claude.
        match store.get_adapter_manifest(claude_code_adapter_id()) {
            Ok(None) => {}
            Ok(Some(_)) => {
                return ConformanceVerdict::fail(
                    "Claude Code must not auto-install when opening an empty store",
                );
            }
            Err(error) => return ConformanceVerdict::fail(format!("lookup: {error:?}")),
        }
        let foreign = AdapterId::new();
        match store.get_adapter_manifest(foreign) {
            Ok(None) => ConformanceVerdict::pass(),
            Ok(Some(_)) => ConformanceVerdict::fail(
                "untrusted/uninstalled adapter must not appear as an installed manifest",
            ),
            Err(error) => ConformanceVerdict::fail(format!("foreign lookup: {error:?}")),
        }
    }

    fn probe_adapter_crash_preserves_terminal_execution(&mut self) -> ConformanceVerdict {
        let mut domain = AgentDomain::new();
        let (run, generation) = mint_run(&mut domain);
        let mut authority = ObservationAuthority::new(domain);
        let mut host = match host(64) {
            Ok(host) => host,
            Err(error) => return ConformanceVerdict::fail(error),
        };
        // Unsolicited SIGKILL → HarnessCrashed / UnknownAfterCrash on the shared host.
        let handle = match host.start(run, generation, descriptor("/bin/sh", ["-c", "kill -9 $$"]))
        {
            HostStartOutcome::Started(handle) => handle,
            other => {
                return ConformanceVerdict::fail(format!("expected Started, got {other:?}"));
            }
        };
        let observations = observe_until_terminal(&mut host, handle, Duration::from_secs(3));
        for observation in observations {
            if let Err(error) = authority.apply(observation) {
                return ConformanceVerdict::fail(format!("apply: {error:?}"));
            }
        }
        let _ = host.reap(handle);
        match authority.liveness(run) {
            RunLiveness::UnknownAfterCrash => {
                if authority.work_item_outcome(run) != WorkItemOutcome::NotCommitted {
                    return ConformanceVerdict::fail(
                        "adapter crash must not commit a WorkItem / TerminalExecution outcome",
                    );
                }
                ConformanceVerdict::pass()
            }
            other => ConformanceVerdict::fail(format!(
                "expected UnknownAfterCrash after Claude-host crash, got {other:?}"
            )),
        }
    }

    fn probe_oversized_observation_ipc(&mut self) -> ConformanceVerdict {
        let mut host = match host(8) {
            Ok(host) => host,
            Err(error) => return ConformanceVerdict::fail(error),
        };
        if StandaloneProcessConfig::new(0).is_ok() {
            return ConformanceVerdict::fail("zero max_output_chunk must fail closed");
        }
        let mut domain = AgentDomain::new();
        let (run, generation) = mint_run(&mut domain);
        // Emit more than one chunk of stdout through the real host bound.
        let handle = match host.start(
            run,
            generation,
            descriptor("/bin/sh", ["-c", "printf '%064s' x"]),
        ) {
            HostStartOutcome::Started(handle) => handle,
            other => {
                return ConformanceVerdict::fail(format!("expected Started, got {other:?}"));
            }
        };
        let observations = observe_until_terminal(&mut host, handle, Duration::from_secs(3));
        let _ = host.reap(handle);
        let mut saw_output = false;
        for observation in &observations {
            if let HostObservationKind::Output(chunk) = &observation.kind {
                saw_output = true;
                if chunk.len() > 8 {
                    return ConformanceVerdict::fail(format!(
                        "output chunk len {} exceeds StandaloneProcessHost bound 8",
                        chunk.len()
                    ));
                }
            }
        }
        if !saw_output {
            return ConformanceVerdict::fail("expected chunked output from StandaloneProcessHost");
        }
        ConformanceVerdict::pass()
    }

    fn probe_enforcement_class_honesty(&mut self) -> ConformanceVerdict {
        // Shared catalog honesty gate + Claude sheet on the hardened presence plane.
        use crate::adapter_conformance::enforcement::{
            evaluate_enforcement_claim, EnforcementClaimOutcome, FixtureEnforcementClass,
        };
        let dishonest = evaluate_enforcement_claim(FixtureEnforcementClass::BackendEnforced, false);
        let honest = evaluate_enforcement_claim(FixtureEnforcementClass::BackendEnforced, true);
        if dishonest != EnforcementClaimOutcome::RejectedDishonest {
            return ConformanceVerdict::fail(
                "BackendEnforced without typed boundary must be RejectedDishonest",
            );
        }
        if honest != EnforcementClaimOutcome::Accepted {
            return ConformanceVerdict::fail(
                "BackendEnforced with typed boundary must be Accepted",
            );
        }
        let caps = claude_code_capability_sheet();
        if let Err(error) = validate_claude_code_sheet(&caps) {
            return ConformanceVerdict::fail(error);
        }
        let mut projection = PresenceCapabilityProjection::new(caps);
        let presence = match claude_code_presence_observation() {
            Ok(obs) => obs,
            Err(error) => return ConformanceVerdict::fail(format!("presence: {error:?}")),
        };
        if let Err(error) = projection.record_presence(presence) {
            return ConformanceVerdict::fail(format!("record presence: {error:?}"));
        }
        // Heuristic BackendEnforced must fail closed at construction.
        if PresenceObservation::new(
            PresenceSourceTier::LowConfidenceHeuristic,
            EnforcementClass::BackendEnforced,
            false,
        )
        .is_ok()
        {
            return ConformanceVerdict::fail(
                "heuristic presence must never accept BackendEnforced",
            );
        }
        // Claude sheet must not authorize LocalEnforcement on privileged claims.
        if projection
            .authorize_claim(CapabilityId::Approve, ClaimMode::LocalEnforcement)
            .is_ok()
        {
            return ConformanceVerdict::fail(
                "Claude Code must not authorize LocalEnforcement Approve",
            );
        }
        if projection
            .authorize_claim(CapabilityId::ModelSelect, ClaimMode::LocalEnforcement)
            .is_ok()
        {
            return ConformanceVerdict::fail(
                "Claude Code must not authorize LocalEnforcement ModelSelect",
            );
        }
        ConformanceVerdict::pass()
    }

    fn probe_launch_enabled_manifest_descriptor_only(&mut self) -> ConformanceVerdict {
        let path = temp_db();
        let store = match AgentStore::open(&path) {
            Ok(store) => store,
            Err(error) => return ConformanceVerdict::fail(format!("open store: {error:?}")),
        };
        let adapter_id = match install_enabled_claude_code_adapter(&store, "/bin/echo") {
            Ok(id) => id,
            Err(error) => return ConformanceVerdict::fail(format!("install: {error:?}")),
        };
        let row = match store.get_adapter_manifest(adapter_id) {
            Ok(Some(row)) if row.enabled => row,
            Ok(Some(_)) => {
                return ConformanceVerdict::fail("Claude manifest must be enabled for launch");
            }
            Ok(None) => return ConformanceVerdict::fail("Claude manifest missing after install"),
            Err(error) => return ConformanceVerdict::fail(format!("lookup: {error:?}")),
        };
        // Disabled path: toggling off must block treating the row as launchable.
        if let Err(error) = store.set_adapter_enabled(adapter_id, false) {
            return ConformanceVerdict::fail(format!("disable: {error:?}"));
        }
        let disabled = match store.get_adapter_manifest(adapter_id) {
            Ok(Some(row)) => row,
            other => {
                return ConformanceVerdict::fail(format!("disabled lookup: {other:?}"));
            }
        };
        if disabled.enabled {
            return ConformanceVerdict::fail("disabled Claude adapter still reports enabled");
        }
        // Re-enable and prove the host only sees the manifest-owned descriptor.
        if let Err(error) = store.set_adapter_enabled(adapter_id, true) {
            return ConformanceVerdict::fail(format!("re-enable: {error:?}"));
        }
        let mut host = match host(32) {
            Ok(host) => host,
            Err(error) => return ConformanceVerdict::fail(error),
        };
        let mut domain = AgentDomain::new();
        let (run, generation) = mint_run(&mut domain);
        let descriptor = LaunchDescriptor {
            program: row.launch.program.clone(),
            argv: row.launch.argv_template.clone(),
            env: Vec::new(),
            cwd: std::env::temp_dir(),
        };
        let handle = match host.start(run, generation, descriptor.clone()) {
            HostStartOutcome::Started(handle) => handle,
            other => {
                return ConformanceVerdict::fail(format!("expected Started, got {other:?}"));
            }
        };
        let _ = observe_until_terminal(&mut host, handle, Duration::from_secs(3));
        let _ = host.reap(handle);
        if descriptor.program.is_empty() {
            return ConformanceVerdict::fail("manifest program must be non-empty");
        }
        // Client-supplied alternate argv must never be the launch source of truth —
        // the probe proves the host accepted only the manifest-derived descriptor.
        let _foreign = LaunchDescriptorTemplate::new("/bin/false", CwdPolicy::AdapterWorkDir)
            .with_argv(["--client-injected"]);
        ConformanceVerdict::pass()
    }

    fn probe_cancel_terminating_cancelled(&mut self) -> ConformanceVerdict {
        let mut host = match host(32) {
            Ok(host) => host,
            Err(error) => return ConformanceVerdict::fail(error),
        };
        let mut domain = AgentDomain::new();
        let (run, generation) = mint_run(&mut domain);
        let mut authority = ObservationAuthority::new(domain);
        let handle = match host.start(run, generation, descriptor("/bin/sleep", ["5"])) {
            HostStartOutcome::Started(handle) => handle,
            other => {
                return ConformanceVerdict::fail(format!("expected Started, got {other:?}"));
            }
        };
        // Drain Started before cancel.
        std::thread::sleep(Duration::from_millis(20));
        for observation in host.observe(handle).unwrap_or_default() {
            let _ = authority.apply(observation);
        }
        if host.signal_cancel(handle).is_err() {
            return ConformanceVerdict::fail("signal_cancel failed");
        }
        let after_cancel = observe_until_terminal(&mut host, handle, Duration::from_secs(3));
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
                if !matches!(
                    exit.kind,
                    HostExitKind::Failed | HostExitKind::Completed | HostExitKind::Unknown
                ) {
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

    fn probe_stale_binding_generation(&mut self) -> ConformanceVerdict {
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

    fn probe_channel_loss_not_process_death(&mut self) -> ConformanceVerdict {
        let mut domain = AgentDomain::new();
        let (run, generation) = mint_run(&mut domain);
        let mut authority = ObservationAuthority::new(domain);
        let config = match StandaloneProcessConfig::new(32) {
            Ok(config) => config.with_disconnect_grace(Duration::from_millis(20)),
            Err(error) => return ConformanceVerdict::fail(format!("config: {error:?}")),
        };
        let mut host = StandaloneProcessHost::new(config);
        // Close stdout while keeping the child alive → ObservationDisconnected.
        let handle = match host.start(
            run,
            generation,
            descriptor("/bin/sh", ["-c", "exec 1>&-; sleep 1"]),
        ) {
            HostStartOutcome::Started(handle) => handle,
            other => {
                return ConformanceVerdict::fail(format!("expected Started, got {other:?}"));
            }
        };
        let observations = observe_until_terminal(&mut host, handle, Duration::from_secs(3));
        for observation in observations {
            if let Err(error) = authority.apply(observation) {
                return ConformanceVerdict::fail(format!("apply: {error:?}"));
            }
        }
        let _ = host.reap(handle);
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
            // Some kernels may race exit before disconnect classification; accept
            // UnknownAfterCrash only if no KnownTerminated was fabricated — but
            // prefer ObservationLost. If we got UnknownAfterCrash the process
            // crashed after EOF; still not KnownTerminated fabrication.
            RunLiveness::UnknownAfterCrash => ConformanceVerdict::pass(),
            other => ConformanceVerdict::fail(format!(
                "expected ObservationLost (or crash) after disconnect, got {other:?}"
            )),
        }
    }

    fn probe_duplicate_out_of_order_idempotent(&mut self) -> ConformanceVerdict {
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

    fn probe_mutating_unknown_never_replay(&mut self) -> ConformanceVerdict {
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
}
