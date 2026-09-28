//! M003 M1 (#1164) provisioning latency and live-execution scaling.
//!
//! Measures the portable C1 path against a real Runtime (P3 create / P4 dispose
//! seam already on the branch). Does not change provisioning behavior.
//! Every printed number is diagnostic (`performance_claim=false`); absolute
//! product budgets are not asserted here.

use std::time::Instant;

#[cfg(target_os = "macos")]
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    process,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

#[cfg(target_os = "macos")]
use seyal_client::provisioning::{
    CreateOutcome, PaneGeometry, ProvisioningEffect, ProvisioningSession,
};
#[cfg(target_os = "macos")]
use seyal_core::{AttachmentId, ExecutionId, PaneId};
#[cfg(target_os = "macos")]
use seyal_runtime::{
    local_ipc::framing::{
        encode_frame, Attach, Attached, ClientHello, CreateExecutionRequest, CreateExecutionResult,
        CreateExecutionResultCode, FrameHeader, Lifecycle, LifecycleMessage, MessageType, Role,
        ServerHello, TerminateExecutionRequest, TerminateExecutionResult,
        TerminateExecutionResultCode, CAP_EXECUTION_PROVISIONING, HEADER_LEN,
    },
    LocalIpcMode, Runtime, RuntimeConfig,
};

const PERFORMANCE_CLAIM: &str = "performance_claim=false";

#[cfg(target_os = "macos")]
const LATENCY_SAMPLES: usize = 40;
#[cfg(target_os = "macos")]
const LATENCY_WARMUPS: usize = 4;
#[cfg(target_os = "macos")]
const LIVE_POPULATIONS: &[usize] = &[1, 10, 50, 100];
#[cfg(target_os = "macos")]
static HARNESS_COUNTER: AtomicU64 = AtomicU64::new(0);

fn main() {
    let _contract_clock = Instant::now();

    #[cfg(not(target_os = "macos"))]
    {
        println!(
            "m003_provisioning PLATFORM_LIMITED target_os!=macos evidence_class=PLATFORM_LIMITED {PERFORMANCE_CLAIM}"
        );
        return;
    }

    #[cfg(target_os = "macos")]
    run_macos();
}

#[cfg(target_os = "macos")]
fn run_macos() {
    println!(
        "m003_provisioning architecture=C1_ProvisioningSession_plus_P3_CreateExecution_UDS evidence_class=CI percentile_method=nearest_rank repetitions={LATENCY_SAMPLES} {PERFORMANCE_CLAIM}"
    );
    print_host_metadata();
    measure_latencies();
    measure_live_execution_scaling();
    println!(
        "m003_provisioning_presentation_scaling counts=1,10,50,100 evidence_class=PLATFORM_LIMITED reason=headed_tab_and_split_chrome_require_C2_and_C3 {PERFORMANCE_CLAIM}"
    );
}

#[cfg(target_os = "macos")]
struct RuntimeHarness {
    socket_path: std::path::PathBuf,
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
                std::env::temp_dir().join(format!("m1b-{suffix:x}-{}.lock", process::id()));
            config.local_ipc = LocalIpcMode::Enabled {
                runtime_dir_override: Some(
                    std::env::temp_dir().join(format!("m1bd-{suffix:x}-{}", process::id())),
                ),
            };
            config.graceful_termination = Duration::from_millis(50);
            config.forced_reap = Duration::from_millis(250);
            config.final_drain = Duration::from_millis(100);
            let mut runtime = Runtime::new(config).expect("Runtime");
            let socket_path = runtime
                .local_ipc_socket_path()
                .expect("local IPC socket")
                .to_path_buf();
            ready_tx.send(socket_path).expect("ready receiver");
            while stop_rx.try_recv().is_err() {
                runtime
                    .poll_once(Some(Duration::from_millis(5)))
                    .expect("Runtime poll");
            }
            runtime.begin_shutdown().expect("begin shutdown");
            let _ = runtime.run_until_empty(Instant::now() + Duration::from_secs(3));
        });
        let socket_path = ready_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("Runtime ready");
        Self {
            socket_path,
            stop: stop_tx,
            join,
        }
    }

    fn finish(self) {
        let _ = self.stop.send(());
        self.join.join().expect("Runtime benchmark thread");
    }
}

#[cfg(target_os = "macos")]
struct WireClient {
    stream: UnixStream,
    buffered: Vec<u8>,
}

#[cfg(target_os = "macos")]
impl WireClient {
    fn connect(socket: &std::path::Path) -> Self {
        let deadline = Instant::now() + Duration::from_secs(3);
        let stream = loop {
            match UnixStream::connect(socket) {
                Ok(stream) => break stream,
                Err(_) => {
                    assert!(Instant::now() < deadline, "connect timeout");
                    thread::sleep(Duration::from_millis(5));
                }
            }
        };
        stream.set_nonblocking(true).expect("nonblocking");
        Self {
            stream,
            buffered: Vec::new(),
        }
    }

    fn send(&mut self, kind: MessageType, payload: &[u8]) {
        let bytes = encode_frame(kind, payload);
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut sent = 0;
        while sent < bytes.len() {
            match self.stream.write(&bytes[sent..]) {
                Ok(0) => panic!("zero write"),
                Ok(count) => sent += count,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("write: {error}"),
            }
            assert!(Instant::now() < deadline, "write timeout");
        }
    }

    fn expect_frame(&mut self, deadline: Instant) -> (u16, Vec<u8>) {
        loop {
            if self.buffered.len() >= HEADER_LEN {
                let header =
                    FrameHeader::decode(&self.buffered[..HEADER_LEN]).expect("valid header");
                let total = HEADER_LEN + header.payload_len as usize;
                if self.buffered.len() >= total {
                    let frame = self.buffered.drain(..total).collect::<Vec<_>>();
                    return (header.message_type, frame[HEADER_LEN..].to_vec());
                }
            }
            let mut chunk = [0u8; 8192];
            match self.stream.read(&mut chunk) {
                Ok(0) => panic!("connection closed while awaiting frame"),
                Ok(count) => self.buffered.extend_from_slice(&chunk[..count]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("read: {error}"),
            }
            assert!(Instant::now() < deadline, "timed out awaiting frame");
        }
    }

    fn hello(&mut self) {
        self.send(
            MessageType::ClientHello,
            &ClientHello {
                client_capabilities: CAP_EXECUTION_PROVISIONING,
            }
            .encode(),
        );
        let (kind, payload) = self.expect_frame(Instant::now() + Duration::from_secs(2));
        assert_eq!(kind, MessageType::ServerHello as u16);
        let _ = ServerHello::decode(&payload).expect("ServerHello");
    }

    fn create_timed(&mut self, request_id: u64) -> (CreateExecutionResult, Duration) {
        let started = Instant::now();
        self.send(
            MessageType::CreateExecutionRequest,
            &CreateExecutionRequest {
                workspace_id: 0,
                request_id,
                launch_profile: 0,
                rows: 24,
                columns: 80,
            }
            .encode(),
        );
        loop {
            let (kind, payload) = self.expect_frame(Instant::now() + Duration::from_secs(3));
            if kind == MessageType::CreateExecutionResult as u16 {
                let result = CreateExecutionResult::decode(&payload).expect("create result");
                return (result, started.elapsed());
            }
        }
    }

    fn attach_controller(&mut self, execution_id: ExecutionId) -> Attached {
        self.send(
            MessageType::Attach,
            &Attach {
                execution_id,
                requested_role: Role::Controller,
            }
            .encode(),
        );
        loop {
            let (kind, payload) = self.expect_frame(Instant::now() + Duration::from_secs(2));
            if kind == MessageType::Attached as u16 {
                return Attached::decode(&payload).expect("Attached");
            }
        }
    }

    fn terminate(
        &mut self,
        attachment_id: AttachmentId,
        execution_id: ExecutionId,
        request_id: u64,
    ) -> TerminateExecutionResult {
        self.send(
            MessageType::TerminateExecutionRequest,
            &TerminateExecutionRequest {
                attachment_id,
                execution_id,
                request_id,
            }
            .encode(),
        );
        loop {
            let (kind, payload) = self.expect_frame(Instant::now() + Duration::from_secs(3));
            if kind == MessageType::TerminateExecutionResult as u16 {
                return TerminateExecutionResult::decode(&payload).expect("terminate result");
            }
        }
    }

    fn wait_finalized(&mut self, execution_id: ExecutionId) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let (kind, payload) = self.expect_frame(deadline);
            if kind == MessageType::Lifecycle as u16 {
                let message = LifecycleMessage::decode(&payload).expect("Lifecycle");
                if message.execution_id == execution_id && message.lifecycle == Lifecycle::Finalized
                {
                    return;
                }
            }
        }
        panic!("execution never finalized");
    }
}

#[cfg(target_os = "macos")]
fn measure_latencies() {
    let harness = RuntimeHarness::start();
    thread::sleep(Duration::from_millis(20));
    let mut wire = WireClient::connect(&harness.socket_path);
    wire.hello();

    let mut published_us = Vec::with_capacity(LATENCY_SAMPLES);
    let mut bound_pane_us = Vec::with_capacity(LATENCY_SAMPLES);
    let mut next_request = 1u64;

    for sample in 0..(LATENCY_WARMUPS + LATENCY_SAMPLES) {
        let mut session = ProvisioningSession::new();
        let pane = PaneId::new();
        let effect = session
            .begin_intent(
                pane,
                Some(PaneGeometry {
                    rows: 24,
                    columns: 80,
                }),
            )
            .expect("begin_intent");
        let ProvisioningEffect::SendCreate {
            owner,
            request_id: session_request,
            ..
        } = effect
        else {
            panic!("expected SendCreate");
        };

        let path_started = Instant::now();
        let (created, published_elapsed) = wire.create_timed(next_request);
        next_request += 1;
        assert_eq!(created.result_code, CreateExecutionResultCode::Created);

        let effects = session.apply_create_result(
            owner,
            session_request,
            CreateOutcome::Created(created.execution_id),
        );
        assert!(matches!(
            effects.as_slice(),
            [ProvisioningEffect::AttachController { .. }]
        ));
        let attached = wire.attach_controller(created.execution_id);
        let effects = session.apply_attach_success(owner, session_request, attached.attachment_id);
        assert!(matches!(
            effects.as_slice(),
            [ProvisioningEffect::BindPane { .. }]
        ));
        let _ = session.apply_bind_success(pane, created.execution_id);
        assert_eq!(session.recorded_execution(pane), Some(created.execution_id));
        let bound_elapsed = path_started.elapsed();

        if sample >= LATENCY_WARMUPS {
            published_us.push(published_elapsed.as_micros().min(u128::from(u64::MAX)) as u64);
            bound_pane_us.push(bound_elapsed.as_micros().min(u128::from(u64::MAX)) as u64);
        }

        let terminated = wire.terminate(attached.attachment_id, created.execution_id, next_request);
        next_request += 1;
        assert_eq!(
            terminated.result_code,
            TerminateExecutionResultCode::TerminationRequested
        );
        wire.wait_finalized(created.execution_id);
    }

    published_us.sort_unstable();
    bound_pane_us.sort_unstable();
    println!(
        "m003_provisioning_latency boundary=request_to_published_execution evidence_class=CI sample_count={} p50_us={} p95_us={} p99_us={} max_us={} {PERFORMANCE_CLAIM}",
        published_us.len(),
        percentile_us(&published_us, 50),
        percentile_us(&published_us, 95),
        percentile_us(&published_us, 99),
        published_us.last().copied().unwrap_or(0),
    );
    println!(
        "m003_provisioning_latency boundary=request_to_usable_bound_pane evidence_class=CI sample_count={} p50_us={} p95_us={} p99_us={} max_us={} path=begin_intent_create_attach_bind {PERFORMANCE_CLAIM}",
        bound_pane_us.len(),
        percentile_us(&bound_pane_us, 50),
        percentile_us(&bound_pane_us, 95),
        percentile_us(&bound_pane_us, 99),
        bound_pane_us.last().copied().unwrap_or(0),
    );

    harness.finish();
}

#[cfg(target_os = "macos")]
fn measure_live_execution_scaling() {
    for &population in LIVE_POPULATIONS {
        let harness = RuntimeHarness::start();
        thread::sleep(Duration::from_millis(20));
        let mut wire = WireClient::connect(&harness.socket_path);
        wire.hello();
        let started = Instant::now();
        let mut created = 0usize;
        let mut next_request = 1u64;
        let mut limited = None;
        while created < population {
            let (result, _) = wire.create_timed(next_request);
            next_request += 1;
            match result.result_code {
                CreateExecutionResultCode::Created => created += 1,
                CreateExecutionResultCode::Error(code) => {
                    limited = Some(format!("{code:?}"));
                    break;
                }
            }
        }
        let elapsed_ms = started.elapsed().as_secs_f64() * 1_000.0;
        if let Some(reason) = limited {
            println!(
                "m003_provisioning_live_executions population_requested={population} population_achieved={created} evidence_class=PLATFORM_LIMITED reason={reason} elapsed_ms={elapsed_ms:.3} {PERFORMANCE_CLAIM}"
            );
        } else {
            println!(
                "m003_provisioning_live_executions population_requested={population} population_achieved={created} presentation_pane_count=PLATFORM_LIMITED presentation_reason=headed_chrome_requires_C2_C3 evidence_class=CI elapsed_ms={elapsed_ms:.3} {PERFORMANCE_CLAIM}"
            );
        }
        harness.finish();
    }
}

#[cfg(target_os = "macos")]
fn percentile_us(sorted: &[u64], percentile: u8) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (usize::from(percentile) * sorted.len()) / 100;
    sorted[rank.min(sorted.len() - 1)]
}

#[cfg(target_os = "macos")]
fn print_host_metadata() {
    let sha = process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .unwrap_or_else(|| "unknown".to_owned());
    println!(
        "m003_provisioning_host os={} arch={} sha={} build=release_bench evidence_class=CI {PERFORMANCE_CLAIM}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        sha.trim(),
    );
}
