//! Adapter conformance catalog harness (#1277) + offline replay adapter (#1278).
//!
//! Fixture-host proves the harness; replay registers as `ReplayAdapter` and
//! must pass the full catalog offline. Production builds never enable
//! `fixture-host` for the daemon binary.

#![cfg(feature = "fixture-host")]

use seyal_agent_backend::adapter_conformance::{
    assert_catalog_integrity, run_cases, run_catalog, validate_registration,
    AdapterConformanceDriver, ConformanceDriverKind, FixtureHostConformanceDriver,
    ReplayAdapterConformanceDriver, FIXTURE_HOST_REGISTRATION, FIXTURE_HOST_SMOKE_CASE_IDS,
    REPLAY_ADAPTER_REGISTRATION,
};

#[test]
fn adapter_conformance_catalog_ids_stable() {
    assert_catalog_integrity();
    validate_registration(&FIXTURE_HOST_REGISTRATION).expect("fixture registration");
    validate_registration(&REPLAY_ADAPTER_REGISTRATION).expect("replay registration");
    assert_eq!(
        FIXTURE_HOST_REGISTRATION.covered_case_ids.len(),
        seyal_agent_backend::adapter_conformance::CATALOG_CASE_COUNT
    );
    assert_eq!(
        REPLAY_ADAPTER_REGISTRATION.covered_case_ids.len(),
        seyal_agent_backend::adapter_conformance::CATALOG_CASE_COUNT
    );
    assert_eq!(
        REPLAY_ADAPTER_REGISTRATION.driver_kind,
        ConformanceDriverKind::ReplayAdapter
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

#[test]
fn adapter_conformance_replay_full_catalog() {
    let mut driver = ReplayAdapterConformanceDriver::new();
    assert_eq!(driver.kind(), ConformanceDriverKind::ReplayAdapter);
    assert_eq!(driver.adapter_label(), "replay-adapter");
    let report = run_catalog(&mut driver);
    assert_eq!(
        report.results.len(),
        seyal_agent_backend::adapter_conformance::CATALOG_CASE_COUNT
    );
    assert_eq!(report.adapter_label, "replay-adapter");
    assert!(
        report.all_passed(),
        "replay full catalog failures: {:?}",
        report
            .failures()
            .map(|r| format!("{}: {:?}", r.case_id, r.verdict))
            .collect::<Vec<_>>()
    );
}

#[test]
fn adapter_conformance_replay_crash_and_stale_binding() {
    let mut driver = ReplayAdapterConformanceDriver::new();
    let report = run_cases(
        &mut driver,
        &[
            "isolation.adapter_crash_preserves_terminal_execution",
            "fence.stale_binding_generation",
            "capability.enforcement_class_honesty",
        ],
    );
    assert!(
        report.all_passed(),
        "replay crash/stale/honesty failures: {:?}",
        report
            .failures()
            .map(|r| format!("{}: {:?}", r.case_id, r.verdict))
            .collect::<Vec<_>>()
    );
}

#[test]
fn adapter_conformance_replay_production_binary_excludes_fixture_host() {
    // Production daemon composes StandaloneProcessHost only. FakeExecutionHost /
    // replay remain behind the optional `fixture-host` feature (qualification
    // binary + tests), never as a production composition default.
    let main_rs = include_str!("../src/main.rs");
    assert!(
        main_rs.contains("StandaloneProcessHost"),
        "production main must compose StandaloneProcessHost"
    );
    assert!(
        !main_rs.contains("use ")
            || !main_rs.lines().any(|l| {
                let t = l.trim();
                t.starts_with("use ") && t.contains("FakeExecutionHost")
            }),
        "production main must not import FakeExecutionHost"
    );
    assert!(
        !main_rs.contains("ReplayAdapterConformanceDriver")
            && !main_rs.contains("REPLAY_ADAPTER_REGISTRATION"),
        "production main must not compose the replay adapter"
    );
    // Negative prose may name FakeExecutionHost; affirmative composition must not.
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

    let cargo_toml = include_str!("../Cargo.toml");
    assert!(
        cargo_toml.contains("fixture-host = []"),
        "fixture-host must remain an optional empty feature"
    );
    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("default") {
            assert!(
                !trimmed.contains("fixture-host"),
                "default features must not enable fixture-host: {trimmed}"
            );
        }
    }

    let lib_rs = include_str!("../src/lib.rs");
    assert!(
        lib_rs.contains("#[cfg(feature = \"fixture-host\")]"),
        "FakeExecutionHost / fixture surfaces must stay feature-gated in lib.rs"
    );
}
