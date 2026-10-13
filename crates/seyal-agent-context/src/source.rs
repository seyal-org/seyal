//! Typed SPEC-013 §3 source classes and authority/sensitivity dimensions.

/// Canonical Local Context Engine source classes (SPEC-013 §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SourceClass {
    NormativeInstruction,
    RepositoryFile,
    WorktreeFile,
    GitState,
    UserPinnedContext,
    ArtifactRef,
    LspDocumentResult,
    SymbolOrIndexResult,
    TypedExternalSource,
}

impl SourceClass {
    pub const fn code(self) -> u8 {
        match self {
            Self::NormativeInstruction => 1,
            Self::RepositoryFile => 2,
            Self::WorktreeFile => 3,
            Self::GitState => 4,
            Self::UserPinnedContext => 5,
            Self::ArtifactRef => 6,
            Self::LspDocumentResult => 7,
            Self::SymbolOrIndexResult => 8,
            Self::TypedExternalSource => 9,
        }
    }
}

/// Authority class used before relevance ranking (SPEC-013 §7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AuthorityClass {
    SecurityCapability = 1,
    NormativeInstruction = 2,
    RepositoryWorktreeTruth = 3,
    ExactTaskPin = 4,
    DurableMemory = 5,
    RunEvidence = 6,
    LexicalRelevance = 7,
    OptionalSemantic = 8,
}

/// Sensitivity class for policy filtering (deny-by-default).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SensitivityClass {
    Public = 1,
    Internal = 2,
    Secret = 3,
    Excluded = 4,
}

/// Tracked / untracked / ignored provenance status (SPEC-013 §10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VcsMembership {
    Tracked,
    Untracked,
    Ignored,
    Unknown,
}

/// Why a candidate was excluded before ranking.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExclusionReason {
    OutsideAuthorizedRoot,
    SiblingWorktreeLeak,
    IgnoredByDefault,
    SensitivityDenied,
    PolicyDenied,
    SymlinkEscape,
    SymlinkCycle,
    TraversalBudgetExhausted,
    PathTraversal,
    MalformedIdentity,
    NestedRepoBoundary,
    GenerationStale,
    IntegrityMismatch,
    ExecutionForbidden,
    SelfClassifiedInstruction,
    Cancelled,
    Degraded,
    SourceUnavailable,
}

/// Discovery/index operational health.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiscoveryHealth {
    Ok,
    Degraded,
    Cancelled,
}
