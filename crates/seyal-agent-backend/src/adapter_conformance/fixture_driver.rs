//! Fixture-host conformance driver — harness proof substrate (#1277).
//!
//! Runs the shared scripted probes against FakeExecutionHost before/alongside
//! the permanent offline replay adapter (#1278). Available only with
//! `--features fixture-host`.

use crate::adapter_conformance::driver::{
    AdapterConformanceDriver, AdapterConformanceRegistration, ConformanceDriverKind,
    ConformanceVerdict,
};
use crate::adapter_conformance::harness::FIXTURE_HOST_SMOKE_CASE_IDS;
use crate::adapter_conformance::scripted_probes::{self, FULL_CATALOG_CASE_IDS};

/// Registration published by the fixture-host driver (full catalog coverage).
pub const FIXTURE_HOST_REGISTRATION: AdapterConformanceRegistration =
    AdapterConformanceRegistration {
        adapter_label: "fixture-host",
        driver_kind: ConformanceDriverKind::FixtureHost,
        covered_case_ids: FULL_CATALOG_CASE_IDS,
    };

/// Driver that exercises catalog probes on the FakeExecutionHost substrate.
pub struct FixtureHostConformanceDriver {
    label: &'static str,
}

impl FixtureHostConformanceDriver {
    pub fn new() -> Self {
        Self {
            label: FIXTURE_HOST_REGISTRATION.adapter_label,
        }
    }

    pub fn smoke_case_ids() -> &'static [&'static str] {
        FIXTURE_HOST_SMOKE_CASE_IDS
    }
}

impl Default for FixtureHostConformanceDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl AdapterConformanceDriver for FixtureHostConformanceDriver {
    fn kind(&self) -> ConformanceDriverKind {
        ConformanceDriverKind::FixtureHost
    }

    fn adapter_label(&self) -> &str {
        self.label
    }

    fn probe_manifest_schema_protocol_version(&mut self) -> ConformanceVerdict {
        scripted_probes::probe_manifest_schema_protocol_version()
    }

    fn probe_discovery_duplicate_id(&mut self) -> ConformanceVerdict {
        scripted_probes::probe_discovery_duplicate_id()
    }

    fn probe_untrusted_repo_no_auto_execute(&mut self) -> ConformanceVerdict {
        scripted_probes::probe_untrusted_repo_no_auto_execute()
    }

    fn probe_adapter_crash_preserves_terminal_execution(&mut self) -> ConformanceVerdict {
        scripted_probes::probe_adapter_crash_preserves_terminal_execution()
    }

    fn probe_oversized_observation_ipc(&mut self) -> ConformanceVerdict {
        scripted_probes::probe_oversized_observation_ipc()
    }

    fn probe_launch_enabled_manifest_descriptor_only(&mut self) -> ConformanceVerdict {
        scripted_probes::probe_launch_enabled_manifest_descriptor_only()
    }

    fn probe_cancel_terminating_cancelled(&mut self) -> ConformanceVerdict {
        scripted_probes::probe_cancel_terminating_cancelled()
    }

    fn probe_stale_binding_generation(&mut self) -> ConformanceVerdict {
        scripted_probes::probe_stale_binding_generation()
    }

    fn probe_channel_loss_not_process_death(&mut self) -> ConformanceVerdict {
        scripted_probes::probe_channel_loss_not_process_death()
    }

    fn probe_duplicate_out_of_order_idempotent(&mut self) -> ConformanceVerdict {
        scripted_probes::probe_duplicate_out_of_order_idempotent()
    }

    fn probe_mutating_unknown_never_replay(&mut self) -> ConformanceVerdict {
        scripted_probes::probe_mutating_unknown_never_replay()
    }
}
