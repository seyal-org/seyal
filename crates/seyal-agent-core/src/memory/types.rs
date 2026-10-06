//! MemoryRecord domain enums (SPEC-012 §§4–7, §3.3).

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MemoryKind {
    Decision,
    EngineeringFact,
    FailurePattern,
    Procedure,
    EnvironmentFact,
    UserPreference,
    Heuristic,
}

impl MemoryKind {
    pub const fn code(self) -> u8 {
        match self {
            Self::Decision => 1,
            Self::EngineeringFact => 2,
            Self::FailurePattern => 3,
            Self::Procedure => 4,
            Self::EnvironmentFact => 5,
            Self::UserPreference => 6,
            Self::Heuristic => 7,
        }
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Decision),
            2 => Some(Self::EngineeringFact),
            3 => Some(Self::FailurePattern),
            4 => Some(Self::Procedure),
            5 => Some(Self::EnvironmentFact),
            6 => Some(Self::UserPreference),
            7 => Some(Self::Heuristic),
            _ => None,
        }
    }
}

/// MemoryRecord authority is capped at A2 (SPEC-012 §3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AuthorityClass {
    A2TypedObservation = 2,
    A3SourceClaim = 3,
    A4DerivedOrHeuristic = 4,
}

impl AuthorityClass {
    pub const fn code(self) -> u8 {
        match self {
            Self::A2TypedObservation => 2,
            Self::A3SourceClaim => 3,
            Self::A4DerivedOrHeuristic => 4,
        }
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            2 => Some(Self::A2TypedObservation),
            3 => Some(Self::A3SourceClaim),
            4 => Some(Self::A4DerivedOrHeuristic),
            _ => None,
        }
    }

    /// Clamp provenance that mirrors A0/A1 normative/source evidence into MemoryRecord cap.
    pub const fn clamp_for_memory(code: u8) -> Option<Self> {
        match code {
            0..=2 => Some(Self::A2TypedObservation),
            3 => Some(Self::A3SourceClaim),
            4 => Some(Self::A4DerivedOrHeuristic),
            _ => None,
        }
    }

    pub const fn is_at_most_as_strong(self, other: Self) -> bool {
        (self as u8) >= (other as u8)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Sensitivity {
    Public = 1,
    Internal = 2,
    Sensitive = 3,
    Restricted = 4,
}

impl Sensitivity {
    pub const fn code(self) -> u8 {
        self as u8
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Public),
            2 => Some(Self::Internal),
            3 => Some(Self::Sensitive),
            4 => Some(Self::Restricted),
            _ => None,
        }
    }

    pub const fn max(self, other: Self) -> Self {
        if (self as u8) >= (other as u8) {
            self
        } else {
            other
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MemoryState {
    Proposed,
    Accepted,
    Superseded,
    Revoked,
    Expired,
}

impl MemoryState {
    pub const fn code(self) -> u8 {
        match self {
            Self::Proposed => 1,
            Self::Accepted => 2,
            Self::Superseded => 3,
            Self::Revoked => 4,
            Self::Expired => 5,
        }
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Proposed),
            2 => Some(Self::Accepted),
            3 => Some(Self::Superseded),
            4 => Some(Self::Revoked),
            5 => Some(Self::Expired),
            _ => None,
        }
    }
}

/// Ordinary content-use mode. Safety maintenance remains available under Disabled/ReadOnly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MemoryMode {
    Disabled = 0,
    ReadOnly = 1,
    Curated = 2,
    Assisted = 3,
}

impl MemoryMode {
    pub const fn code(self) -> u8 {
        self as u8
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Disabled),
            1 => Some(Self::ReadOnly),
            2 => Some(Self::Curated),
            3 => Some(Self::Assisted),
            _ => None,
        }
    }

    /// Most restrictive mode wins (Disabled > ReadOnly > Curated > Assisted).
    pub fn compose(modes: impl IntoIterator<Item = Self>) -> Self {
        modes.into_iter().min().unwrap_or(Self::Disabled)
    }

    pub const fn allows_ordinary_read(self) -> bool {
        matches!(self, Self::ReadOnly | Self::Curated | Self::Assisted)
    }

    pub const fn allows_ordinary_write(self) -> bool {
        matches!(self, Self::Curated | Self::Assisted)
    }

    pub const fn allows_assisted_auto_accept(self) -> bool {
        matches!(self, Self::Assisted)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransitionReason {
    Accept,
    QualityReject,
    Forget,
    DoNotRemember,
    ProposalTtl,
    Expiry,
    Supersede,
    RevalidateRefresh,
    PrivacyRevocation,
    IntegrityQuarantine,
}

impl TransitionReason {
    pub const fn code(self) -> u8 {
        match self {
            Self::Accept => 1,
            Self::QualityReject => 2,
            Self::Forget => 3,
            Self::DoNotRemember => 4,
            Self::ProposalTtl => 5,
            Self::Expiry => 6,
            Self::Supersede => 7,
            Self::RevalidateRefresh => 8,
            Self::PrivacyRevocation => 9,
            Self::IntegrityQuarantine => 10,
        }
    }

    pub const fn is_forget_or_privacy(self) -> bool {
        matches!(
            self,
            Self::Forget | Self::DoNotRemember | Self::PrivacyRevocation
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ScopeKind {
    UserLocal = 1,
    Project = 2,
    Repository = 3,
    Workspace = 4,
    Worktree = 5,
    WorkItem = 6,
    Attempt = 7,
}

impl ScopeKind {
    pub const fn code(self) -> u8 {
        self as u8
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::UserLocal),
            2 => Some(Self::Project),
            3 => Some(Self::Repository),
            4 => Some(Self::Workspace),
            5 => Some(Self::Worktree),
            6 => Some(Self::WorkItem),
            7 => Some(Self::Attempt),
            _ => None,
        }
    }
}
