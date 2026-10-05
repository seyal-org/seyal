//! Adapter conformance catalog harness (#1277).
//!
//! Runs the retained case IDs against the fixture-host driver. Replay and real
//! StandaloneProcessHost adapters register against the same harness.

#![cfg(feature = "fixture-host")]

use seyal_agent_backend::adapter_conformance::{
    assert_catalog_integrity, run_cases, run_catalog, validate_registration,
    FixtureHostConformanceDriver, FIXTURE_HOST_REGISTRATION, FIXTURE_HOST_SMOKE_CASE_IDS,
};

#[test]
fn adapter_conformance_catalog_ids_stable() {
    assert_catalog_integrity();
    validate_registration(&FIXTURE_HOST_REGISTRATION).expect("fixture registration");
    assert_eq!(
        FIXTURE_HOST_REGISTRATION.covered_case_ids.len(),
        seyal_agent_backend::adapter_conformance::CATALOG_CASE_COUNT
    );
}

#[test]
fn adapter_conformance_fixture_host_smoke() {
    let mut driver = FixtureHostConformanceDriver::new();
    let report = run_cases(&mut driver, FIXTURE_HOST_SMOKE_CASE_IDS);
    assert!(
        report.all_passed(),
        "fixture-host smoke failures: {:?}",
        report
            .failures()
            .map(|r| (&r.case_id, &r.verdict))
            .collect::<Vec<_>>()
    );
}

#[test]
fn adapter_conformance_stale_binding_fenced() {
    let mut driver = FixtureHostConformanceDriver::new();
    let report = run_cases(&mut driver, &["fence.stale_binding_generation"]);
    assert!(report.all_passed(), "{report:?}");
}

#[test]
fn adapter_conformance_crash_isolates_terminal_execution() {
    let mut driver = FixtureHostConformanceDriver::new();
    let report = run_cases(
        &mut driver,
        &["isolation.adapter_crash_preserves_terminal_execution"],
    );
    assert!(report.all_passed(), "{report:?}");
}

#[test]
fn adapter_conformance_enforcement_class_honesty() {
    let mut driver = FixtureHostConformanceDriver::new();
    let report = run_cases(&mut driver, &["capability.enforcement_class_honesty"]);
    assert!(report.all_passed(), "{report:?}");
}

#[test]
fn adapter_conformance_fixture_host_full_catalog() {
    let mut driver = FixtureHostConformanceDriver::new();
    let report = run_catalog(&mut driver);
    assert_eq!(
        report.results.len(),
        seyal_agent_backend::adapter_conformance::CATALOG_CASE_COUNT
    );
    assert!(
        report.all_passed(),
        "full catalog failures: {:?}",
        report
            .failures()
            .map(|r| format!("{}: {:?}", r.case_id, r.verdict))
            .collect::<Vec<_>>()
    );
}
