//! M003 W7 (#1221) §8.2 lifecycle measurements (performance-gate).
//!
//! Measures portable shell/ApplicationRoot create and focus/switch latency.
//! Multi-live Metal / headed pane-ops / occluded idle CPU·RSS that require
//! unmerged #936 / PT5 / C3 are labelled PLATFORM_LIMITED — not invented as PASS.
//! Every printed number is diagnostic (`performance_claim=false`).

use std::time::Instant;

use seyal_client::app::{AppAction, ApplicationRoot, WindowNativeEvent, QUIT_CLEANUP_DEADLINE_MS};
use seyal_client::shell::PresentationTier;
use seyal_core::TabId;

const PERFORMANCE_CLAIM: &str = "performance_claim=false";
const SAMPLES: usize = 200;
const WARMUPS: usize = 20;

fn main() {
    let _contract_clock = Instant::now();
    print_host_metadata();
    measure_create_window_latency();
    measure_create_tab_latency();
    measure_select_tab_latency();
    measure_presentation_scaling_shell_only();
    print_quit_cleanup_derivation();
    print_platform_limited_rows();
}

fn print_host_metadata() {
    let sha = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .unwrap_or_else(|| "unknown".into());
    println!(
        "m003_w7_lifecycle_host os={} arch={} sha={} build=release_bench evidence_class=controlled-host percentile_method=nearest_rank warmups={WARMUPS} samples={SAMPLES} {PERFORMANCE_CLAIM}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        sha.trim(),
    );
}

fn percentile_us(sorted: &[u64], percentile: u8) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (usize::from(percentile) * sorted.len()) / 100;
    sorted[rank.min(sorted.len() - 1)]
}

fn report(name: &str, samples: &mut [u64]) {
    samples.sort_unstable();
    println!(
        "{name} evidence_class=controlled-host sample_count={} p50_us={} p95_us={} p99_us={} max_us={} {PERFORMANCE_CLAIM}",
        samples.len(),
        percentile_us(samples, 50),
        percentile_us(samples, 95),
        percentile_us(samples, 99),
        samples.last().copied().unwrap_or(0),
    );
}

fn measure_create_window_latency() {
    let mut samples = Vec::with_capacity(SAMPLES);
    for sample in 0..(WARMUPS + SAMPLES) {
        let mut root = ApplicationRoot::new();
        while !root.snapshot().pending_effects.is_empty() {
            root.apply(AppAction::AckEffect).unwrap();
        }
        // Close bootstrap window so CreateWindow is zero-window re-entry cost.
        let window = root.snapshot().shell.active_window.expect("window");
        root.apply(AppAction::CloseWindow { id: window }).unwrap();
        while !root.snapshot().pending_effects.is_empty() {
            root.apply(AppAction::AckEffect).unwrap();
        }
        let started = Instant::now();
        root.apply(AppAction::CreateWindow).unwrap();
        let elapsed = started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
        if sample >= WARMUPS {
            samples.push(elapsed);
        }
    }
    report(
        "m003_w7_create_latency boundary=AppAction_CreateWindow_zero_window_reentry",
        &mut samples,
    );
}

fn measure_create_tab_latency() {
    let mut samples = Vec::with_capacity(SAMPLES);
    for sample in 0..(WARMUPS + SAMPLES) {
        let mut root = ApplicationRoot::new();
        while !root.snapshot().pending_effects.is_empty() {
            root.apply(AppAction::AckEffect).unwrap();
        }
        let started = Instant::now();
        root.apply(AppAction::CreateTab).unwrap();
        let elapsed = started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
        if sample >= WARMUPS {
            samples.push(elapsed);
        }
    }
    report(
        "m003_w7_create_latency boundary=AppAction_CreateTab_shell_admit_no_wire",
        &mut samples,
    );
    println!(
        "m003_w7_create_latency boundary=Pane_split_create evidence_class=PLATFORM_LIMITED reason=blocked_on_unmerged_C3_1238_and_PT5_1219 {PERFORMANCE_CLAIM}"
    );
}

fn measure_select_tab_latency() {
    let mut samples = Vec::with_capacity(SAMPLES);
    let mut root = ApplicationRoot::new();
    while !root.snapshot().pending_effects.is_empty() {
        root.apply(AppAction::AckEffect).unwrap();
    }
    root.apply(AppAction::CreateTab).unwrap();
    let tabs: Vec<TabId> = root
        .snapshot()
        .shell
        .tabs
        .iter()
        .map(|tab| tab.id)
        .collect();
    assert!(tabs.len() >= 2);
    for sample in 0..(WARMUPS + SAMPLES) {
        let target = tabs[sample % tabs.len()];
        let started = Instant::now();
        root.apply(AppAction::SelectTab { id: target }).unwrap();
        let elapsed = started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
        if sample >= WARMUPS {
            samples.push(elapsed);
        }
    }
    report(
        "m003_w7_focus_switch_latency boundary=AppAction_SelectTab",
        &mut samples,
    );
}

/// Presentation scaling at the ApplicationRoot tier layer (no real PTY / no multi-live Metal).
fn measure_presentation_scaling_shell_only() {
    for &count in &[1usize, 10, 50, 100] {
        let mut root = ApplicationRoot::new();
        while !root.snapshot().pending_effects.is_empty() {
            root.apply(AppAction::AckEffect).unwrap();
        }
        while root.snapshot().shell.tabs.len() < count {
            root.apply(AppAction::CreateTab).unwrap();
        }
        let window = root.snapshot().shell.active_window.expect("window");
        let tab_ids: Vec<TabId> = root
            .snapshot()
            .shell
            .windows
            .iter()
            .find(|entry| entry.id == window)
            .expect("window")
            .tabs
            .iter()
            .map(|tab| tab.id)
            .collect();
        assert_eq!(tab_ids.len(), count);
        let started = Instant::now();
        root.apply(AppAction::ReportWindowEvent {
            window,
            event: WindowNativeEvent::OcclusionChanged,
            occluded: true,
        })
        .unwrap();
        root.apply(AppAction::ReportWindowEvent {
            window,
            event: WindowNativeEvent::OcclusionChanged,
            occluded: false,
        })
        .unwrap();
        for id in &tab_ids {
            root.apply(AppAction::SelectTab { id: *id }).unwrap();
        }
        let elapsed_us = started.elapsed().as_micros();
        let snap = root.snapshot();
        let host = snap
            .shell
            .windows
            .iter()
            .find(|entry| entry.id == window)
            .expect("window");
        let hidden = host
            .tabs
            .iter()
            .filter(|tab| tab.id != host.active_tab)
            .filter(|tab| {
                tab.panes
                    .first()
                    .is_some_and(|pane| pane.presentation_tier == PresentationTier::Hidden)
            })
            .count();
        println!(
            "m003_w7_presentation_scaling presentation_count={count} real_pty_count=0 hidden_inactive_tabs={hidden} evidence_class=controlled-host boundary=AppRoot_occlusion_and_select_all_tabs elapsed_us={elapsed_us} note=separates_presentation_count_from_pty_count {PERFORMANCE_CLAIM}"
        );
    }
}

fn print_quit_cleanup_derivation() {
    // SPEC-009 detach cleanup p99 ≈ 250µs (Pass 9 controlled baseline).
    // Headroom 2000× for renderer/GPU release across ≤16 attachments:
    // 250µs × 2000 = 500_000µs = 500ms. Recorded in native_effect.rs and here.
    const SPEC009_DETACH_P99_US: u64 = 250;
    const HEADROOM: u64 = 2000;
    const MAX_ATTACHMENTS_ASSUMED: u64 = 16;
    let derived_ms = (SPEC009_DETACH_P99_US * HEADROOM) / 1000;
    assert_eq!(derived_ms, 500);
    assert_eq!(QUIT_CLEANUP_DEADLINE_MS, derived_ms);
    println!(
        "m003_w7_quit_cleanup_deadline derived_ms={derived_ms} formula=spec009_detach_p99_us_{SPEC009_DETACH_P99_US}_x_headroom_{HEADROOM}_for_up_to_{MAX_ATTACHMENTS_ASSUMED}_attachments evidence_class=controlled-host skill=performance-gate {PERFORMANCE_CLAIM}"
    );
}

fn print_platform_limited_rows() {
    println!(
        "m003_w7_idle_cpu_rss_hidden_occluded evidence_class=PLATFORM_LIMITED reason=headed_idle_sampling_requires_Seyal_app_plus_unmerged_936_multi_live_and_PT5_pane_ops {PERFORMANCE_CLAIM}"
    );
    println!(
        "m003_w7_presentation_scaling_headed_multi_live counts=1,10,50,100 evidence_class=PLATFORM_LIMITED reason=blocked_on_unmerged_PR_1250_issue_936_and_PR_1259_issue_1219 {PERFORMANCE_CLAIM}"
    );
    println!(
        "m003_w7_pane_ops_equalize_swap_move evidence_class=PLATFORM_LIMITED reason=blocked_on_unmerged_PT4_1249_PT5_1259 {PERFORMANCE_CLAIM}"
    );
}
