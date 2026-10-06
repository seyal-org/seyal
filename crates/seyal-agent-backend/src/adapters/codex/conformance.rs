//! Codex `AdapterConformanceDriver` on `StandaloneProcessHost` (#1280).

use crate::adapter_conformance::driver::{
    AdapterConformanceDriver, AdapterConformanceRegistration, ConformanceDriverKind,
    ConformanceVerdict,
};
use crate::adapter_conformance::registration::standalone_adapter_registration;

use super::host_probes;
use super::manifest::CODEX_ADAPTER_LABEL;

/// Full Track E catalog coverage for the Codex StandaloneProcessHost adapter.
pub const CODEX_COVERED_CASE_IDS: &[&str] = &[
    "manifest.schema.protocol_version",
    "discovery.duplicate_id",
    "trust.untrusted_repo_no_auto_execute",
    "isolation.adapter_crash_preserves_terminal_execution",
    "bounds.oversized_observation_ipc",
    "capability.enforcement_class_honesty",
    "launch.enabled_manifest_descriptor_only",
    "lifecycle.cancel_terminating_cancelled",
    "fence.stale_binding_generation",
    "liveness.channel_loss_not_process_death",
    "events.duplicate_out_of_order_idempotent",
    "cache.mutating_unknown_never_replay",
];

/// Registration published by the first-party Codex adapter.
pub const CODEX_ADAPTER_REGISTRATION: AdapterConformanceRegistration =
    AdapterConformanceRegistration {
        adapter_label: CODEX_ADAPTER_LABEL,
        driver_kind: ConformanceDriverKind::StandaloneProcessAdapter,
        covered_case_ids: CODEX_COVERED_CASE_IDS,
    };

/// Real StandaloneProcessHost-backed Codex adapter judged by the shared catalog.
pub struct CodexAdapterConformanceDriver {
    label: &'static str,
}

impl CodexAdapterConformanceDriver {
    pub fn new() -> Self {
        let _ = standalone_adapter_registration(
            CODEX_ADAPTER_REGISTRATION.adapter_label,
            CODEX_ADAPTER_REGISTRATION.covered_case_ids,
        )
        .expect("codex registration must cover only catalog members");
        Self {
            label: CODEX_ADAPTER_REGISTRATION.adapter_label,
        }
    }
}

impl Default for CodexAdapterConformanceDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl AdapterConformanceDriver for CodexAdapterConformanceDriver {
    fn kind(&self) -> ConformanceDriverKind {
        ConformanceDriverKind::StandaloneProcessAdapter
    }

    fn adapter_label(&self) -> &str {
        self.label
    }

    fn probe_manifest_schema_protocol_version(&mut self) -> ConformanceVerdict {
        host_probes::probe_manifest_schema_protocol_version()
    }

    fn probe_discovery_duplicate_id(&mut self) -> ConformanceVerdict {
        host_probes::probe_discovery_duplicate_id()
    }

    fn probe_untrusted_repo_no_auto_execute(&mut self) -> ConformanceVerdict {
        host_probes::probe_untrusted_repo_no_auto_execute()
    }

    fn probe_adapter_crash_preserves_terminal_execution(&mut self) -> ConformanceVerdict {
        host_probes::probe_adapter_crash_preserves_terminal_execution()
    }

    fn probe_oversized_observation_ipc(&mut self) -> ConformanceVerdict {
        host_probes::probe_oversized_observation_ipc()
    }

    fn probe_enforcement_class_honesty(&mut self) -> ConformanceVerdict {
        host_probes::probe_enforcement_class_honesty()
    }

    fn probe_launch_enabled_manifest_descriptor_only(&mut self) -> ConformanceVerdict {
        host_probes::probe_launch_enabled_manifest_descriptor_only()
    }

    fn probe_cancel_terminating_cancelled(&mut self) -> ConformanceVerdict {
        host_probes::probe_cancel_terminating_cancelled()
    }

    fn probe_stale_binding_generation(&mut self) -> ConformanceVerdict {
        host_probes::probe_stale_binding_generation()
    }

    fn probe_channel_loss_not_process_death(&mut self) -> ConformanceVerdict {
        host_probes::probe_channel_loss_not_process_death()
    }

    fn probe_duplicate_out_of_order_idempotent(&mut self) -> ConformanceVerdict {
        host_probes::probe_duplicate_out_of_order_idempotent()
    }

    fn probe_mutating_unknown_never_replay(&mut self) -> ConformanceVerdict {
        host_probes::probe_mutating_unknown_never_replay()
    }
}
