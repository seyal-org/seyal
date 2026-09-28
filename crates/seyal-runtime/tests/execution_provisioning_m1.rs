#![cfg(target_os = "macos")]
#![allow(unsafe_code)]

//! M003 M1 (#1164) — provisioning fairness and 100-cycle resource evidence.
//!
//! Does not change provisioning behavior. Records whether spawn-inside-dispatch
//! trips the fairness gate (ADR-017 §16); never adds a worker.

use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    process::Command,
    sync::Mutex,
    time::{Duration, Instant},
};

use seyal_exec::{CommandSpec, WindowSize};
use seyal_runtime::{
    local_ipc::framing::{
        encode_frame, Attach, Attached, ClientHello, CreateExecutionRequest, CreateExecutionResult,
        CreateExecutionResultCode, FrameHeader, Lifecycle, LifecycleMessage, MessageType, Role,
        ServerHello, TerminateExecutionRequest, TerminateExecutionResult,
        TerminateExecutionResultCode, CAP_EXECUTION_PROVISIONING, HEADER_LEN,
    },
    AttachmentId, ExecutionId, LocalIpcMode, Runtime, RuntimeConfig,
};

static FD_SERIAL: Mutex<()> = Mutex::new(());

fn config() -> RuntimeConfig {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let suffix = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let process = std::process::id();
    let mut config = RuntimeConfig::m001().expect("config");
    config.singleton_path = std::env::temp_dir().join(format!("m1-{process}-{suffix:x}.lock"));
    config.local_ipc = LocalIpcMode::Enabled {
        runtime_dir_override: Some(std::env::temp_dir().join(format!("m1d-{process}-{suffix:x}"))),
    };
    config.graceful_termination = Duration::from_millis(80);
    config.forced_reap = Duration::from_millis(250);
    config.final_drain = Duration::from_millis(200);
    config
}

fn fd_count() -> usize {
    (0..1024)
        .filter(|fd| {
            // SAFETY: F_GETFD only inspects the integer descriptor.
            (unsafe { libc::fcntl(*fd, libc::F_GETFD) }) >= 0
        })
        .count()
}

fn rss_kib() -> u64 {
    let output = Command::new("/bin/ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

fn median_rss_kib() -> u64 {
    let mut samples = [0u64; 5];
    for sample in &mut samples {
        *sample = rss_kib();
        std::thread::sleep(Duration::from_millis(20));
    }
    samples.sort_unstable();
    samples[2]
}

struct Client {
    stream: UnixStream,
    buffered: Vec<u8>,
}

struct Harness {
    runtime: Runtime,
    clients: Vec<Client>,
}

impl Drop for Harness {
    fn drop(&mut self) {
        if self.runtime.begin_shutdown().is_ok() {
            let _ = self
                .runtime
                .run_until_empty(Instant::now() + Duration::from_secs(3));
        }
    }
}

impl Harness {
    fn with_streamer() -> (Self, ExecutionId) {
        let mut runtime = Runtime::new(config()).expect("Runtime");
        let streamer = runtime
            .create_execution(
                CommandSpec::new("/bin/sh").args([
                    "-c",
                    "i=0; while true; do printf 'STREAM-%s\\n' \"$i\"; i=$((i+1)); sleep 0.01; done",
                ]),
                WindowSize::cells(80, 24).expect("size"),
            )
            .expect("streamer");
        let mut harness = Self {
            runtime,
            clients: Vec::new(),
        };
        harness.connect();
        (harness, streamer)
    }

    fn empty() -> Self {
        let runtime = Runtime::new(config()).expect("Runtime");
        let mut harness = Self {
            runtime,
            clients: Vec::new(),
        };
        harness.connect();
        harness
    }

    fn connect(&mut self) -> usize {
        let socket = self
            .runtime
            .local_ipc_socket_path()
            .expect("socket")
            .to_path_buf();
        let deadline = Instant::now() + Duration::from_secs(2);
        let stream = loop {
            match UnixStream::connect(&socket) {
                Ok(stream) => break stream,
                Err(_) => {
                    assert!(Instant::now() < deadline, "connect timeout");
                    self.runtime
                        .poll_once(Some(Duration::from_millis(5)))
                        .unwrap();
                }
            }
        };
        stream.set_nonblocking(true).unwrap();
        self.runtime
            .poll_once(Some(Duration::from_millis(5)))
            .unwrap();
        self.clients.push(Client {
            stream,
            buffered: Vec::new(),
        });
        self.clients.len() - 1
    }

    fn pump(&mut self) {
        self.runtime
            .poll_once(Some(Duration::from_millis(5)))
            .expect("poll");
    }

    fn pump_for_deadlines(&mut self) {
        self.runtime
            .poll_once(Some(Duration::from_millis(50)))
            .expect("poll");
    }

    fn send(&mut self, client: usize, kind: MessageType, payload: &[u8]) {
        let bytes = encode_frame(kind, payload);
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut sent = 0;
        while sent < bytes.len() {
            match self.clients[client].stream.write(&bytes[sent..]) {
                Ok(0) => panic!("zero write"),
                Ok(count) => sent += count,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => self.pump(),
                Err(error) => panic!("write: {error}"),
            }
            assert!(Instant::now() < deadline, "write timeout");
        }
        self.pump();
    }

    fn expect_frame(&mut self, client: usize, deadline: Instant) -> (u16, Vec<u8>) {
        loop {
            if self.clients[client].buffered.len() >= HEADER_LEN {
                let header = FrameHeader::decode(&self.clients[client].buffered[..HEADER_LEN])
                    .expect("valid header");
                let total = HEADER_LEN + header.payload_len as usize;
                if self.clients[client].buffered.len() >= total {
                    let frame = self.clients[client]
                        .buffered
                        .drain(..total)
                        .collect::<Vec<_>>();
                    return (header.message_type, frame[HEADER_LEN..].to_vec());
                }
            }
            let mut chunk = [0u8; 8192];
            match self.clients[client].stream.read(&mut chunk) {
                Ok(0) => panic!("connection closed while awaiting frame"),
                Ok(count) => self.clients[client]
                    .buffered
                    .extend_from_slice(&chunk[..count]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => self.pump(),
                Err(error) => panic!("read: {error}"),
            }
            assert!(Instant::now() < deadline, "timed out awaiting frame");
        }
    }

    fn try_frame(&mut self, client: usize, wait: Duration) -> Option<(u16, Vec<u8>)> {
        let deadline = Instant::now() + wait;
        loop {
            if self.clients[client].buffered.len() >= HEADER_LEN {
                let header = FrameHeader::decode(&self.clients[client].buffered[..HEADER_LEN])
                    .expect("valid header");
                let total = HEADER_LEN + header.payload_len as usize;
                if self.clients[client].buffered.len() >= total {
                    let frame = self.clients[client]
                        .buffered
                        .drain(..total)
                        .collect::<Vec<_>>();
                    return Some((header.message_type, frame[HEADER_LEN..].to_vec()));
                }
            }
            if Instant::now() >= deadline {
                return None;
            }
            let mut chunk = [0u8; 8192];
            match self.clients[client].stream.read(&mut chunk) {
                Ok(0) => return None,
                Ok(count) => self.clients[client]
                    .buffered
                    .extend_from_slice(&chunk[..count]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => self.pump(),
                Err(error) => panic!("read: {error}"),
            }
        }
    }

    fn hello(&mut self, client: usize) {
        self.send(
            client,
            MessageType::ClientHello,
            &ClientHello {
                client_capabilities: CAP_EXECUTION_PROVISIONING,
            }
            .encode(),
        );
        let (kind, payload) = self.expect_frame(client, Instant::now() + Duration::from_secs(2));
        assert_eq!(kind, MessageType::ServerHello as u16);
        let _ = ServerHello::decode(&payload).expect("ServerHello");
    }

    fn create_timed(
        &mut self,
        client: usize,
        request_id: u64,
    ) -> (CreateExecutionResult, Duration) {
        let started = Instant::now();
        self.send(
            client,
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
            let (kind, payload) =
                self.expect_frame(client, Instant::now() + Duration::from_secs(3));
            if kind == MessageType::CreateExecutionResult as u16 {
                let result = CreateExecutionResult::decode(&payload).expect("create result");
                return (result, started.elapsed());
            }
        }
    }

    fn attach(&mut self, client: usize, execution_id: ExecutionId, role: Role) -> Attached {
        self.send(
            client,
            MessageType::Attach,
            &Attach {
                execution_id,
                requested_role: role,
            }
            .encode(),
        );
        loop {
            let (kind, payload) =
                self.expect_frame(client, Instant::now() + Duration::from_secs(2));
            if kind == MessageType::Attached as u16 {
                return Attached::decode(&payload).expect("Attached");
            }
        }
    }

    fn terminate(
        &mut self,
        client: usize,
        attachment_id: AttachmentId,
        execution_id: ExecutionId,
        request_id: u64,
    ) -> TerminateExecutionResult {
        self.send(
            client,
            MessageType::TerminateExecutionRequest,
            &TerminateExecutionRequest {
                attachment_id,
                execution_id,
                request_id,
            }
            .encode(),
        );
        loop {
            let (kind, payload) =
                self.expect_frame(client, Instant::now() + Duration::from_secs(3));
            if kind == MessageType::TerminateExecutionResult as u16 {
                return TerminateExecutionResult::decode(&payload).expect("terminate result");
            }
        }
    }

    fn wait_lifecycle_finalized(&mut self, client: usize, execution_id: ExecutionId) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            self.pump_for_deadlines();
            while let Some((kind, payload)) = self.try_frame(client, Duration::from_millis(5)) {
                if kind == MessageType::Lifecycle as u16 {
                    let message = LifecycleMessage::decode(&payload).expect("Lifecycle");
                    if message.execution_id == execution_id
                        && message.lifecycle == Lifecycle::Finalized
                    {
                        assert!(self.runtime.lookup(execution_id).is_none());
                        return;
                    }
                }
            }
            if self.runtime.lookup(execution_id).is_none() {
                return;
            }
        }
        panic!("execution never finalized");
    }

    fn total_attachment_count(&self) -> usize {
        self.runtime
            .list()
            .into_iter()
            .map(|summary| summary.attachment_count)
            .sum()
    }

    fn controller_lease_count(&self) -> usize {
        self.runtime
            .list()
            .into_iter()
            .filter(|summary| summary.attachment_count > 0)
            .count()
    }

    fn workspace_association_count(&self) -> usize {
        // Each published execution carries exactly one Workspace association.
        self.runtime.execution_count()
    }
}

/// Hot-output streamer plus provisioning bursts: unrelated output must keep
/// progressing. Records spawn cost and whether ADR-017 §16 fairness is tripped.
#[test]
fn provisioning_burst_preserves_streamer_fairness() {
    let (mut h, streamer) = Harness::with_streamer();
    h.hello(0);

    // Warm the streamer so damage_generation is advancing before the burst.
    let warm_deadline = Instant::now() + Duration::from_secs(2);
    let mut gen_before = h
        .runtime
        .execution(streamer)
        .unwrap()
        .terminal()
        .damage_generation();
    while Instant::now() < warm_deadline {
        h.pump();
        let damage = h
            .runtime
            .execution(streamer)
            .unwrap()
            .terminal()
            .damage_generation();
        if damage > gen_before {
            gen_before = damage;
            break;
        }
    }
    assert!(
        gen_before > 0,
        "streamer must produce output before the burst"
    );

    let mut spawn_us = Vec::new();
    let burst_started = Instant::now();
    let mut longest_stall = Duration::ZERO;
    let mut last_progress = Instant::now();
    let mut gen_cursor = gen_before;

    for next_request in 1u64..=8 {
        let (created, elapsed) = h.create_timed(0, next_request);
        assert_eq!(created.result_code, CreateExecutionResultCode::Created);
        spawn_us.push(elapsed.as_micros().min(u128::from(u64::MAX)) as u64);

        let damage = h
            .runtime
            .execution(streamer)
            .unwrap()
            .terminal()
            .damage_generation();
        if damage > gen_cursor {
            gen_cursor = damage;
            last_progress = Instant::now();
        } else {
            let stall = last_progress.elapsed();
            if stall > longest_stall {
                longest_stall = stall;
            }
        }
    }

    let progress_deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < progress_deadline {
        h.pump();
        let damage = h
            .runtime
            .execution(streamer)
            .unwrap()
            .terminal()
            .damage_generation();
        if damage > gen_cursor {
            gen_cursor = damage;
            break;
        }
        let stall = last_progress.elapsed();
        if stall > longest_stall {
            longest_stall = stall;
        }
    }

    spawn_us.sort_unstable();
    let p50 = spawn_us[spawn_us.len() / 2];
    let p95_rank = (spawn_us.len() * 95) / 100;
    let p95 = spawn_us[p95_rank.min(spawn_us.len() - 1)];
    let streamer_advanced = gen_cursor > gen_before;
    // Operational fairness gate for M1: the hot streamer must advance during
    // the burst window, and must not stall longer than 500 ms while creates
    // run on the same reactor (spawn remains inside dispatch per ADR-017 §16).
    let fairness_tripped = !streamer_advanced || longest_stall > Duration::from_millis(500);
    let fairness_verdict = if fairness_tripped { "MISS" } else { "PASS" };

    println!(
        "m003_m1_fairness evidence_class=CI burst_creates={} spawn_p50_us={} spawn_p95_us={} spawn_max_us={} streamer_gen_before={} streamer_gen_after={} longest_stall_ms={} burst_wall_ms={} fairness_gate={} adr017_section=16 performance_claim=false",
        spawn_us.len(),
        p50,
        p95,
        spawn_us.last().copied().unwrap_or(0),
        gen_before,
        gen_cursor,
        longest_stall.as_millis(),
        burst_started.elapsed().as_millis(),
        fairness_verdict,
    );

    assert!(
        streamer_advanced,
        "unrelated streamer must keep progressing during provisioning burst"
    );
    assert!(
        !fairness_tripped,
        "spawn-inside-dispatch tripped the fairness gate (ADR-017 §16); record MISS in evidence — do not add a worker in #1164"
    );
}

/// At least 100 provision/dispose cycles return named counters to baseline.
#[test]
fn one_hundred_provision_dispose_cycles_return_to_baseline() {
    let _guard = FD_SERIAL
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let mut h = Harness::empty();
    h.hello(0);

    // Warm the path so first-use allocations are outside the measured baseline.
    let warm = h.create_timed(0, 1).0;
    assert_eq!(warm.result_code, CreateExecutionResultCode::Created);
    let warm_attached = h.attach(0, warm.execution_id, Role::Controller);
    let _ = h.terminate(0, warm_attached.attachment_id, warm.execution_id, 2);
    h.wait_lifecycle_finalized(0, warm.execution_id);
    for _ in 0..10 {
        h.pump_for_deadlines();
    }

    let baseline_fds = fd_count();
    let baseline_executions = h.runtime.execution_count();
    let baseline_blocks = h.runtime.block_count();
    let baseline_attachments = h.total_attachment_count();
    let baseline_controllers = h.controller_lease_count();
    let baseline_workspaces = h.workspace_association_count();
    let baseline_rss_kib = median_rss_kib();

    let mut next_request = 3u64;
    const CYCLES: usize = 100;
    for cycle in 0..CYCLES {
        let (created, _) = h.create_timed(0, next_request);
        next_request += 1;
        assert_eq!(
            created.result_code,
            CreateExecutionResultCode::Created,
            "cycle {cycle} create"
        );
        assert_eq!(
            h.workspace_association_count(),
            baseline_workspaces + 1,
            "cycle {cycle} workspace association"
        );
        let attached = h.attach(0, created.execution_id, Role::Controller);
        assert_eq!(
            h.runtime
                .lookup(created.execution_id)
                .unwrap()
                .attachment_count,
            1
        );
        assert_eq!(h.controller_lease_count(), baseline_controllers + 1);
        let result = h.terminate(
            0,
            attached.attachment_id,
            created.execution_id,
            next_request,
        );
        next_request += 1;
        assert_eq!(
            result.result_code,
            TerminateExecutionResultCode::TerminationRequested
        );
        h.wait_lifecycle_finalized(0, created.execution_id);
        assert_eq!(
            h.runtime.execution_count(),
            baseline_executions,
            "cycle {cycle} registrations/executions"
        );
        assert_eq!(
            h.runtime.block_count(),
            baseline_blocks,
            "cycle {cycle} blocks"
        );
        assert_eq!(
            h.total_attachment_count(),
            baseline_attachments,
            "cycle {cycle} attachments"
        );
        assert_eq!(
            h.controller_lease_count(),
            baseline_controllers,
            "cycle {cycle} controller leases"
        );
        assert_eq!(
            h.workspace_association_count(),
            baseline_workspaces,
            "cycle {cycle} workspace associations"
        );
    }

    for _ in 0..20 {
        h.pump_for_deadlines();
    }

    assert_eq!(h.runtime.execution_count(), baseline_executions);
    assert_eq!(h.runtime.block_count(), baseline_blocks);
    assert_eq!(h.total_attachment_count(), baseline_attachments);
    assert_eq!(h.controller_lease_count(), baseline_controllers);
    assert_eq!(h.workspace_association_count(), baseline_workspaces);
    assert_eq!(
        fd_count(),
        baseline_fds,
        "descriptor count must return to baseline after {CYCLES} cycles"
    );

    let after_rss_kib = median_rss_kib();
    // Exact counters gate the leak. RSS uses a 2 MiB noise band on CI hosts
    // (allocator retention); growth beyond that fails the cycle test.
    const RSS_NOISE_KIB: u64 = 2048;
    assert!(
        after_rss_kib <= baseline_rss_kib.saturating_add(RSS_NOISE_KIB),
        "RSS did not return near baseline after {CYCLES} cycles: baseline={baseline_rss_kib} KiB after={after_rss_kib} KiB noise_band={RSS_NOISE_KIB} KiB"
    );

    println!(
        "m003_m1_cycle_leak evidence_class=CI cycles={CYCLES} fds_baseline={baseline_fds} fds_after={} executions_baseline={baseline_executions} attachments_baseline={baseline_attachments} controllers_baseline={baseline_controllers} workspaces_baseline={baseline_workspaces} rss_baseline_kib={baseline_rss_kib} rss_after_kib={after_rss_kib} rss_noise_band_kib={RSS_NOISE_KIB} performance_claim=false",
        fd_count(),
    );

    // Controller lease / association released: a fresh Controller attach succeeds.
    let (probe, _) = h.create_timed(0, next_request);
    let probe_attached = h.attach(0, probe.execution_id, Role::Controller);
    assert_eq!(probe_attached.granted_role, Role::Controller);
}
