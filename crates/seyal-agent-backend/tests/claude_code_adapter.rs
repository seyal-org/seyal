//! Claude Code adapter conformance (#1279) against the shared #1277 catalog.

#![cfg(all(unix, feature = "fixture-host"))]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use seyal_agent_backend::adapter_conformance::{
    run_catalog, validate_registration, CATALOG_CASE_COUNT,
};
use seyal_agent_backend::{
    claude_code_adapter_id, claude_code_capability_sheet, install_enabled_claude_code_adapter,
    resolve_claude_code_program, resolve_claude_code_program_from, validate_claude_code_sheet,
    AuthorizationRepository, ClaudeCodeConformanceDriver, ClaudeCodeInstallError, ClientScope,
    PrincipalKind, CLAUDE_CODE_DEFAULT_PROGRAM, CLAUDE_CODE_REGISTRATION,
};
use seyal_agent_store::AgentStore;

static NEXT: AtomicU64 = AtomicU64::new(1);

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "seyal-claude-1279-{}-{}-{}-{}",
        label,
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    dir
}

#[test]
fn adapter_conformance_claude_code_registration_covers_full_catalog() {
    validate_registration(&CLAUDE_CODE_REGISTRATION).expect("registration");
    assert_eq!(
        CLAUDE_CODE_REGISTRATION.covered_case_ids.len(),
        CATALOG_CASE_COUNT
    );
    assert_eq!(
        CLAUDE_CODE_REGISTRATION.driver_kind,
        seyal_agent_backend::adapter_conformance::ConformanceDriverKind::StandaloneProcessAdapter
    );
}

#[test]
fn adapter_conformance_claude_code_full_catalog() {
    let mut driver = ClaudeCodeConformanceDriver::new();
    let report = run_catalog(&mut driver);
    assert_eq!(report.results.len(), CATALOG_CASE_COUNT);
    assert!(
        report.all_passed(),
        "Claude Code catalog failures: {:?}",
        report
            .failures()
            .map(|r| format!("{}: {:?}", r.case_id, r.verdict))
            .collect::<Vec<_>>()
    );
}

#[test]
fn claude_code_sheet_honesty_unit() {
    let caps = claude_code_capability_sheet();
    validate_claude_code_sheet(&caps).expect("no BackendEnforced");
}

#[test]
fn claude_code_missing_absolute_binary_fails_closed() {
    let dir = temp_dir("missing-bin");
    let store = AgentStore::open(dir.join("agent.db")).unwrap();
    let err = install_enabled_claude_code_adapter(&store, "/no/such/claude-1279")
        .expect_err("missing binary");
    assert!(matches!(err, ClaudeCodeInstallError::MissingBinary { .. }));
    assert!(store
        .get_adapter_manifest(claude_code_adapter_id())
        .unwrap()
        .is_none());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn claude_code_disabled_adapter_is_not_launchable_from_catalog() {
    let dir = temp_dir("disabled");
    let store = AgentStore::open(dir.join("agent.db")).unwrap();
    let adapter_id = install_enabled_claude_code_adapter(&store, "/bin/echo").unwrap();
    store.set_adapter_enabled(adapter_id, false).unwrap();
    let row = store.get_adapter_manifest(adapter_id).unwrap().unwrap();
    assert!(!row.enabled);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn claude_code_missing_adapter_execute_grant_is_observable() {
    let dir = temp_dir("no-execute");
    let store = AgentStore::open(dir.join("agent.db")).unwrap();
    let adapter_id = install_enabled_claude_code_adapter(&store, "/bin/echo").unwrap();
    let mut auth = AuthorizationRepository::default();
    let principal = auth.register_principal_with_evidence(
        PrincipalKind::FirstPartyCli,
        [
            ClientScope::RunsCreate,
            ClientScope::RunsObserve,
            ClientScope::RunsControl,
        ],
        b"cli".to_vec(),
    );
    // Install alone does not grant adapter.execute (SPEC-027 §7).
    assert!(store.adapter_execute_grants(principal).unwrap().is_empty());
    store
        .grant_adapter_execute(principal, adapter_id)
        .expect("grant");
    assert_eq!(
        store.adapter_execute_grants(principal).unwrap(),
        vec![adapter_id]
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn claude_code_program_resolution_honors_env_override() {
    // Avoid process-global env mutation (unsafe under Rust 2024). The resolver
    // treats empty overrides as unset; non-empty absolute paths are returned as-is.
    assert_eq!(
        resolve_claude_code_program_from(Some("/custom/claude")),
        "/custom/claude".to_string()
    );
    assert_eq!(
        resolve_claude_code_program_from(Some("  ")),
        CLAUDE_CODE_DEFAULT_PROGRAM.to_string()
    );
    assert_eq!(
        resolve_claude_code_program_from(None),
        CLAUDE_CODE_DEFAULT_PROGRAM.to_string()
    );
}

/// Opt-in live Claude Code probe (documented reproducible path).
///
/// ```sh
/// SEYAL_CLAUDE_CODE_LIVE=1 cargo test -p seyal-agent-backend --features fixture-host --offline -- claude_code_live
/// ```
#[test]
fn claude_code_live_version_probe_opt_in() {
    if std::env::var_os("SEYAL_CLAUDE_CODE_LIVE").is_none() {
        return;
    }
    let program = resolve_claude_code_program();
    let output = std::process::Command::new(&program)
        .arg("--version")
        .output()
        .unwrap_or_else(|error| panic!("spawn {program}: {error}"));
    assert!(
        output.status.success(),
        "claude --version failed: status={:?} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.to_ascii_lowercase().contains("claude") || !text.trim().is_empty(),
        "unexpected version output: {text:?}"
    );
}
