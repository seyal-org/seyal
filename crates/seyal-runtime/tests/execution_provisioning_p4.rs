#![cfg(target_os = "macos")]
#![allow(unsafe_code)]

//! SPEC-004 §18.4–§18.5 / SPEC-003 §11 — M003 P4 explicit execution disposition.

use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    sync::Mutex,
    time::{Duration, Instant},
};

use seyal_exec::{CommandSpec, WindowSize};
use seyal_runtime::{
    local_ipc::framing::{
        encode_frame, Attach, Attached, ClientHello, CreateExecutionRequest, CreateExecutionResult,
        CreateExecutionResultCode, Detach, Detached, ErrorCode, ErrorMessage, FrameHeader,
        Lifecycle, LifecycleMessage, MessageType, Role, ServerHello, TerminateExecutionRequest,
        TerminateExecutionResult, TerminateExecutionResultCode, CAP_EXECUTION_PROVISIONING,
        HEADER_LEN,
    },
    AttachmentId, ExecutionId, ExecutionLifecycle, LocalIpcMode, Runtime, RuntimeConfig,
};

#[cfg(feature = "test-fault-injection")]
use seyal_runtime::test_fault::{self, FaultPoint};

static FD_SERIAL: Mutex<()> = Mutex::new(());

fn hold_fd_serial() -> std::sync::MutexGuard<'static, ()> {
    FD_SERIAL
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

fn config() -> RuntimeConfig {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let suffix = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let process = std::process::id();
    let mut config = RuntimeConfig::m001().expect("config");
    config.singleton_path = std::env::temp_dir().join(format!("p4-{process}-{suffix:x}.lock"));
    config.local_ipc = LocalIpcMode::Enabled {
        runtime_dir_override: Some(std::env::temp_dir().join(format!("p4d-{process}-{suffix:x}"))),
    };
    // Short, testable SPEC-003 §11 deadlines; never client-supplied.
    config.graceful_termination = Duration::from_millis(80);
    config.forced_reap = Duration::from_millis(250);
    // Long enough that DrainingAfterPrimaryExit remains observable after attach.
    config.final_drain = Duration::from_millis(400);
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
    fn empty() -> Self {
        let runtime = Runtime::new(config()).expect("Runtime");
        let mut harness = Self {
            runtime,
            clients: Vec::new(),
        };
        harness.connect();
        harness
    }

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

    fn send(&mut self, client: usize, kind: MessageType, payload: &[u8]) {
        self.write_frame(client, kind, payload);
        self.pump();
    }

    /// Queue a frame on the connection without pumping the reactor.
    fn write_frame(&mut self, client: usize, kind: MessageType, payload: &[u8]) {
        let bytes = encode_frame(kind, payload);
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut sent = 0;
        while sent < bytes.len() {
            match self.clients[client].stream.write(&bytes[sent..]) {
                Ok(0) => panic!("zero write"),
                Ok(count) => sent += count,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("write: {error}"),
            }
            assert!(Instant::now() < deadline, "write timeout");
        }
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

    fn hello(&mut self, client: usize, capabilities: u32) -> ServerHello {
        self.send(
            client,
            MessageType::ClientHello,
            &ClientHello {
                client_capabilities: capabilities,
            }
            .encode(),
        );
        let (kind, payload) = self.expect_frame(client, Instant::now() + Duration::from_secs(2));
        assert_eq!(kind, MessageType::ServerHello as u16);
        ServerHello::decode(&payload).expect("ServerHello")
    }

    fn create(
        &mut self,
        client: usize,
        request_id: u64,
        rows: u16,
        columns: u16,
    ) -> CreateExecutionResult {
        self.send(
            client,
            MessageType::CreateExecutionRequest,
            &CreateExecutionRequest {
                workspace_id: 0,
                request_id,
                launch_profile: 0,
                rows,
                columns,
            }
            .encode(),
        );
        loop {
            let (kind, payload) =
                self.expect_frame(client, Instant::now() + Duration::from_secs(3));
            if kind == MessageType::CreateExecutionResult as u16 {
                return CreateExecutionResult::decode(&payload).expect("create result");
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
        let (kind, payload) = self.expect_frame(client, Instant::now() + Duration::from_secs(2));
        assert_eq!(kind, MessageType::Attached as u16);
        Attached::decode(&payload).expect("Attached")
    }

    /// Skip display / composer / block frames until the expected control kind.
    fn expect_control_frame(&mut self, client: usize, want: MessageType) -> Vec<u8> {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let (kind, payload) = self.expect_frame(client, deadline);
            if kind == want as u16 {
                return payload;
            }
        }
    }

    fn detach(&mut self, client: usize, attachment_id: AttachmentId) {
        self.send(
            client,
            MessageType::Detach,
            &Detach { attachment_id }.encode(),
        );
        let payload = self.expect_control_frame(client, MessageType::Detached);
        let _ = Detached::decode(&payload).expect("Detached");
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
        let payload = self.expect_control_frame(client, MessageType::TerminateExecutionResult);
        TerminateExecutionResult::decode(&payload).expect("terminate result")
    }

    fn expect_error(&mut self, client: usize, code: ErrorCode, offending: MessageType) {
        let payload = self.expect_control_frame(client, MessageType::Error);
        let error = ErrorMessage::decode(&payload).expect("Error");
        assert_eq!(error.error_code, code as u16);
        assert_eq!(error.offending_message_type, offending as u16);
    }

    fn wait_row_contains(&mut self, execution_id: ExecutionId, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            self.pump_for_deadlines();
            if self
                .runtime
                .execution(execution_id)
                .and_then(|execution| execution.terminal().row_text(0))
                .is_some_and(|row| row.contains(needle))
            {
                return;
            }
        }
        panic!("row never contained {needle:?}");
    }

    fn pump_for_deadlines(&mut self) {
        self.runtime
            .poll_once(Some(Duration::from_millis(50)))
            .expect("poll");
    }

    fn wait_lifecycle_finalized(&mut self, client: usize, execution_id: ExecutionId) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            self.pump_for_deadlines();
            while let Some((kind, payload)) = self.try_frame(client, Duration::from_millis(5)) {
                if kind == MessageType::Lifecycle as u16 {
                    let message = LifecycleMessage::decode(&payload).expect("Lifecycle");
                    assert_eq!(message.execution_id, execution_id);
                    assert_eq!(message.lifecycle, Lifecycle::Finalized);
                    assert!(self.runtime.lookup(execution_id).is_none());
                    return;
                }
            }
            if self.runtime.lookup(execution_id).is_none()
                && let Some((kind, payload)) = self.try_frame(client, Duration::from_millis(200))
                && kind == MessageType::Lifecycle as u16
            {
                let message = LifecycleMessage::decode(&payload).expect("Lifecycle");
                assert_eq!(message.execution_id, execution_id);
                assert_eq!(message.lifecycle, Lifecycle::Finalized);
                return;
            }
        }
        panic!("execution never finalized via Lifecycle path");
    }

    fn wait_lifecycle(&mut self, execution_id: ExecutionId, want: ExecutionLifecycle) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            self.pump_for_deadlines();
            match self.runtime.lookup(execution_id).map(|s| s.lifecycle) {
                Some(lifecycle) if lifecycle == want => return,
                None if want == ExecutionLifecycle::DrainingAfterPrimaryExit => {
                    panic!("execution finalized before DrainingAfterPrimaryExit was observable");
                }
                _ => {}
            }
        }
        panic!(
            "never reached {want:?}; last={:?}",
            self.runtime.lookup(execution_id).map(|s| s.lifecycle)
        );
    }
}

#[test]
fn controller_terminate_requests_then_finalizes_via_lifecycle_only() {
    let _guard = hold_fd_serial();
    let (mut h, streamer) = Harness::with_streamer();
    h.hello(0, CAP_EXECUTION_PROVISIONING);
    let gen_before = h
        .runtime
        .execution(streamer)
        .unwrap()
        .terminal()
        .damage_generation();

    let created = h.create(0, 1, 24, 80);
    assert_eq!(created.result_code, CreateExecutionResultCode::Created);
    let attached = h.attach(0, created.execution_id, Role::Controller);

    let result = h.terminate(0, attached.attachment_id, created.execution_id, 2);
    assert_eq!(result.attachment_id, attached.attachment_id);
    assert_eq!(result.request_id, 2);
    assert_eq!(
        result.result_code,
        TerminateExecutionResultCode::TerminationRequested
    );
    assert_eq!(result.detail_code, 0);
    assert_eq!(
        h.runtime.lookup(created.execution_id).unwrap().lifecycle,
        ExecutionLifecycle::TerminatingGraceful
    );

    let progress_deadline = Instant::now() + Duration::from_secs(3);
    let mut saw_progress = false;
    while Instant::now() < progress_deadline {
        h.pump();
        let generation = h
            .runtime
            .execution(streamer)
            .unwrap()
            .terminal()
            .damage_generation();
        if generation > gen_before {
            saw_progress = true;
            break;
        }
    }
    assert!(
        saw_progress,
        "unrelated execution output must keep progressing"
    );

    h.wait_lifecycle_finalized(0, created.execution_id);
    assert!(h.runtime.lookup(created.execution_id).is_none());
    // Streamer remains; only the disposed execution must be gone.
    assert!(h.runtime.lookup(streamer).is_some());
}

#[test]
fn capability_absent_is_unknown_message() {
    let _guard = hold_fd_serial();
    let mut h = Harness::empty();
    h.send(
        0,
        MessageType::TerminateExecutionRequest,
        &TerminateExecutionRequest {
            attachment_id: AttachmentId::from_bytes([1; 16]),
            execution_id: ExecutionId::from_bytes([2; 16]),
            request_id: 1,
        }
        .encode(),
    );
    h.expect_error(
        0,
        ErrorCode::UnknownMessage,
        MessageType::TerminateExecutionRequest,
    );

    let mut h = Harness::empty();
    h.hello(0, 0);
    h.send(
        0,
        MessageType::TerminateExecutionRequest,
        &TerminateExecutionRequest {
            attachment_id: AttachmentId::from_bytes([1; 16]),
            execution_id: ExecutionId::from_bytes([2; 16]),
            request_id: 1,
        }
        .encode(),
    );
    h.expect_error(
        0,
        ErrorCode::UnknownMessage,
        MessageType::TerminateExecutionRequest,
    );
}

#[test]
fn observer_zero_stale_and_mismatched_identity_fail_closed() {
    let _guard = hold_fd_serial();
    let mut h = Harness::empty();
    h.hello(0, CAP_EXECUTION_PROVISIONING);
    let created = h.create(0, 1, 24, 80);
    assert_eq!(created.result_code, CreateExecutionResultCode::Created);
    let controller = h.attach(0, created.execution_id, Role::Controller);

    let observer = h.connect();
    h.hello(observer, CAP_EXECUTION_PROVISIONING);
    let observer_attached = h.attach(observer, created.execution_id, Role::Observer);

    let denied = h.terminate(
        observer,
        observer_attached.attachment_id,
        created.execution_id,
        1,
    );
    assert_eq!(
        denied.result_code,
        TerminateExecutionResultCode::Error(ErrorCode::PermissionDenied)
    );
    assert_eq!(denied.detail_code, 0);
    assert_eq!(
        h.runtime.lookup(created.execution_id).unwrap().lifecycle,
        ExecutionLifecycle::Running
    );

    let zero = h.terminate(
        0,
        AttachmentId::from_bytes([0; 16]),
        created.execution_id,
        2,
    );
    assert_eq!(
        zero.result_code,
        TerminateExecutionResultCode::Error(ErrorCode::InvalidAttachment)
    );

    let foreign = h.terminate(
        0,
        AttachmentId::from_bytes([0xab; 16]),
        created.execution_id,
        3,
    );
    assert_eq!(
        foreign.result_code,
        TerminateExecutionResultCode::Error(ErrorCode::StaleIdentity)
    );

    // Observer's attachment id is foreign to the controller connection.
    let other_conn = h.terminate(0, observer_attached.attachment_id, created.execution_id, 4);
    assert_eq!(
        other_conn.result_code,
        TerminateExecutionResultCode::Error(ErrorCode::StaleIdentity)
    );

    let mismatch = h.terminate(
        0,
        controller.attachment_id,
        ExecutionId::from_bytes([9; 16]),
        5,
    );
    assert_eq!(
        mismatch.result_code,
        TerminateExecutionResultCode::Error(ErrorCode::StaleIdentity)
    );

    // Still running and only the controller can dispose.
    let ok = h.terminate(0, controller.attachment_id, created.execution_id, 6);
    assert_eq!(
        ok.result_code,
        TerminateExecutionResultCode::TerminationRequested
    );
}

#[test]
fn section_18_5_outcome_table_duplicate_draining_and_post_release() {
    let _guard = hold_fd_serial();
    let mut h = Harness::empty();
    h.hello(0, CAP_EXECUTION_PROVISIONING);

    // --- Duplicate terminate while TerminatingGraceful (idempotent) ---
    let trap = h
        .runtime
        .create_execution(
            CommandSpec::new("/bin/sh").args([
                "-c",
                "trap '' TERM; printf ready; while :; do sleep 1; done",
            ]),
            WindowSize::cells(80, 24).expect("size"),
        )
        .expect("trap child");
    let attached = h.attach(0, trap, Role::Controller);
    h.wait_row_contains(trap, "ready");

    let first = h.terminate(0, attached.attachment_id, trap, 1);
    assert_eq!(
        first.result_code,
        TerminateExecutionResultCode::TerminationRequested
    );
    assert_eq!(
        h.runtime.lookup(trap).unwrap().lifecycle,
        ExecutionLifecycle::TerminatingGraceful
    );
    let graceful_started = Instant::now();

    let duplicate = h.terminate(0, attached.attachment_id, trap, 2);
    assert_eq!(
        duplicate.result_code,
        TerminateExecutionResultCode::TerminationRequested
    );
    assert_eq!(
        h.runtime.lookup(trap).unwrap().lifecycle,
        ExecutionLifecycle::TerminatingGraceful
    );

    h.wait_lifecycle(trap, ExecutionLifecycle::TerminatingForced);
    let to_forced = graceful_started.elapsed();
    assert!(
        to_forced < Duration::from_millis(250),
        "duplicate terminate must not reset the graceful deadline (elapsed {to_forced:?})"
    );
    h.wait_lifecycle_finalized(0, trap);

    // --- Terminate during DrainingAfterPrimaryExit ---
    // Attach while the primary is still alive, stop polling until it has exited,
    // enter Draining via AlreadyReaped (no poll), queue type 38, then one poll so
    // IPC runs while the attachment is still live (control before PTY).
    let draining = h
        .runtime
        .create_execution(
            CommandSpec::new("/usr/bin/perl").args([
                "-e",
                r#"my $pid = fork(); die unless defined $pid; if ($pid == 0) { $SIG{HUP}="IGNORE"; $SIG{TERM}="IGNORE"; sleep 30; exit 0; } select(undef,undef,undef,1.0); exit 0;"#,
            ]),
            WindowSize::cells(80, 24).expect("size"),
        )
        .expect("drain child");
    let drain_attached = h.attach(0, draining, Role::Controller);
    assert_eq!(
        h.runtime.lookup(draining).unwrap().lifecycle,
        ExecutionLifecycle::Running
    );
    std::thread::sleep(Duration::from_millis(1200));
    assert_eq!(
        h.runtime.lookup(draining).unwrap().lifecycle,
        ExecutionLifecycle::Running,
        "must not poll between attach and AlreadyReaped drain entry"
    );
    h.runtime
        .request_termination(draining)
        .expect("AlreadyReaped enters Draining");
    assert_eq!(
        h.runtime.lookup(draining).unwrap().lifecycle,
        ExecutionLifecycle::DrainingAfterPrimaryExit
    );
    let drain_entered = Instant::now();

    h.write_frame(
        0,
        MessageType::TerminateExecutionRequest,
        &TerminateExecutionRequest {
            attachment_id: drain_attached.attachment_id,
            execution_id: draining,
            request_id: 3,
        }
        .encode(),
    );
    h.pump_for_deadlines();
    let during_drain = loop {
        let (kind, payload) = h.expect_frame(0, Instant::now() + Duration::from_secs(2));
        if kind == MessageType::TerminateExecutionResult as u16 {
            break TerminateExecutionResult::decode(&payload).expect("terminate result");
        }
    };
    assert_eq!(
        during_drain.result_code,
        TerminateExecutionResultCode::TerminationRequested
    );
    assert_eq!(during_drain.detail_code, 0);

    // Finalization deadline was armed at drain entry; terminate must not extend it.
    h.wait_lifecycle_finalized(0, draining);
    assert!(
        drain_entered.elapsed() <= Duration::from_millis(600),
        "terminate during drain must not reset/extend the finalization deadline"
    );

    // After release: no current attachment → InvalidState (rule 2 / Error frame).
    h.send(
        0,
        MessageType::TerminateExecutionRequest,
        &TerminateExecutionRequest {
            attachment_id: drain_attached.attachment_id,
            execution_id: draining,
            request_id: 4,
        }
        .encode(),
    );
    h.expect_error(
        0,
        ErrorCode::InvalidState,
        MessageType::TerminateExecutionRequest,
    );

    // Attach elsewhere, then stale prior identity → StaleIdentity.
    let next = h
        .runtime
        .create_execution(
            CommandSpec::new("/bin/sh").args(["-c", "sleep 30"]),
            WindowSize::cells(80, 24).expect("size"),
        )
        .expect("next");
    let fresh = h.attach(0, next, Role::Controller);
    let stale = h.terminate(0, drain_attached.attachment_id, draining, 5);
    assert_eq!(
        stale.result_code,
        TerminateExecutionResultCode::Error(ErrorCode::StaleIdentity)
    );
    assert_ne!(fresh.attachment_id, drain_attached.attachment_id);
    assert_eq!(
        h.runtime.lookup(next).unwrap().lifecycle,
        ExecutionLifecycle::Running
    );
}

#[test]
fn terminate_after_explicit_detach_is_invalid_state() {
    let _guard = hold_fd_serial();
    let mut h = Harness::empty();
    h.hello(0, CAP_EXECUTION_PROVISIONING);
    let created = h.create(0, 1, 24, 80);
    let attached = h.attach(0, created.execution_id, Role::Controller);
    h.detach(0, attached.attachment_id);

    h.send(
        0,
        MessageType::TerminateExecutionRequest,
        &TerminateExecutionRequest {
            attachment_id: attached.attachment_id,
            execution_id: created.execution_id,
            request_id: 2,
        }
        .encode(),
    );
    h.expect_error(
        0,
        ErrorCode::InvalidState,
        MessageType::TerminateExecutionRequest,
    );
    assert_eq!(
        h.runtime.lookup(created.execution_id).unwrap().lifecycle,
        ExecutionLifecycle::Running
    );
}

#[test]
fn repeated_create_terminate_cycles_return_counters_to_baseline() {
    let _guard = hold_fd_serial();
    let mut h = Harness::empty();
    h.hello(0, CAP_EXECUTION_PROVISIONING);
    // Warm the connection path before measuring baseline.
    let warm = h.create(0, 1, 24, 80);
    let warm_attached = h.attach(0, warm.execution_id, Role::Controller);
    let _ = h.terminate(0, warm_attached.attachment_id, warm.execution_id, 2);
    h.wait_lifecycle_finalized(0, warm.execution_id);
    // Settle so ephemeral display/reactor bookkeeping is quiet.
    for _ in 0..10 {
        h.pump_for_deadlines();
    }

    let baseline_fds = fd_count();
    let baseline_executions = h.runtime.execution_count();
    let baseline_blocks = h.runtime.block_count();

    let mut next_request = 3u64;
    for _ in 0..5 {
        let created = h.create(0, next_request, 24, 80);
        next_request += 1;
        assert_eq!(created.result_code, CreateExecutionResultCode::Created);
        let attached = h.attach(0, created.execution_id, Role::Controller);
        assert_eq!(
            h.runtime
                .lookup(created.execution_id)
                .unwrap()
                .attachment_count,
            1
        );
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
        assert_eq!(h.runtime.execution_count(), baseline_executions);
        assert_eq!(h.runtime.block_count(), baseline_blocks);
    }

    for _ in 0..10 {
        h.pump_for_deadlines();
    }
    assert_eq!(h.runtime.execution_count(), baseline_executions);
    assert_eq!(h.runtime.block_count(), baseline_blocks);
    assert_eq!(
        fd_count(),
        baseline_fds,
        "descriptor count must return to baseline"
    );

    // Controller lease / association released: a fresh Controller attach succeeds.
    let probe = h.create(0, next_request, 24, 80);
    let probe_attached = h.attach(0, probe.execution_id, Role::Controller);
    assert_eq!(probe_attached.granted_role, Role::Controller);
}

#[test]
fn termination_after_primary_reap_finalizes_exactly_once() {
    let _guard = hold_fd_serial();
    let mut h = Harness::empty();
    h.hello(0, CAP_EXECUTION_PROVISIONING);
    let id = h
        .runtime
        .create_execution(
            CommandSpec::new("/usr/bin/perl").args([
                "-e",
                r#"my $pid = fork(); die unless defined $pid; if ($pid == 0) { $SIG{HUP}="IGNORE"; $SIG{TERM}="IGNORE"; sleep 30; exit 0; } select(undef,undef,undef,1.0); exit 0;"#,
            ]),
            WindowSize::cells(80, 24).expect("size"),
        )
        .expect("short-lived");
    let attached = h.attach(0, id, Role::Controller);
    std::thread::sleep(Duration::from_millis(1200));
    h.runtime.request_termination(id).expect("enter drain");
    assert_eq!(
        h.runtime.lookup(id).unwrap().lifecycle,
        ExecutionLifecycle::DrainingAfterPrimaryExit
    );

    h.write_frame(
        0,
        MessageType::TerminateExecutionRequest,
        &TerminateExecutionRequest {
            attachment_id: attached.attachment_id,
            execution_id: id,
            request_id: 1,
        }
        .encode(),
    );
    h.pump_for_deadlines();
    let result = loop {
        let (kind, payload) = h.expect_frame(0, Instant::now() + Duration::from_secs(2));
        if kind == MessageType::TerminateExecutionResult as u16 {
            break TerminateExecutionResult::decode(&payload).expect("terminate result");
        }
    };
    assert_eq!(
        result.result_code,
        TerminateExecutionResultCode::TerminationRequested
    );

    h.wait_lifecycle_finalized(0, id);
    assert!(h.runtime.lookup(id).is_none());
    let stray = self_try_lifecycle(&mut h, 0);
    assert!(stray.is_none(), "must finalize exactly once, got {stray:?}");
}

fn self_try_lifecycle(h: &mut Harness, client: usize) -> Option<Lifecycle> {
    let deadline = Instant::now() + Duration::from_millis(100);
    while Instant::now() < deadline {
        h.pump_for_deadlines();
        while let Some((kind, payload)) = h.try_frame(client, Duration::from_millis(5)) {
            if kind == MessageType::Lifecycle as u16 {
                return Some(
                    LifecycleMessage::decode(&payload)
                        .expect("Lifecycle")
                        .lifecycle,
                );
            }
        }
    }
    None
}


#[cfg(feature = "test-fault-injection")]
#[test]
fn terminate_reports_invalid_state_when_request_termination_fails() {
    let _guard = hold_fd_serial();
    let mut h = Harness::empty();
    h.hello(0, CAP_EXECUTION_PROVISIONING);
    let created = h.create(0, 1, 24, 80);
    assert_eq!(created.result_code, CreateExecutionResultCode::Created);
    let attached = h.attach(0, created.execution_id, Role::Controller);

    test_fault::fail_next(FaultPoint::RequestTermination);
    let result = h.terminate(0, attached.attachment_id, created.execution_id, 2);
    assert_eq!(
        result.result_code,
        TerminateExecutionResultCode::Error(ErrorCode::InvalidState)
    );
    assert_eq!(
        h.runtime.lookup(created.execution_id).unwrap().lifecycle,
        ExecutionLifecycle::Running,
        "§11 must not be claimed when arming failed"
    );
}

#[test]
fn controller_connection_loss_after_termination_requested_still_finalizes() {
    let _guard = hold_fd_serial();
    let mut h = Harness::empty();
    h.hello(0, CAP_EXECUTION_PROVISIONING);
    let created = h.create(0, 1, 24, 80);
    assert_eq!(created.result_code, CreateExecutionResultCode::Created);
    let attached = h.attach(0, created.execution_id, Role::Controller);

    let result = h.terminate(0, attached.attachment_id, created.execution_id, 2);
    assert_eq!(
        result.result_code,
        TerminateExecutionResultCode::TerminationRequested
    );
    assert_eq!(
        h.runtime.lookup(created.execution_id).unwrap().lifecycle,
        ExecutionLifecycle::TerminatingGraceful
    );

    // Inverse regression: peer disappears mid-termination. §11 must continue
    // through graceful → forced → finalize exactly once without a Controller.
    let dropped = h.clients.remove(0);
    drop(dropped.stream);

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut saw_forced_or_drain = false;
    while Instant::now() < deadline {
        h.pump_for_deadlines();
        match h.runtime.lookup(created.execution_id).map(|s| s.lifecycle) {
            None => {
                // Finalized.
                break;
            }
            Some(ExecutionLifecycle::TerminatingGraceful) => {}
            Some(ExecutionLifecycle::TerminatingForced)
            | Some(ExecutionLifecycle::DrainingAfterPrimaryExit)
            | Some(ExecutionLifecycle::PrimaryExitPending) => {
                saw_forced_or_drain = true;
            }
            Some(other) => panic!("unexpected lifecycle {other:?}"),
        }
    }
    assert!(
        h.runtime.lookup(created.execution_id).is_none(),
        "execution must finalize after controller loss mid-termination"
    );
    let _ = saw_forced_or_drain;
    for _ in 0..20 {
        h.pump_for_deadlines();
        assert!(
            h.runtime.lookup(created.execution_id).is_none(),
            "must finalize exactly once"
        );
    }
}
