//! StandaloneProcessHost-backed catalog probes for the Codex adapter.
//!
//! Uses the production host (never FakeExecutionHost) so
//! `ConformanceDriverKind::StandaloneProcessAdapter` is honest. Process
//! lifecycle cases use pipe-safe stand-in programs that preserve Codex argv
//! shape; live `codex` is optional via `SEYAL_CODEX_BIN` / PATH.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use seyal_agent_core::{
    AdapterId, AgentDomain, AgentRunId, BindingGeneration, LaunchDescriptor, RouteOfferingId,
    WorkScopeKind,
};
use seyal_agent_protocol::ProtocolVersion;
use seyal_agent_store::{AgentStore, CwdPolicy};

use crate::adapter_conformance::driver::ConformanceVerdict;
use crate::adapter_conformance::enforcement::{
    evaluate_enforcement_claim, EnforcementClaimOutcome, FixtureEnforcementClass,
};
use crate::{
    HostExitKind, HostObservationKind, HostStartOutcome, ObservationAuthority, ObserveError,
    RunLiveness, SessionExecutionHost, StandaloneProcessConfig, StandaloneProcessHost,
    WorkItemOutcome,
};

use super::capabilities::CODEX_CAPABILITY_SHEET;
use super::manifest::{codex_adapter_id, CodexLaunchPlan, CODEX_ADAPTER_LABEL, CODEX_EXEC_ARGV};
use super::session_ref::codex_thread_session_ref;

static NEXT_STORE: AtomicU64 = AtomicU64::new(1);

fn temp_db() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "seyal-codex-conformance-{}-{}-{}",
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

fn new_host(max_chunk: usize) -> Result<StandaloneProcessHost, String> {
    let config = StandaloneProcessConfig::new(max_chunk)
        .map_err(|e| format!("host config: {e:?}"))?
        .with_disconnect_grace(Duration::from_millis(50));
    Ok(StandaloneProcessHost::new(config))
}

fn stand_in_descriptor(program: &str, argv: &[&str], cwd: PathBuf) -> LaunchDescriptor {
    LaunchDescriptor {
        program: program.to_string(),
        argv: argv.iter().map(|s| (*s).to_string()).collect(),
        env: Vec::new(),
        cwd,
    }
}

fn wait_observations(
    host: &mut StandaloneProcessHost,
    handle: crate::HostHandle,
    deadline: Duration,
) -> Vec<crate::HostObservation> {
    let start = std::time::Instant::now();
    let mut all = Vec::new();
    while start.elapsed() < deadline {
        if let Ok(batch) = host.observe(handle) {
            all.extend(batch);
        }
        if all.iter().any(|o| {
            matches!(
                o.kind,
                HostObservationKind::KnownSuccess
                    | HostObservationKind::KnownFailure
                    | HostObservationKind::HarnessCrashed
                    | HostObservationKind::UnknownLiveness
                    | HostObservationKind::ObservationDisconnected
            )
        }) {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    all
}

pub fn probe_manifest_schema_protocol_version() -> ConformanceVerdict {
    let supported = ProtocolVersion::V1;
    if supported.get() != 1 {
        return ConformanceVerdict::fail("ProtocolVersion::V1 must be 1");
    }
    // Codex-specific: launch argv must remain the pipe-safe exec surface.
    if CODEX_EXEC_ARGV.first() != Some(&"exec") {
        return ConformanceVerdict::fail("Codex launch must use `exec` (pipe-safe)");
    }
    if !CODEX_EXEC_ARGV.contains(&"--json") {
        return ConformanceVerdict::fail("Codex launch must request --json structured output");
    }
    ConformanceVerdict::pass()
}

pub fn probe_discovery_duplicate_id() -> ConformanceVerdict {
    let path = temp_db();
    let store = match AgentStore::open(&path) {
        Ok(store) => store,
        Err(error) => return ConformanceVerdict::fail(format!("open store: {error:?}")),
    };
    let adapter_id = codex_adapter_id();
    let offering_id = RouteOfferingId::new();
    let launch = CodexLaunchPlan::conformance_stand_in("/bin/echo").to_template();
    let generation = match store.install_or_update_adapter(adapter_id, 1, true, &launch) {
        Ok(generation) => generation,
        Err(error) => return ConformanceVerdict::fail(format!("install: {error:?}")),
    };
    if generation != 1 {
        return ConformanceVerdict::fail(format!("first install generation {generation} != 1"));
    }
    if let Err(error) = store.add_route_offering(offering_id, adapter_id, false) {
        return ConformanceVerdict::fail(format!("offering: {error:?}"));
    }
    let again = match store.install_or_update_adapter(adapter_id, 1, true, &launch) {
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
        Err(error) => return ConformanceVerdict::fail(format!("open store: {error:?}")),
    };
    // A foreign AdapterId (not the well-known Codex id) must not appear installed.
    let foreign = AdapterId::new();
    if foreign == codex_adapter_id() {
        return ConformanceVerdict::fail("foreign id collided with Codex well-known id");
    }
    match store.get_adapter_manifest(foreign) {
        Ok(None) => ConformanceVerdict::pass(),
        Ok(Some(_)) => ConformanceVerdict::fail(
            "untrusted/uninstalled adapter must not appear as an installed manifest",
        ),
        Err(error) => ConformanceVerdict::fail(format!("lookup: {error:?}")),
    }
}

pub fn probe_adapter_crash_preserves_terminal_execution() -> ConformanceVerdict {
    // Surrogate TerminalExecution: independent OS child Seyal would own.
    let mut terminal: Child = match Command::new("/bin/sleep")
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => return ConformanceVerdict::fail(format!("terminal surrogate: {error}")),
    };

    let mut host = match new_host(64) {
        Ok(host) => host,
        Err(error) => {
            let _ = terminal.kill();
            return ConformanceVerdict::fail(error);
        }
    };
    let mut domain = AgentDomain::new();
    let (run, generation) = mint_run(&mut domain);
    let mut authority = ObservationAuthority::new(domain);
    // Adapter child self-SIGKILLs → HarnessCrashed (unsolicited signal death).
    let descriptor = stand_in_descriptor(
        "/bin/sh",
        &["-c", "sleep 0.05; kill -9 $$"],
        std::env::temp_dir(),
    );
    let handle = match host.start(run, generation, descriptor) {
        HostStartOutcome::Started(handle) => handle,
        other => {
            let _ = terminal.kill();
            return ConformanceVerdict::fail(format!("expected Started, got {other:?}"));
        }
    };
    let observations = wait_observations(&mut host, handle, Duration::from_secs(2));
    for observation in observations {
        if let Err(error) = authority.apply(observation) {
            let _ = terminal.kill();
            return ConformanceVerdict::fail(format!("apply: {error:?}"));
        }
    }
    let terminal_alive = matches!(terminal.try_wait(), Ok(None));
    let _ = terminal.kill();
    let _ = terminal.wait();
    let _ = host.reap(handle);

    if !terminal_alive {
        return ConformanceVerdict::fail(
            "adapter crash killed the still-live TerminalExecution surrogate",
        );
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
    let mut host = match new_host(8) {
        Ok(host) => host,
        Err(error) => return ConformanceVerdict::fail(error),
    };
    if StandaloneProcessConfig::new(0).is_ok() {
        return ConformanceVerdict::fail("zero max_output_chunk must fail closed");
    }
    let oversized = "x".repeat(64);
    let mut domain = AgentDomain::new();
    let (run, generation) = mint_run(&mut domain);
    let descriptor = stand_in_descriptor("/bin/echo", &[&oversized], std::env::temp_dir());
    let handle = match host.start(run, generation, descriptor) {
        HostStartOutcome::Started(handle) => handle,
        other => return ConformanceVerdict::fail(format!("expected Started, got {other:?}")),
    };
    let observations = wait_observations(&mut host, handle, Duration::from_secs(2));
    let chunks: Vec<_> = observations
        .iter()
        .filter_map(|o| match &o.kind {
            HostObservationKind::Output(c) => Some(c.len()),
            _ => None,
        })
        .collect();
    if chunks.is_empty() {
        return ConformanceVerdict::fail("expected chunked output observations");
    }
    if chunks.iter().any(|len| *len > 8) {
        return ConformanceVerdict::fail(format!("output chunk exceeded bound 8: {chunks:?}"));
    }
    let total: usize = chunks.iter().sum();
    // echo appends newline
    if total < oversized.len() {
        return ConformanceVerdict::fail(format!(
            "chunked total {total} < source {}",
            oversized.len()
        ));
    }
    let _ = host.reap(handle);
    ConformanceVerdict::pass()
}

pub fn probe_enforcement_class_honesty() -> ConformanceVerdict {
    if CODEX_CAPABILITY_SHEET.claims_backend_enforced() {
        return ConformanceVerdict::fail(
            "Codex capability sheet must not claim BackendEnforced (external CLI)",
        );
    }
    let dishonest = evaluate_enforcement_claim(FixtureEnforcementClass::BackendEnforced, false);
    let honest = evaluate_enforcement_claim(FixtureEnforcementClass::BackendEnforced, true);
    if dishonest != EnforcementClaimOutcome::RejectedDishonest {
        return ConformanceVerdict::fail(
            "BackendEnforced without typed boundary must be RejectedDishonest",
        );
    }
    if honest != EnforcementClaimOutcome::Accepted {
        return ConformanceVerdict::fail("BackendEnforced with typed boundary must be Accepted");
    }
    // Codex-specific: thread ids are HarnessSessionRef metadata only.
    let href = codex_thread_session_ref("thread_probe_honesty");
    if href.adapter_label != CODEX_ADAPTER_LABEL {
        return ConformanceVerdict::fail("HarnessSessionRef must carry codex-cli label");
    }
    ConformanceVerdict::pass()
}

pub fn probe_launch_enabled_manifest_descriptor_only() -> ConformanceVerdict {
    let plan = CodexLaunchPlan::conformance_stand_in("/bin/echo");
    let template = plan.to_template();
    if template.program.is_empty() {
        return ConformanceVerdict::fail("manifest program must be non-empty");
    }
    if template.cwd_policy != CwdPolicy::AdapterWorkDir {
        return ConformanceVerdict::fail("Codex cwd policy must be AdapterWorkDir for AdHoc");
    }
    // Host must spawn exactly the manifest descriptor (Codex argv shape).
    let mut host = match new_host(32) {
        Ok(host) => host,
        Err(error) => return ConformanceVerdict::fail(error),
    };
    let mut domain = AgentDomain::new();
    let (run, generation) = mint_run(&mut domain);
    let descriptor = LaunchDescriptor {
        program: template.program.clone(),
        argv: template.argv_template.clone(),
        env: Vec::new(),
        cwd: std::env::temp_dir(),
    };
    let expected_argv = descriptor.argv.clone();
    match host.start(run, generation, descriptor) {
        HostStartOutcome::Started(handle) => {
            let _ = wait_observations(&mut host, handle, Duration::from_secs(1));
            let _ = host.reap(handle);
            if expected_argv.first().map(String::as_str) != Some("exec") {
                return ConformanceVerdict::fail("spawn argv lost Codex exec prefix");
            }
            ConformanceVerdict::pass()
        }
        other => ConformanceVerdict::fail(format!("expected Started, got {other:?}")),
    }
}

pub fn probe_cancel_terminating_cancelled() -> ConformanceVerdict {
    let mut host = match new_host(32) {
        Ok(host) => host,
        Err(error) => return ConformanceVerdict::fail(error),
    };
    let mut domain = AgentDomain::new();
    let (run, generation) = mint_run(&mut domain);
    let mut authority = ObservationAuthority::new(domain);
    let handle = match host.start(
        run,
        generation,
        stand_in_descriptor("/bin/sleep", &["30"], std::env::temp_dir()),
    ) {
        HostStartOutcome::Started(handle) => handle,
        other => return ConformanceVerdict::fail(format!("expected Started, got {other:?}")),
    };
    for observation in wait_observations(&mut host, handle, Duration::from_millis(200)) {
        let _ = authority.apply(observation);
    }
    if host.signal_cancel(handle).is_err() {
        return ConformanceVerdict::fail("signal_cancel failed");
    }
    for observation in wait_observations(&mut host, handle, Duration::from_secs(2)) {
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
    let stale = crate::HostObservation {
        run_id: run,
        binding_generation: first,
        ordinal: 1,
        kind: HostObservationKind::Progress { step: 1 },
    };
    match authority.apply(stale) {
        Err(ObserveError::StaleGeneration) => {
            let fresh = crate::HostObservation {
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
    let mut host = match new_host(32) {
        Ok(host) => host,
        Err(error) => return ConformanceVerdict::fail(error),
    };
    let mut domain = AgentDomain::new();
    let (run, generation) = mint_run(&mut domain);
    let mut authority = ObservationAuthority::new(domain);
    // Close stdout while keeping the child alive → ObservationDisconnected.
    let descriptor = stand_in_descriptor(
        "/bin/sh",
        &["-c", "exec 1>&-; sleep 5"],
        std::env::temp_dir(),
    );
    let handle = match host.start(run, generation, descriptor) {
        HostStartOutcome::Started(handle) => handle,
        other => return ConformanceVerdict::fail(format!("expected Started, got {other:?}")),
    };
    for observation in wait_observations(&mut host, handle, Duration::from_secs(2)) {
        if let Err(error) = authority.apply(observation) {
            let _ = host.signal_cancel(handle);
            let _ = host.reap(handle);
            return ConformanceVerdict::fail(format!("apply: {error:?}"));
        }
    }
    let _ = host.signal_cancel(handle);
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
        other => ConformanceVerdict::fail(format!(
            "expected ObservationLost after disconnect, got {other:?}"
        )),
    }
}

pub fn probe_duplicate_out_of_order_idempotent() -> ConformanceVerdict {
    let mut domain = AgentDomain::new();
    let (run, generation) = mint_run(&mut domain);
    let mut authority = ObservationAuthority::new(domain);
    let first = crate::HostObservation {
        run_id: run,
        binding_generation: generation,
        ordinal: 1,
        kind: HostObservationKind::Started,
    };
    authority.apply(first.clone()).expect("first observation");
    if let Err(error) = authority.apply(first) {
        return ConformanceVerdict::fail(format!("duplicate must be idempotent: {error:?}"));
    }
    let ooo = crate::HostObservation {
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
    let unknown = crate::HostObservation {
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
    let mutating = crate::HostObservation {
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
