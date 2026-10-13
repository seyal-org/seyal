//! Codex capability sheet with ADR-012 enforcement classes (honest; no over-claim).

use seyal_agent_core::{CapabilityId, EnforcementClass, NegotiatedCapability, PresenceSourceTier};

/// One declared Codex capability with its honest enforcement class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodexCapabilityEntry {
    pub id: CapabilityId,
    pub enforcement: EnforcementClass,
    pub notes: &'static str,
}

/// Full Codex sheet used for presence projection and conformance honesty.
///
/// Codex is an external CLI: privileged controls are at most
/// [`EnforcementClass::UpstreamRequestable`]. No `BackendEnforced` claims —
/// that requires a typed Seyal backend boundary (ADR-012 / SPEC-016).
pub struct CodexCapabilitySheet {
    pub presence_source: PresenceSourceTier,
    pub entries: &'static [CodexCapabilityEntry],
}

/// Canonical first-party Codex capability advertisement.
pub const CODEX_CAPABILITY_SHEET: CodexCapabilitySheet = CodexCapabilitySheet {
    // StructuredAdapter: App Server / `codex exec --json` control plane.
    // Materially different from Claude OfficialHooks / stream-JSON mapping.
    presence_source: PresenceSourceTier::StructuredAdapter,
    entries: &[
        CodexCapabilityEntry {
            id: CapabilityId::LifecycleObservation,
            enforcement: EnforcementClass::Observed,
            notes: "thread/turn lifecycle events via JSON / App Server",
        },
        CodexCapabilityEntry {
            id: CapabilityId::ActivityState,
            enforcement: EnforcementClass::Observed,
            notes: "observed turn progress only",
        },
        CodexCapabilityEntry {
            id: CapabilityId::Usage,
            enforcement: EnforcementClass::Observed,
            notes: "adapter-reported usage when present; unknown stays unknown",
        },
        CodexCapabilityEntry {
            id: CapabilityId::StructuredToolCalls,
            enforcement: EnforcementClass::Observed,
            notes: "JSON tool/event stream is data, not approval truth",
        },
        CodexCapabilityEntry {
            id: CapabilityId::Cancel,
            enforcement: EnforcementClass::UpstreamRequestable,
            notes: "turn/thread interrupt + StandaloneProcessHost signal_cancel",
        },
        CodexCapabilityEntry {
            id: CapabilityId::Resume,
            enforcement: EnforcementClass::UpstreamRequestable,
            notes: "Codex thread resume; token stored as HarnessSessionRef only",
        },
        CodexCapabilityEntry {
            id: CapabilityId::Fork,
            enforcement: EnforcementClass::UpstreamRequestable,
            notes: "Codex fork is upstream; new Seyal Attempt when work retries",
        },
        CodexCapabilityEntry {
            id: CapabilityId::RequestInputApproval,
            enforcement: EnforcementClass::UpstreamRequestable,
            notes: "external CLI never BackendEnforced (SPEC-016)",
        },
        CodexCapabilityEntry {
            id: CapabilityId::ModelSelect,
            enforcement: EnforcementClass::UpstreamRequestable,
            notes: "CLI/App Server model config request only",
        },
        CodexCapabilityEntry {
            id: CapabilityId::Approve,
            enforcement: EnforcementClass::UpstreamRequestable,
            notes: "must not claim local BackendEnforced approval authority",
        },
        CodexCapabilityEntry {
            id: CapabilityId::Deny,
            enforcement: EnforcementClass::UpstreamRequestable,
            notes: "must not claim local BackendEnforced deny authority",
        },
        CodexCapabilityEntry {
            id: CapabilityId::Pause,
            enforcement: EnforcementClass::UpstreamRequestable,
            notes: "upstream pause request; not typed-boundary LocalEnforcement",
        },
    ],
};

impl CodexCapabilitySheet {
    pub fn negotiated(&self) -> Vec<NegotiatedCapability> {
        self.entries
            .iter()
            .map(|entry| NegotiatedCapability::supported(entry.id, entry.enforcement))
            .collect()
    }

    pub fn claims_backend_enforced(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.enforcement == EnforcementClass::BackendEnforced)
    }

    pub fn entry(&self, id: CapabilityId) -> Option<&CodexCapabilityEntry> {
        self.entries.iter().find(|e| e.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use seyal_agent_core::PresenceSourceTier;

    #[test]
    fn sheet_never_overclaims_backend_enforced() {
        assert!(!CODEX_CAPABILITY_SHEET.claims_backend_enforced());
        assert_eq!(
            CODEX_CAPABILITY_SHEET.presence_source,
            PresenceSourceTier::StructuredAdapter
        );
        assert!(
            CODEX_CAPABILITY_SHEET.presence_source.max_enforcement()
                >= EnforcementClass::UpstreamRequestable
        );
        let approve = CODEX_CAPABILITY_SHEET
            .entry(CapabilityId::Approve)
            .expect("approve");
        assert_eq!(approve.enforcement, EnforcementClass::UpstreamRequestable);
    }

    #[test]
    fn negotiated_caps_round_trip_ids() {
        let caps = CODEX_CAPABILITY_SHEET.negotiated();
        assert!(!caps.is_empty());
        assert!(caps.iter().any(|c| c.id == CapabilityId::Cancel));
        assert!(caps.iter().any(|c| c.id == CapabilityId::Resume));
    }
}
