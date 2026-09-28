//! ADR-018 §3.3 / W6: terminate live-unpresented without stalling siblings.
#![cfg(target_os = "macos")]

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use seyal_exec::{CommandSpec, WindowSize};
use seyal_runtime::{ExecutionLifecycle, LocalIpcMode, Runtime, RuntimeConfig};

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

fn shutdown(runtime: &mut Runtime) {
    runtime.begin_shutdown().expect("begin shutdown");
    runtime
        .run_until_empty(Instant::now() + Duration::from_secs(3))
        .expect("shutdown completes");
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
    let mut runtime = Runtime::new(config("sibling-progress")).expect("Runtime");

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

    runtime
        .request_termination(unpresented)
        .expect("terminate unpresented");
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
    exec_fault::fail_times(ExecFaultPoint::ChildTryWait, 64);
    runtime.request_termination(target).expect("terminate");

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
