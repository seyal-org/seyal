//! ADR-012 §12 enforcement-qualified capabilities and SY-006 presence tiers.
//!
//! Product synonym: user-facing copy may say `SeyalEnforced` only when it maps
//! 1:1 to [`EnforcementClass::BackendEnforced`]. Wire and implementation use
//! the ADR-012 names exclusively.
//!
//! This module is the permanent production presence/capability enforcement
//! plane. It does not own AgentRun transitions (ADR-016) and never treats
//! terminal text or heuristics as approval/audit/`BackendEnforced` truth.

use std::fmt;

/// ADR-012 §12 enforcement class on a negotiated capability or presence fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum EnforcementClass {
    /// Seyal / Agent Backend can observe and report only.
    Observed = 1,
    /// Seyal can request the upstream behavior but cannot claim local enforcement.
    UpstreamRequestable = 2,
    /// Operation crosses a backend-owned typed authority boundary where policy
    /// is enforceable. Product synonym: SeyalEnforced.
    BackendEnforced = 3,
}

impl EnforcementClass {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Observed),
            2 => Some(Self::UpstreamRequestable),
            3 => Some(Self::BackendEnforced),
            _ => None,
        }
    }

    /// Wire/label name (ADR-012). Never emits the product synonym.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "Observed",
            Self::UpstreamRequestable => "UpstreamRequestable",
            Self::BackendEnforced => "BackendEnforced",
        }
    }
}

impl fmt::Display for EnforcementClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// SY-006 presence-source tiers (highest confidence first).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum PresenceSourceTier {
    StructuredAdapter = 1,
    OfficialHooks = 2,
    ProcessShellSignals = 3,
    LowConfidenceHeuristic = 4,
}

impl PresenceSourceTier {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::StructuredAdapter),
            2 => Some(Self::OfficialHooks),
            3 => Some(Self::ProcessShellSignals),
            4 => Some(Self::LowConfidenceHeuristic),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StructuredAdapter => "StructuredAdapter",
            Self::OfficialHooks => "OfficialHooks",
            Self::ProcessShellSignals => "ProcessShellSignals",
            Self::LowConfidenceHeuristic => "LowConfidenceHeuristic",
        }
    }

    /// Maximum enforcement class this source may ever carry without an illegal
    /// upgrade. `BackendEnforced` additionally requires a typed backend
    /// boundary (see [`PresenceObservation::validate`]).
    pub const fn max_enforcement(self) -> EnforcementClass {
        match self {
            Self::StructuredAdapter => EnforcementClass::BackendEnforced,
            Self::OfficialHooks => EnforcementClass::UpstreamRequestable,
            Self::ProcessShellSignals | Self::LowConfidenceHeuristic => EnforcementClass::Observed,
        }
    }

    /// True for process/shell or heuristic tiers that must stay non-authoritative
    /// for approval, audit, billing, and model-selection claims.
    pub const fn is_low_authority(self) -> bool {
        matches!(
            self,
            Self::ProcessShellSignals | Self::LowConfidenceHeuristic
        )
    }
}

impl fmt::Display for PresenceSourceTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Negotiated adapter capability identifiers (ADR-012 §12 representative set
/// plus privileged claim kinds that must never be implicit).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u16)]
pub enum CapabilityId {
    LifecycleObservation = 1,
    ActivityState = 2,
    Subagents = 3,
    Artifacts = 4,
    Usage = 5,
    StructuredToolCalls = 6,
    RequestInputApproval = 7,
    PromptDelivery = 8,
    Resume = 9,
    Cancel = 10,
    Fork = 11,
    ModelProviderConfig = 12,
    Pause = 13,
    Deny = 14,
    Approve = 15,
    Account = 16,
    ModelSelect = 17,
}

impl CapabilityId {
    pub const fn as_u16(self) -> u16 {
        self as u16
    }

    pub const fn from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(Self::LifecycleObservation),
            2 => Some(Self::ActivityState),
            3 => Some(Self::Subagents),
            4 => Some(Self::Artifacts),
            5 => Some(Self::Usage),
            6 => Some(Self::StructuredToolCalls),
            7 => Some(Self::RequestInputApproval),
            8 => Some(Self::PromptDelivery),
            9 => Some(Self::Resume),
            10 => Some(Self::Cancel),
            11 => Some(Self::Fork),
            12 => Some(Self::ModelProviderConfig),
            13 => Some(Self::Pause),
            14 => Some(Self::Deny),
            15 => Some(Self::Approve),
            16 => Some(Self::Account),
            17 => Some(Self::ModelSelect),
            _ => None,
        }
    }

    /// Privileged claims that require `BackendEnforced` for local enforcement
    /// (or may be `UpstreamRequestable` only as a request, never as local
    /// enforcement).
    pub const fn is_privileged_control(self) -> bool {
        matches!(
            self,
            Self::Pause
                | Self::Deny
                | Self::Approve
                | Self::Account
                | Self::ModelSelect
                | Self::RequestInputApproval
                | Self::ModelProviderConfig
                | Self::Cancel
        )
    }
}

/// Explicit support state — unsupported/unknown stay explicit (SPEC-018 §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CapabilitySupport {
    Unsupported,
    Unknown,
    Supported { enforcement: EnforcementClass },
}

impl CapabilitySupport {
    pub const fn enforcement(self) -> Option<EnforcementClass> {
        match self {
            Self::Supported { enforcement } => Some(enforcement),
            Self::Unsupported | Self::Unknown => None,
        }
    }

    pub const fn as_u8(self) -> u8 {
        match self {
            Self::Unsupported => 0,
            Self::Unknown => 1,
            Self::Supported { enforcement } => match enforcement {
                EnforcementClass::Observed => 2,
                EnforcementClass::UpstreamRequestable => 3,
                EnforcementClass::BackendEnforced => 4,
            },
        }
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Unsupported),
            1 => Some(Self::Unknown),
            2 => Some(Self::Supported {
                enforcement: EnforcementClass::Observed,
            }),
            3 => Some(Self::Supported {
                enforcement: EnforcementClass::UpstreamRequestable,
            }),
            4 => Some(Self::Supported {
                enforcement: EnforcementClass::BackendEnforced,
            }),
            _ => None,
        }
    }
}

/// One negotiated capability with an explicit enforcement class (or explicit
/// unsupported/unknown). There is no implicit “full control”.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NegotiatedCapability {
    pub id: CapabilityId,
    pub support: CapabilitySupport,
}

impl NegotiatedCapability {
    pub const fn supported(id: CapabilityId, enforcement: EnforcementClass) -> Self {
        Self {
            id,
            support: CapabilitySupport::Supported { enforcement },
        }
    }

    pub const fn unsupported(id: CapabilityId) -> Self {
        Self {
            id,
            support: CapabilitySupport::Unsupported,
        }
    }

    pub const fn unknown(id: CapabilityId) -> Self {
        Self {
            id,
            support: CapabilitySupport::Unknown,
        }
    }

    pub const fn enforcement(self) -> Option<EnforcementClass> {
        self.support.enforcement()
    }
}

/// How a client/backend intends to use a privileged capability.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClaimMode {
    /// Observe/report only.
    Observe,
    /// Request upstream behavior without claiming local enforcement.
    UpstreamRequest,
    /// Claim pause/deny/approve/account/model-select as locally enforced.
    LocalEnforcement,
}

/// Fail-closed presence / enforcement errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresenceError {
    /// Heuristic/process/hooks source cannot carry the requested class.
    IllegalEnforcementForSource {
        source: PresenceSourceTier,
        requested: EnforcementClass,
        max: EnforcementClass,
    },
    /// `BackendEnforced` without a Seyal-owned typed authority boundary.
    BackendEnforcedRequiresTypedBoundary,
    /// Attempted to raise enforcement class without new authoritative evidence.
    IllegalEnforcementUpgrade {
        from: EnforcementClass,
        to: EnforcementClass,
    },
    /// Capability is unsupported or unknown.
    CapabilityNotSupported { id: CapabilityId },
    /// Observed-only (or weaker) cannot satisfy the claim mode.
    EnforcementInsufficient {
        id: CapabilityId,
        have: EnforcementClass,
        mode: ClaimMode,
    },
    /// Terminal text / heuristic evidence cannot authorize privileged claims.
    TerminalTextNotAuthority,
    /// External CLI effect observed only via heuristic/text cannot be
    /// `BackendEnforced` Action evidence (SPEC-016 / ADR-014).
    ExternalCliEffectNotBackendEnforced,
}

/// SY-006 presence observation: source tier + enforcement class + typed-boundary flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PresenceObservation {
    pub source: PresenceSourceTier,
    pub enforcement: EnforcementClass,
    /// True only when the observation/operation crosses a Seyal-owned typed
    /// authority boundary (Agent Backend dispatch / Action fence). Required
    /// for [`EnforcementClass::BackendEnforced`].
    pub typed_backend_boundary: bool,
}

impl PresenceObservation {
    pub fn new(
        source: PresenceSourceTier,
        enforcement: EnforcementClass,
        typed_backend_boundary: bool,
    ) -> Result<Self, PresenceError> {
        let obs = Self {
            source,
            enforcement,
            typed_backend_boundary,
        };
        obs.validate()?;
        Ok(obs)
    }

    /// Fail-closed validation of source ↔ enforcement ↔ typed-boundary.
    pub fn validate(self) -> Result<(), PresenceError> {
        let max = self.source.max_enforcement();
        if self.enforcement > max {
            return Err(PresenceError::IllegalEnforcementForSource {
                source: self.source,
                requested: self.enforcement,
                max,
            });
        }
        if self.enforcement == EnforcementClass::BackendEnforced && !self.typed_backend_boundary {
            return Err(PresenceError::BackendEnforcedRequiresTypedBoundary);
        }
        if self.source.is_low_authority() && self.enforcement == EnforcementClass::BackendEnforced {
            return Err(PresenceError::IllegalEnforcementForSource {
                source: self.source,
                requested: self.enforcement,
                max: EnforcementClass::Observed,
            });
        }
        Ok(())
    }

    /// Reject illegal self-upgrade of an existing observation's class.
    pub fn reclassify(self, to: EnforcementClass) -> Result<Self, PresenceError> {
        if to > self.enforcement {
            return Err(PresenceError::IllegalEnforcementUpgrade {
                from: self.enforcement,
                to,
            });
        }
        Self::new(self.source, to, self.typed_backend_boundary)
    }
}

/// Projection surface carrying negotiated capabilities + latest presence
/// observation. Not a second AgentRun authority — clients read this; only the
/// Agent Backend writes AgentRun transitions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PresenceCapabilityProjection {
    pub capabilities: Vec<NegotiatedCapability>,
    pub presence: Option<PresenceObservation>,
}

impl PresenceCapabilityProjection {
    pub fn new(capabilities: Vec<NegotiatedCapability>) -> Self {
        Self {
            capabilities,
            presence: None,
        }
    }

    pub fn capability(&self, id: CapabilityId) -> Option<NegotiatedCapability> {
        self.capabilities.iter().copied().find(|c| c.id == id)
    }

    pub fn record_presence(
        &mut self,
        observation: PresenceObservation,
    ) -> Result<(), PresenceError> {
        observation.validate()?;
        self.presence = Some(observation);
        Ok(())
    }

    /// Authorize a privileged claim against the negotiated capability set.
    ///
    /// - `Observe` requires at least `Observed` support.
    /// - `UpstreamRequest` requires `UpstreamRequestable` or `BackendEnforced`.
    /// - `LocalEnforcement` requires `BackendEnforced` only.
    ///
    /// Low-authority presence (heuristic / process signals) and raw terminal
    /// text never authorize privileged local claims.
    pub fn authorize_claim(
        &self,
        id: CapabilityId,
        mode: ClaimMode,
    ) -> Result<EnforcementClass, PresenceError> {
        if matches!(
            mode,
            ClaimMode::LocalEnforcement | ClaimMode::UpstreamRequest
        ) && let Some(presence) = self.presence
            && presence.source.is_low_authority()
        {
            return Err(PresenceError::TerminalTextNotAuthority);
        }

        let Some(cap) = self.capability(id) else {
            return Err(PresenceError::CapabilityNotSupported { id });
        };
        let Some(have) = cap.enforcement() else {
            return Err(PresenceError::CapabilityNotSupported { id });
        };

        match mode {
            ClaimMode::Observe => {
                if have >= EnforcementClass::Observed {
                    Ok(have)
                } else {
                    Err(PresenceError::EnforcementInsufficient { id, have, mode })
                }
            }
            ClaimMode::UpstreamRequest => {
                if have >= EnforcementClass::UpstreamRequestable {
                    Ok(have)
                } else {
                    Err(PresenceError::EnforcementInsufficient { id, have, mode })
                }
            }
            ClaimMode::LocalEnforcement => {
                if have == EnforcementClass::BackendEnforced {
                    Ok(have)
                } else {
                    Err(PresenceError::EnforcementInsufficient { id, have, mode })
                }
            }
        }
    }
}

/// SPEC-016 / ADR-014: an external CLI effect observed only via terminal text
/// or heuristic evidence must never be labeled `BackendEnforced` Action evidence.
pub fn classify_external_cli_effect_evidence(
    source: PresenceSourceTier,
    typed_backend_boundary: bool,
) -> Result<EnforcementClass, PresenceError> {
    if source.is_low_authority() || !typed_backend_boundary {
        return Err(PresenceError::ExternalCliEffectNotBackendEnforced);
    }
    // Structured adapter / official hooks without a typed boundary still fail
    // above; with typed boundary, structured adapter may yield BackendEnforced.
    match source {
        PresenceSourceTier::StructuredAdapter if typed_backend_boundary => {
            Ok(EnforcementClass::BackendEnforced)
        }
        PresenceSourceTier::OfficialHooks => Ok(EnforcementClass::UpstreamRequestable),
        PresenceSourceTier::StructuredAdapter => {
            Err(PresenceError::BackendEnforcedRequiresTypedBoundary)
        }
        PresenceSourceTier::ProcessShellSignals | PresenceSourceTier::LowConfidenceHeuristic => {
            Err(PresenceError::ExternalCliEffectNotBackendEnforced)
        }
    }
}

/// Raw terminal text is never approval/control/audit truth (ADR-012 §13).
pub fn terminal_text_authorizes_approval(_text: &str) -> Result<(), PresenceError> {
    Err(PresenceError::TerminalTextNotAuthority)
}

#[cfg(test)]
#[path = "presence_tests.rs"]
mod tests;
