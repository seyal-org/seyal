//! Versioned adapter conformance catalog — single source of truth for case IDs.
//!
//! Replay (#1278) and real StandaloneProcessHost adapters (#1279/#1280) must
//! satisfy the **same** inventoriable case IDs. Authority citations map each
//! case to Accepted SPEC-018 §16, SPEC-027 §9/§11, and ADR-012 fencing rules.

/// Catalog schema version. Bump only when case IDs are added, renamed, or
/// retired with an explicit migration note in the engineering guide.
pub const CATALOG_VERSION: u32 = 1;

/// Frozen case count for CI shrink detection. Raising this is a deliberate
/// catalog expansion; lowering it without Issue/review is a merge-blocking
/// regression (`adapter_conformance_catalog_ids_stable`).
pub const CATALOG_CASE_COUNT: usize = 12;

/// One retained conformance obligation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatalogCase {
    /// Stable machine ID. Never reuse a retired ID for a different obligation.
    pub id: &'static str,
    /// Short human summary for docs and failure messages.
    pub summary: &'static str,
    /// Accepted authority citation (SPEC/ADR section).
    pub authority: &'static str,
}

/// Complete catalog. Order is stable for review diffs; harness may run in any order.
pub const CASES: &[CatalogCase] = &[
    CatalogCase {
        id: "manifest.schema.protocol_version",
        summary: "Manifest/schema/protocol-version validation fails closed on unknown versions",
        authority: "SPEC-018 §16.1; harness RD adapter suite",
    },
    CatalogCase {
        id: "discovery.duplicate_id",
        summary: "Discovery rejects or de-duplicates colliding adapter IDs",
        authority: "SPEC-027 §5.2; harness RD adapter suite",
    },
    CatalogCase {
        id: "trust.untrusted_repo_no_auto_execute",
        summary: "Untrusted/uninstalled repository adapter content does not auto-execute",
        authority: "SPEC-027 §5.2 / §10; harness RD adapter suite",
    },
    CatalogCase {
        id: "isolation.adapter_crash_preserves_terminal_execution",
        summary: "Adapter crash/restart leaves still-live TerminalExecution alive (no fabricated death)",
        authority: "SPEC-018 §16.13; SPEC-027 §9.3; ADR-012 isolation",
    },
    CatalogCase {
        id: "bounds.oversized_observation_ipc",
        summary: "Bounded/oversized observation or IPC payloads fail closed without unbounded retention",
        authority: "SPEC-018 §7; SPEC-027 §9.1 observe bounds",
    },
    CatalogCase {
        id: "capability.enforcement_class_honesty",
        summary: "Cannot claim BackendEnforced without a typed backend authority boundary",
        authority: "ADR-012 §12; SPEC-018 §6",
    },
    CatalogCase {
        id: "launch.enabled_manifest_descriptor_only",
        summary: "Launch uses enabled manifest launch descriptor only (never client-supplied argv)",
        authority: "SPEC-027 §5 / §11.5",
    },
    CatalogCase {
        id: "lifecycle.cancel_terminating_cancelled",
        summary: "Cancel/abort yields Terminating then evidenced Terminated(Cancelled) / known-terminated",
        authority: "SPEC-027 §9.4 / §11.13",
    },
    CatalogCase {
        id: "fence.stale_binding_generation",
        summary: "Stale binding generation cannot control the current run",
        authority: "SPEC-018 §16.2; ADR-012 fencing; SPEC-027 §9.2",
    },
    CatalogCase {
        id: "liveness.channel_loss_not_process_death",
        summary: "Structured-channel loss while process may remain live must not fabricate process death",
        authority: "SPEC-027 §9.3 / §11.14",
    },
    CatalogCase {
        id: "events.duplicate_out_of_order_idempotent",
        summary: "Duplicate/out-of-order events are handled explicitly and idempotently where applicable",
        authority: "ADR-012 §13; SPEC-018 §7",
    },
    CatalogCase {
        id: "cache.mutating_unknown_never_replay",
        summary: "Mutating or unknown-effect observations are never treated as cache replay",
        authority: "SPEC-018 §13 / §16.10–§16.11",
    },
];

/// Look up a catalog case by stable ID.
pub fn case_by_id(id: &str) -> Option<&'static CatalogCase> {
    CASES.iter().find(|case| case.id == id)
}

/// All catalog IDs in declaration order.
pub fn catalog_ids() -> impl Iterator<Item = &'static str> {
    CASES.iter().map(|case| case.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn adapter_conformance_catalog_ids_stable() {
        assert_eq!(CASES.len(), CATALOG_CASE_COUNT);
        assert_eq!(CATALOG_VERSION, 1);

        let mut seen = BTreeSet::new();
        for case in CASES {
            assert!(!case.id.is_empty(), "catalog case must have a non-empty id");
            assert!(
                case.id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_'),
                "case id {:?} must be lowercase dotted-snake",
                case.id
            );
            assert!(
                seen.insert(case.id),
                "duplicate catalog case id: {}",
                case.id
            );
            assert!(!case.summary.is_empty());
            assert!(!case.authority.is_empty());
        }
        assert_eq!(seen.len(), CATALOG_CASE_COUNT);

        // Frozen ID set — expanding requires bumping CATALOG_CASE_COUNT and review.
        let expected: BTreeSet<&str> = [
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
        ]
        .into_iter()
        .collect();
        assert_eq!(seen, expected);
    }
}
