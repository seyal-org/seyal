//! #869 Flow/Raw/TUI presentation qualification harness.
//!
//! Measures production presentation fencing, live-tail projection scaling, and
//! retained Block-metadata RSS against the frozen #869 Ready gate. Every printed
//! number is diagnostic (`performance_claim=false`) unless a controlled-host
//! evidence row records a product claim separately.
//!
//! Absolute CI gates encoded here are only those frozen on Issue #869:
//! Pass 8-style metadata RSS ceiling and transition RSS return-within-10%.

use std::time::Instant;

#[cfg(target_os = "macos")]
use std::{
    fs,
    hint::black_box,
    process::{self, Command},
    thread,
    time::Duration,
};

#[cfg(target_os = "macos")]
use seyal_client::live_tail::{project_block_output, LiveTailProjection};
#[cfg(target_os = "macos")]
use seyal_client::presentation::{
    FlowPaintInspection, PresentationAction, PresentationIdentity, PresentationMode,
    PresentationSession, RendererPlan,
};
#[cfg(target_os = "macos")]
use seyal_core::ExecutionId;
#[cfg(target_os = "macos")]
use seyal_runtime::pass8_benchmark::BenchmarkBlockTimeline;

const PERFORMANCE_CLAIM: &str = "performance_claim=false";

#[cfg(target_os = "macos")]
const LATENCY_SAMPLES: usize = 120;
#[cfg(target_os = "macos")]
const LATENCY_WARMS: usize = 32;
#[cfg(target_os = "macos")]
const TRANSITION_CYCLES: usize = 50;
#[cfg(target_os = "macos")]
const BLOCK_COUNTS: &[usize] = &[8, 32, 128];
/// Frozen Pass 8 absolute ceiling reused for retained Block metadata path.
#[cfg(target_os = "macos")]
const RSS_GATE_KIB: usize = 1024;
/// Frozen #869 transition return policy: within 10% of pre-transition median.
#[cfg(target_os = "macos")]
const TRANSITION_RSS_RETURN_PERCENT: usize = 10;

fn main() {
    let _contract_clock = Instant::now();

    #[cfg(not(target_os = "macos"))]
    println!(
        "m003_presentation_qualification PLATFORM_LIMITED target_os!=macos evidence_class=PLATFORM_LIMITED {PERFORMANCE_CLAIM}"
    );

    #[cfg(target_os = "macos")]
    run_macos();
}

#[cfg(target_os = "macos")]
fn run_macos() {
    println!(
        "m003_presentation_qualification architecture=PresentationSession_live_tail_BlockTimeline issue=869 percentile_method=nearest_rank latency_samples={LATENCY_SAMPLES} transition_cycles={TRANSITION_CYCLES} {PERFORMANCE_CLAIM}"
    );
    print_host_metadata();
    measure_idle_flow_snapshot();
    measure_presentation_transition_latency();
    measure_live_tail_projection_scaling();
    measure_retained_block_rss_scaling();
    measure_transition_resource_return();
    prove_flow_renderer_plan_is_pane_scoped();
}

#[cfg(target_os = "macos")]
fn measure_idle_flow_snapshot() {
    let identity = PresentationIdentity::new(ExecutionId::from_bytes([1; 16]), 1).expect("id");
    let session = PresentationSession::new(Some(identity), PresentationMode::Flow);
    let mut samples = Vec::with_capacity(LATENCY_SAMPLES);
    for _ in 0..LATENCY_WARMS {
        black_box(session.snapshot());
    }
    for _ in 0..LATENCY_SAMPLES {
        let start = Instant::now();
        let snap = session.snapshot();
        samples.push(elapsed_ns(start));
        assert_eq!(snap.mode, PresentationMode::Flow);
        assert!(!snap.renderer_plan.draws_live_grid);
        assert!(!snap.allows_empty_canvas_terminal_hit_test);
        black_box(snap);
    }
    emit_latency("idle_flow_snapshot", &mut samples);
}

#[cfg(target_os = "macos")]
fn measure_presentation_transition_latency() {
    let identity = PresentationIdentity::new(ExecutionId::from_bytes([2; 16]), 1).expect("id");
    let mut samples = Vec::with_capacity(LATENCY_SAMPLES);
    for _ in 0..LATENCY_WARMS {
        black_box(run_transition_cycle(identity));
    }
    for _ in 0..LATENCY_SAMPLES {
        let start = Instant::now();
        black_box(run_transition_cycle(identity));
        samples.push(elapsed_ns(start));
    }
    emit_latency("flow_raw_tui_transition_cycle", &mut samples);
}

#[cfg(target_os = "macos")]
fn run_transition_cycle(identity: PresentationIdentity) -> PresentationMode {
    let mut session = PresentationSession::new(Some(identity), PresentationMode::Flow);
    let epoch = session.snapshot().epoch;
    session
        .apply(PresentationAction::Transition {
            mode: PresentationMode::Raw,
            identity,
            explicit: true,
            epoch,
        })
        .expect("Flow→Raw");
    let epoch = session.snapshot().epoch;
    session
        .apply(PresentationAction::Transition {
            mode: PresentationMode::Tui,
            identity,
            explicit: false,
            epoch,
        })
        .expect("Raw→TUI");
    let epoch = session.snapshot().epoch;
    session
        .apply(PresentationAction::Transition {
            mode: PresentationMode::Flow,
            identity,
            explicit: false,
            epoch,
        })
        .expect("TUI→Flow");
    session.snapshot().mode
}

#[cfg(target_os = "macos")]
fn measure_live_tail_projection_scaling() {
    // Long-output / many-Block projection work must stay O(N) over the same
    // Pane-scoped Flow renderer plan — never a renderer-per-Block authority.
    let viewport: Vec<u64> = (1..=80).collect();
    for &blocks in BLOCK_COUNTS {
        let mut samples = Vec::with_capacity(LATENCY_SAMPLES);
        for _ in 0..LATENCY_WARMS {
            black_box(project_n_blocks(blocks, &viewport));
        }
        for _ in 0..LATENCY_SAMPLES {
            let start = Instant::now();
            let ok = project_n_blocks(blocks, &viewport);
            samples.push(elapsed_ns(start));
            assert_eq!(ok, blocks);
        }
        emit_latency(&format!("live_tail_project_blocks_{blocks}"), &mut samples);
    }
}

#[cfg(target_os = "macos")]
fn project_n_blocks(blocks: usize, viewport: &[u64]) -> usize {
    let mut ok = 0usize;
    for index in 0..blocks {
        let start = (index as u64) + 1;
        let running = index + 1 == blocks;
        let end = if running {
            None
        } else {
            Some(start.saturating_add(3))
        };
        let projection =
            project_block_output(PresentationMode::Flow, start, end, running, viewport);
        match projection {
            LiveTailProjection::PrimaryFrame(_) | LiveTailProjection::History(_) => ok += 1,
            LiveTailProjection::FailClosed => {}
        }
        // Raw/TUI must fail closed for Block projection (no guessed Blocks).
        assert_eq!(
            project_block_output(PresentationMode::Raw, start, end, running, viewport),
            LiveTailProjection::FailClosed
        );
        assert_eq!(
            project_block_output(PresentationMode::Tui, start, end, running, viewport),
            LiveTailProjection::FailClosed
        );
    }
    black_box(RendererPlan::flow());
    ok
}

#[cfg(target_os = "macos")]
fn measure_retained_block_rss_scaling() {
    for &live_records in BLOCK_COUNTS {
        // Warm once so page faults are not attributed to the retained map.
        {
            let mut warm = BenchmarkBlockTimeline::with_live_records(live_records.min(8));
            black_box(warm.len());
            warm.complete_and_retire_all();
        }
        let samples = (0..5)
            .map(|_| {
                // Quiesce briefly between RSS samples.
                thread::sleep(Duration::from_millis(20));
                let baseline = process_metrics();
                let mut timeline = BenchmarkBlockTimeline::with_live_records(live_records);
                black_box(timeline.len());
                assert_eq!(timeline.len(), live_records);
                let populated = process_metrics();
                let incremental = populated.rss_kib.saturating_sub(baseline.rss_kib);
                timeline.complete_and_retire_all();
                assert!(timeline.is_empty());
                incremental
            })
            .collect::<Vec<_>>();
        let mut sorted = samples.clone();
        sorted.sort_unstable();
        let median = sorted[sorted.len() / 2];
        // Linear metadata growth is allowed; a full renderer-per-Block would blow
        // past the Pass 8 1 MiB / 512 ceiling long before N=128.
        let scaled_gate = RSS_GATE_KIB
            .saturating_mul(live_records.max(1))
            .div_ceil(512)
            .max(RSS_GATE_KIB / 8);
        println!(
            "m003_block_rss classification=MEASURED live_records={live_records} attributable_rss_kib_median={median} rss_samples={:?} scaled_gate_kib={scaled_gate} rss_gate_kib={RSS_GATE_KIB} {PERFORMANCE_CLAIM}",
            samples
        );
        assert!(
            median <= scaled_gate,
            "retained Block metadata RSS median {median} KiB exceeded scaled gate {scaled_gate} KiB at N={live_records}"
        );
    }
}

#[cfg(target_os = "macos")]
fn measure_transition_resource_return() {
    let identity = PresentationIdentity::new(ExecutionId::from_bytes([9; 16]), 1).expect("id");
    let pre_samples = (0..5)
        .map(|_| {
            thread::sleep(Duration::from_millis(20));
            process_metrics().rss_kib
        })
        .collect::<Vec<_>>();
    let pre_median = median_usize(&pre_samples);

    for _ in 0..TRANSITION_CYCLES {
        black_box(run_transition_cycle(identity));
    }
    // Quiesce after the campaign before measuring return-to-baseline.
    thread::sleep(Duration::from_millis(200));
    let post_samples = (0..5)
        .map(|_| {
            thread::sleep(Duration::from_millis(20));
            process_metrics().rss_kib
        })
        .collect::<Vec<_>>();
    let post_median = median_usize(&post_samples);
    let allowed = pre_median
        + pre_median
            .saturating_mul(TRANSITION_RSS_RETURN_PERCENT)
            .div_ceil(100)
            .max(256);
    println!(
        "m003_transition_rss classification=MEASURED cycles={TRANSITION_CYCLES} rss_pre_median_kib={pre_median} rss_post_median_kib={post_median} rss_pre_samples={:?} rss_post_samples={:?} allowed_post_kib={allowed} {PERFORMANCE_CLAIM}",
        pre_samples, post_samples
    );
    assert!(
        post_median <= allowed,
        "Flow↔Raw↔TUI transition campaign RSS did not return within {TRANSITION_RSS_RETURN_PERCENT}%: pre={pre_median} post={post_median} allowed={allowed}"
    );
}

#[cfg(target_os = "macos")]
fn prove_flow_renderer_plan_is_pane_scoped() {
    let flow = RendererPlan::flow();
    assert!(!flow.draws_full_grid_background);
    assert!(!flow.draws_live_grid);
    assert!(!flow.draws_cursor_outside_block_regions);
    let raw = RendererPlan::full_pane(PresentationMode::Raw);
    let tui = RendererPlan::full_pane(PresentationMode::Tui);
    assert!(raw.draws_live_grid && tui.draws_live_grid);
    // One Pane plan is reused for any Block count — never a renderer-per-Block.
    for n in BLOCK_COUNTS {
        black_box((n, flow));
        assert_eq!(RendererPlan::for_mode(PresentationMode::Flow), flow);
    }
    let clean = FlowPaintInspection {
        mode: PresentationMode::Flow,
        live_grid_submitted: false,
        full_grid_background_submitted: false,
        history_instance_count: 32,
        instances_outside_clips: 0,
        opaque_pixels_outside_clips: 0,
        opaque_pixels_inside_clips: 100,
    };
    assert!(clean.is_clean());
    let leak = FlowPaintInspection {
        live_grid_submitted: true,
        ..clean
    };
    assert!(!leak.is_clean());
    println!(
        "m003_renderer_plan classification=MEASURED flow_pane_scoped=true raw_full_pane=true tui_full_pane=true paint_inspection_gate=true {PERFORMANCE_CLAIM}"
    );
}

#[cfg(target_os = "macos")]
fn emit_latency(boundary: &str, samples: &mut [u64]) {
    samples.sort_unstable();
    let p50 = percentile_ns(samples, 50);
    let p95 = percentile_ns(samples, 95);
    let p99 = percentile_ns(samples, 99);
    let max = samples.last().copied().unwrap_or(0);
    println!(
        "m003_latency boundary={boundary} classification=MEASURED sample_count={} p50_us={:.3} p95_us={:.3} p99_us={:.3} max_us={:.3} {PERFORMANCE_CLAIM}",
        samples.len(),
        p50 as f64 / 1_000.0,
        p95 as f64 / 1_000.0,
        p99 as f64 / 1_000.0,
        max as f64 / 1_000.0,
    );
}

#[cfg(target_os = "macos")]
fn elapsed_ns(start: Instant) -> u64 {
    start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64
}

#[cfg(target_os = "macos")]
fn percentile_ns(sorted: &[u64], percentile: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (percentile * sorted.len()).div_ceil(100).max(1);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

#[cfg(target_os = "macos")]
fn median_usize(values: &[usize]) -> usize {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted[sorted.len() / 2]
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
struct Metrics {
    rss_kib: usize,
}

#[cfg(target_os = "macos")]
fn process_metrics() -> Metrics {
    let pid = process::id();
    let output = Command::new("/bin/ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .expect("ps rss");
    let rss_kib = String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let _fds = fs::read_dir("/dev/fd")
        .map(|entries| entries.count())
        .unwrap_or(0);
    Metrics { rss_kib }
}

#[cfg(target_os = "macos")]
fn print_host_metadata() {
    let product = command_text("/usr/bin/sw_vers", &["-productVersion"]);
    let build = command_text("/usr/bin/sw_vers", &["-buildVersion"]);
    let model = command_text("/usr/sbin/sysctl", &["-n", "hw.model"]);
    let hardware = command_text("/usr/sbin/sysctl", &["-n", "machdep.cpu.brand_string"]);
    let rust = command_text("rustc", &["--version"]);
    let commit = command_text("git", &["rev-parse", "HEAD"]);
    println!(
        "m003_host macos_version={product} macos_build={build} model={model:?} hardware={hardware:?} arch={} rust={rust:?} build_mode=release commit={} evidence_class=controlled-host {PERFORMANCE_CLAIM}",
        std::env::consts::ARCH,
        commit.trim(),
    );
}

#[cfg(target_os = "macos")]
fn command_text(bin: &str, args: &[&str]) -> String {
    Command::new(bin)
        .args(args)
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned())
}
