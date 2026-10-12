//! Codex CLI adapter conformance + install path (#1280).
//!
//! Runs the shared #1277 catalog against
//! [`CodexAdapterConformanceDriver`] (`StandaloneProcessAdapter`). Does not
//! require `fixture-host` — production `StandaloneProcessHost` only.

#![cfg(unix)]

use seyal_agent_backend::adapter_conformance::{
    run_cases, run_catalog, validate_registration, AdapterConformanceDriver, ConformanceDriverKind,
    CATALOG_CASE_COUNT,
};
use seyal_agent_backend::{
    codex_adapter_id, CodexAdapterConformanceDriver, CodexLaunchPlan, CODEX_ADAPTER_LABEL,
    CODEX_ADAPTER_REGISTRATION,
};
use seyal_agent_backend::{
    HostStartOutcome, SessionExecutionHost, StandaloneProcessConfig, StandaloneProcessHost,
};
use seyal_agent_core::{AgentDomain, LaunchDescriptor, WorkScopeKind};

#[test]
fn adapter_conformance_codex_registration() {
    validate_registration(&CODEX_ADAPTER_REGISTRATION).expect("codex registration");
    assert_eq!(
        CODEX_ADAPTER_REGISTRATION.driver_kind,
        ConformanceDriverKind::StandaloneProcessAdapter
    );
    assert_eq!(
        CODEX_ADAPTER_REGISTRATION.adapter_label,
        CODEX_ADAPTER_LABEL
    );
    assert_eq!(
        CODEX_ADAPTER_REGISTRATION.covered_case_ids.len(),
        CATALOG_CASE_COUNT
    );
    assert_eq!(codex_adapter_id().to_bytes(), *b"SEYALCODEXV10001");
}

#[test]
fn adapter_conformance_codex_full_catalog() {
    let mut driver = CodexAdapterConformanceDriver::new();
    assert_eq!(
        driver.kind(),
        ConformanceDriverKind::StandaloneProcessAdapter
    );
    assert_eq!(driver.adapter_label(), CODEX_ADAPTER_LABEL);
    let report = run_catalog(&mut driver);
    assert_eq!(report.results.len(), CATALOG_CASE_COUNT);
    assert_eq!(report.adapter_label, CODEX_ADAPTER_LABEL);
    assert!(
        report.all_passed(),
        "codex full catalog failures: {:?}",
        report
            .failures()
            .map(|r| format!("{}: {:?}", r.case_id, r.verdict))
            .collect::<Vec<_>>()
    );
}

#[test]
fn adapter_conformance_codex_crash_and_stale_binding() {
    let mut driver = CodexAdapterConformanceDriver::new();
    let report = run_cases(
        &mut driver,
        &[
            "isolation.adapter_crash_preserves_terminal_execution",
            "fence.stale_binding_generation",
            "capability.enforcement_class_honesty",
            "lifecycle.cancel_terminating_cancelled",
        ],
    );
    assert!(
        report.all_passed(),
        "codex crash/stale/honesty/cancel failures: {:?}",
        report
            .failures()
            .map(|r| format!("{}: {:?}", r.case_id, r.verdict))
            .collect::<Vec<_>>()
    );
}

#[test]
fn codex_enabled_manifest_starts_via_standalone_process_host() {
    let plan = CodexLaunchPlan::conformance_stand_in("/bin/echo");
    let template = plan.to_template();
    let config = StandaloneProcessConfig::new(64).expect("config");
    let mut host = StandaloneProcessHost::new(config);
    let mut domain = AgentDomain::new();
    let scope = domain.create_work_scope(WorkScopeKind::AdHoc);
    let item = domain.create_work_item(scope).expect("item");
    let attempt = domain.create_attempt(item).expect("attempt");
    let run = domain.create_agent_run(attempt).expect("run");
    let generation = domain.agent_run(run).expect("run").binding_generation();
    let descriptor = LaunchDescriptor {
        program: template.program,
        argv: template.argv_template,
        env: Vec::new(),
        cwd: std::env::temp_dir(),
    };
    match host.start(run, generation, descriptor) {
        HostStartOutcome::Started(handle) => {
            let _ = host.reap(handle);
        }
        other => panic!("expected Started from Codex manifest descriptor, got {other:?}"),
    }
}

#[test]
fn codex_production_binary_excludes_fake_execution_host() {
    let main_rs = include_str!("../src/main.rs");
    assert!(
        main_rs.contains("StandaloneProcessHost"),
        "production main must compose StandaloneProcessHost"
    );
    let affirmative = main_rs.lines().any(|line| {
        let t = line.trim();
        !t.starts_with("//")
            && !t.starts_with("//!")
            && (t.contains("FakeExecutionHost::")
                || t.contains("Box::new(FakeExecutionHost")
                || t.contains("FakeExecutionHost::new"))
    });
    assert!(
        !affirmative,
        "production main must not construct FakeExecutionHost"
    );
    assert!(
        !main_rs.contains("CodexAdapterConformanceDriver"),
        "production main must not compose the conformance driver"
    );
}
