use std::time::Instant;

#[cfg(target_os = "macos")]
use std::{
    fs,
    process::{self, Command},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

#[cfg(target_os = "macos")]
use seyal_client::pass7_benchmark::{
    pass7_client_benchmark_marks, pass7_client_benchmark_now_ns, reset_pass7_client_benchmark_marks,
};
#[cfg(target_os = "macos")]
use seyal_client::{GridGeometry, LocalDisplayClient};
#[cfg(target_os = "macos")]
use seyal_exec::{CommandSpec, WindowSize};
#[cfg(target_os = "macos")]
use seyal_protocol::display::DisplayCache;
#[cfg(target_os = "macos")]
use seyal_runtime::local_ipc::framing::Role;
#[cfg(target_os = "macos")]
use seyal_runtime::pass7_benchmark::{
    pass7_benchmark_now_ns, pass7_runtime_benchmark_marks, reset_pass7_runtime_benchmark_marks,
};
#[cfg(target_os = "macos")]
use seyal_runtime::{ExecutionId, LocalIpcMode, Runtime, RuntimeConfig};

const PERFORMANCE_CLAIM: &str = "performance_claim=false";
#[cfg(target_os = "macos")]
const REPETITIONS: usize = 120;
#[cfg(target_os = "macos")]
static HARNESS_COUNTER: AtomicU64 = AtomicU64::new(0);
#[cfg(target_os = "macos")]
static INPUT_MARKER_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A unique per-sample marker so a completion check can prove the observed
/// cache-generation bump was actually caused by admitting THIS sample's
/// input, not by any other concurrent source advancing the cache (M002 #673
/// Q3/Q4 correlation fix).
#[cfg(target_os = "macos")]
fn next_input_marker() -> String {
    format!(
        "m{:07}",
        INPUT_MARKER_COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

/// True when `marker` is visible somewhere in the committed display cache.
/// Used to correlate a cache-generation advance with the specific input that
/// produced it, rather than accepting any generation bump as proof.
#[cfg(target_os = "macos")]
fn cache_contains_marker(cache: &DisplayCache, marker: &str) -> bool {
    if cache.columns == 0 || marker.is_empty() {
        return marker.is_empty();
    }
    let needle: Vec<char> = marker.chars().collect();
    let columns = cache.columns as usize;
    cache.cells.chunks(columns).any(|row| {
        row.windows(needle.len()).any(|window| {
            window
                .iter()
                .zip(&needle)
                .all(|(cell, expected)| cell.scalar == *expected)
        })
    })
}

fn main() {
    // Required by the repository benchmark contract. `Instant::now()` remains
    // the portable harness/deadline clock; benchmark timing marks use monotonic
    // clocks inside the production boundaries being measured.
    let _contract_clock = Instant::now();

    #[cfg(not(target_os = "macos"))]
    {
        if std::env::var("SEYAL_M002_CONTRACT_GATE")
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
        {
            eprintln!(
                "pass7_input_resize PLATFORM_LIMITED target_os!=macos gate=input_visible_proxy"
            );
            std::process::exit(1);
        }
        println!("pass7_input_resize PLATFORM_LIMITED target_os!=macos {PERFORMANCE_CLAIM}");
    }

    #[cfg(target_os = "macos")]
    run_macos();
}

#[cfg(target_os = "macos")]
include!("../../../benches/m002_contract_support.rs");

#[cfg(target_os = "macos")]
fn sample_input_visible_proxy(client: &mut LocalDisplayClient, marker: &str) -> f64 {
    settle_client(client);
    let before = client.cache().generation;
    let started = Instant::now();
    client
        .submit_committed_text(marker)
        .expect("M002 visible-proxy input");
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if client.wants_write() {
            client.flush_control_write().expect("visible-proxy flush");
        }
        match client.poll_prepare() {
            Ok(_) => {}
            Err(error) => panic!("visible-proxy poll failed: {error:?}"),
        }
        // A generation bump alone is not proof this sample's input was what
        // advanced the cache: require the cache to also visibly contain this
        // sample's unique marker text.
        if client.cache().generation > before && cache_contains_marker(client.cache(), marker) {
            return started.elapsed().as_secs_f64() * 1_000.0;
        }
        assert!(
            Instant::now() < deadline,
            "input_visible_proxy cache advance timed out"
        );
        thread::yield_now();
    }
}

#[cfg(target_os = "macos")]
fn run_m002_contract_cohort() {
    let gate = m002_contract_gate().expect("contract gate");
    if gate != "input_visible_proxy" {
        panic!("unsupported M002 contract gate {gate:?}");
    }
    let cohort = m002_parse_usize_env("SEYAL_M002_COHORT", 1);
    let warmups = m002_parse_usize_env("SEYAL_M002_WARMUPS", 20);
    let samples = m002_parse_usize_env("SEYAL_M002_SAMPLES", 100);
    let out = std::env::var("SEYAL_M002_COHORT_OUT").expect("SEYAL_M002_COHORT_OUT");
    let runtime = RuntimeHarness::start();
    let mut client = runtime.connect_controller();
    for _ in 0..warmups {
        let _ = sample_input_visible_proxy(&mut client, &next_input_marker());
    }
    let mut retained = Vec::with_capacity(samples);
    for _ in 0..samples {
        retained.push(sample_input_visible_proxy(
            &mut client,
            &next_input_marker(),
        ));
    }
    drop(client);
    runtime.finish();
    m002_write_cohort_file(&out, cohort, &retained);
    println!(
        "pass7_input_resize m002_contract gate={gate} cohort={cohort} warmups={warmups} samples={samples} out={out} boundary=native_input_admission_to_client_display_cache_proxy {PERFORMANCE_CLAIM}"
    );
}

#[cfg(target_os = "macos")]
fn run_macos() {
    if m002_contract_gate().is_some() {
        run_m002_contract_cohort();
        return;
    }
    let mut args = std::env::args();
    let _ = args.next();
    let first = args.next();
    if first.as_deref() == Some("--selftest-correlation") {
        selftest_correlation();
        return;
    }
    if first.as_deref() == Some("--worker") {
        let case = args.next().expect("Pass 7 benchmark worker case");
        worker(&case);
        return;
    }

    println!(
        "pass7_input_resize architecture=production_client_UDS_Runtime_PTY {PERFORMANCE_CLAIM} percentile_method=nearest_rank repetitions={REPETITIONS} pass6_baseline_reference=docs/engineering/M001-PASS6-METAL-RENDERER.md"
    );
    print_host_metadata();

    let executable = std::env::current_exe().expect("benchmark executable");
    for case in [
        "input",
        "resize_120x40",
        "resize_512x256",
        "idle_resource",
        "pass8_resize_attribution",
    ] {
        let status = Command::new("/usr/bin/time")
            .arg("-lp")
            .arg(&executable)
            .args(["--worker", case])
            .status()
            .expect("launch Pass 7 benchmark worker");
        assert!(status.success(), "Pass 7 benchmark worker {case} failed");
    }
}

/// Proves cache_contains_marker()'s correlation predicate directly, without
/// spawning a Runtime/PTY: a generation bump caused by unrelated concurrent
/// activity (no marker text present) must NOT be accepted as proof of this
/// sample's own input, while a generation bump that genuinely carries the
/// marker text must be accepted. The old logic (`generation > before` alone)
/// would have wrongly accepted the first case.
#[cfg(target_os = "macos")]
fn selftest_correlation() {
    use seyal_protocol::display::{DisplayAttributes, DisplayCell, DisplayColor};

    fn cache_with_row_text(generation: u64, text: &str, columns: u16) -> DisplayCache {
        let mut cells = vec![DisplayCell::blank(); columns as usize];
        for (index, ch) in text.chars().enumerate() {
            if index >= cells.len() {
                break;
            }
            cells[index] = DisplayCell::lead_scalar(
                ch,
                1,
                DisplayColor::Default,
                DisplayColor::Default,
                DisplayAttributes::default(),
            );
        }
        DisplayCache {
            generation,
            rows: 1,
            columns,
            cursor_row: 0,
            cursor_col: 0,
            cursor_visible: true,
            alternate_screen: false,
            cells,
        }
    }

    let marker = "m0000001";

    // Simulates an unrelated concurrent cache bump: generation advanced, but
    // the visible content has nothing to do with our sample's own input. The
    // old `generation > before` check alone would have falsely accepted this
    // as proof of completion.
    let unrelated = cache_with_row_text(2, "unrelated-content", 80);
    assert!(
        unrelated.generation > 1,
        "precondition: generation advanced"
    );
    assert!(
        !cache_contains_marker(&unrelated, marker),
        "FAIL: an unrelated generation bump was wrongly accepted as correlated with our marker"
    );

    // A generation bump that genuinely carries this sample's marker text
    // must be accepted.
    let correlated = cache_with_row_text(2, marker, 80);
    assert!(
        correlated.generation > 1,
        "precondition: generation advanced"
    );
    assert!(
        cache_contains_marker(&correlated, marker),
        "FAIL: a correlated generation bump carrying our marker was wrongly rejected"
    );

    // Marker present but generation did NOT advance: still not proof of
    // completion (guards the `generation > before` half of the predicate).
    let stale = cache_with_row_text(1, marker, 80);
    assert!(
        !(stale.generation > 1 && cache_contains_marker(&stale, marker)),
        "FAIL: a stale (non-advanced) generation was wrongly accepted"
    );

    println!("pass7_input_resize selftest_correlation PASSED {PERFORMANCE_CLAIM}");
}

#[cfg(target_os = "macos")]
fn worker(case: &str) {
    match case {
        "input" => measure_input_boundaries(),
        "resize_120x40" => measure_resize_boundary(
            "resize_120x40",
            GridGeometry {
                rows: 40,
                columns: 120,
            },
            GridGeometry {
                rows: 40,
                columns: 121,
            },
        ),
        "resize_512x256" => measure_resize_boundary(
            "resize_512x256",
            GridGeometry {
                rows: 256,
                columns: 512,
            },
            GridGeometry {
                rows: 255,
                columns: 511,
            },
        ),
        "idle_resource" => measure_idle_resources(),
        "pass8_resize_attribution" => measure_pass8_resize_attribution(),
        other => panic!("unknown Pass 7 benchmark worker {other}"),
    }
}

#[cfg(target_os = "macos")]
struct RuntimeHarness {
    socket_path: std::path::PathBuf,
    execution_id: ExecutionId,
    stop: mpsc::Sender<()>,
    join: thread::JoinHandle<()>,
}

#[cfg(target_os = "macos")]
impl RuntimeHarness {
    fn start() -> Self {
        let suffix = HARNESS_COUNTER.fetch_add(1, Ordering::Relaxed);
        let (ready_tx, ready_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();
        let join = thread::spawn(move || {
            let mut config = RuntimeConfig::m001().expect("M001 Runtime config");
            config.singleton_path =
                std::env::temp_dir().join(format!("s7b-{suffix:x}-{}.lock", process::id()));
            let runtime_dir =
                std::env::temp_dir().join(format!("s7bd-{suffix:x}-{}", process::id()));
            config.local_ipc = LocalIpcMode::Enabled {
                runtime_dir_override: Some(runtime_dir.clone()),
            };
            config.graceful_termination = Duration::from_millis(50);
            config.forced_reap = Duration::from_millis(250);
            config.final_drain = Duration::from_millis(100);

            let mut runtime = Runtime::new(config).expect("Runtime");
            let execution_id = runtime
                .create_execution(
                    CommandSpec::new("/bin/cat"),
                    WindowSize::cells(80, 24).expect("initial geometry"),
                )
                .expect("execution");
            let socket_path = runtime
                .local_ipc_socket_path()
                .expect("local IPC socket")
                .to_path_buf();
            ready_tx
                .send((socket_path, execution_id, runtime_dir))
                .expect("benchmark ready receiver");

            while stop_rx.try_recv().is_err() {
                runtime
                    .poll_once(Some(Duration::from_secs(1)))
                    .expect("Runtime poll");
            }
            runtime.begin_shutdown().expect("begin shutdown");
            runtime
                .run_until_empty(Instant::now() + Duration::from_secs(3))
                .expect("Runtime shutdown");
        });
        let (socket_path, execution_id, _runtime_dir) = ready_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("Runtime ready");
        Self {
            socket_path,
            execution_id,
            stop: stop_tx,
            join,
        }
    }

    fn connect_controller(&self) -> LocalDisplayClient {
        LocalDisplayClient::connect_execution(
            &self.socket_path,
            self.execution_id,
            Role::Controller,
        )
        .expect("controller attach")
    }

    fn finish(self) {
        let _ = self.stop.send(());
        self.join.join().expect("Runtime benchmark thread");
    }
}

#[cfg(target_os = "macos")]
struct PairedRuntimeHarness {
    socket_path: std::path::PathBuf,
    disabled_execution_id: ExecutionId,
    enabled_execution_id: ExecutionId,
    stop: mpsc::Sender<()>,
    join: thread::JoinHandle<()>,
}

#[cfg(target_os = "macos")]
impl PairedRuntimeHarness {
    fn start() -> Self {
        let suffix = HARNESS_COUNTER.fetch_add(1, Ordering::Relaxed);
        let (ready_tx, ready_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();
        let join = thread::spawn(move || {
            let mut config = RuntimeConfig::m001().expect("M001 Runtime config");
            config.singleton_path =
                std::env::temp_dir().join(format!("s7bp-{suffix:x}-{}.lock", process::id()));
            let runtime_dir =
                std::env::temp_dir().join(format!("s7bpd-{suffix:x}-{}", process::id()));
            config.local_ipc = LocalIpcMode::Enabled {
                runtime_dir_override: Some(runtime_dir),
            };
            config.graceful_termination = Duration::from_millis(50);
            config.forced_reap = Duration::from_millis(250);
            config.final_drain = Duration::from_millis(100);

            let mut runtime = Runtime::new(config).expect("Runtime");
            let disabled_execution_id = runtime
                .create_execution(
                    CommandSpec::new("/bin/cat"),
                    WindowSize::cells(121, 40).expect("initial geometry"),
                )
                .expect("disabled execution");
            let enabled_execution_id = runtime
                .create_execution(
                    CommandSpec::new("/bin/cat"),
                    WindowSize::cells(121, 40).expect("initial geometry"),
                )
                .expect("enabled execution");
            let socket_path = runtime
                .local_ipc_socket_path()
                .expect("local IPC socket")
                .to_path_buf();
            ready_tx
                .send((socket_path, disabled_execution_id, enabled_execution_id))
                .expect("paired benchmark ready receiver");

            while stop_rx.try_recv().is_err() {
                runtime
                    .poll_once(Some(Duration::from_secs(1)))
                    .expect("Runtime poll");
            }
            runtime.begin_shutdown().expect("begin shutdown");
            runtime
                .run_until_empty(Instant::now() + Duration::from_secs(3))
                .expect("Runtime shutdown");
        });
        let (socket_path, disabled_execution_id, enabled_execution_id) = ready_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("paired Runtime ready");
        Self {
            socket_path,
            disabled_execution_id,
            enabled_execution_id,
            stop: stop_tx,
            join,
        }
    }

    fn finish(self) {
        let _ = self.stop.send(());
        self.join.join().expect("paired Runtime benchmark thread");
    }
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct Samples {
    values_ns: Vec<u64>,
}

#[cfg(target_os = "macos")]
impl Samples {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            values_ns: Vec::with_capacity(capacity),
        }
    }

    fn push_delta(&mut self, end_ns: u64, start_ns: u64, label: &str) {
        assert!(end_ns >= start_ns, "non-monotonic Pass 7 mark for {label}");
        self.values_ns.push(end_ns - start_ns);
    }

    fn stats_us(&mut self) -> Stats {
        self.values_ns.sort_unstable();
        Stats {
            count: self.values_ns.len(),
            p50_us: percentile_ns(&self.values_ns, 50) as f64 / 1_000.0,
            p95_us: percentile_ns(&self.values_ns, 95) as f64 / 1_000.0,
            p99_us: percentile_ns(&self.values_ns, 99) as f64 / 1_000.0,
            max_us: self.values_ns.last().copied().unwrap_or(0) as f64 / 1_000.0,
        }
    }
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
struct Stats {
    count: usize,
    p50_us: f64,
    p95_us: f64,
    p99_us: f64,
    max_us: f64,
}

#[cfg(target_os = "macos")]
fn measure_input_boundaries() {
    let baseline = process_metrics();
    let runtime = RuntimeHarness::start();
    let mut client = runtime.connect_controller();
    settle_client(&mut client);
    let populated = process_metrics();

    for _ in 0..16 {
        run_input_sample(&mut client, false, None);
    }

    let mut native_to_client = Samples::with_capacity(REPETITIONS);
    let mut admission_to_socket = Samples::with_capacity(REPETITIONS);
    let mut runtime_to_pty = Samples::with_capacity(REPETITIONS);
    let mut native_to_pty = Samples::with_capacity(REPETITIONS);
    let mut client_queue_high_water = 0usize;
    let mut runtime_queue_high_water = 0usize;

    for _ in 0..REPETITIONS {
        run_input_sample(
            &mut client,
            true,
            Some((
                &mut native_to_client,
                &mut admission_to_socket,
                &mut runtime_to_pty,
                &mut native_to_pty,
                &mut client_queue_high_water,
                &mut runtime_queue_high_water,
            )),
        );
    }

    let native_client_stats = native_to_client.stats_us();
    let client_socket_stats = admission_to_socket.stats_us();
    let runtime_pty_stats = runtime_to_pty.stats_us();
    let native_pty_stats = native_to_pty.stats_us();
    let measured = process_metrics();

    print_stats(
        "controlled_native_callback_to_client_admission",
        native_client_stats,
    );
    print_stats("client_admission_to_socket_complete", client_socket_stats);
    print_stats("runtime_frame_admission_to_pty_write", runtime_pty_stats);
    print_stats("controlled_native_callback_to_pty_write", native_pty_stats);
    println!(
        "pass7_input_resources classification=MEASURED measurement_phase=post_input_workload client_queue_high_water_bytes={} runtime_queue_high_water_bytes={} rss_baseline_kib={} rss_populated_kib={} rss_measured_kib={} incremental_post_workload_rss_kib={} cpu_percent_sample={} threads_baseline={} threads_populated={} threads_measured={} fds_baseline={} fds_populated={} fds_measured={} native_boundary_classification=CONTROLLED_FFI_EQUIVALENT_APPKIT_EVENT_NOT_CLAIMED {}",
        client_queue_high_water,
        runtime_queue_high_water,
        baseline.rss_kib,
        populated.rss_kib,
        measured.rss_kib,
        measured.rss_kib.saturating_sub(baseline.rss_kib),
        measured.cpu_percent,
        baseline.threads,
        populated.threads,
        measured.threads,
        baseline.fds,
        populated.fds,
        measured.fds,
        PERFORMANCE_CLAIM,
    );

    drop(client);
    runtime.finish();
}

#[cfg(target_os = "macos")]
type InputSampleSinks<'a> = (
    &'a mut Samples,
    &'a mut Samples,
    &'a mut Samples,
    &'a mut Samples,
    &'a mut usize,
    &'a mut usize,
);

#[cfg(target_os = "macos")]
fn run_input_sample(
    client: &mut LocalDisplayClient,
    measured: bool,
    sinks: Option<InputSampleSinks<'_>>,
) {
    settle_client(client);
    reset_pass7_client_benchmark_marks();
    reset_pass7_runtime_benchmark_marks();

    let runtime_native_start = pass7_benchmark_now_ns();
    let client_native_start = pass7_client_benchmark_now_ns();
    client
        .submit_committed_text("x")
        .expect("Pass 7 committed input");
    wait_for_input_completion(client);

    if !measured {
        return;
    }
    let client_marks = pass7_client_benchmark_marks();
    let runtime_marks = pass7_runtime_benchmark_marks();
    assert_eq!(
        client_marks.admission_count, 1,
        "one client admission per sample"
    );
    assert_eq!(
        client_marks.socket_complete_count, 1,
        "one client socket completion per sample"
    );
    assert_eq!(
        runtime_marks.input_admission_count, 1,
        "one Runtime input admission per sample"
    );
    assert!(runtime_marks.pty_write_count >= 1, "PTY write mark missing");

    let Some((
        native_to_client,
        admission_to_socket,
        runtime_to_pty,
        native_to_pty,
        client_high_water,
        runtime_high_water,
    )) = sinks
    else {
        return;
    };
    native_to_client.push_delta(
        client_marks.admission_ns,
        client_native_start,
        "native->client",
    );
    admission_to_socket.push_delta(
        client_marks.socket_complete_ns,
        client_marks.admission_ns,
        "client->socket",
    );
    runtime_to_pty.push_delta(
        runtime_marks.pty_write_ns,
        runtime_marks.input_admission_ns,
        "Runtime->PTY",
    );
    native_to_pty.push_delta(
        runtime_marks.pty_write_ns,
        runtime_native_start,
        "native->PTY",
    );
    *client_high_water = (*client_high_water).max(client_marks.queue_high_water_bytes);
    *runtime_high_water = (*runtime_high_water).max(runtime_marks.runtime_queue_high_water_bytes);
}

#[cfg(target_os = "macos")]
fn wait_for_input_completion(client: &mut LocalDisplayClient) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if client.wants_write() {
            client.flush_control_write().expect("client writable flush");
        }
        match client.poll_prepare() {
            Ok(_) => {}
            Err(error) => panic!("client poll failed during Pass 7 input benchmark: {error:?}"),
        }
        let runtime_marks = pass7_runtime_benchmark_marks();
        let client_marks = pass7_client_benchmark_marks();
        if runtime_marks.pty_write_count > 0 && client_marks.socket_complete_count > 0 {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "Pass 7 input benchmark timed out"
        );
        thread::yield_now();
    }
}

#[cfg(target_os = "macos")]
fn measure_resize_boundary(label: &str, target: GridGeometry, reset: GridGeometry) {
    let baseline = process_metrics();
    let runtime = RuntimeHarness::start();
    let mut client = runtime.connect_controller();
    converge_geometry(&mut client, reset);
    let populated = process_metrics();

    for _ in 0..8 {
        run_resize_sample(&mut client, target, reset, false, None);
    }

    let mut receipt_to_commit = Samples::with_capacity(REPETITIONS);
    let mut client_queue_high_water = 0usize;
    let mut runtime_queue_high_water = 0usize;
    for _ in 0..REPETITIONS {
        run_resize_sample(
            &mut client,
            target,
            reset,
            true,
            Some((
                &mut receipt_to_commit,
                &mut client_queue_high_water,
                &mut runtime_queue_high_water,
            )),
        );
    }
    let stats = receipt_to_commit.stats_us();
    let measured = process_metrics();
    print_stats(label, stats);
    println!(
        "pass7_resize_resources case={} geometry={}x{} classification=MEASURED measurement_phase=post_resize_workload client_queue_high_water_bytes={} runtime_queue_high_water_bytes={} rss_baseline_kib={} rss_populated_kib={} rss_measured_kib={} incremental_post_resize_rss_kib={} cpu_percent_sample={} {}",
        label,
        target.columns,
        target.rows,
        client_queue_high_water,
        runtime_queue_high_water,
        baseline.rss_kib,
        populated.rss_kib,
        measured.rss_kib,
        measured.rss_kib.saturating_sub(baseline.rss_kib),
        measured.cpu_percent,
        PERFORMANCE_CLAIM,
    );

    drop(client);
    runtime.finish();
}

#[cfg(target_os = "macos")]
fn measure_pass8_resize_attribution() {
    const COHORTS: usize = 7;
    const SAMPLES_PER_MODE: usize = 512;
    let target = GridGeometry {
        rows: 40,
        columns: 120,
    };
    let reset = GridGeometry {
        rows: 40,
        columns: 121,
    };

    // Keep both cohorts on the same Runtime reactor.  Separate Runtime
    // threads can receive materially different scheduler/thermal treatment,
    // which would make this an attribution experiment for Runtime placement
    // rather than Pass 8 negotiation.
    let runtime = PairedRuntimeHarness::start();
    let mut disabled_client = LocalDisplayClient::connect_execution_without_block_metadata(
        &runtime.socket_path,
        runtime.disabled_execution_id,
        Role::Controller,
    )
    .expect("controller attach without Pass 8 metadata");
    let mut enabled_client = LocalDisplayClient::connect_execution(
        &runtime.socket_path,
        runtime.enabled_execution_id,
        Role::Controller,
    )
    .expect("controller attach");
    converge_geometry(&mut disabled_client, reset);
    converge_geometry(&mut enabled_client, reset);

    for _ in 0..32 {
        run_resize_sample(&mut disabled_client, target, reset, false, None);
        run_resize_sample(&mut enabled_client, target, reset, false, None);
    }

    let mut cohort_deltas = Vec::with_capacity(COHORTS);
    let mut disabled_p99s = Vec::with_capacity(COHORTS);
    let mut enabled_p99s = Vec::with_capacity(COHORTS);

    for cohort in 0..COHORTS {
        let mut disabled = Samples::with_capacity(SAMPLES_PER_MODE);
        let mut enabled = Samples::with_capacity(SAMPLES_PER_MODE);
        let mut disabled_client_hwm = 0usize;
        let mut disabled_runtime_hwm = 0usize;
        let mut enabled_client_hwm = 0usize;
        let mut enabled_runtime_hwm = 0usize;

        for sample in 0..SAMPLES_PER_MODE {
            let disabled_sink = Some((
                &mut disabled,
                &mut disabled_client_hwm,
                &mut disabled_runtime_hwm,
            ));
            let enabled_sink = Some((
                &mut enabled,
                &mut enabled_client_hwm,
                &mut enabled_runtime_hwm,
            ));
            if (cohort + sample) % 2 == 0 {
                run_resize_sample(&mut disabled_client, target, reset, true, disabled_sink);
                run_resize_sample(&mut enabled_client, target, reset, true, enabled_sink);
            } else {
                run_resize_sample(&mut enabled_client, target, reset, true, enabled_sink);
                run_resize_sample(&mut disabled_client, target, reset, true, disabled_sink);
            }
        }

        let disabled_p99 = disabled.stats_us().p99_us;
        let enabled_p99 = enabled.stats_us().p99_us;
        let delta_percent = if disabled_p99 > 0.0 {
            ((enabled_p99 / disabled_p99) - 1.0) * 100.0
        } else {
            0.0
        };
        disabled_p99s.push(disabled_p99);
        enabled_p99s.push(enabled_p99);
        cohort_deltas.push(delta_percent);
        println!(
            "pass8_attribution_cohort boundary=resize_120x40 classification=MEASURED cohort={} samples_per_mode={} pass8_disabled_p99_us={:.3} pass8_enabled_p99_us={:.3} delta_percent={:.2} {}",
            cohort + 1,
            SAMPLES_PER_MODE,
            disabled_p99,
            enabled_p99,
            delta_percent,
            PERFORMANCE_CLAIM,
        );
    }

    disabled_p99s.sort_by(f64::total_cmp);
    enabled_p99s.sort_by(f64::total_cmp);
    cohort_deltas.sort_by(f64::total_cmp);
    let disabled_median = disabled_p99s[COHORTS / 2];
    let enabled_median = enabled_p99s[COHORTS / 2];
    let median_paired_delta = cohort_deltas[COHORTS / 2];
    // Shared macOS runners show double-digit percent swings on sub-100µs
    // baselines. Require the median cohort AND a majority of cohorts to exceed
    // the product threshold before failing CI (foundation AUD Pass 8 flake).
    const BLOCKING_THRESHOLD_PERCENT: f64 = 10.0;
    let cohorts_over_threshold = cohort_deltas
        .iter()
        .filter(|delta| **delta > BLOCKING_THRESHOLD_PERCENT)
        .count();
    let majority_regress = cohorts_over_threshold * 2 > COHORTS;
    println!(
        "pass8_attribution boundary=resize_120x40 classification=MEASURED method=paired_live_runtimes_interleaved_7x512 pass8_disabled_p99_median_us={:.3} pass8_enabled_p99_median_us={:.3} paired_delta_median_percent={:.2} cohorts_over_threshold={}/{} blocking_threshold_percent={:.2} {}",
        disabled_median,
        enabled_median,
        median_paired_delta,
        cohorts_over_threshold,
        COHORTS,
        BLOCKING_THRESHOLD_PERCENT,
        PERFORMANCE_CLAIM,
    );
    assert!(
        !(median_paired_delta > BLOCKING_THRESHOLD_PERCENT && majority_regress),
        "Pass 8 attributable 120x40 resize p99 regression {median_paired_delta:.2}% exceeds {BLOCKING_THRESHOLD_PERCENT}% with {cohorts_over_threshold}/{COHORTS} cohorts over threshold"
    );

    drop(disabled_client);
    drop(enabled_client);
    runtime.finish();
}

#[cfg(target_os = "macos")]
fn run_resize_sample(
    client: &mut LocalDisplayClient,
    target: GridGeometry,
    reset: GridGeometry,
    measured: bool,
    sinks: Option<(&mut Samples, &mut usize, &mut usize)>,
) {
    converge_geometry(client, reset);
    reset_pass7_client_benchmark_marks();
    reset_pass7_runtime_benchmark_marks();

    client
        .set_desired_geometry(target)
        .expect("measured correlated resize admission");
    wait_for_resize_commit(client, target);

    if measured {
        let runtime_marks = pass7_runtime_benchmark_marks();
        let client_marks = pass7_client_benchmark_marks();
        assert_eq!(runtime_marks.resize_receipt_count, 1);
        assert_eq!(runtime_marks.resize_commit_count, 1);
        if let Some((samples, client_high_water, runtime_high_water)) = sinks {
            samples.push_delta(
                runtime_marks.resize_commit_ns,
                runtime_marks.resize_receipt_ns,
                "resize receipt->canonical commit",
            );
            *client_high_water = (*client_high_water).max(client_marks.queue_high_water_bytes);
            *runtime_high_water =
                (*runtime_high_water).max(runtime_marks.runtime_queue_high_water_bytes);
        }
    }
}

#[cfg(target_os = "macos")]
fn converge_geometry(client: &mut LocalDisplayClient, geometry: GridGeometry) {
    client
        .set_desired_geometry(geometry)
        .expect("correlated resize proposal");
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if client.wants_write() {
            client.flush_control_write().expect("resize writable flush");
        }
        match client.poll_prepare() {
            Ok(_) => {}
            Err(error) => panic!("client poll failed while converging geometry: {error:?}"),
        }
        if client.cache().rows == geometry.rows
            && client.cache().columns == geometry.columns
            && !client.wants_write()
            && client.resize_failure().is_none()
        {
            return;
        }
        assert!(Instant::now() < deadline, "geometry convergence timed out");
        thread::yield_now();
    }
}

#[cfg(target_os = "macos")]
fn wait_for_resize_commit(client: &mut LocalDisplayClient, geometry: GridGeometry) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if client.wants_write() {
            client.flush_control_write().expect("resize writable flush");
        }
        match client.poll_prepare() {
            Ok(_) => {}
            Err(error) => panic!("client poll failed during resize benchmark: {error:?}"),
        }
        let marks = pass7_runtime_benchmark_marks();
        if marks.resize_commit_count > 0
            && client.cache().rows == geometry.rows
            && client.cache().columns == geometry.columns
            && client.resize_failure().is_none()
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "Pass 7 resize benchmark timed out"
        );
        thread::yield_now();
    }
}

#[cfg(target_os = "macos")]
fn settle_client(client: &mut LocalDisplayClient) {
    let deadline = Instant::now() + Duration::from_millis(250);
    while Instant::now() < deadline {
        if client.wants_write() {
            client.flush_control_write().expect("settle writable flush");
        }
        let _ = client.poll_prepare();
        if !client.wants_write() {
            thread::sleep(Duration::from_micros(50));
            let _ = client.poll_prepare();
            return;
        }
    }
    panic!("Pass 7 client did not settle");
}

#[cfg(target_os = "macos")]
fn measure_idle_resources() {
    let baseline = process_metrics();
    let runtime = RuntimeHarness::start();
    let mut client = runtime.connect_controller();
    settle_client(&mut client);
    let populated = process_metrics();

    // There is deliberately no Pass 7 timer or busy-retry driver here. Leave
    // the real Runtime reactor and client idle, then sample the same process.
    thread::sleep(Duration::from_millis(500));
    let idle = process_metrics();
    println!(
        "pass7_idle_resource classification=MEASURED idle_window_ms=500 rss_baseline_kib={} rss_populated_kib={} rss_idle_kib={} incremental_idle_rss_kib={} cpu_percent_sample={} threads_baseline={} threads_idle={} fds_baseline={} fds_idle={} client_wants_write={} {}",
        baseline.rss_kib,
        populated.rss_kib,
        idle.rss_kib,
        idle.rss_kib.saturating_sub(baseline.rss_kib),
        idle.cpu_percent,
        baseline.threads,
        idle.threads,
        baseline.fds,
        idle.fds,
        client.wants_write(),
        PERFORMANCE_CLAIM,
    );

    drop(client);
    runtime.finish();
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
fn print_stats(boundary: &str, stats: Stats) {
    println!(
        "pass7_latency boundary={} classification=MEASURED sample_count={} p50_us={:.3} p95_us={:.3} p99_us={:.3} max_us={:.3} {}",
        boundary,
        stats.count,
        stats.p50_us,
        stats.p95_us,
        stats.p99_us,
        stats.max_us,
        PERFORMANCE_CLAIM,
    );
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
struct Metrics {
    rss_kib: usize,
    cpu_percent: f32,
    threads: usize,
    fds: usize,
}

#[cfg(target_os = "macos")]
fn process_metrics() -> Metrics {
    let pid = process::id();
    let output = Command::new("/bin/ps")
        .args(["-o", "rss=,%cpu=,thcount=", "-p", &pid.to_string()])
        .output()
        .expect("ps metrics");
    let line = String::from_utf8_lossy(&output.stdout);
    let mut fields = line.split_whitespace();
    let rss_kib = fields
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let cpu_percent = fields
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0.0);
    let parsed_threads = fields
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let threads = if parsed_threads == 0 {
        Command::new("/bin/ps")
            .args(["-M", "-p", &pid.to_string()])
            .output()
            .ok()
            .map(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .skip(1)
                    .count()
            })
            .unwrap_or(0)
    } else {
        parsed_threads
    };
    let fds = fs::read_dir("/dev/fd")
        .map(|entries| entries.count())
        .unwrap_or(0);
    Metrics {
        rss_kib,
        cpu_percent,
        threads,
        fds,
    }
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
        "pass7_host macos_version={} macos_build={} model={:?} hardware={:?} arch={} rust={:?} build_mode=release commit={} repetitions={} percentile_method=nearest_rank cpu_rss_sources=ps_and_usr_bin_time {}",
        product,
        build,
        model,
        hardware,
        std::env::consts::ARCH,
        rust,
        commit,
        REPETITIONS,
        PERFORMANCE_CLAIM,
    );
}

#[cfg(target_os = "macos")]
fn command_text(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|| "unavailable".to_owned())
}
