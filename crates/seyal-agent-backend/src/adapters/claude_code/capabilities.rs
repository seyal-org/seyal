//! Claude Code capability sheet — honest ADR-012 enforcement classes.
//!
//! External Claude Code CLI never claims [`EnforcementClass::BackendEnforced`].
//! Privileged approve/deny/account/model-select are Unsupported or at most
//! UpstreamRequestable; heuristics/terminal text never become approval truth.

use seyal_agent_core::{
    CapabilityId, CapabilitySupport, EnforcementClass, NegotiatedCapability, PresenceError,
    PresenceObservation, PresenceSourceTier,
};

/// One documented capability row (Developer Guide sheet).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClaudeCodeCapabilityEntry {
    pub id: CapabilityId,
    pub support: CapabilitySupport,
    pub rationale: &'static str,
}

/// Pipe-safe stream-json / official hooks presence for Claude Code.
///
/// Max enforcement for OfficialHooks is UpstreamRequestable (SY-006); typed
/// BackendEnforced local enforcement is impossible on this plane without a
/// Seyal-owned boundary Claude Code does not provide.
pub fn claude_code_presence_observation() -> Result<PresenceObservation, PresenceError> {
    PresenceObservation::new(
        PresenceSourceTier::OfficialHooks,
        EnforcementClass::UpstreamRequestable,
        false,
    )
}

/// Negotiated capability list installed into the hardened presence plane.
pub fn claude_code_capability_sheet() -> Vec<NegotiatedCapability> {
    claude_code_capability_entries()
        .iter()
        .map(|entry| NegotiatedCapability {
            id: entry.id,
            support: entry.support,
        })
        .collect()
}

/// Full documented sheet (including rationales) for docs + honesty tests.
pub fn claude_code_capability_entries() -> &'static [ClaudeCodeCapabilityEntry] {
    &[
        ClaudeCodeCapabilityEntry {
            id: CapabilityId::LifecycleObservation,
            support: CapabilitySupport::Supported {
                enforcement: EnforcementClass::Observed,
            },
            rationale: "stream-json / process lifecycle is observe-only evidence",
        },
        ClaudeCodeCapabilityEntry {
            id: CapabilityId::ActivityState,
            support: CapabilitySupport::Supported {
                enforcement: EnforcementClass::Observed,
            },
            rationale: "structured progress events when stream-json is available",
        },
        ClaudeCodeCapabilityEntry {
            id: CapabilityId::Cancel,
            support: CapabilitySupport::Supported {
                enforcement: EnforcementClass::UpstreamRequestable,
            },
            rationale: "supervised host cancel/signal; not a Seyal typed-boundary enforce",
        },
        ClaudeCodeCapabilityEntry {
            id: CapabilityId::Resume,
            support: CapabilitySupport::Supported {
                enforcement: EnforcementClass::UpstreamRequestable,
            },
            rationale: "CLI --resume is an upstream request, not BackendEnforced",
        },
        ClaudeCodeCapabilityEntry {
            id: CapabilityId::PromptDelivery,
            support: CapabilitySupport::Supported {
                enforcement: EnforcementClass::UpstreamRequestable,
            },
            rationale: "non-interactive -p delivery is upstream-controlled",
        },
        ClaudeCodeCapabilityEntry {
            id: CapabilityId::StructuredToolCalls,
            support: CapabilitySupport::Supported {
                enforcement: EnforcementClass::Observed,
            },
            rationale: "tool events observed via structured channel when present",
        },
        ClaudeCodeCapabilityEntry {
            id: CapabilityId::RequestInputApproval,
            support: CapabilitySupport::Supported {
                enforcement: EnforcementClass::UpstreamRequestable,
            },
            rationale:
                "permission hooks are upstream requests; terminal text is never approval truth",
        },
        ClaudeCodeCapabilityEntry {
            id: CapabilityId::Usage,
            support: CapabilitySupport::Supported {
                enforcement: EnforcementClass::Observed,
            },
            rationale: "usage/telemetry when exposed by structured result metadata",
        },
        ClaudeCodeCapabilityEntry {
            id: CapabilityId::Artifacts,
            support: CapabilitySupport::Supported {
                enforcement: EnforcementClass::Observed,
            },
            rationale: "derive observe-only artifact/diff signals from structured output",
        },
        ClaudeCodeCapabilityEntry {
            id: CapabilityId::Subagents,
            support: CapabilitySupport::Unknown,
            rationale: "subagent surface is version-sensitive; do not over-claim",
        },
        ClaudeCodeCapabilityEntry {
            id: CapabilityId::Fork,
            support: CapabilitySupport::Unsupported,
            rationale: "no first-party fork control in M005 Claude slice",
        },
        ClaudeCodeCapabilityEntry {
            id: CapabilityId::ModelProviderConfig,
            support: CapabilitySupport::Supported {
                enforcement: EnforcementClass::UpstreamRequestable,
            },
            rationale: "CLI/config model selection is upstream-requestable only",
        },
        ClaudeCodeCapabilityEntry {
            id: CapabilityId::ModelSelect,
            support: CapabilitySupport::Supported {
                enforcement: EnforcementClass::UpstreamRequestable,
            },
            rationale: "never LocalEnforcement / BackendEnforced for external CLI",
        },
        ClaudeCodeCapabilityEntry {
            id: CapabilityId::Pause,
            support: CapabilitySupport::Unsupported,
            rationale: "no honest BackendEnforced pause boundary on Claude Code",
        },
        ClaudeCodeCapabilityEntry {
            id: CapabilityId::Deny,
            support: CapabilitySupport::Unsupported,
            rationale: "deny is not a Seyal-enforced Claude Code capability",
        },
        ClaudeCodeCapabilityEntry {
            id: CapabilityId::Approve,
            support: CapabilitySupport::Unsupported,
            rationale: "approvals never from terminal text; no BackendEnforced approve path",
        },
        ClaudeCodeCapabilityEntry {
            id: CapabilityId::Account,
            support: CapabilitySupport::Unsupported,
            rationale: "account/billing is outside Seyal typed boundary for Claude Code",
        },
    ]
}

/// Fail closed if the sheet ever claims BackendEnforced (external CLI invariant).
pub fn validate_claude_code_sheet(caps: &[NegotiatedCapability]) -> Result<(), String> {
    for cap in caps {
        if let CapabilitySupport::Supported {
            enforcement: EnforcementClass::BackendEnforced,
        } = cap.support
        {
            return Err(format!(
                "Claude Code must not claim BackendEnforced for {:?}",
                cap.id
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use seyal_agent_core::{ClaimMode, PresenceCapabilityProjection};

    #[test]
    fn sheet_never_claims_backend_enforced() {
        let caps = claude_code_capability_sheet();
        validate_claude_code_sheet(&caps).expect("honest sheet");
        assert!(caps
            .iter()
            .any(|c| c.id == CapabilityId::LifecycleObservation));
        assert!(caps.iter().any(|c| {
            c.id == CapabilityId::Approve && matches!(c.support, CapabilitySupport::Unsupported)
        }));
    }

    #[test]
    fn presence_plane_rejects_local_enforcement_for_claude_caps() {
        let mut projection = PresenceCapabilityProjection::new(claude_code_capability_sheet());
        projection
            .record_presence(claude_code_presence_observation().expect("presence"))
            .expect("record");
        // UpstreamRequestable cancel may authorize UpstreamRequest.
        assert!(projection
            .authorize_claim(CapabilityId::Cancel, ClaimMode::UpstreamRequest)
            .is_ok());
        // LocalEnforcement must fail closed — no typed BackendEnforced boundary.
        assert!(projection
            .authorize_claim(CapabilityId::Cancel, ClaimMode::LocalEnforcement)
            .is_err());
        assert!(projection
            .authorize_claim(CapabilityId::ModelSelect, ClaimMode::LocalEnforcement)
            .is_err());
        assert!(projection
            .authorize_claim(CapabilityId::Approve, ClaimMode::Observe)
            .is_err());
    }

    #[test]
    fn heuristic_presence_cannot_upgrade_to_backend_enforced() {
        let err = PresenceObservation::new(
            PresenceSourceTier::LowConfidenceHeuristic,
            EnforcementClass::BackendEnforced,
            false,
        );
        assert!(err.is_err());
    }
}
