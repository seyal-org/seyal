//! Permanent offline replay / fake adapter conformance driver (#1278).
//!
//! Registers as [`ConformanceDriverKind::ReplayAdapter`] and satisfies the
//! full Track E catalog without a live Claude/Codex CLI. Available only with
//! `--features fixture-host` — never composed into the production daemon.

use crate::adapter_conformance::driver::{
    AdapterConformanceDriver, AdapterConformanceRegistration, ConformanceDriverKind,
    ConformanceVerdict,
};
use crate::adapter_conformance::registration::replay_adapter_registration;
use crate::adapter_conformance::scripted_probes::{self, FULL_CATALOG_CASE_IDS};

/// Registration published by the offline replay adapter (full catalog coverage).
pub const REPLAY_ADAPTER_REGISTRATION: AdapterConformanceRegistration =
    AdapterConformanceRegistration {
        adapter_label: "replay-adapter",
        driver_kind: ConformanceDriverKind::ReplayAdapter,
        covered_case_ids: FULL_CATALOG_CASE_IDS,
    };

/// Deterministic offline replay adapter judged by the shared catalog harness.
pub struct ReplayAdapterConformanceDriver {
    label: &'static str,
}

impl ReplayAdapterConformanceDriver {
    pub fn new() -> Self {
        // Validate registration shape at construction (unknown IDs fail closed).
        let _ = replay_adapter_registration(
            REPLAY_ADAPTER_REGISTRATION.adapter_label,
            REPLAY_ADAPTER_REGISTRATION.covered_case_ids,
        )
        .expect("replay registration must cover only catalog members");
        Self {
            label: REPLAY_ADAPTER_REGISTRATION.adapter_label,
        }
    }
}

impl Default for ReplayAdapterConformanceDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl AdapterConformanceDriver for ReplayAdapterConformanceDriver {
    fn kind(&self) -> ConformanceDriverKind {
        ConformanceDriverKind::ReplayAdapter
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
