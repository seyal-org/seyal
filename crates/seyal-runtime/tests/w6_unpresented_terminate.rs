//! ADR-018 §3.3 / W6: terminate live-unpresented without stalling siblings.
#![cfg(target_os = "macos")]

use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    sync::Mutex,
    time::{Duration, Instant},
};

use seyal_exec::{CommandSpec, WindowSize};
use seyal_runtime::{
    local_ipc::framing::{encode_frame, ClientHello, FrameHeader, MessageType, HEADER_LEN},
    ExecutionId, ExecutionLifecycle, LocalIpcMode, Runtime, RuntimeConfig,
};

#[cfg(feature = "test-fault-injection")]
use seyal_exec::test_fault::{self as exec_fault, FaultPoint as ExecFaultPoint};

static TEST_SERIAL: Mutex<()> = Mutex::new(());

fn serialized() -> std::sync::MutexGuard<'static, ()> {
    TEST_SERIAL
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

fn size() -> WindowSize {
    WindowSize::new(80, 24, 0, 0).expect("valid terminal size")
}

fn config(test: &str) -> RuntimeConfig {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mut config = RuntimeConfig::m001().expect("M001 config");
    config.singleton_path = std::env::temp_dir().join(format!(
        "seyal-w6-{}-{suffix:x}-{test}.lock",
        std::process::id()
    ));
    config.local_ipc = LocalIpcMode::Disabled;
    config.graceful_termination = Duration::from_millis(40);
    config.forced_reap = Duration::from_millis(80);
    config.final_drain = Duration::from_millis(40);
    config
}

fn config_with_ipc(test: &str) -> RuntimeConfig {
    let mut config = config(test);
    // macOS `sockaddr_un` is 104 bytes. `std::env::temp_dir()` is too long
    // once `control.sock` is appended, so the test socket lives under `/tmp`.
    config.local_ipc = LocalIpcMode::Enabled {
        runtime_dir_override: Some(std::path::PathBuf::from(format!(
            "/tmp/w6-{}-{test}",
            std::process::id()
        ))),
    };
    config
}

fn shutdown(runtime: &mut Runtime) {
    runtime.begin_shutdown().expect("begin shutdown");
    runtime
        .run_until_empty(Instant::now() + Duration::from_secs(3))
        .expect("shutdown completes");
}

fn write_frame(stream: &mut UnixStream, kind: MessageType, payload: &[u8]) {
    let frame = encode_frame(kind, payload);
    stream.set_nonblocking(false).expect("blocking write");
    stream.write_all(&frame).expect("write frame");
}

fn read_frame(runtime: &mut Runtime, stream: &mut UnixStream, deadline: Instant) -> (u16, Vec<u8>) {
    stream.set_nonblocking(true).expect("nonblocking");
    let mut buffered = Vec::new();
    loop {
        if buffered.len() >= HEADER_LEN {
            let header = FrameHeader::decode(&buffered[..HEADER_LEN]).expect("header");
            let total = HEADER_LEN + header.payload_len as usize;
            if buffered.len() >= total {
                let frame = buffered.drain(..total).collect::<Vec<_>>();
                return (header.message_type, frame[HEADER_LEN..].to_vec());
            }
        }
        let mut chunk = [0u8; 4096];
        match stream.read(&mut chunk) {
            Ok(0) => panic!("control connection closed"),
            Ok(count) => buffered.extend_from_slice(&chunk[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                runtime
                    .poll_once(Some(Duration::from_millis(10)))
                    .expect("poll");
            }
            Err(error) => panic!("control read failed: {error}"),
        }
        assert!(Instant::now() < deadline, "timed out reading control frame");
    }
}

/// Hello into Ready, then write `TerminateExecution` without polling it in.
/// The caller polls so the production handler reaches `request_termination`.
fn queue_terminate_execution(runtime: &mut Runtime, id: ExecutionId) -> UnixStream {
    let path = runtime
        .local_ipc_socket_path()
        .expect("local IPC bound")
        .to_path_buf();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut stream = loop {
        match UnixStream::connect(&path) {
            Ok(stream) => break stream,
            Err(_) => {
                assert!(Instant::now() < deadline, "control connect timed out");
                runtime
                    .poll_once(Some(Duration::from_millis(10)))
                    .expect("poll");
            }
        }
    };
    write_frame(
        &mut stream,
        MessageType::ClientHello,
        &ClientHello {
            client_capabilities: 0,
        }
        .encode(),
    );
    let (kind, _) = read_frame(runtime, &mut stream, deadline);
    assert_eq!(kind, MessageType::ServerHello as u16);
    write_frame(&mut stream, MessageType::TerminateExecution, &id.to_bytes());
    stream
}

fn dispatch_terminate(runtime: &mut Runtime, id: ExecutionId) {
    let _control = queue_terminate_execution(runtime, id);
    let deadline = Instant::now() + Duration::from_secs(2);
    while runtime.lookup(id).map(|summary| summary.lifecycle) == Some(ExecutionLifecycle::Running) {
        assert!(
            Instant::now() < deadline,
            "terminate frame was not dispatched"
        );
        runtime
            .poll_once(Some(Duration::from_millis(10)))
            .expect("poll");
    }
}

fn wait_until(runtime: &mut Runtime, deadline: Instant, mut pred: impl FnMut(&Runtime) -> bool) {
    while Instant::now() < deadline {
        if pred(runtime) {
            return;
        }
        runtime
            .poll_once(Some(Duration::from_millis(10)))
            .expect("poll");
    }
    panic!("deadline exceeded");
}

#[test]
fn terminate_unpresented_does_not_stall_sibling_pty_output() {
    let _guard = serialized();
    let mut runtime = Runtime::new(config_with_ipc("sibling-progress")).expect("Runtime");

    // Unpresented: created, never attached/bound — W6 terminate target.
    let unpresented = runtime
        .create_execution(CommandSpec::new("/bin/sh").args(["-c", "sleep 30"]), size())
        .expect("unpresented execution");

    // Sibling keeps producing output while terminate runs.
    let sibling = runtime
        .create_execution(
            CommandSpec::new("/bin/sh").args([
                "-c",
                "i=0; while [ $i -lt 200 ]; do printf 'tick-%s\\n' \"$i\"; i=$((i+1)); sleep 0.02; done",
            ]),
            size(),
        )
        .expect("sibling execution");

    wait_until(
        &mut runtime,
        Instant::now() + Duration::from_secs(2),
        |runtime| {
            runtime
                .execution(sibling)
                .and_then(|execution| execution.terminal().row_text(0))
                .is_some_and(|row| row.contains("tick-"))
        },
    );
    let before = runtime
        .execution(sibling)
        .unwrap()
        .terminal()
        .damage_generation();

    dispatch_terminate(&mut runtime, unpresented);
    assert_ne!(
        runtime.lookup(unpresented).map(|s| s.lifecycle),
        Some(ExecutionLifecycle::Running)
    );

    // Sibling damage must advance while the unpresented execution drains.
    let progress_deadline = Instant::now() + Duration::from_secs(2);
    let mut advanced = false;
    while Instant::now() < progress_deadline {
        runtime
            .poll_once(Some(Duration::from_millis(10)))
            .expect("non-blocking poll");
        let now = runtime
            .execution(sibling)
            .expect("sibling stays live")
            .terminal()
            .damage_generation();
        if now > before {
            advanced = true;
            break;
        }
    }
    assert!(
        advanced,
        "sibling PTY progress stalled while terminating unpresented execution"
    );

    wait_until(
        &mut runtime,
        Instant::now() + Duration::from_secs(2),
        |runtime| runtime.lookup(unpresented).is_none(),
    );
    assert!(runtime.lookup(sibling).is_some());
    shutdown(&mut runtime);
}

#[cfg(feature = "test-fault-injection")]
#[test]
fn n_times_reap_failure_stays_bounded_while_sibling_progresses() {
    let _guard = serialized();
    let mut cfg = config("reap-bound-sibling");
    cfg.graceful_termination = Duration::from_millis(20);
    cfg.forced_reap = Duration::from_millis(20);
    cfg.local_ipc = config_with_ipc("reap-bound-sibling").local_ipc;
    let mut runtime = Runtime::new(cfg).expect("Runtime");

    let target = runtime
        .create_execution(CommandSpec::new("/bin/sh").args(["-c", "sleep 30"]), size())
        .expect("target");
    let sibling = runtime
        .create_execution(
            CommandSpec::new("/bin/sh").args([
                "-c",
                "i=0; while [ $i -lt 400 ]; do printf 'live-%s\\n' \"$i\"; i=$((i+1)); sleep 0.01; done",
            ]),
            size(),
        )
        .expect("sibling");

    wait_until(
        &mut runtime,
        Instant::now() + Duration::from_secs(2),
        |runtime| {
            runtime
                .execution(sibling)
                .and_then(|execution| execution.terminal().row_text(0))
                .is_some_and(|row| row.contains("live-"))
        },
    );
    let before = runtime
        .execution(sibling)
        .unwrap()
        .terminal()
        .damage_generation();

    // Persistent unreapability: bounded retries, no fixed-frequency hot loop.
    // The frame is queued on a Ready connection before the fault is armed so
    // handshake polls do not consume the failure budget.
    let _control = queue_terminate_execution(&mut runtime, target);
    exec_fault::fail_times(ExecFaultPoint::ChildTryWait, 64);

    let failed_deadline = Instant::now() + Duration::from_secs(3);
    let mut saw_failed = false;
    let mut sibling_advanced = false;
    while Instant::now() < failed_deadline {
        runtime
            .poll_once(Some(Duration::from_millis(10)))
            .expect("bounded poll");
        if runtime.lookup(target).map(|s| s.lifecycle)
            == Some(ExecutionLifecycle::TerminationFailed)
        {
            saw_failed = true;
        }
        let now = runtime
            .execution(sibling)
            .expect("sibling remains")
            .terminal()
            .damage_generation();
        if now > before {
            sibling_advanced = true;
        }
        if saw_failed && sibling_advanced {
            break;
        }
    }
    assert!(
        saw_failed,
        "N-times reap failure never entered TerminationFailed"
    );
    assert!(
        sibling_advanced,
        "sibling PTY work stalled during bounded reap-failure retries"
    );

    exec_fault::fail_times(ExecFaultPoint::ChildTryWait, 0);
    wait_until(
        &mut runtime,
        Instant::now() + Duration::from_secs(3),
        |runtime| runtime.lookup(target).is_none(),
    );
    assert!(runtime.lookup(sibling).is_some());
    shutdown(&mut runtime);
}

#[test]
fn adopt_attach_allocates_fresh_attachment_same_execution() {
    let _guard = serialized();
    let mut runtime = Runtime::new(config("adopt-attach")).expect("Runtime");
    let id = runtime
        .create_execution(CommandSpec::new("/bin/sh").args(["-c", "sleep 30"]), size())
        .expect("execution");
    let first = runtime.attach(id).expect("first attachment");
    runtime.detach(id, first).expect("detach → unpresented");
    let second = runtime.attach(id).expect("adopt attach");
    assert_ne!(first, second);
    assert_eq!(
        runtime.lookup(id).unwrap().lifecycle,
        ExecutionLifecycle::Running
    );
    runtime.detach(id, second).unwrap();
    runtime.request_termination(id).unwrap();
    wait_until(
        &mut runtime,
        Instant::now() + Duration::from_secs(2),
        |runtime| runtime.lookup(id).is_none(),
    );
    shutdown(&mut runtime);
}
