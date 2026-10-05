//! Permanent adapter conformance catalog + harness (Issue #1277 / #679).
//!
//! One inventoriable contract judges offline replay adapters and real
//! `StandaloneProcessHost`-backed CLI adapters by the same stable case IDs.
//! FakeExecutionHost and the offline replay adapter remain `fixture-host`-only;
//! production binaries never embed them.

pub mod catalog;
pub mod driver;
pub mod enforcement;
pub mod harness;
pub mod registration;

#[cfg(feature = "fixture-host")]
pub mod fixture_driver;
#[cfg(feature = "fixture-host")]
pub mod replay_driver;
#[cfg(feature = "fixture-host")]
mod scripted_probes;

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
#[cfg(feature = "fixture-host")]
pub use replay_driver::{ReplayAdapterConformanceDriver, REPLAY_ADAPTER_REGISTRATION};
