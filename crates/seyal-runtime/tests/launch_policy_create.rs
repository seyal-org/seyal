//! L2/L3 Runtime interactive create: SPEC-023 §12 items 11–12, 15–17 plus SPEC-003 rollback.
#![cfg(target_os = "macos")]

use std::time::{Duration, Instant};

use seyal_exec::{CommandSpec, WindowSize};
use seyal_protocol::framing::ErrorCode;
use seyal_runtime::{
    encode_created_warnings, encode_launch_policy_failure, CapabilityPolicy, LaunchPolicyFailure,
    LaunchPolicyWarning, LocalIpcMode, Runtime, RuntimeConfig, RuntimeError,
};

fn config(test: &str) -> RuntimeConfig {
    let mut config = RuntimeConfig::m001().expect("bundled capability profile");
    config.singleton_path = std::env::temp_dir().join(format!(
        "seyal-launch-policy-{}-{}-{test}.lock",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    config.local_ipc = LocalIpcMode::Disabled;
    config.graceful_termination = Duration::from_millis(25);
    config.forced_reap = Duration::from_millis(500);
    config.final_drain = Duration::from_millis(100);
    config
}

fn size() -> WindowSize {
    WindowSize::new(80, 24, 0, 0).unwrap()
}

fn shutdown(runtime: &mut Runtime) {
    runtime.begin_shutdown().unwrap();
    runtime
        .run_until_empty(Instant::now() + Duration::from_secs(2))
        .unwrap();
}

/// §12 item 11: CapabilityUnavailable leaves zero published executions.
#[test]
fn capability_unavailable_publishes_zero_executions() {
    let missing = std::env::temp_dir().join(format!(
        "seyal-cap-empty-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&missing).unwrap();
    let mut config = config("cap-unavailable");
    config.capability_policy = CapabilityPolicy::from_terminfo_dir(&missing).unwrap();
    assert!(!config.capability_policy.is_available());

    let mut runtime = Runtime::new(config).unwrap();
    let err = runtime.create_interactive_execution(size()).unwrap_err();
    assert!(matches!(
        err,
        RuntimeError::LaunchPolicy(LaunchPolicyFailure::CapabilityUnavailable)
    ));
    let wire = err.create_result_wire().expect("launch-policy wire");
    assert_eq!(wire.result_code, ErrorCode::LaunchPolicyRejected as u16);
    assert_eq!(wire.detail_code, 4);
    assert_ne!(wire.result_code, ErrorCode::InternalFailure as u16);
    assert_eq!(runtime.execution_count(), 0);
    assert!(runtime.list().is_empty());
    std::fs::remove_dir_all(&missing).unwrap();
}

/// §12 item 12: developer/test explicit argv still creates one execution.
#[test]
fn developer_explicit_argv_still_creates_one_execution() {
    let mut runtime = Runtime::new(config("explicit-argv")).unwrap();
    let id = runtime
        .create_execution(
            CommandSpec::new("/bin/sh").args(["-c", "printf READY; sleep 30"]),
            size(),
        )
        .unwrap();
    assert_eq!(runtime.execution_count(), 1);
    assert_eq!(runtime.list().len(), 1);
    assert_eq!(runtime.list()[0].id, id);
    shutdown(&mut runtime);
}

/// Profile-0 interactive create publishes exactly one execution when policy succeeds.
#[test]
fn interactive_create_publishes_one_execution() {
    let mut runtime = Runtime::new(config("interactive")).unwrap();
    let outcome = runtime.create_interactive_execution(size()).unwrap();
    assert_eq!(runtime.execution_count(), 1);
    assert_eq!(
        runtime.lookup(outcome.execution_id).unwrap().id,
        outcome.execution_id
    );
    // Reserved warning bits stay clear on an ordinary account-shell success.
    assert_eq!(outcome.detail_code & !0b11, 0);
    shutdown(&mut runtime);
}

/// §12 item 17 + one assertion per failure class: sole encoding is code 17.
#[test]
fn launch_policy_failure_classes_encode_as_code_17_not_14() {
    for (failure, detail) in [
        (LaunchPolicyFailure::AccountRecordUnavailable, 1u32),
        (LaunchPolicyFailure::ShellFallbackExhausted, 2),
        (LaunchPolicyFailure::CwdInvalid, 3),
        (LaunchPolicyFailure::CapabilityUnavailable, 4),
    ] {
        let wire = encode_launch_policy_failure(failure);
        assert_eq!(wire.result_code, ErrorCode::LaunchPolicyRejected as u16);
        assert_eq!(wire.detail_code, detail);
        assert_ne!(wire.result_code, ErrorCode::InternalFailure as u16);
        let err = RuntimeError::LaunchPolicy(failure);
        assert_eq!(err.create_result_wire(), Some(wire));
    }
}

/// §12 item 15 / 17: empty/invalid pw_shell warning is Created bit 0, never a failure.
#[test]
fn configured_shell_invalid_warning_is_created_bit_not_failure() {
    let wire = encode_created_warnings(&[LaunchPolicyWarning::ConfiguredShellInvalid]);
    assert_eq!(wire.result_code, 0);
    assert_eq!(wire.detail_code, 1 << 0);
    assert_ne!(wire.result_code, ErrorCode::LaunchPolicyRejected as u16);
}

/// §12 item 17: without CAP_LAUNCH_POLICY_DETAIL, Created.detail_code stays 0.
#[test]
fn created_detail_code_is_zero_when_launch_policy_detail_not_negotiated() {
    let mut runtime = Runtime::new(config("detail-cap-off")).unwrap();
    let outcome = runtime
        .create_interactive_execution_with_detail_cap(size(), false)
        .unwrap();
    assert_eq!(
        outcome.detail_code, 0,
        "ADR-020 §3.10: non-negotiating peer must not see Created warning bits"
    );
    shutdown(&mut runtime);
}

/// Existing SPEC-003 invalid-command rollback stays green beside the new path.
#[test]
fn invalid_command_still_rolls_back_with_zero_publications() {
    let mut runtime = Runtime::new(config("invalid-command")).unwrap();
    assert!(runtime
        .create_execution(CommandSpec::new("/definitely/not/a/seyal-command"), size())
        .is_err());
    assert_eq!(runtime.execution_count(), 0);
    assert!(runtime.list().is_empty());
}

/// L5 / SPEC-023 §11: N-times launch-policy failure injection cannot hot-loop
/// the reactor, and an unrelated live streaming execution keeps advancing.
///
/// Orthogonal facts kept separate: streamer child alive, interactive create
/// rejected pre-spawn, reactor still drains PTY readiness, shutdown retains a
/// signal/reap path. Progress is asserted via `damage_generation` only (no
/// terminal contents in assertions).
#[test]
fn launch_policy_failure_n_times_does_not_hot_loop_or_starve_streaming_pty() {
    const FAILURES: usize = 8;
    const CREATE_BUDGET: Duration = Duration::from_millis(50);
    const MAX_POLL_TURNS: usize = 500;

    let missing = std::env::temp_dir().join(format!(
        "seyal-cap-nfail-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&missing).unwrap();
    let mut cfg = config("n-times-fail");
    cfg.capability_policy = CapabilityPolicy::from_terminfo_dir(&missing).unwrap();
    assert!(!cfg.capability_policy.is_available());

    let mut runtime = Runtime::new(cfg).unwrap();
    // Explicit argv bypasses profile-0 policy; CapabilityUnavailable gates only
    // interactive create. Streamer stays live across the injected failures.
    let stream_id = runtime
        .create_execution(
            CommandSpec::new("/bin/sh").args(["-c", "while :; do printf .; sleep 0.05; done"]),
            size(),
        )
        .expect("streaming execution");
    assert_eq!(runtime.execution_count(), 1);

    // Prime at least one VT mutation before injection so later advances are
    // unambiguous relative to a known baseline.
    let primed = Instant::now() + Duration::from_secs(2);
    let mut baseline = 0u64;
    while Instant::now() < primed {
        runtime
            .poll_once(Some(Duration::from_millis(25)))
            .expect("prime poll");
        baseline = runtime
            .execution(stream_id)
            .expect("streamer still registered")
            .terminal()
            .damage_generation();
        if baseline > 0 {
            break;
        }
    }
    assert!(baseline > 0, "streaming execution never produced VT damage");

    let mut poll_turns = 0usize;
    let mut last_generation = baseline;
    for _ in 0..FAILURES {
        let generation_before = runtime
            .execution(stream_id)
            .expect("streamer live before failure")
            .terminal()
            .damage_generation();

        let create_started = Instant::now();
        let err = runtime
            .create_interactive_execution(size())
            .expect_err("interactive create must fail while capability is unavailable");
        assert!(
            create_started.elapsed() < CREATE_BUDGET,
            "create_interactive_execution exceeded one-attempt budget (possible retry hot loop)"
        );
        assert!(matches!(
            err,
            RuntimeError::LaunchPolicy(LaunchPolicyFailure::CapabilityUnavailable)
        ));
        let wire = err.create_result_wire().expect("launch-policy wire");
        assert_eq!(wire.result_code, ErrorCode::LaunchPolicyRejected as u16);
        assert_eq!(wire.detail_code, 4);
        // Failures stay pre-spawn: only the unrelated streamer is published.
        assert_eq!(runtime.execution_count(), 1);
        assert_eq!(runtime.list().len(), 1);
        assert_eq!(runtime.list()[0].id, stream_id);

        let progress_deadline = Instant::now() + Duration::from_secs(2);
        let mut advanced = false;
        while Instant::now() < progress_deadline {
            poll_turns += 1;
            assert!(
                poll_turns < MAX_POLL_TURNS,
                "reactor poll turns unbounded under launch-policy failure injection"
            );
            runtime
                .poll_once(Some(Duration::from_millis(25)))
                .expect("policy failure must not poison reactor polling");
            let generation = runtime
                .execution(stream_id)
                .expect("streamer remains registered during failures")
                .terminal()
                .damage_generation();
            if generation > generation_before {
                assert!(
                    generation > last_generation,
                    "streaming damage_generation must keep advancing across failures"
                );
                last_generation = generation;
                advanced = true;
                break;
            }
        }
        assert!(
            advanced,
            "unrelated streaming PTY stopped advancing during launch-policy failure injection"
        );
    }

    assert!(
        last_generation > baseline,
        "streaming execution made no net progress across N policy failures"
    );
    assert_eq!(runtime.execution_count(), 1);

    // Termination invariant: while the primary child is still live, shutdown
    // retains a signalling/reap path regardless of the failed creates.
    shutdown(&mut runtime);
    assert_eq!(runtime.execution_count(), 0);
    std::fs::remove_dir_all(&missing).unwrap();
}
