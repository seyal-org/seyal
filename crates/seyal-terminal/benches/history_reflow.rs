use std::{env, fs, hint::black_box, path::PathBuf, process::Command, time::Instant};

use seyal_terminal::TerminalState;

#[cfg(not(feature = "history-reflow-contract"))]
use stats_alloc::{Region, StatsAlloc, INSTRUMENTED_SYSTEM};

#[cfg(not(feature = "history-reflow-contract"))]
#[global_allocator]
static GLOBAL: &StatsAlloc<std::alloc::System> = &INSTRUMENTED_SYSTEM;

// `make bench` is a harness smoke (`performance_claim=false`), not SPEC-010
// §18.1. Keep the default path small enough for native-macos-smoke's 35-minute
// budget; the 100k/full matrix stays behind SEYAL_HISTORY_BENCH_FULL=1.
const DEFAULT_LINES: &[usize] = &[2_000];
const DEFAULT_EXECUTIONS: &[usize] = &[1];
const DEFAULT_COLUMNS: &[u16] = &[80];
const DEFAULT_WORKLOADS: &[&str] = &["ascii"];
const FULL_LINES: &[usize] = &[10_000, 100_000, 1_000_000];
const FULL_EXECUTIONS: &[usize] = &[1, 10, 50, 100];
const FULL_COLUMNS: &[u16] = &[40, 48, 64, 80, 96, 132, 160];
const FULL_WORKLOADS: &[&str] = &["ascii", "styled", "cjk", "emoji-combining"];
const DEFAULT_SAMPLES: usize = 8;
const ACTIVE_WINDOW_ROWS: usize = 120;

fn parse_scales<T>(name: &str, defaults: &[T]) -> Vec<T>
where
    T: Copy + From<u8> + PartialOrd + std::str::FromStr,
{
    env::var(name)
        .ok()
        .map(|value| {
            value
                .split(',')
                .filter_map(|part| part.trim().parse().ok())
                .filter(|value: &T| *value > T::from(0))
                .collect::<Vec<T>>()
        })
        .filter(|values| !values.is_empty())
        .unwrap_or_else(|| defaults.to_vec())
}

fn full_matrix_enabled() -> bool {
    env::var("SEYAL_HISTORY_BENCH_FULL").as_deref() == Ok("1")
}

fn parse_samples() -> usize {
    env::var("SEYAL_HISTORY_BENCH_SAMPLES")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value: &usize| *value > 0)
        .unwrap_or(DEFAULT_SAMPLES)
}

fn parse_usize_env(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value: &usize| *value > 0)
        .unwrap_or(default)
}

fn contract_gate() -> Option<String> {
    env::var("SEYAL_M002_CONTRACT_GATE")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn populate(lines: usize, workload: &str) -> TerminalState {
    let (line, _) = line_for(workload, 0);
    let mut terminal = TerminalState::new(120, 40).expect("valid benchmark geometry");
    for _ in 0..lines {
        terminal.feed(&line).expect("history feed succeeds");
    }
    terminal
}

fn sample_ms(gate: &str, terminal: &mut TerminalState, columns: u16) -> f64 {
    match gate {
        "history_active_reflow_ms" => {
            terminal.drop_primary_history_derived_cache();
            let started = Instant::now();
            black_box(terminal.primary_history_reflow(columns, ACTIVE_WINDOW_ROWS));
            started.elapsed().as_secs_f64() * 1_000.0
        }
        "history_sealed_segment_reflow_ms" => {
            let started = Instant::now();
            black_box(terminal.primary_history_reflow_uncached(columns, ACTIVE_WINDOW_ROWS));
            started.elapsed().as_secs_f64() * 1_000.0
        }
        other => panic!("unsupported M002 contract gate {other:?}"),
    }
}

/// A single cohort file's matrix identity: exactly which point of the
/// accepted retained_content x execution_populations x columns x workloads
/// matrix (docs/evidence/M002-PERFORMANCE-CONTRACT-V1.toml `[matrix]`) this
/// cohort's samples were collected at. The contract-gate cohort collector
/// always operates on a single execution's HistoryStore (history reflow is
/// a per-execution cost, not a population-scaling one), so `executions` is
/// always 1 here -- that is the correct/only valid value for this family,
/// not an unmeasured placeholder.
struct MatrixPoint {
    lines: usize,
    columns: u16,
    workload: &'static str,
    executions: usize,
}

fn write_cohort_file(
    path: &str,
    cohort: usize,
    point: &MatrixPoint,
    commit: &str,
    samples: &[f64],
) {
    let mut body = format!(
        "cohort = {cohort}\ncommit = \"{commit}\"\nlines = {}\ncolumns = {}\nworkload = \"{}\"\nexecutions = {}\nsamples = [",
        point.lines, point.columns, point.workload, point.executions,
    );
    for (index, value) in samples.iter().enumerate() {
        if index > 0 {
            body.push_str(", ");
        }
        body.push_str(&format!("{value:.9}"));
    }
    body.push_str("]\n");
    fs::write(PathBuf::from(path), body).expect("write M002 cohort file");
}

fn run_contract_cohort() {
    let gate = contract_gate().expect("contract gate");
    let cohort = parse_usize_env("SEYAL_M002_COHORT", 1);
    let warmups = parse_usize_env("SEYAL_M002_WARMUPS", 20);
    let samples = parse_usize_env("SEYAL_M002_SAMPLES", 100);
    let out = env::var("SEYAL_M002_COHORT_OUT").expect("SEYAL_M002_COHORT_OUT");
    let lines = parse_scales("SEYAL_HISTORY_BENCH_LINES", &[10_000])[0];
    let columns = parse_scales("SEYAL_HISTORY_BENCH_COLUMNS", &[80])[0];
    let workload = workload_names()[0];
    let point = MatrixPoint {
        lines,
        columns,
        workload,
        executions: 1,
    };
    let mut terminal = populate(lines, workload);
    for _ in 0..warmups {
        let _ = sample_ms(&gate, &mut terminal, columns);
    }
    let mut retained = Vec::with_capacity(samples);
    for _ in 0..samples {
        retained.push(sample_ms(&gate, &mut terminal, columns));
    }
    let commit = benchmark_commit();
    write_cohort_file(&out, cohort, &point, &commit, &retained);
    println!(
        "[seyal history benchmark] m002_contract gate={gate} cohort={cohort} warmups={warmups} samples={samples} lines={lines} columns={columns} workload={workload} out={out}"
    );
}

fn workload_names() -> Vec<&'static str> {
    let defaults = if full_matrix_enabled() {
        FULL_WORKLOADS
    } else {
        DEFAULT_WORKLOADS
    };
    env::var("SEYAL_HISTORY_BENCH_WORKLOADS")
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| match value {
                    "ascii" => "ascii",
                    "styled" => "styled",
                    "cjk" => "cjk",
                    "emoji-combining" => "emoji-combining",
                    other => panic!("unknown history benchmark workload {other:?}"),
                })
                .collect()
        })
        .filter(|values: &Vec<&str>| !values.is_empty())
        .unwrap_or_else(|| defaults.to_vec())
}

fn line_for(workload: &str, execution: usize) -> (Vec<u8>, String) {
    let marker = format!("needle-{execution:03}");
    let line = match workload {
        "ascii" => format!("ascii-{marker}-history\r\n"),
        "styled" => format!("\x1b[31mstyled-{marker}-history\x1b[0m\r\n"),
        "cjk" => format!("cjk-界-{marker}-界\r\n"),
        "emoji-combining" => format!("emoji-👨‍👩‍👧‍👦-e\u{301}-{marker}\r\n"),
        _ => unreachable!("workload names are validated by workload_names"),
    };
    (line.into_bytes(), marker)
}

fn percentile(samples: &mut [u128], percent: usize) -> u128 {
    assert!(
        !samples.is_empty(),
        "benchmark must collect at least one sample"
    );
    samples.sort_unstable();
    let index = ((samples.len() * percent).saturating_add(99) / 100)
        .saturating_sub(1)
        .min(samples.len() - 1);
    samples[index]
}

fn process_rss_kib() -> Option<u64> {
    let output = Command::new("ps")
        .args(["-p", &std::process::id().to_string(), "-o", "rss="])
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

fn benchmark_commit() -> String {
    env::var("SEYAL_BENCH_COMMIT")
        .or_else(|_| env::var("GITHUB_SHA"))
        .unwrap_or_else(|_| "unknown".to_owned())
}

fn measure(
    lines: usize,
    executions: usize,
    columns: u16,
    workload: &str,
    samples: usize,
    commit: &str,
) {
    let rss_before = process_rss_kib();
    #[cfg(not(feature = "history-reflow-contract"))]
    let allocation_region = Region::new(GLOBAL);
    let append_samples_per_execution = samples.min(lines);
    let mut append_samples = Vec::with_capacity(executions * append_samples_per_execution);
    let mut terminals = Vec::with_capacity(executions);

    for execution in 0..executions {
        let (line, needle) = line_for(workload, execution);
        let mut terminal = TerminalState::new(120, 40).expect("valid benchmark geometry");
        let base_lines_per_sample = lines / append_samples_per_execution;
        let remainder = lines % append_samples_per_execution;
        for sample in 0..append_samples_per_execution {
            let lines_for_sample = base_lines_per_sample + usize::from(sample < remainder);
            let started = Instant::now();
            for _ in 0..lines_for_sample {
                terminal.feed(&line).expect("history feed succeeds");
            }
            append_samples.push(started.elapsed().as_nanos());
        }
        let _ = terminal.take_damage();
        terminals.push((terminal, needle));
    }

    let mut reflow_samples = Vec::with_capacity(executions * samples);
    for _ in 0..samples {
        for (terminal, _) in &mut terminals {
            terminal.drop_primary_history_derived_cache();
            let started = Instant::now();
            black_box(terminal.primary_history_reflow(columns, ACTIVE_WINDOW_ROWS));
            reflow_samples.push(started.elapsed().as_nanos());
        }
    }

    let mut search_samples = Vec::with_capacity(executions * samples);
    let mut anchor_samples = Vec::with_capacity(executions * samples);
    let mut resolved_anchors = 0usize;
    for _ in 0..samples {
        for (terminal, needle) in &terminals {
            let started = Instant::now();
            let matches = terminal.primary_history_search(needle, 1);
            search_samples.push(started.elapsed().as_nanos());
            let Some(found) = matches.first() else {
                anchor_samples.push(0);
                continue;
            };
            let started = Instant::now();
            black_box(terminal.primary_history_unit(found.start));
            anchor_samples.push(started.elapsed().as_nanos());
            resolved_anchors = resolved_anchors.saturating_add(1);
        }
    }

    let resident_history_bytes: usize = terminals
        .iter()
        .map(|(terminal, _)| terminal.primary_history_resident_bytes())
        .sum();
    let derived_cache_bytes: usize = terminals
        .iter()
        .map(|(terminal, _)| terminal.primary_history_derived_cache_bytes())
        .sum();
    let rss_after = process_rss_kib();
    let rss_before_value = rss_before.unwrap_or(0);
    let rss_after_value = rss_after.unwrap_or(0);
    let rss_delta = rss_after_value.saturating_sub(rss_before_value);
    #[cfg(not(feature = "history-reflow-contract"))]
    let allocation_stats = allocation_region.change();
    #[cfg(not(feature = "history-reflow-contract"))]
    let (allocation_calls, allocated_bytes, deallocated_bytes) = (
        allocation_stats.allocations,
        allocation_stats.bytes_allocated,
        allocation_stats.bytes_deallocated,
    );
    #[cfg(feature = "history-reflow-contract")]
    let (allocation_calls, allocated_bytes, deallocated_bytes) = (0, 0, 0);

    println!(
        "[seyal history benchmark] case workload={workload} lines={lines} executions={executions} columns={columns} commit={commit} resident_history_bytes={resident_history_bytes} derived_cache_bytes={derived_cache_bytes} rss_before_kib={rss_before_value} rss_after_kib={rss_after_value} rss_delta_kib={rss_delta} rss_available={} append_observations={} append_samples_per_execution={append_samples_per_execution} append_p50_ns={} append_p95_ns={} append_p99_ns={} reflow_p50_ns={} reflow_p95_ns={} reflow_p99_ns={} search_p50_ns={} search_p95_ns={} search_p99_ns={} anchor_p50_ns={} anchor_p95_ns={} anchor_p99_ns={} resolved_anchors={resolved_anchors} allocation_calls={} allocated_bytes={} deallocated_bytes={} allocation_status=measured samples={samples} percentile_method=nearest-rank performance_claim=false evidence_scope=TerminalState-comparative",
        rss_before.is_some(),
        append_samples.len(),
        percentile(&mut append_samples, 50),
        percentile(&mut append_samples, 95),
        percentile(&mut append_samples, 99),
        percentile(&mut reflow_samples, 50),
        percentile(&mut reflow_samples, 95),
        percentile(&mut reflow_samples, 99),
        percentile(&mut search_samples, 50),
        percentile(&mut search_samples, 95),
        percentile(&mut search_samples, 99),
        percentile(&mut anchor_samples, 50),
        percentile(&mut anchor_samples, 95),
        percentile(&mut anchor_samples, 99),
        allocation_calls,
        allocated_bytes,
        deallocated_bytes,
    );
}

fn main() {
    if contract_gate().is_some() {
        run_contract_cohort();
        return;
    }

    let full = full_matrix_enabled();
    let lines = parse_scales(
        "SEYAL_HISTORY_BENCH_LINES",
        if full { FULL_LINES } else { DEFAULT_LINES },
    );
    let executions = parse_scales(
        "SEYAL_HISTORY_BENCH_EXECUTIONS",
        if full {
            FULL_EXECUTIONS
        } else {
            DEFAULT_EXECUTIONS
        },
    );
    let columns = parse_scales(
        "SEYAL_HISTORY_BENCH_COLUMNS",
        if full { FULL_COLUMNS } else { DEFAULT_COLUMNS },
    );
    let workloads = workload_names();
    let samples = parse_samples();
    let commit = benchmark_commit();

    println!(
        "[seyal history benchmark] build_mode={}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    println!("[seyal history benchmark] target_os={}", env::consts::OS);
    println!(
        "[seyal history benchmark] target_arch={}",
        env::consts::ARCH
    );
    println!("[seyal history benchmark] commit={commit}");
    println!("[seyal history benchmark] full_matrix={full}");
    println!("[seyal history benchmark] percentile_method=nearest-rank");
    println!("[seyal history benchmark] performance_claim=false");
    println!("[seyal history benchmark] evidence_scope=TerminalState-comparative");

    for workload in workloads {
        for &execution_count in &executions {
            for &line_count in &lines {
                for &column_count in &columns {
                    measure(
                        line_count,
                        execution_count,
                        column_count,
                        workload,
                        samples,
                        &commit,
                    );
                }
            }
        }
    }
}
