//! Thin documentation + registration helper for StandaloneProcessHost adapters.
//!
//! Claude Code (#1279) and Codex (#1280) implement [`AdapterConformanceDriver`]
//! with [`ConformanceDriverKind::StandaloneProcessAdapter`]. This module does
//! not implement those adapters — it only re-exports the shared registration
//! type so siblings opt into the same catalog without a parallel engine.

use crate::adapter_conformance::driver::{AdapterConformanceRegistration, ConformanceDriverKind};
use crate::adapter_conformance::harness::validate_registration;

/// Build a registration for a real StandaloneProcessHost-backed adapter.
///
/// `covered_case_ids` must be catalog members; unknown IDs fail closed.
pub fn standalone_adapter_registration(
    adapter_label: &'static str,
    covered_case_ids: &'static [&'static str],
) -> Result<AdapterConformanceRegistration, String> {
    let registration = AdapterConformanceRegistration {
        adapter_label,
        driver_kind: ConformanceDriverKind::StandaloneProcessAdapter,
        covered_case_ids,
    };
    validate_registration(&registration)?;
    Ok(registration)
}

/// Build a registration for the offline replay adapter (#1278).
pub fn replay_adapter_registration(
    adapter_label: &'static str,
    covered_case_ids: &'static [&'static str],
) -> Result<AdapterConformanceRegistration, String> {
    let registration = AdapterConformanceRegistration {
        adapter_label,
        driver_kind: ConformanceDriverKind::ReplayAdapter,
        covered_case_ids,
    };
    validate_registration(&registration)?;
    Ok(registration)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_rejects_unknown_case_ids() {
        let err = standalone_adapter_registration("example", &["not.a.real.case"]).unwrap_err();
        assert!(err.contains("unknown case id"));
    }

    #[test]
    fn registration_accepts_catalog_members() {
        let reg = standalone_adapter_registration(
            "example-cli",
            &[
                "fence.stale_binding_generation",
                "capability.enforcement_class_honesty",
            ],
        )
        .expect("valid");
        assert_eq!(
            reg.driver_kind,
            ConformanceDriverKind::StandaloneProcessAdapter
        );
    }
}
