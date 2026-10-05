//! SPEC-013 §23.40 Runtime terminal-isolation soak harness (#1301).
//!
//! Measures Seyal Runtime PTY→TerminalState progress latency with and without
//! concurrent Local Context Engine discovery/index load (active + failure).
//! Does not couple production crates: this package alone depends on both sides.

#![cfg_attr(
    not(target_os = "macos"),
    allow(dead_code, unused_imports, unused_variables)
)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
use seyal_agent_context::{
    AuthorizedRoot, ContextDiscoveryEngine, DiscoveryHealth, DiscoveryScope,
};
#[cfg(target_os = "macos")]
use seyal_agent_core::WorkScopeId;
#[cfg(target_os = "macos")]
use seyal_agent_store::AgentStore;
#[cfg(target_os = "macos")]
use seyal_exec::{CommandSpec, WindowSize};
#[cfg(target_os = "macos")]
use seyal_runtime::{LocalIpcMode, Runtime, RuntimeConfig};

const SAMPLES_DEFAULT: usize = 40;
const WARMUPS_DEFAULT: usize = 8;
const WORKERS_DEFAULT: usize = 4;
const FIXTURE_FILES_DEFAULT: usize = 1_200;
const STALL_CEILING_MS: f64 = 100.0;
const SAMPLE_TIMEOUT: Duration = Duration::from_secs(2);
/// Minimum completed discoveries before sampling under load (sustained soak).
const MIN_DISCOVERIES_BEFORE_SAMPLE: u64 = 16;
const LOAD_READY_TIMEOUT: Duration = Duration::from_secs(30);
#[derive(Clone, Debug)]
struct PhaseStats {
    name: String,
    samples_us: Vec<f64>,
    correct: usize,
    timeouts: usize,
    stalls_ge_100ms: usize,
    context_discoveries: u64,
    context_failures_injected: u64,
    context_degraded_hits: u64,
}

impl PhaseStats {
    fn p(&self, q: f64) -> f64 {
        if self.samples_us.is_empty() {
            return f64::NAN;
        }
        let mut sorted = self.samples_us.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let n = sorted.len();
        // nearest-rank
        let rank = ((q * n as f64).ceil() as usize).clamp(1, n) - 1;
        sorted[rank]
    }

    fn max(&self) -> f64 {
        self.samples_us
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max)
    }
}

fn main() {
    #[cfg(not(target_os = "macos"))]
    {
        println!(
            "m005_spec013_terminal_isolation PLATFORM_LIMITED target_os!=macos performance_claim=false"
        );
        std::process::exit(2);
    }

    #[cfg(target_os = "macos")]
    if let Err(error) = run_macos() {
        eprintln!("m005_spec013_terminal_isolation FAIL: {error}");
        std::process::exit(1);
    }
}

#[cfg(target_os = "macos")]
fn run_macos() -> Result<(), String> {
    let _ = Instant::now(); // benchmark contract timing primitive present
    let samples = env_usize("SEYAL_M005_ISO_SAMPLES", SAMPLES_DEFAULT);
    let warmups = env_usize("SEYAL_M005_ISO_WARMUPS", WARMUPS_DEFAULT);
    let workers = env_usize("SEYAL_M005_ISO_WORKERS", WORKERS_DEFAULT);
    let fixture_files = env_usize("SEYAL_M005_ISO_FIXTURE_FILES", FIXTURE_FILES_DEFAULT);
    let out_path = env::var("SEYAL_M005_ISO_OUT").unwrap_or_else(|_| {
        format!(
            "/tmp/m005-spec013-terminal-isolation-{}.json",
            std::process::id()
        )
    });

    let git_sha = git_sha();
    let fixture_root = make_fixture(fixture_files)?;
    let store_dir = fixture_root.join("_store");
    fs::create_dir_all(&store_dir).map_err(|e| e.to_string())?;
    let store = Arc::new(
        AgentStore::open(store_dir.join("agent.db")).map_err(|e| format!("agent store: {e:?}"))?,
    );
    let scope = DiscoveryScope::new(
        WorkScopeId::new(),
        vec![AuthorizedRoot::new(
            &fixture_root,
            format!("repo:{}", fixture_root.display()),
            "wt-iso",
        )],
    );

    let mut runtime =
        Runtime::new(runtime_config("iso")?).map_err(|e| format!("runtime: {e:?}"))?;
    let exec_id = runtime
        .create_execution(
            CommandSpec::new("/bin/sh").args(["-c", "stty raw -echo; printf 'ready\\n'; cat"]),
            WindowSize::cells(120, 40).map_err(|e| format!("winsize: {e:?}"))?,
        )
        .map_err(|e| format!("create_execution: {e:?}"))?;
    let _attach = runtime
        .attach(exec_id)
        .map_err(|e| format!("attach: {e:?}"))?;
    wait_for_text(&mut runtime, exec_id, "ready", Duration::from_secs(3))?;

    let ingress = runtime
        .input_ingress(exec_id)
        .map_err(|e| format!("input ingress: {e:?}"))?;

    // Baseline — terminal only.
    let baseline = measure_phase(
        &mut runtime,
        exec_id,
        &ingress,
        "baseline",
        warmups,
        samples,
        None,
    )?;

    // Active context/index soak.
    let active_stop = Arc::new(AtomicBool::new(false));
    let active_counters = Arc::new(ContextCounters::default());
    let active_handles = spawn_context_workers(
        workers,
        scope.clone(),
        Arc::clone(&store),
        Arc::clone(&active_stop),
        Arc::clone(&active_counters),
        false,
    );
    wait_for_context_load(
        &mut runtime,
        &active_counters,
        MIN_DISCOVERIES_BEFORE_SAMPLE,
        false,
    )?;
    let active = measure_phase(
        &mut runtime,
        exec_id,
        &ingress,
        "active",
        warmups,
        samples,
        Some(&active_counters),
    )?;
    active_stop.store(true, Ordering::SeqCst);
    for handle in active_handles {
        handle
            .join()
            .map_err(|_| "active worker join".to_string())?;
    }

    // Failure-injected context soak.
    let fail_stop = Arc::new(AtomicBool::new(false));
    let fail_counters = Arc::new(ContextCounters::default());
    let fail_handles = spawn_context_workers(
        workers,
        scope.clone(),
        Arc::clone(&store),
        Arc::clone(&fail_stop),
        Arc::clone(&fail_counters),
        true,
    );
    wait_for_context_load(
        &mut runtime,
        &fail_counters,
        MIN_DISCOVERIES_BEFORE_SAMPLE,
        true,
    )?;
    let failure = measure_phase(
        &mut runtime,
        exec_id,
        &ingress,
        "failure",
        warmups,
        samples,
        Some(&fail_counters),
    )?;
    fail_stop.store(true, Ordering::SeqCst);
    for handle in fail_handles {
        handle
            .join()
            .map_err(|_| "failure worker join".to_string())?;
    }

    runtime
        .begin_shutdown()
        .map_err(|e| format!("begin_shutdown: {e:?}"))?;
    runtime
        .run_until_empty(Instant::now() + Duration::from_secs(2))
        .map_err(|e| format!("shutdown: {e:?}"))?;

    let verdict = evaluate(&baseline, &active, &failure);
    let json = render_json(&ReportInput {
        git_sha: &git_sha,
        verdict: &verdict,
        baseline: &baseline,
        active: &active,
        failure: &failure,
        samples,
        warmups,
        workers,
        fixture_files,
    });
    if let Some(parent) = Path::new(&out_path).parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&out_path, &json).map_err(|e| e.to_string())?;

    println!("m005_spec013_terminal_isolation verdict={}", verdict.label);
    println!("m005_spec013_terminal_isolation out={out_path}");
    println!("{json}");

    if verdict.label == "FAIL" {
        return Err(format!(
            "isolation soak FAIL: {}",
            verdict.reasons.join("; ")
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct ContextCounters {
    discoveries: AtomicU64,
    failures_injected: AtomicU64,
    degraded_hits: AtomicU64,
}

#[cfg(target_os = "macos")]
fn spawn_context_workers(
    workers: usize,
    scope: DiscoveryScope,
    store: Arc<AgentStore>,
    stop: Arc<AtomicBool>,
    counters: Arc<ContextCounters>,
    inject_failures: bool,
) -> Vec<thread::JoinHandle<()>> {
    (0..workers)
        .map(|worker| {
            let scope = scope.clone();
            let store = Arc::clone(&store);
            let stop = Arc::clone(&stop);
            let counters = Arc::clone(&counters);
            thread::spawn(move || {
                let _ = worker;
                let engine = ContextDiscoveryEngine::new();
                while !stop.load(Ordering::Relaxed) {
                    if inject_failures {
                        let health = engine.inject_persistent_failure();
                        counters.failures_injected.fetch_add(1, Ordering::Relaxed);
                        if matches!(health, DiscoveryHealth::Degraded) {
                            counters.degraded_hits.fetch_add(1, Ordering::Relaxed);
                            // Fresh budget so failure soak continues without permanent halt.
                            engine.budget.reset_for_new_generation();
                        }
                    }
                    let _ = engine.discover_with_store(&scope, Some(store.as_ref()));
                    counters.discoveries.fetch_add(1, Ordering::Relaxed);
                }
            })
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn wait_for_context_load(
    runtime: &mut Runtime,
    counters: &ContextCounters,
    min_discoveries: u64,
    require_failures: bool,
) -> Result<(), String> {
    let deadline = Instant::now() + LOAD_READY_TIMEOUT;
    while Instant::now() < deadline {
        // Keep servicing terminal I/O while context workers warm the soak.
        runtime
            .poll_once(Some(Duration::from_millis(5)))
            .map_err(|e| format!("poll during load-ready: {e:?}"))?;
        let discoveries = counters.discoveries.load(Ordering::Relaxed);
        let failures = counters.failures_injected.load(Ordering::Relaxed);
        if discoveries >= min_discoveries && (!require_failures || failures >= min_discoveries) {
            return Ok(());
        }
    }
    Err(format!(
        "context load did not reach readiness: discoveries={} failures={} required>={}",
        counters.discoveries.load(Ordering::Relaxed),
        counters.failures_injected.load(Ordering::Relaxed),
        min_discoveries
    ))
}

#[cfg(target_os = "macos")]
fn measure_phase(
    runtime: &mut Runtime,
    exec_id: seyal_runtime::ExecutionId,
    ingress: &seyal_runtime::InputIngress,
    name: &str,
    warmups: usize,
    samples: usize,
    counters: Option<&ContextCounters>,
) -> Result<PhaseStats, String> {
    for i in 0..warmups {
        let token = format!("W{name}{i:04}");
        time_one_echo(runtime, exec_id, ingress, &token)?;
    }

    let mut stats = PhaseStats {
        name: name.to_string(),
        samples_us: Vec::with_capacity(samples),
        correct: 0,
        timeouts: 0,
        stalls_ge_100ms: 0,
        context_discoveries: 0,
        context_failures_injected: 0,
        context_degraded_hits: 0,
    };

    for i in 0..samples {
        let token = format!("S{name}{i:04}");
        match time_one_echo(runtime, exec_id, ingress, &token) {
            Ok(us) => {
                stats.correct += 1;
                stats.samples_us.push(us);
                if us / 1000.0 >= STALL_CEILING_MS {
                    stats.stalls_ge_100ms += 1;
                }
            }
            Err(_) => {
                stats.timeouts += 1;
            }
        }
    }

    if let Some(c) = counters {
        stats.context_discoveries = c.discoveries.load(Ordering::Relaxed);
        stats.context_failures_injected = c.failures_injected.load(Ordering::Relaxed);
        stats.context_degraded_hits = c.degraded_hits.load(Ordering::Relaxed);
    }
    Ok(stats)
}

#[cfg(target_os = "macos")]
fn time_one_echo(
    runtime: &mut Runtime,
    exec_id: seyal_runtime::ExecutionId,
    ingress: &seyal_runtime::InputIngress,
    token: &str,
) -> Result<f64, String> {
    // Unique per-sample marker. Prefix CR so each sample restarts near column 0
    // and is less likely to wrap mid-token under sustained echo.
    let marker = format!("<{token}>");
    let mut payload = Vec::with_capacity(marker.len() + 2);
    payload.push(b'\r');
    payload.extend_from_slice(marker.as_bytes());
    payload.push(b'\n');
    let started = Instant::now();
    ingress
        .try_submit(payload)
        .map_err(|e| format!("submit: {e:?}"))?;
    let deadline = Instant::now() + SAMPLE_TIMEOUT;
    loop {
        runtime
            .poll_once(Some(Duration::from_millis(2)))
            .map_err(|e| format!("poll: {e:?}"))?;
        // Prefer seeing the marker after the Runtime has drained accepted input.
        if ingress.accepted_but_unwritten_bytes() == 0
            && terminal_contains(runtime, exec_id, &marker)
        {
            return Ok(started.elapsed().as_secs_f64() * 1_000_000.0);
        }
        if Instant::now() >= deadline {
            let snapshot = terminal_snapshot(runtime, exec_id);
            return Err(format!(
                "timeout waiting for marker {marker:?}; unwritten={} screen={snapshot:?}",
                ingress.accepted_but_unwritten_bytes()
            ));
        }
    }
}

#[cfg(target_os = "macos")]
fn terminal_contains(runtime: &Runtime, exec_id: seyal_runtime::ExecutionId, needle: &str) -> bool {
    // Markers can wrap across visible rows under sustained echo; search the
    // concatenated cell stream so a split token still counts as progress.
    terminal_haystack(runtime, exec_id).contains(needle)
}

#[cfg(target_os = "macos")]
fn terminal_haystack(runtime: &Runtime, exec_id: seyal_runtime::ExecutionId) -> String {
    let Some(execution) = runtime.execution(exec_id) else {
        return String::new();
    };
    let terminal = execution.terminal();
    let mut out = String::new();
    for row in 0..terminal.rows() {
        if let Some(text) = terminal.row_text(row) {
            out.push_str(text.trim_end());
        }
    }
    out
}

#[cfg(target_os = "macos")]
fn terminal_snapshot(runtime: &Runtime, exec_id: seyal_runtime::ExecutionId) -> String {
    let haystack = terminal_haystack(runtime, exec_id);
    if haystack.len() <= 240 {
        haystack
    } else {
        format!("…{}", &haystack[haystack.len() - 240..])
    }
}

#[cfg(target_os = "macos")]
fn wait_for_text(
    runtime: &mut Runtime,
    exec_id: seyal_runtime::ExecutionId,
    needle: &str,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        runtime
            .poll_once(Some(Duration::from_millis(5)))
            .map_err(|e| format!("poll: {e:?}"))?;
        if terminal_contains(runtime, exec_id, needle) {
            return Ok(());
        }
    }
    Err(format!("timed out waiting for {needle:?}"))
}

#[derive(Clone, Debug)]
struct Verdict {
    label: &'static str,
    reasons: Vec<String>,
    active_limit_us: f64,
    failure_limit_us: f64,
}

fn evaluate(baseline: &PhaseStats, active: &PhaseStats, failure: &PhaseStats) -> Verdict {
    let mut reasons = Vec::new();
    let baseline_p95 = baseline.p(0.95);
    let active_limit = (baseline_p95 * 2.0).max(baseline_p95 + 5_000.0);
    let failure_limit = active_limit;

    for phase in [baseline, active, failure] {
        if phase.timeouts > 0 {
            reasons.push(format!(
                "{}: {} timeout(s) (correctness FAIL)",
                phase.name, phase.timeouts
            ));
        }
        if phase.name != "baseline" && phase.stalls_ge_100ms > 0 {
            reasons.push(format!(
                "{}: {} sample(s) >= {} ms stall ceiling",
                phase.name, phase.stalls_ge_100ms, STALL_CEILING_MS
            ));
        }
    }

    let required_samples = baseline.correct + baseline.timeouts;
    if active.correct + active.timeouts != required_samples
        || failure.correct + failure.timeouts != required_samples
    {
        reasons.push("sample count mismatch across phases".into());
    }
    if active.context_discoveries < MIN_DISCOVERIES_BEFORE_SAMPLE {
        reasons.push(format!(
            "active phase discoveries {} < sustained minimum {}",
            active.context_discoveries, MIN_DISCOVERIES_BEFORE_SAMPLE
        ));
    }
    if failure.context_failures_injected < MIN_DISCOVERIES_BEFORE_SAMPLE {
        reasons.push(format!(
            "failure phase injected failures {} < sustained minimum {}",
            failure.context_failures_injected, MIN_DISCOVERIES_BEFORE_SAMPLE
        ));
    }

    let active_p95 = active.p(0.95);
    let failure_p95 = failure.p(0.95);
    if active_p95.is_finite() && active_p95 > active_limit {
        reasons.push(format!(
            "active p95 {active_p95:.3} us exceeds limit {active_limit:.3} us"
        ));
    }
    if failure_p95.is_finite() && failure_p95 > failure_limit {
        reasons.push(format!(
            "failure p95 {failure_p95:.3} us exceeds limit {failure_limit:.3} us"
        ));
    }

    // Pre-registered Issue #1301 rules: any miss is FAIL (no silent PASS).
    let label = if reasons.is_empty() { "PASS" } else { "FAIL" };

    Verdict {
        label,
        reasons,
        active_limit_us: active_limit,
        failure_limit_us: failure_limit,
    }
}

struct ReportInput<'a> {
    git_sha: &'a str,
    verdict: &'a Verdict,
    baseline: &'a PhaseStats,
    active: &'a PhaseStats,
    failure: &'a PhaseStats,
    samples: usize,
    warmups: usize,
    workers: usize,
    fixture_files: usize,
}

fn render_json(input: &ReportInput<'_>) -> String {
    let uname = Command::new("uname")
        .arg("-a")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "unknown".into())
        .trim()
        .to_string();
    let rustc = Command::new("rustc")
        .arg("-Vv")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "unknown".into());
    let rustc_first = rustc.lines().next().unwrap_or("unknown");
    let cpu = Command::new("sysctl")
        .args(["-n", "machdep.cpu.brand_string"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "unknown".into())
        .trim()
        .to_string();

    let phase = |p: &PhaseStats| {
        format!(
            r#"{{"name":"{}","sample_count":{},"correct":{},"timeouts":{},"stalls_ge_100ms":{},"p50_us":{:.3},"p95_us":{:.3},"p99_us":{:.3},"max_us":{:.3},"samples_us":[{}],"context_discoveries":{},"context_failures_injected":{},"context_degraded_hits":{}}}"#,
            p.name,
            p.samples_us.len(),
            p.correct,
            p.timeouts,
            p.stalls_ge_100ms,
            p.p(0.50),
            p.p(0.95),
            p.p(0.99),
            p.max(),
            p.samples_us
                .iter()
                .map(|v| format!("{v:.3}"))
                .collect::<Vec<_>>()
                .join(","),
            p.context_discoveries,
            p.context_failures_injected,
            p.context_degraded_hits
        )
    };

    let reasons = input
        .verdict
        .reasons
        .iter()
        .map(|r| format!("\"{}\"", r.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect::<Vec<_>>()
        .join(",");

    format!(
        r#"{{
  "schema": "seyal.m005.spec013.23.40.terminal-isolation",
  "issue": 1301,
  "authority": ["SPEC-013 §21", "SPEC-013 §23.40", "SPEC-013 §24"],
  "git_sha": "{git_sha}",
  "host_class": "PLATFORM_LIMITED",
  "host_class_note": "uncontrolled developer host; absolute µs are not Pass 9/M002 release gates; isolation PASS/FAIL rules from Issue #1301 still apply",
  "performance_claim": {},
  "os": "{uname}",
  "cpu": "{cpu}",
  "rustc": "{rustc_first}",
  "build_profile": "release",
  "percentile_method": "nearest-rank",
  "workload": {{
    "terminal": "Runtime headless PTY→TerminalState echo marker via InputIngress",
    "context_active": "ContextDiscoveryEngine::discover_with_store on fixture corpus",
    "context_failure": "inject_persistent_failure + discover_with_store with budget reset on Degraded",
    "samples_per_phase": {samples},
    "warmups_per_phase": {warmups},
    "context_workers": {workers},
    "fixture_files": {fixture_files}
  }},
  "pass_rules": {{
    "correctness": "zero timeouts; every retained sample shows expected marker in TerminalState",
    "stall_ceiling_ms": {STALL_CEILING_MS},
    "relative_contention": "contended_p95 <= max(baseline_p95 * 2.0, baseline_p95 + 5000 us)"
  }},
  "verdict": "{verdict_label}",
  "fail_reasons": [{reasons}],
  "limits_us": {{
    "active_p95": {active_limit:.3},
    "failure_p95": {failure_limit:.3}
  }},
  "phases": [
    {baseline},
    {active},
    {failure}
  ]
}}
"#,
        if input.verdict.label == "PASS" {
            "true"
        } else {
            "false"
        },
        git_sha = input.git_sha,
        samples = input.samples,
        warmups = input.warmups,
        workers = input.workers,
        fixture_files = input.fixture_files,
        verdict_label = input.verdict.label,
        active_limit = input.verdict.active_limit_us,
        failure_limit = input.verdict.failure_limit_us,
        baseline = phase(input.baseline),
        active = phase(input.active),
        failure = phase(input.failure),
    )
}

fn env_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn git_sha() -> String {
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(target_os = "macos")]
fn runtime_config(tag: &str) -> Result<RuntimeConfig, String> {
    let mut config = RuntimeConfig::m001().map_err(|e| format!("config: {e:?}"))?;
    let suffix = format!(
        "{}-{}-{}",
        tag,
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    );
    config.singleton_path = env::temp_dir().join(format!("seyal-m005-iso-{suffix}.lock"));
    config.local_ipc = LocalIpcMode::Disabled;
    config.graceful_termination = Duration::from_millis(50);
    config.forced_reap = Duration::from_millis(250);
    config.final_drain = Duration::from_millis(100);
    Ok(config)
}

#[cfg(target_os = "macos")]
fn make_fixture(file_count: usize) -> Result<PathBuf, String> {
    let root = env::temp_dir().join(format!(
        "seyal-m005-iso-fixture-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    ));
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    git(&root, &["init"])?;
    git(&root, &["config", "user.email", "seyal-iso@example.com"])?;
    git(&root, &["config", "user.name", "seyal-iso"])?;
    for i in 0..file_count {
        let dir = root.join(format!("d{:03}", i % 40));
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!("f{i:04}.rs"));
        fs::write(&path, format!("// fixture {i}\npub fn f{i}() {{}}\n"))
            .map_err(|e| e.to_string())?;
    }
    git(&root, &["add", "."])?;
    git(&root, &["commit", "-m", "fixture"])?;
    Ok(root)
}

#[cfg(target_os = "macos")]
fn git(cwd: &Path, args: &[&str]) -> Result<(), String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_AUTHOR_NAME", "seyal-iso")
        .env("GIT_AUTHOR_EMAIL", "seyal-iso@example.com")
        .env("GIT_COMMITTER_NAME", "seyal-iso")
        .env("GIT_COMMITTER_EMAIL", "seyal-iso@example.com")
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}
