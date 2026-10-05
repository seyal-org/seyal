//! Permanent adapter conformance catalog + harness (Issue #1277 / #679).
//!
//! One inventoriable contract judges offline replay adapters and real
//! `StandaloneProcessHost`-backed CLI adapters by the same stable case IDs.
//! FakeExecutionHost remains `fixture-host`-only; production binaries never
//! embed it.

pub mod catalog;
pub mod driver;
pub mod enforcement;
pub mod harness;
pub mod registration;

#[cfg(feature = "fixture-host")]
pub mod fixture_driver;

pub use catalog::{
    case_by_id, catalog_ids, CatalogCase, CASES, CATALOG_CASE_COUNT, CATALOG_VERSION,
};
pub use driver::{
    AdapterConformanceDriver, AdapterConformanceRegistration, CaseResult, ConformanceDriverKind,
    ConformanceVerdict,
};
pub use enforcement::{
    evaluate_enforcement_claim, EnforcementClaimOutcome, FixtureEnforcementClass,
};
pub use harness::{
    assert_catalog_integrity, run_cases, run_catalog, validate_registration, CatalogRunReport,
    FIXTURE_HOST_SMOKE_CASE_IDS,
};
pub use registration::{replay_adapter_registration, standalone_adapter_registration};

#[cfg(feature = "fixture-host")]
pub use fixture_driver::{FixtureHostConformanceDriver, FIXTURE_HOST_REGISTRATION};
