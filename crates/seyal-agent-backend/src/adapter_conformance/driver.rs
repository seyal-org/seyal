//! Adapter conformance driver seam — one contract for replay and real adapters.
//!
//! Drivers implement probe surfaces; the harness owns case IDs and Pass/Fail
//! interpretation. There is no skip-as-pass: unsupported behavior must Fail
//! with an explicit detail string.

use crate::adapter_conformance::enforcement::{
    evaluate_enforcement_claim, EnforcementClaimOutcome, FixtureEnforcementClass,
};

/// Which adapter surface is under test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConformanceDriverKind {
    /// Scripted `FakeExecutionHost` / fixture-host composition substrate.
    FixtureHost,
    /// Offline replay / fake adapter (sibling #1278).
    ReplayAdapter,
    /// Real CLI adapter composed on `StandaloneProcessHost` (#1279/#1280).
    StandaloneProcessAdapter,
}

/// Explicit Pass or Fail — never silent skip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConformanceVerdict {
    Pass,
    Fail { detail: String },
}

impl ConformanceVerdict {
    pub fn pass() -> Self {
        Self::Pass
    }

    pub fn fail(detail: impl Into<String>) -> Self {
        Self::Fail {
            detail: detail.into(),
        }
    }

    pub const fn is_pass(&self) -> bool {
        matches!(self, Self::Pass)
    }
}

/// One catalog case result with stable ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseResult {
    pub case_id: &'static str,
    pub verdict: ConformanceVerdict,
}

/// Surfaces every catalog case probes. Replay and real adapters implement this
/// trait; the fixture-host driver proves the harness before those siblings land.
pub trait AdapterConformanceDriver {
    fn kind(&self) -> ConformanceDriverKind;

    /// Human-readable registration label (adapter id / fixture name).
    fn adapter_label(&self) -> &str;

    /// `manifest.schema.protocol_version`
    fn probe_manifest_schema_protocol_version(&mut self) -> ConformanceVerdict;

    /// `discovery.duplicate_id`
    fn probe_discovery_duplicate_id(&mut self) -> ConformanceVerdict;

    /// `trust.untrusted_repo_no_auto_execute`
    fn probe_untrusted_repo_no_auto_execute(&mut self) -> ConformanceVerdict;

    /// `isolation.adapter_crash_preserves_terminal_execution`
    fn probe_adapter_crash_preserves_terminal_execution(&mut self) -> ConformanceVerdict;

    /// `bounds.oversized_observation_ipc`
    fn probe_oversized_observation_ipc(&mut self) -> ConformanceVerdict;

    /// `capability.enforcement_class_honesty`
    fn probe_enforcement_class_honesty(&mut self) -> ConformanceVerdict {
        // Default honesty gate is shared so every driver cannot invent a weaker rule.
        let dishonest = evaluate_enforcement_claim(FixtureEnforcementClass::BackendEnforced, false);
        let honest = evaluate_enforcement_claim(FixtureEnforcementClass::BackendEnforced, true);
        if dishonest != EnforcementClaimOutcome::RejectedDishonest {
            return ConformanceVerdict::fail(
                "BackendEnforced without typed boundary must be RejectedDishonest",
            );
        }
        if honest != EnforcementClaimOutcome::Accepted {
            return ConformanceVerdict::fail(
                "BackendEnforced with typed boundary must be Accepted",
            );
        }
        ConformanceVerdict::pass()
    }

    /// `launch.enabled_manifest_descriptor_only`
    fn probe_launch_enabled_manifest_descriptor_only(&mut self) -> ConformanceVerdict;

    /// `lifecycle.cancel_terminating_cancelled`
    fn probe_cancel_terminating_cancelled(&mut self) -> ConformanceVerdict;

    /// `fence.stale_binding_generation`
    fn probe_stale_binding_generation(&mut self) -> ConformanceVerdict;

    /// `liveness.channel_loss_not_process_death`
    fn probe_channel_loss_not_process_death(&mut self) -> ConformanceVerdict;

    /// `events.duplicate_out_of_order_idempotent`
    fn probe_duplicate_out_of_order_idempotent(&mut self) -> ConformanceVerdict;

    /// `cache.mutating_unknown_never_replay`
    fn probe_mutating_unknown_never_replay(&mut self) -> ConformanceVerdict;
}

/// Declares which catalog cases an adapter claims to cover when registering.
///
/// Siblings (#1278–#1280) publish a static registration; the harness still runs
/// every catalog ID and fails any missing/weak probe — coverage claims cannot
/// shrink the catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdapterConformanceRegistration {
    pub adapter_label: &'static str,
    pub driver_kind: ConformanceDriverKind,
    /// Case IDs this adapter intends to satisfy. Must be a subset of the catalog.
    pub covered_case_ids: &'static [&'static str],
}
