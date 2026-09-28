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
