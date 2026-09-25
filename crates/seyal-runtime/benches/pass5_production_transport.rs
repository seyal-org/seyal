#[cfg(all(target_os = "macos", not(feature = "m002-contract")))]
use stats_alloc::Region;
#[cfg(not(feature = "m002-contract"))]
use stats_alloc::{StatsAlloc, INSTRUMENTED_SYSTEM};
#[cfg(not(feature = "m002-contract"))]
use std::alloc::System;

#[cfg(not(feature = "m002-contract"))]
#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

#[cfg(target_os = "macos")]
use std::{
    env, fs,
    io::{Read, Write},
    os::unix::net::UnixStream,
    process::{self, Command},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

#[cfg(target_os = "macos")]
use seyal_exec::{CommandSpec, WindowSize};
#[cfg(target_os = "macos")]
use seyal_runtime::{
    display::{
        benchmark_display_counters, decode_chunk, empty_cache, reset_benchmark_display_counters,
        DecodedDisplayChunk, DisplayAttributes, DisplayCache, DisplayCell, DisplayColor,
    },
    local_ipc::{
        fd_transfer::{benchmark_syscall_counters, reset_benchmark_syscall_counters},
        framing::{
            encode_frame, Attach, Attached, ClientHello, FrameHeader, InputRef, MessageType,
            Resize, Role, ServerHello, HEADER_LEN,
        },
    },
    ExecutionId, LocalIpcMode, Runtime, RuntimeConfig,
};

#[cfg(target_os = "macos")]
include!("../../../benches/m002_contract_support.rs");

#[cfg(all(target_os = "macos", feature = "m002-contract"))]
struct ContractAllocStats {
    allocations: usize,
    reallocations: usize,
    bytes_allocated: usize,
}

fn main() {
    #[cfg(not(target_os = "macos"))]
    println!(
        "pass5_production_transport PLATFORM_LIMITED target_os!=macos performance_claim=false"
    );

    #[cfg(target_os = "macos")]
    run_macos();
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Workload {
    Interactive,
    Command,
    Token,
    Burst,
    Sustained,
    TuiPartial,
    Tui,
    Alternate,
}

#[cfg(target_os = "macos")]
impl Workload {
    fn name(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::Command => "normal_command",
            Self::Token => "token_stream",
            Self::Burst => "burst_scroll",
            Self::Sustained => "sustained_high_output_2s",
            Self::TuiPartial => "tui_partial_redraw",
            Self::Tui => "tui_full_redraw",
            Self::Alternate => "alternate_screen",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "interactive" => Self::Interactive,
            "normal_command" => Self::Command,
            "token_stream" => Self::Token,
            "burst_scroll" => Self::Burst,
            "sustained_high_output_2s" => Self::Sustained,
            "tui_partial_redraw" => Self::TuiPartial,
            "tui_full_redraw" => Self::Tui,
            "alternate_screen" => Self::Alternate,
            _ => panic!("unknown workload {value}"),
        }
    }
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
struct Case {
    workload: Workload,
    population: usize,
    columns: u16,
    rows: u16,
    fanout: usize,
}

#[cfg(target_os = "macos")]
struct ContractSession {
    runtime: Runtime,
    runtime_dir: std::path::PathBuf,
    clients: Vec<BenchClient>,
    controller_attachment: seyal_runtime::AttachmentId,
}

#[cfg(target_os = "macos")]
fn start_contract_session(workload: Workload, columns: u16, rows: u16) -> ContractSession {
    let suffix = format!("{:x}", process::id());
    let mut config = RuntimeConfig::m001().expect("M001 config");
    config.singleton_path = env::temp_dir().join(format!("s5c-{suffix}.lock"));
    let runtime_dir = env::temp_dir().join(format!("s5c-{suffix}"));
    config.local_ipc = LocalIpcMode::Enabled {
        runtime_dir_override: Some(runtime_dir.clone()),
    };
    config.max_executions = 1;
    config.graceful_termination = Duration::from_millis(100);
    config.forced_reap = Duration::from_millis(500);
    config.final_drain = Duration::from_millis(150);
    let mut runtime = Runtime::new(config).expect("Runtime");
    let socket_path = runtime
        .local_ipc_socket_path()
        .expect("production local IPC")
        .to_path_buf();
    let execution_id = runtime
        .create_execution(
            workload_command(workload),
            WindowSize::new(columns, rows, 0, 0).expect("valid geometry"),
        )
        .expect("create contract execution");
    let mut clients = vec![BenchClient::connect_and_attach(
        &mut runtime,
        &socket_path,
        execution_id,
        Role::Controller,
    )];
    let controller_attachment = clients[0].attachment_id;
    clients[0].reset_measurement_counters();
    ContractSession {
        runtime,
        runtime_dir,
        clients,
        controller_attachment,
    }
}

#[cfg(target_os = "macos")]
fn finish_contract_session(mut session: ContractSession) {
    drop(session.clients);
    let _ = session.runtime.begin_shutdown();
    let _ = session
        .runtime
        .run_until_empty(Instant::now() + Duration::from_secs(10));
    drop(session.runtime);
    let _ = fs::remove_dir_all(session.runtime_dir);
}

#[cfg(target_os = "macos")]
fn wait_cache_advance(session: &mut ContractSession, before: u64, deadline: Instant) {
    loop {
        session
            .runtime
            .poll_once(Some(Duration::from_millis(1)))
            .expect("Runtime poll");
        session.clients[0].drain_available().expect("client drain");
        if session.clients[0].cache.generation > before {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "contract cache advance timed out"
        );
    }
}

/// Like `wait_cache_advance`, but additionally requires the cache to
/// visibly contain `marker` before treating a generation bump as proof of
/// completion. A bare generation bump is not proof THIS input caused it --
/// any concurrent source (e.g. a sustained-output stream also writing to
/// the same cache) can advance the generation too. Used for measured
/// samples; `wait_cache_advance` itself stays uncorrelated for call sites
/// that are not measuring a specific input's latency (e.g. kicking off a
/// stream).
#[cfg(target_os = "macos")]
fn wait_cache_advance_correlated(
    session: &mut ContractSession,
    before: u64,
    marker: &[u8],
    deadline: Instant,
) {
    loop {
        session
            .runtime
            .poll_once(Some(Duration::from_millis(1)))
            .expect("Runtime poll");
        session.clients[0].drain_available().expect("client drain");
        if session.clients[0].cache.generation > before
            && cache_contains_ascii(&session.clients[0].cache, marker)
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "contract cache advance timed out"
        );
    }
}

#[cfg(target_os = "macos")]
static DAMAGE_MARKER_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A unique per-sample marker so completion can be attributed to THIS
/// sample's own input rather than any concurrent source (M002 #673 Q3/Q4
/// correlation fix). ASCII-only and short enough to fit a narrow test
/// geometry (columns as low as 80).
#[cfg(target_os = "macos")]
fn next_damage_marker() -> String {
    format!(
        "d{:06}",
        DAMAGE_MARKER_COUNTER.fetch_add(1, Ordering::Relaxed) % 1_000_000
    )
}

#[cfg(target_os = "macos")]
fn sample_damage_to_client_cache(session: &mut ContractSession) -> f64 {
    let marker = next_damage_marker();
    let before = session.clients[0].cache.generation;
    let started = Instant::now();
    send_client_frame(
        &mut session.runtime,
        &mut session.clients[0],
        MessageType::Input,
        &InputRef {
            attachment_id: session.controller_attachment,
            bytes: marker.as_bytes(),
        }
        .encode(),
    );
    wait_cache_advance_correlated(
        session,
        before,
        marker.as_bytes(),
        Instant::now() + Duration::from_secs(2),
    );
    started.elapsed().as_secs_f64() * 1_000.0
}

#[cfg(target_os = "macos")]
fn start_sustained_stream(session: &mut ContractSession) {
    let before = session.clients[0].cache.generation;
    send_client_frame(
        &mut session.runtime,
        &mut session.clients[0],
        MessageType::Input,
        &InputRef {
            attachment_id: session.controller_attachment,
            bytes: b"\n",
        }
        .encode(),
    );
    wait_cache_advance(session, before, Instant::now() + Duration::from_secs(2));
}

#[cfg(target_os = "macos")]
fn activate_resize_and_scroll(session: &mut ContractSession, columns: u16, rows: u16) {
    send_client_frame(
        &mut session.runtime,
        &mut session.clients[0],
        MessageType::Resize,
        &Resize {
            attachment_id: session.controller_attachment,
            rows,
            columns,
        }
        .encode(),
    );
    send_client_frame(
        &mut session.runtime,
        &mut session.clients[0],
        MessageType::Input,
        &InputRef {
            attachment_id: session.controller_attachment,
            bytes: b"\x1b[5S",
        }
        .encode(),
    );
}

#[cfg(target_os = "macos")]
fn sample_high_output_responsiveness(session: &mut ContractSession, flip: bool) -> f64 {
    if flip {
        activate_resize_and_scroll(session, 100, 30);
    } else {
        activate_resize_and_scroll(session, 80, 24);
    }
    sample_damage_to_client_cache(session)
}

/// True while the Sustained workload's child stream is still known to be
/// actively producing output: the client's committed cache has not yet
/// observed the workload's terminal "DONE" marker. A retained
/// high_output_responsiveness sample is only meaningful while genuinely
/// concurrent with this stream; a bare "total session elapsed > 2s" check
/// (the old logic) does not prove any INDIVIDUAL sample overlapped with
/// live stream activity -- the stream could have already finished while
/// later samples kept being collected.
#[cfg(target_os = "macos")]
fn sustained_stream_active(session: &ContractSession) -> bool {
    !cache_contains_ascii(&session.clients[0].cache, b"DONE")
}

#[cfg(target_os = "macos")]
fn run_m002_contract_cohort() {
    let gate = m002_contract_gate().expect("contract gate");
    let cohort = m002_parse_usize_env("SEYAL_M002_COHORT", 1);
    let warmups = m002_parse_usize_env("SEYAL_M002_WARMUPS", 20);
    let samples = m002_parse_usize_env("SEYAL_M002_SAMPLES", 100);
    let out = env::var("SEYAL_M002_COHORT_OUT").expect("SEYAL_M002_COHORT_OUT");
    let mut retained = Vec::with_capacity(samples);
    match gate.as_str() {
        "damage_to_client_cache" => {
            let mut session = start_contract_session(Workload::Interactive, 80, 24);
            for _ in 0..warmups {
                let _ = sample_damage_to_client_cache(&mut session);
            }
            for _ in 0..samples {
                retained.push(sample_damage_to_client_cache(&mut session));
            }
            finish_contract_session(session);
        }
        "high_output_responsiveness" => {
            let mut session = start_contract_session(Workload::Sustained, 80, 24);
            start_sustained_stream(&mut session);
            let stream_started = Instant::now();
            for _ in 0..warmups {
                let _ = sample_high_output_responsiveness(&mut session, false);
                assert!(
                    sustained_stream_active(&session),
                    "high_output_responsiveness warmup sample ran after the sustained \
                     stream finished (DONE observed); it no longer reflects concurrent-\
                     stream responsiveness"
                );
            }
            for index in 0..samples {
                let sample = sample_high_output_responsiveness(&mut session, index % 8 == 0);
                assert!(
                    sustained_stream_active(&session),
                    "high_output_responsiveness sample {index} ran after the sustained \
                     stream finished (DONE observed); a retained sample must reflect \
                     genuinely concurrent stream activity, not just total session \
                     elapsed time"
                );
                retained.push(sample);
            }
            while stream_started.elapsed() < Duration::from_secs(2) {
                session
                    .runtime
                    .poll_once(Some(Duration::from_millis(10)))
                    .expect("keep stream alive");
                let _ = session.clients[0].drain_available();
            }
            finish_contract_session(session);
        }
        other => panic!("unsupported M002 contract gate {other:?}"),
    }
    m002_write_cohort_file(&out, cohort, &retained);
    println!(
        "[seyal pass5] m002_contract gate={gate} cohort={cohort} warmups={warmups} samples={samples} out={out} performance_claim=false"
    );
}

#[cfg(target_os = "macos")]
fn run_macos() {
    if m002_contract_gate().is_some() {
        run_m002_contract_cohort();
        return;
    }
    if env::args().nth(1).as_deref() == Some("--selftest-correlation") {
        selftest_correlation();
        return;
    }
    if env::args().nth(1).as_deref() == Some("--worker") {
        worker();
        return;
    }

    println!(
        "pass5_production_transport architecture=Candidate-D performance_claim=false path=real_child->PTY->Seyal_VT->canonical_damage->damage_sized_model_update->production_UDS->client_DisplayCache"
    );
    println!(
        "measurement_labels=MEASURED|PLATFORM_LIMITED percentile_method=nearest_rank streaming_latency_boundary=pty_read_damage_commit_to_client_cache_apply syscall_counts=feature_gated_exact_invocations"
    );
    print_host_metadata();

    let executable = env::current_exe().expect("benchmark executable");
    let mut cases = Vec::new();

    for fanout in [1usize, 2, 4, 8, 16] {
        cases.push(Case {
            workload: Workload::Interactive,
            population: 1,
            columns: 80,
            rows: 24,
            fanout,
        });
    }
    for fanout in [1usize, 2, 4, 8, 16] {
        cases.push(Case {
            workload: Workload::Sustained,
            population: 1,
            columns: 200,
            rows: 60,
            fanout,
        });
    }
    for population in [10usize, 50, 100] {
        cases.push(Case {
            workload: Workload::Interactive,
            population,
            columns: 80,
            rows: 24,
            fanout: 1,
        });
    }
    for (columns, rows) in [(120u16, 40u16), (200, 60), (512, 256)] {
        cases.push(Case {
            workload: Workload::Interactive,
            population: 1,
            columns,
            rows,
            fanout: 1,
        });
    }
    for workload in [
        Workload::Command,
        Workload::Token,
        Workload::Burst,
        Workload::TuiPartial,
        Workload::Tui,
        Workload::Alternate,
    ] {
        cases.push(Case {
            workload,
            population: 1,
            columns: 120,
            rows: 40,
            fanout: 16,
        });
    }

    for case in cases {
        run_worker(&executable, case);
    }
}

#[cfg(target_os = "macos")]
fn run_worker(executable: &std::path::Path, case: Case) {
    let args = [
        "--worker".to_owned(),
        case.workload.name().to_owned(),
        case.population.to_string(),
        case.columns.to_string(),
        case.rows.to_string(),
        case.fanout.to_string(),
    ];
    let status = Command::new("/usr/bin/time")
        .arg("-l")
        .arg(executable)
        .args(args)
        .status()
        .expect("launch measured Pass-5 worker");
    assert!(
        status.success(),
        "Pass-5 production benchmark worker failed"
    );
}

/// Proves, without spawning a Runtime/PTY, that (1) an unrelated cache
/// generation bump cannot be mistaken for proof that this sample's own
/// input admission completed (M002 #673 Q3/Q4), and (2) the sustained-
/// stream "active" predicate correctly distinguishes a cache that has not
/// yet observed the workload's DONE marker from one that has (Q5).
#[cfg(target_os = "macos")]
fn selftest_correlation() {
    let unrelated = cache_with_row_text(2, "unrelated-content", 80);
    assert!(
        unrelated.generation > 1,
        "precondition: generation advanced"
    );
    assert!(
        !cache_contains_ascii(&unrelated, b"d000001"),
        "FAIL: an unrelated generation bump was wrongly accepted as correlated with our marker"
    );

    let correlated = cache_with_row_text(2, "d000001", 80);
    assert!(
        correlated.generation > 1,
        "precondition: generation advanced"
    );
    assert!(
        cache_contains_ascii(&correlated, b"d000001"),
        "FAIL: a correlated generation bump carrying our marker was wrongly rejected"
    );

    let still_streaming = cache_with_row_text(3, "0004096", 80);
    assert!(
        !cache_contains_ascii(&still_streaming, b"DONE"),
        "FAIL: a cache without DONE was wrongly treated as stream-finished"
    );

    let finished = cache_with_row_text(4, "DONE", 80);
    assert!(
        cache_contains_ascii(&finished, b"DONE"),
        "FAIL: a cache carrying DONE was wrongly treated as stream-active"
    );

    println!("pass5_production_transport selftest_correlation PASSED performance_claim=false");
}

#[cfg(target_os = "macos")]
fn cache_with_row_text(generation: u64, text: &str, columns: u16) -> DisplayCache {
    let mut cells = vec![DisplayCell::blank(); columns as usize];
    for (index, byte) in text.bytes().enumerate() {
        if index >= cells.len() {
            break;
        }
        cells[index] = DisplayCell::lead_scalar(
            byte as char,
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

#[cfg(target_os = "macos")]
fn worker() {
    let args = env::args().collect::<Vec<_>>();
    let workload = Workload::parse(&args[2]);
    let requested_population = args[3].parse::<usize>().expect("population");
    let columns = args[4].parse::<u16>().expect("columns");
    let rows = args[5].parse::<u16>().expect("rows");
    let requested_fanout = args[6].parse::<usize>().expect("fanout");

    let suffix = format!("{:x}", process::id());
    let mut config = RuntimeConfig::m001().expect("M001 config");
    config.singleton_path = env::temp_dir().join(format!("s5p-{suffix}.lock"));
    let runtime_dir = env::temp_dir().join(format!("s5d-{suffix}"));
    config.local_ipc = LocalIpcMode::Enabled {
        runtime_dir_override: Some(runtime_dir.clone()),
    };
    config.max_executions = requested_population.max(1);
    config.graceful_termination = Duration::from_millis(100);
    config.forced_reap = Duration::from_millis(500);
    config.final_drain = Duration::from_millis(150);

    let baseline = process_metrics();
    let mut runtime = Runtime::new(config).expect("Runtime");
    let socket_path = runtime
        .local_ipc_socket_path()
        .expect("production local IPC")
        .to_path_buf();

    let create_start = Instant::now();
    let mut ids = Vec::with_capacity(requested_population);
    let mut platform_error = None;
    for index in 0..requested_population {
        let command = if index == 0 {
            workload_command(workload)
        } else {
            CommandSpec::new("/bin/cat")
        };
        match runtime.create_execution(
            command,
            WindowSize::new(columns, rows, 0, 0).expect("valid geometry"),
        ) {
            Ok(id) => ids.push(id),
            Err(error) => {
                platform_error = Some(error.to_string());
                break;
            }
        }
    }
    let create_us = create_start.elapsed().as_micros();
    let Some(&execution_id) = ids.first() else {
        println!(
            "pass5_production_result workload={} population_requested={} population_created=0 population_attached=0 population_hidden=0 fanout_requested={} geometry={}x{} classification=PLATFORM_LIMITED platform_error={:?}",
            workload.name(),
            requested_population,
            requested_fanout,
            columns,
            rows,
            platform_error
        );
        return;
    };

    let fanout = requested_fanout.min(16);
    let setup_start = Instant::now();
    let mut clients = Vec::with_capacity(fanout);
    for index in 0..fanout {
        let role = if index == 0 {
            Role::Controller
        } else {
            Role::Observer
        };
        clients.push(BenchClient::connect_and_attach(
            &mut runtime,
            &socket_path,
            execution_id,
            role,
        ));
    }
    let setup_us = setup_start.elapsed().as_micros();
    let controller_attachment = clients[0].attachment_id;
    for client in &mut clients {
        client.reset_measurement_counters();
    }

    // Only `execution_id` (ids[0]) ever receives an attachment in this
    // benchmark; population beyond that exists purely to measure Runtime
    // capacity, matching Workstream D's required attached-vs-hidden split.
    let population_attached = ids
        .iter()
        .filter(|&&id| {
            runtime
                .lookup(id)
                .is_some_and(|summary| summary.attachment_count > 0)
        })
        .count();
    let population_hidden = ids.len() - population_attached;

    // Preallocate benchmark bookkeeping before the measured allocation region.
    let mut latency_samples = Vec::with_capacity(32_768);
    runtime.reset_benchmark_runtime_counters();
    reset_benchmark_display_counters();
    reset_benchmark_syscall_counters();

    #[cfg(not(feature = "m002-contract"))]
    let allocation_region = Region::new(GLOBAL);
    let measurement = if workload == Workload::Interactive {
        measure_interactive(
            &mut runtime,
            &mut clients,
            controller_attachment,
            &mut latency_samples,
        )
    } else {
        measure_streaming(
            &mut runtime,
            &mut clients,
            controller_attachment,
            execution_id,
            workload,
            &mut latency_samples,
        )
    };
    #[cfg(not(feature = "m002-contract"))]
    let allocation_stats = allocation_region.change();
    #[cfg(feature = "m002-contract")]
    let allocation_stats = ContractAllocStats {
        allocations: 0,
        reallocations: 0,
        bytes_allocated: 0,
    };
    let syscall_counters = benchmark_syscall_counters();
    let display_counters = benchmark_display_counters();
    let runtime_diagnostics = runtime.benchmark_runtime_diagnostics(execution_id);
    assert!(clients.iter().all(|client| client.cache.generation > 0));

    let aggregate_uds_bytes = clients
        .iter()
        .map(|client| client.bytes_received)
        .sum::<usize>();
    let display_batches = clients
        .iter()
        .map(|client| client.display_batches)
        .sum::<usize>();
    let snapshots_received = clients.iter().map(|client| client.snapshots).sum::<usize>();
    let deltas_received = clients.iter().map(|client| client.deltas).sum::<usize>();
    let client_read_syscalls = clients
        .iter()
        .map(|client| client.read_syscalls)
        .sum::<usize>();
    let current_generation = clients
        .iter()
        .map(|client| client.cache.generation)
        .max()
        .unwrap_or(0);

    let source_pty_bps =
        bytes_per_second(runtime_diagnostics.pty_bytes_read, measurement.elapsed_us);
    let display_model_bytes = display_counters
        .snapshot_bytes
        .saturating_add(display_counters.delta_bytes);
    let display_model_bps = bytes_per_second(display_model_bytes, measurement.elapsed_us);
    let aggregate_uds_bps = bytes_per_second(aggregate_uds_bytes as u64, measurement.elapsed_us);
    let per_viewer_uds_bps = if fanout == 0 {
        0
    } else {
        aggregate_uds_bps / fanout as u128
    };

    // At the 16-viewer connection cap, reconnect by replacing one observer.
    let replace_existing_viewer = clients.len() == 16;
    if replace_existing_viewer {
        drop(clients.pop().expect("observer available for reconnect"));
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            runtime
                .poll_once(Some(Duration::from_millis(1)))
                .expect("Runtime poll while reclaiming disconnected viewer");
            if runtime
                .lookup(execution_id)
                .map(|summary| summary.attachment_count)
                .unwrap_or(0)
                < fanout
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "disconnected viewer was not reclaimed"
            );
        }
    }

    let reconnect_start = Instant::now();
    let mut reconnect =
        BenchClient::connect_and_attach(&mut runtime, &socket_path, execution_id, Role::Observer);
    let reconnect_us = reconnect_start.elapsed().as_micros();
    assert!(reconnect.cache.generation >= current_generation);
    reconnect.drain_available().expect("reconnect drain");
    if replace_existing_viewer {
        clients.push(reconnect);
    } else {
        drop(reconnect);
    }

    let populated = process_metrics();
    drop(clients);
    for _ in 0..8 {
        let _ = runtime.poll_once(Some(Duration::from_millis(2)));
    }
    let teardown_start = Instant::now();
    runtime.begin_shutdown().expect("begin shutdown");
    let shutdown = runtime.run_until_empty(Instant::now() + Duration::from_secs(10));
    let teardown_us = teardown_start.elapsed().as_micros();
    let final_metrics = process_metrics();

    let classification = if ids.len() == requested_population && requested_fanout <= 16 {
        "MEASURED"
    } else {
        "PLATFORM_LIMITED"
    };
    let latency_boundary = if workload == Workload::Interactive {
        "controller_input_send_to_all_viewers_advanced"
    } else {
        "pty_read_damage_commit_to_client_cache_apply"
    };

    println!(
        "pass5_production_result workload={} population_requested={} population_created={} population_attached={} population_hidden={} fanout_requested={} fanout_attached={} geometry={}x{} classification={} latency_boundary={} latency_sample_count={} latency_p50_us={} latency_p95_us={} latency_p99_us={} create_us={} attach_setup_us={} runtime_poll_phase_us={} client_decode_apply_phase_us={} elapsed_us={} source_pty_bytes={} source_pty_bytes_per_sec={} pty_read_calls={} source_timestamp_samples={} display_model_bytes={} display_model_bytes_per_sec={} snapshot_encodes={} delta_encodes={} aggregate_uds_bytes={} aggregate_uds_bytes_per_sec={} per_viewer_uds_bytes_per_sec={} client_read_syscalls={} server_send_syscalls={} server_sendmsg_syscalls={} runtime_recvmsg_syscalls={} display_batches_received={} snapshots_received={} deltas_received={} process_allocations={} process_reallocations={} process_bytes_allocated={} reconnect_full_snapshot_us={} rss_baseline_kib={} rss_populated_kib={} rss_final_kib={} incremental_rss_kib={} cpu_percent_sample={} threads_baseline={} threads_populated={} threads_final={} fd_baseline={} fd_populated={} fd_final={} teardown_us={} shutdown_ok={} aggregate_pending_input_final={} semantic_generation={} latest_damage_generation={} platform_error={:?}",
        workload.name(),
        requested_population,
        ids.len(),
        population_attached,
        population_hidden,
        requested_fanout,
        fanout,
        columns,
        rows,
        classification,
        latency_boundary,
        measurement.sample_count,
        measurement.p50_us,
        measurement.p95_us,
        measurement.p99_us,
        create_us,
        setup_us,
        measurement.runtime_poll_us,
        measurement.client_apply_us,
        measurement.elapsed_us,
        runtime_diagnostics.pty_bytes_read,
        source_pty_bps,
        runtime_diagnostics.pty_read_calls,
        runtime_diagnostics.source_timestamp_samples,
        display_model_bytes,
        display_model_bps,
        display_counters.snapshot_encodes,
        display_counters.delta_encodes,
        aggregate_uds_bytes,
        aggregate_uds_bps,
        per_viewer_uds_bps,
        client_read_syscalls,
        syscall_counters.send,
        syscall_counters.sendmsg,
        syscall_counters.recvmsg,
        display_batches,
        snapshots_received,
        deltas_received,
        allocation_stats.allocations,
        allocation_stats.reallocations,
        allocation_stats.bytes_allocated,
        reconnect_us,
        baseline.rss_kib,
        populated.rss_kib,
        final_metrics.rss_kib,
        populated.rss_kib.saturating_sub(baseline.rss_kib),
        populated.cpu_percent,
        baseline.threads,
        populated.threads,
        final_metrics.threads,
        baseline.fds,
        populated.fds,
        final_metrics.fds,
        teardown_us,
        shutdown.is_ok(),
        runtime.aggregate_accepted_but_unwritten_bytes(),
        current_generation,
        runtime_diagnostics.latest_damage_generation,
        platform_error,
    );

    shutdown.expect("controlled Runtime teardown");
    drop(runtime);
    let _ = fs::remove_dir_all(runtime_dir);
}

#[cfg(target_os = "macos")]
fn workload_command(workload: Workload) -> CommandSpec {
    match workload {
        Workload::Interactive => CommandSpec::new("/bin/cat"),
        Workload::Command => CommandSpec::new("/bin/sh").args([
            "-c",
            "read _; /bin/ls -la /usr/bin | /usr/bin/head -n 200; printf 'DONE\r\n'; sleep 1",
        ]),
        Workload::Token => CommandSpec::new("/bin/sh").args([
            "-c",
            "read _; i=0; while [ $i -lt 200 ]; do printf 'tok%04d ' \"$i\"; sleep 0.005; i=$((i+1)); done; printf 'DONE\r\n'; sleep 1",
        ]),
        Workload::Burst => CommandSpec::new("/bin/sh").args([
            "-c",
            "read _; yes BURST | head -n 10000; printf 'DONE\r\n'; sleep 1",
        ]),
        Workload::Sustained => CommandSpec::new("/bin/sh").args([
            "-c",
            "read _; i=0; while [ $i -lt 220 ]; do printf '%04096d\r\n' 0; sleep 0.01; i=$((i+1)); done; printf 'DONE\r\n'; sleep 1",
        ]),
        Workload::TuiPartial => CommandSpec::new("/bin/sh").args([
            "-c",
            "read _; printf '\x1b[2J'; i=0; while [ $i -lt 100 ]; do printf '\x1b[2;1HPART%04d' \"$i\"; printf '\x1b[10;1Hvalue%04d' \"$i\"; sleep 0.01; i=$((i+1)); done; printf '\x1b[1;1HDONE'; sleep 1",
        ]),
        Workload::Tui => CommandSpec::new("/bin/sh").args([
            "-c",
            "read _; i=0; while [ $i -lt 100 ]; do printf '\x1b[HFRAME%04d\r\n' \"$i\"; j=0; while [ $j -lt 30 ]; do printf 'row%02d value%04d\r\n' \"$j\" \"$i\"; j=$((j+1)); done; sleep 0.01; i=$((i+1)); done; printf 'DONE\r\n'; sleep 1",
        ]),
        Workload::Alternate => CommandSpec::new("/bin/sh").args([
            "-c",
            "read _; printf '\x1b[?1049h'; i=0; while [ $i -lt 100 ]; do printf '\x1b[HAFRAME%04d\r\n' \"$i\"; j=0; while [ $j -lt 30 ]; do printf 'alt%02d value%04d\r\n' \"$j\" \"$i\"; j=$((j+1)); done; sleep 0.01; i=$((i+1)); done; printf 'DONE\r\n'; sleep 1",
        ]),
    }
}

#[cfg(target_os = "macos")]
struct Measurement {
    sample_count: usize,
    p50_us: u128,
    p95_us: u128,
    p99_us: u128,
    runtime_poll_us: u128,
    client_apply_us: u128,
    elapsed_us: u128,
}

#[cfg(target_os = "macos")]
fn measure_interactive(
    runtime: &mut Runtime,
    clients: &mut [BenchClient],
    controller_attachment: seyal_runtime::AttachmentId,
    samples: &mut Vec<u128>,
) -> Measurement {
    samples.clear();
    let mut runtime_poll_us = 0u128;
    let mut client_apply_us = 0u128;
    let overall = Instant::now();

    for _ in 0..32 {
        let mut before = [0u64; 16];
        for (index, client) in clients.iter().enumerate() {
            before[index] = client.cache.generation;
        }
        let started = Instant::now();
        send_client_frame(
            runtime,
            &mut clients[0],
            MessageType::Input,
            &InputRef {
                attachment_id: controller_attachment,
                bytes: b"x",
            }
            .encode(),
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let poll_start = Instant::now();
            runtime
                .poll_once(Some(Duration::from_millis(1)))
                .expect("Runtime poll");
            runtime_poll_us += poll_start.elapsed().as_micros();

            let client_start = Instant::now();
            for client in clients.iter_mut() {
                client.drain_available().expect("client drain");
            }
            client_apply_us += client_start.elapsed().as_micros();
            if clients
                .iter()
                .enumerate()
                .all(|(index, client)| client.cache.generation > before[index])
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "interactive display latency timed out"
            );
        }
        samples.push(started.elapsed().as_micros());
    }

    samples.sort_unstable();
    Measurement {
        sample_count: samples.len(),
        p50_us: percentile(samples, 50),
        p95_us: percentile(samples, 95),
        p99_us: percentile(samples, 99),
        runtime_poll_us,
        client_apply_us,
        elapsed_us: overall.elapsed().as_micros(),
    }
}

#[cfg(target_os = "macos")]
fn measure_streaming(
    runtime: &mut Runtime,
    clients: &mut [BenchClient],
    controller_attachment: seyal_runtime::AttachmentId,
    execution_id: ExecutionId,
    workload: Workload,
    samples: &mut Vec<u128>,
) -> Measurement {
    samples.clear();
    let started = Instant::now();
    send_client_frame(
        runtime,
        &mut clients[0],
        MessageType::Input,
        &InputRef {
            attachment_id: controller_attachment,
            bytes: b"GO\n",
        }
        .encode(),
    );
    // These deadlines are hang-detector safety margins, not performance
    // assertions. Latency/throughput are measured independently by the
    // repeated source-to-client samples and result counters below.
    //
    // Issue #651 evidence shows that sleep-paced shell workloads stretch
    // substantially on GitHub's virtual Apple-Silicon runners even when
    // Runtime is caught up. For sustained output, the old 8s detector fired
    // after only 586161/901120 source bytes while PTY reads averaged ~819 B,
    // far below Runtime's 16 KiB read quantum; the measured producer pacing
    // implied ~12.3s to finish, so that workload keeps a 20s detector.
    //
    // The final merge-result CI found the same effect on the shorter paced
    // workloads: TuiPartial and Tui completed in ~5.53s and ~5.55s, while
    // Alternate reached 54500 bytes in 6.00s with 604 PTY reads (~90 B/read),
    // nearly the expected ~55.6 KiB source volume. That is producer pacing,
    // not a Runtime backlog. Give sleep-paced token/TUI workloads a 12s
    // detector while preserving their workload, measurement boundaries and
    // percentile calculations unchanged.
    let deadline = Instant::now()
        + match workload {
            Workload::Sustained => Duration::from_secs(20),
            Workload::Token | Workload::TuiPartial | Workload::Tui | Workload::Alternate => {
                Duration::from_secs(12)
            }
            Workload::Command | Workload::Burst => Duration::from_secs(6),
            Workload::Interactive => unreachable!("interactive uses measure_interactive"),
        };
    let mut runtime_poll_us = 0u128;
    let mut client_apply_us = 0u128;

    loop {
        let poll_start = Instant::now();
        runtime
            .poll_once(Some(Duration::from_millis(1)))
            .expect("Runtime poll");
        runtime_poll_us += poll_start.elapsed().as_micros();

        let client_start = Instant::now();
        for client in clients.iter_mut() {
            client
                .drain_available_with_batch_hook(|generation| {
                    let source = runtime
                        .benchmark_source_timestamp(execution_id, generation)
                        .unwrap_or_else(|| {
                            panic!(
                                "committed display generation {generation} has no benchmark source timestamp"
                            )
                        });
                    samples.push(Instant::now().duration_since(source).as_micros());
                })
                .expect("client drain");
        }
        client_apply_us += client_start.elapsed().as_micros();

        if clients
            .iter()
            .all(|client| cache_contains_ascii(&client.cache, b"DONE"))
        {
            break;
        }
        if Instant::now() >= deadline {
            print_streaming_timeout_diagnostics(runtime, clients, execution_id, workload, started);
            panic!("streaming workload timed out");
        }
    }

    assert!(
        !samples.is_empty(),
        "streaming benchmark produced no source-to-client latency samples"
    );
    let elapsed_us = started.elapsed().as_micros();
    samples.sort_unstable();
    Measurement {
        sample_count: samples.len(),
        p50_us: percentile(samples, 50),
        p95_us: percentile(samples, 95),
        p99_us: percentile(samples, 99),
        runtime_poll_us,
        client_apply_us,
        elapsed_us,
    }
}

#[cfg(target_os = "macos")]
fn print_streaming_timeout_diagnostics(
    runtime: &Runtime,
    clients: &[BenchClient],
    execution_id: ExecutionId,
    workload: Workload,
    started: Instant,
) {
    let runtime_diagnostics = runtime.benchmark_runtime_diagnostics(execution_id);
    let display = benchmark_display_counters();
    let syscalls = benchmark_syscall_counters();
    let aggregate_uds_bytes = clients
        .iter()
        .map(|client| client.bytes_received)
        .sum::<usize>();
    let display_batches = clients
        .iter()
        .map(|client| client.display_batches)
        .sum::<usize>();
    let client_reads = clients
        .iter()
        .map(|client| client.read_syscalls)
        .sum::<usize>();
    eprintln!(
        "pass5_streaming_timeout workload={} elapsed_us={} pty_bytes_read={} pty_read_calls={} latest_damage_generation={} source_timestamp_samples={} display_model_bytes={} snapshot_encodes={} delta_encodes={} aggregate_uds_bytes={} display_batches_received={} client_read_syscalls={} server_send_syscalls={} server_sendmsg_syscalls={} runtime_recvmsg_syscalls={}",
        workload.name(),
        started.elapsed().as_micros(),
        runtime_diagnostics.pty_bytes_read,
        runtime_diagnostics.pty_read_calls,
        runtime_diagnostics.latest_damage_generation,
        runtime_diagnostics.source_timestamp_samples,
        display.snapshot_bytes.saturating_add(display.delta_bytes),
        display.snapshot_encodes,
        display.delta_encodes,
        aggregate_uds_bytes,
        display_batches,
        client_reads,
        syscalls.send,
        syscalls.sendmsg,
        syscalls.recvmsg,
    );
}

#[cfg(target_os = "macos")]
fn percentile(sorted: &[u128], percentile: usize) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (percentile * sorted.len()).div_ceil(100).max(1);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

#[cfg(target_os = "macos")]
fn bytes_per_second(bytes: u64, elapsed_us: u128) -> u128 {
    (bytes as u128 * 1_000_000)
        .checked_div(elapsed_us)
        .unwrap_or(0)
}

#[cfg(target_os = "macos")]
struct PendingBatch {
    kind: MessageType,
    expected: usize,
    chunks: Vec<DecodedDisplayChunk>,
}

#[cfg(target_os = "macos")]
struct BenchClient {
    stream: UnixStream,
    buffered: Vec<u8>,
    cache: DisplayCache,
    attachment_id: seyal_runtime::AttachmentId,
    pending: Option<PendingBatch>,
    bytes_received: usize,
    read_syscalls: usize,
    display_batches: usize,
    snapshots: usize,
    deltas: usize,
}

#[cfg(target_os = "macos")]
impl BenchClient {
    fn connect_and_attach(
        runtime: &mut Runtime,
        socket_path: &std::path::Path,
        execution_id: ExecutionId,
        role: Role,
    ) -> Self {
        let stream = UnixStream::connect(socket_path).expect("connect production UDS");
        stream.set_nonblocking(true).expect("nonblocking client");
        let mut client = Self {
            stream,
            buffered: Vec::with_capacity(512 * 1024),
            cache: empty_cache(),
            attachment_id: seyal_runtime::AttachmentId::from_bytes([0; 16]),
            pending: None,
            bytes_received: 0,
            read_syscalls: 0,
            display_batches: 0,
            snapshots: 0,
            deltas: 0,
        };
        send_client_frame(
            runtime,
            &mut client,
            MessageType::ClientHello,
            &ClientHello {
                client_capabilities: 0,
            }
            .encode(),
        );
        let (_, hello_payload) = await_frame(runtime, &mut client, MessageType::ServerHello);
        let hello = ServerHello::decode(&hello_payload).expect("ServerHello");
        assert_ne!(
            hello.server_capabilities & seyal_runtime::local_ipc::framing::CAP_BINARY_DISPLAY,
            0
        );

        send_client_frame(
            runtime,
            &mut client,
            MessageType::Attach,
            &Attach {
                execution_id,
                requested_role: role,
            }
            .encode(),
        );
        let (_, attached_payload) = await_frame(runtime, &mut client, MessageType::Attached);
        let attached = Attached::decode(&attached_payload).expect("Attached");
        client.attachment_id = attached.attachment_id;
        let chunks = await_display_batch(runtime, &mut client, MessageType::DisplaySnapshot);
        client.cache.apply_chunks(&chunks).expect("attach snapshot");
        assert_eq!(client.cache.generation, attached.current_generation);
        client
    }

    fn reset_measurement_counters(&mut self) {
        self.bytes_received = 0;
        self.read_syscalls = 0;
        self.display_batches = 0;
        self.snapshots = 0;
        self.deltas = 0;
    }

    fn drain_available(&mut self) -> std::io::Result<()> {
        self.drain_available_with_batch_hook(|_| {})
    }

    fn drain_available_with_batch_hook<F>(&mut self, mut on_batch: F) -> std::io::Result<()>
    where
        F: FnMut(u64),
    {
        let mut buffer = [0u8; 32 * 1024];
        loop {
            match self.stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    self.bytes_received += count;
                    self.read_syscalls += 1;
                    self.buffered.extend_from_slice(&buffer[..count]);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error),
            }
        }

        while let Some((kind, payload)) = take_frame(&mut self.buffered) {
            let Some(message_type) = MessageType::from_u16(kind) else {
                continue;
            };
            if !matches!(
                message_type,
                MessageType::DisplaySnapshot | MessageType::DisplayDelta
            ) {
                continue;
            }
            let chunk =
                decode_chunk(&encode_frame(message_type, &payload)).expect("display decode");
            let expected = chunk.chunk_count as usize;
            if self.pending.is_none() {
                self.pending = Some(PendingBatch {
                    kind: message_type,
                    expected,
                    chunks: Vec::with_capacity(expected),
                });
            }
            let pending = self.pending.as_mut().expect("pending batch");
            assert_eq!(
                pending.kind, message_type,
                "display chunks must remain contiguous"
            );
            assert_eq!(pending.expected, expected, "chunk count changed mid-batch");
            pending.chunks.push(chunk);
            if pending.chunks.len() == pending.expected {
                let complete = self.pending.take().expect("complete batch");
                self.cache
                    .apply_chunks(&complete.chunks)
                    .expect("client cache apply");
                self.display_batches += 1;
                match complete.kind {
                    MessageType::DisplaySnapshot => self.snapshots += 1,
                    MessageType::DisplayDelta => self.deltas += 1,
                    _ => unreachable!(),
                }
                // One nonblocking socket drain may contain multiple complete
                // presentation batches. Observe each committed cache generation
                // here so percentile sampling cannot silently collapse them.
                on_batch(self.cache.generation);
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn send_client_frame(
    runtime: &mut Runtime,
    client: &mut BenchClient,
    message_type: MessageType,
    payload: &[u8],
) {
    let frame = encode_frame(message_type, payload);
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut sent = 0usize;
    while sent < frame.len() {
        match client.stream.write(&frame[sent..]) {
            Ok(0) => panic!("client socket closed while writing"),
            Ok(count) => sent += count,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                runtime
                    .poll_once(Some(Duration::from_millis(1)))
                    .expect("Runtime poll");
            }
            Err(error) => panic!("client write failed: {error}"),
        }
        assert!(Instant::now() < deadline, "client send timed out");
    }
    runtime
        .poll_once(Some(Duration::from_millis(1)))
        .expect("Runtime poll");
}

#[cfg(target_os = "macos")]
fn await_frame(
    runtime: &mut Runtime,
    client: &mut BenchClient,
    expected: MessageType,
) -> (u16, Vec<u8>) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(frame) = take_frame(&mut client.buffered) {
            assert_eq!(frame.0, expected as u16);
            return frame;
        }
        let mut buffer = [0u8; 16 * 1024];
        match client.stream.read(&mut buffer) {
            Ok(0) => panic!("client socket closed"),
            Ok(count) => client.buffered.extend_from_slice(&buffer[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                runtime
                    .poll_once(Some(Duration::from_millis(1)))
                    .expect("Runtime poll");
            }
            Err(error) => panic!("client read failed: {error}"),
        }
        assert!(Instant::now() < deadline, "frame wait timed out");
    }
}

#[cfg(target_os = "macos")]
fn await_display_batch(
    runtime: &mut Runtime,
    client: &mut BenchClient,
    expected: MessageType,
) -> Vec<DecodedDisplayChunk> {
    let (_, payload) = await_frame(runtime, client, expected);
    let first = decode_chunk(&encode_frame(expected, &payload)).expect("display chunk");
    let count = first.chunk_count as usize;
    let mut chunks = vec![first];
    for _ in 1..count {
        let (_, payload) = await_frame(runtime, client, expected);
        chunks.push(decode_chunk(&encode_frame(expected, &payload)).expect("display chunk"));
    }
    chunks
}

#[cfg(target_os = "macos")]
fn take_frame(buffer: &mut Vec<u8>) -> Option<(u16, Vec<u8>)> {
    if buffer.len() < HEADER_LEN {
        return None;
    }
    let header = FrameHeader::decode(&buffer[..HEADER_LEN]).ok()?;
    let total = HEADER_LEN.checked_add(header.payload_len as usize)?;
    if buffer.len() < total {
        return None;
    }
    let frame = buffer.drain(..total).collect::<Vec<_>>();
    Some((header.message_type, frame[HEADER_LEN..].to_vec()))
}

#[cfg(target_os = "macos")]
fn cache_contains_ascii(cache: &DisplayCache, needle: &[u8]) -> bool {
    if cache.columns == 0 || needle.is_empty() || needle.iter().any(|byte| !byte.is_ascii()) {
        return needle.is_empty();
    }
    let columns = cache.columns as usize;
    cache.cells.chunks(columns).any(|row| {
        row.windows(needle.len()).any(|window| {
            window
                .iter()
                .zip(needle)
                .all(|(cell, byte)| cell.scalar == *byte as char)
        })
    })
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
    let pty_max = command_text("/usr/sbin/sysctl", &["-n", "kern.tty.ptmx_max"]);
    let rust = command_text("rustc", &["--version"]);
    let commit = command_text("git", &["rev-parse", "HEAD"]);
    println!(
        "pass5_production_host macos_version={} macos_build={} model={:?} hardware={:?} rust={:?} pty_max={} build_mode=release commit={} cpu_measurement=/usr/bin/time_-l repetitions=case_defined",
        product, build, model, hardware, rust, pty_max, commit
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
