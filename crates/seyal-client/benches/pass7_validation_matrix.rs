use std::time::Instant;

#[cfg(target_os = "macos")]
use std::{
    process,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

#[cfg(target_os = "macos")]
use seyal_client::{ClientError, GridGeometry, LocalDisplayClient};
#[cfg(target_os = "macos")]
use seyal_exec::{CommandSpec, WindowSize};
#[cfg(target_os = "macos")]
use seyal_runtime::local_ipc::framing::{Role, TerminalKeyKind};
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
const KEY_REPEAT_BURST: usize = 64;
#[cfg(target_os = "macos")]
static HARNESS_COUNTER: AtomicU64 = AtomicU64::new(0);

fn main() {
    // Required by the repository benchmark contract. `Instant::now()` is also
    // used for bounded harness deadlines; production timing remains untouched.
    let _contract_clock = Instant::now();

    #[cfg(not(target_os = "macos"))]
    println!("pass7_validation_matrix PLATFORM_LIMITED target_os!=macos {PERFORMANCE_CLAIM}");

    #[cfg(target_os = "macos")]
    run_macos();
}

#[cfg(target_os = "macos")]
fn run_macos() {
    println!(
        "pass7_matrix_host classification=MEASURED repetitions={REPETITIONS} key_repeat_burst={KEY_REPEAT_BURST} architecture=production_client_UDS_Runtime_PTY {PERFORMANCE_CLAIM}"
    );
    measure_commit_size("commit_1b", 1);
    measure_commit_size("commit_16kib", 16 * 1024);
    measure_commit_size("commit_64kib", 64 * 1024);
    measure_oversized_rejection();
    measure_key_repeat_burst();
    measure_input_under_output();
    measure_alternate_screen_path();
    println!(
        "pass7_matrix_remaining validation=persistent_runtime_resize_failure_and_true_AppKit_event_boundary classification=NOT_CLAIMED {PERFORMANCE_CLAIM}"
    );
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
    fn start(command: CommandSpec) -> Self {
        let suffix = HARNESS_COUNTER.fetch_add(1, Ordering::Relaxed);
        let (ready_tx, ready_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();
        let join = thread::spawn(move || {
            let mut config = RuntimeConfig::m001()
                .expect("M001 Runtime config")
                .isolated_to(
                    std::env::temp_dir().join(format!("s7md-{suffix:x}-{}", process::id())),
                );
            config.graceful_termination = Duration::from_millis(50);
            config.forced_reap = Duration::from_millis(250);
            config.final_drain = Duration::from_millis(100);

            let mut runtime = Runtime::new(config).expect("Runtime");
            let execution_id = runtime
                .create_execution(
                    command,
                    WindowSize::cells(80, 24).expect("initial geometry"),
                )
                .expect("execution");
            let socket_path = runtime
                .local_ipc_socket_path()
                .expect("local IPC socket")
                .to_path_buf();
            ready_tx
                .send((socket_path, execution_id))
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
        let (socket_path, execution_id) = ready_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("Runtime ready");
        Self {
            socket_path,
            execution_id,
            stop: stop_tx,
            join,
        }
    }

    fn controller(&self) -> LocalDisplayClient {
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
fn raw_sink_command() -> CommandSpec {
    CommandSpec::new("/bin/sh").args(["-c", "stty raw -echo; cat >/dev/null"])
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct Samples(Vec<u64>);

#[cfg(target_os = "macos")]
impl Samples {
    fn push_ns(&mut self, value: u64) {
        self.0.push(value);
    }

    fn stats(&mut self) -> Stats {
        self.0.sort_unstable();
        Stats {
            count: self.0.len(),
            p50_us: percentile(&self.0, 50) as f64 / 1_000.0,
            p95_us: percentile(&self.0, 95) as f64 / 1_000.0,
            p99_us: percentile(&self.0, 99) as f64 / 1_000.0,
            max_us: self.0.last().copied().unwrap_or(0) as f64 / 1_000.0,
        }
    }
}

#[cfg(target_os = "macos")]
struct Stats {
    count: usize,
    p50_us: f64,
    p95_us: f64,
    p99_us: f64,
    max_us: f64,
}

#[cfg(target_os = "macos")]
fn measure_commit_size(label: &str, bytes: usize) {
    let runtime = RuntimeHarness::start(raw_sink_command());
    let mut client = runtime.controller();
    let payload = "x".repeat(bytes);
    let mut samples = Samples::default();
    let mut max_client_queue = 0usize;
    let mut max_runtime_queue = 0usize;
    let expected = u64::try_from(bytes).expect("commit size fits u64");

    for _ in 0..REPETITIONS {
        settle(&mut client);
        reset_pass7_runtime_benchmark_marks();
        let start_ns = pass7_benchmark_now_ns();
        client
            .submit_committed_text(&payload)
            .expect("legal committed-text admission");
        wait_for_pty_bytes(&mut client, expected);
        let marks = pass7_runtime_benchmark_marks();
        assert_eq!(marks.input_admission_count, 1, "one Input frame per commit");
        assert_eq!(
            marks.pty_write_bytes, expected,
            "complete commit reached PTY"
        );
        assert!(marks.pty_write_ns >= start_ns);
        samples.push_ns(marks.pty_write_ns - start_ns);
        max_client_queue = max_client_queue.max(client.outbound_wire_bytes());
        max_runtime_queue = max_runtime_queue.max(marks.runtime_queue_high_water_bytes);
    }

    print_case(
        label,
        bytes,
        samples.stats(),
        max_client_queue,
        max_runtime_queue,
    );
    drop(client);
    runtime.finish();
}

#[cfg(target_os = "macos")]
fn measure_oversized_rejection() {
    let runtime = RuntimeHarness::start(raw_sink_command());
    let mut client = runtime.controller();
    let payload = "x".repeat(65_537);
    let mut samples = Samples::default();

    for _ in 0..REPETITIONS {
        settle(&mut client);
        reset_pass7_runtime_benchmark_marks();
        let start = Instant::now();
        assert_eq!(
            client.submit_committed_text(&payload),
            Err(ClientError::CommitTooLarge)
        );
        samples.push_ns(u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX));
        let marks = pass7_runtime_benchmark_marks();
        assert_eq!(marks.input_admission_count, 0);
        assert_eq!(marks.pty_write_bytes, 0);
        assert_eq!(client.outbound_wire_bytes(), 0);
    }

    let stats = samples.stats();
    println!(
        "pass7_matrix case=reject_65537b classification=MEASURED sample_count={} p50_us={:.3} p95_us={:.3} p99_us={:.3} max_us={:.3} atomic_rejection=true pty_write_bytes=0 client_queue_bytes=0 {PERFORMANCE_CLAIM}",
        stats.count, stats.p50_us, stats.p95_us, stats.p99_us, stats.max_us
    );
    drop(client);
    runtime.finish();
}

#[cfg(target_os = "macos")]
fn measure_key_repeat_burst() {
    let runtime = RuntimeHarness::start(raw_sink_command());
    let mut client = runtime.controller();
    let expected = u64::try_from(KEY_REPEAT_BURST * 3).expect("burst bytes");
    let mut samples = Samples::default();
    let mut max_runtime_queue = 0usize;

    for _ in 0..REPETITIONS {
        settle(&mut client);
        reset_pass7_runtime_benchmark_marks();
        let start_ns = pass7_benchmark_now_ns();
        for _ in 0..KEY_REPEAT_BURST {
            client
                .submit_terminal_key(TerminalKeyKind::ArrowUp, 0)
                .expect("semantic key repeat admission");
        }
        wait_for_pty_bytes(&mut client, expected);
        let marks = pass7_runtime_benchmark_marks();
        assert_eq!(marks.input_admission_count as usize, KEY_REPEAT_BURST);
        assert_eq!(marks.pty_write_bytes, expected);
        samples.push_ns(marks.pty_write_ns - start_ns);
        max_runtime_queue = max_runtime_queue.max(marks.runtime_queue_high_water_bytes);
    }

    let stats = samples.stats();
    println!(
        "pass7_matrix case=key_repeat_arrow_up classification=MEASURED sample_count={} keys_per_burst={} encoded_bytes_per_burst={} p50_us={:.3} p95_us={:.3} p99_us={:.3} max_us={:.3} runtime_queue_high_water_bytes={} {PERFORMANCE_CLAIM}",
        stats.count,
        KEY_REPEAT_BURST,
        expected,
        stats.p50_us,
        stats.p95_us,
        stats.p99_us,
        stats.max_us,
        max_runtime_queue
    );
    drop(client);
    runtime.finish();
}

#[cfg(target_os = "macos")]
fn measure_input_under_output() {
    let command = CommandSpec::new("/bin/sh").args([
        "-c",
        "(i=0; while [ $i -lt 250000 ]; do printf 'LOAD%06d\\r\\n' \"$i\"; i=$((i+1)); done) & stty raw -echo; cat >/dev/null",
    ]);
    let runtime = RuntimeHarness::start(command);
    let mut client = runtime.controller();
    let initial_generation = client.cache().generation;
    let mut samples = Samples::default();
    let mut observed_output_progress = false;

    for _ in 0..REPETITIONS {
        reset_pass7_runtime_benchmark_marks();
        let start_ns = pass7_benchmark_now_ns();
        client
            .submit_committed_text("x")
            .expect("input admission under output");
        wait_for_pty_bytes(&mut client, 1);
        let marks = pass7_runtime_benchmark_marks();
        samples.push_ns(marks.pty_write_ns - start_ns);
        if client.poll_prepare().ok().flatten().is_some()
            || client.cache().generation > initial_generation
        {
            observed_output_progress = true;
        }
    }
    assert!(
        observed_output_progress,
        "sustained output path was not observed"
    );

    let stats = samples.stats();
    println!(
        "pass7_matrix case=input_under_sustained_output classification=MEASURED sample_count={} p50_us={:.3} p95_us={:.3} p99_us={:.3} max_us={:.3} output_progress_observed=true {PERFORMANCE_CLAIM}",
        stats.count, stats.p50_us, stats.p95_us, stats.p99_us, stats.max_us
    );
    drop(client);
    runtime.finish();
}

#[cfg(target_os = "macos")]
fn measure_alternate_screen_path() {
    let command = CommandSpec::new("/bin/sh").args([
        "-c",
        "printf '\\033[?1049hALT\\r\\n'; stty raw -echo; cat >/dev/null",
    ]);
    let runtime = RuntimeHarness::start(command);
    let mut client = runtime.controller();
    wait_until(&mut client, |client| client.cache().alternate_screen);
    let mut samples = Samples::default();

    for _ in 0..REPETITIONS {
        reset_pass7_runtime_benchmark_marks();
        let start_ns = pass7_benchmark_now_ns();
        client
            .submit_committed_text("x")
            .expect("alternate-screen input admission");
        wait_for_pty_bytes(&mut client, 1);
        let marks = pass7_runtime_benchmark_marks();
        samples.push_ns(marks.pty_write_ns - start_ns);
    }

    client
        .set_desired_geometry(GridGeometry {
            rows: 30,
            columns: 100,
        })
        .expect("alternate-screen resize");
    wait_until(&mut client, |client| {
        client.cache().alternate_screen
            && client.cache().rows == 30
            && client.cache().columns == 100
            && client.resize_failure().is_none()
    });

    let stats = samples.stats();
    println!(
        "pass7_matrix case=alternate_screen_input_resize classification=MEASURED sample_count={} p50_us={:.3} p95_us={:.3} p99_us={:.3} max_us={:.3} alternate_screen=true final_geometry=100x30 {PERFORMANCE_CLAIM}",
        stats.count, stats.p50_us, stats.p95_us, stats.p99_us, stats.max_us
    );
    drop(client);
    runtime.finish();
}

#[cfg(target_os = "macos")]
fn wait_for_pty_bytes(client: &mut LocalDisplayClient, expected: u64) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if client.wants_write() {
            client.flush_control_write().expect("client writable flush");
        }
        let _ = client.poll_prepare();
        if pass7_runtime_benchmark_marks().pty_write_bytes >= expected {
            return;
        }
        assert!(Instant::now() < deadline, "PTY byte completion timed out");
        thread::yield_now();
    }
}

#[cfg(target_os = "macos")]
fn wait_until(client: &mut LocalDisplayClient, predicate: impl Fn(&LocalDisplayClient) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if predicate(client) {
            return;
        }
        if client.wants_write() {
            client.flush_control_write().expect("client writable flush");
        }
        let _ = client.poll_prepare();
        assert!(
            Instant::now() < deadline,
            "Pass 7 matrix condition timed out"
        );
        thread::yield_now();
    }
}

#[cfg(target_os = "macos")]
fn settle(client: &mut LocalDisplayClient) {
    let deadline = Instant::now() + Duration::from_millis(250);
    loop {
        if client.wants_write() {
            client.flush_control_write().expect("settle writable flush");
        }
        let _ = client.poll_prepare();
        if !client.wants_write() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "Pass 7 matrix client did not settle"
        );
    }
}

#[cfg(target_os = "macos")]
fn print_case(label: &str, bytes: usize, stats: Stats, client_queue: usize, runtime_queue: usize) {
    println!(
        "pass7_matrix case={} classification=MEASURED sample_count={} committed_bytes={} p50_us={:.3} p95_us={:.3} p99_us={:.3} max_us={:.3} client_queue_high_water_bytes={} runtime_queue_high_water_bytes={} final_pty_completion=true {PERFORMANCE_CLAIM}",
        label,
        stats.count,
        bytes,
        stats.p50_us,
        stats.p95_us,
        stats.p99_us,
        stats.max_us,
        client_queue,
        runtime_queue
    );
}

#[cfg(target_os = "macos")]
fn percentile(sorted: &[u64], value: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (value * sorted.len()).div_ceil(100).max(1);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}
