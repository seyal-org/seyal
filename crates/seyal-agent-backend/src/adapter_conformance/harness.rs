//! Catalog harness — runs stable case IDs against any [`AdapterConformanceDriver`].

use crate::adapter_conformance::catalog::{case_by_id, catalog_ids, CASES, CATALOG_VERSION};
use crate::adapter_conformance::driver::{
    AdapterConformanceDriver, AdapterConformanceRegistration, CaseResult, ConformanceVerdict,
};

/// Result of running the full catalog (or a requested subset).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogRunReport {
    pub catalog_version: u32,
    pub adapter_label: String,
    pub results: Vec<CaseResult>,
}

impl CatalogRunReport {
    pub fn all_passed(&self) -> bool {
        self.results.iter().all(|r| r.verdict.is_pass())
    }

    pub fn failures(&self) -> impl Iterator<Item = &CaseResult> {
        self.results.iter().filter(|r| !r.verdict.is_pass())
    }
}

fn dispatch_case(driver: &mut dyn AdapterConformanceDriver, case_id: &str) -> ConformanceVerdict {
    match case_id {
        "manifest.schema.protocol_version" => driver.probe_manifest_schema_protocol_version(),
        "discovery.duplicate_id" => driver.probe_discovery_duplicate_id(),
        "trust.untrusted_repo_no_auto_execute" => driver.probe_untrusted_repo_no_auto_execute(),
        "isolation.adapter_crash_preserves_terminal_execution" => {
            driver.probe_adapter_crash_preserves_terminal_execution()
        }
        "bounds.oversized_observation_ipc" => driver.probe_oversized_observation_ipc(),
        "capability.enforcement_class_honesty" => driver.probe_enforcement_class_honesty(),
        "launch.enabled_manifest_descriptor_only" => {
            driver.probe_launch_enabled_manifest_descriptor_only()
        }
        "lifecycle.cancel_terminating_cancelled" => driver.probe_cancel_terminating_cancelled(),
        "fence.stale_binding_generation" => driver.probe_stale_binding_generation(),
        "liveness.channel_loss_not_process_death" => driver.probe_channel_loss_not_process_death(),
        "events.duplicate_out_of_order_idempotent" => {
            driver.probe_duplicate_out_of_order_idempotent()
        }
        "cache.mutating_unknown_never_replay" => driver.probe_mutating_unknown_never_replay(),
        unknown => ConformanceVerdict::fail(format!(
            "unknown catalog case id {unknown:?} — catalog and harness dispatch must stay in sync"
        )),
    }
}

/// Run every catalog case against `driver`.
pub fn run_catalog(driver: &mut dyn AdapterConformanceDriver) -> CatalogRunReport {
    let ids: Vec<&'static str> = catalog_ids().collect();
    run_cases(driver, &ids)
}

/// Run an explicit list of case IDs (must exist in the catalog).
pub fn run_cases(driver: &mut dyn AdapterConformanceDriver, case_ids: &[&str]) -> CatalogRunReport {
    let mut results = Vec::with_capacity(case_ids.len());
    for id in case_ids {
        let verdict = if case_by_id(id).is_none() {
            ConformanceVerdict::fail(format!(
                "case id {id:?} is not in catalog v{CATALOG_VERSION}"
            ))
        } else {
            dispatch_case(driver, id)
        };
        // Leak-free: only catalog static IDs reach CaseResult::case_id on success path.
        let case_id = case_by_id(id).map(|c| c.id).unwrap_or("unknown");
        results.push(CaseResult { case_id, verdict });
    }
    CatalogRunReport {
        catalog_version: CATALOG_VERSION,
        adapter_label: driver.adapter_label().to_string(),
        results,
    }
}

/// Validate that a registration's covered IDs are catalog members (no silent extras).
pub fn validate_registration(registration: &AdapterConformanceRegistration) -> Result<(), String> {
    for id in registration.covered_case_ids {
        if case_by_id(id).is_none() {
            return Err(format!(
                "registration {:?} covers unknown case id {id:?}",
                registration.adapter_label
            ));
        }
    }
    Ok(())
}

/// Smoke subset required by #1277 acceptance (start/cancel/crash fence/enforcement).
pub const FIXTURE_HOST_SMOKE_CASE_IDS: &[&str] = &[
    "lifecycle.cancel_terminating_cancelled",
    "isolation.adapter_crash_preserves_terminal_execution",
    "fence.stale_binding_generation",
    "capability.enforcement_class_honesty",
    "liveness.channel_loss_not_process_death",
];

/// Assert the catalog table itself is intact (used by library + integration tests).
pub fn assert_catalog_integrity() {
    assert_eq!(
        CASES.len(),
        crate::adapter_conformance::catalog::CATALOG_CASE_COUNT
    );
    let mut prev = "";
    for case in CASES {
        assert!(!case.id.is_empty());
        // Presence check only; uniqueness is covered by catalog unit test.
        let _ = prev;
        prev = case.id;
    }
}
