//! Policy-safe `SelectionTrace` (SPEC-013 §16) — never a secret store.

use crate::item::ContextItemId;
use crate::source::{AuthorityClass, ExclusionReason, SensitivityClass};

/// Opaque trace identity bound to one bundle build.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SelectionTraceId(pub String);

/// Policy-safe reason codes (§16).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TraceReason {
    IncludedMandatoryAuthority,
    IncludedExactTaskPin,
    IncludedRelevantSource,
    ExcludedScope,
    ExcludedPermissionPolicy,
    ExcludedSensitivityPrivacy,
    ExcludedStaleGeneration,
    ExcludedDuplicateCoalesced,
    ExcludedLowerAuthorityConflict,
    ExcludedBudget,
    SourceUnavailable,
    ExcludedTraversalUnsafe,
    UnableToBuildMandatoryOverflow,
    SemanticFallback,
    SemanticRejectedReintroduce,
    RequiredContextMissing,
    ConflictSurfaced,
    LspOverlayDistinct,
    LspStaleGeneration,
    LspUnavailable,
}

impl TraceReason {
    pub fn from_exclusion(reason: ExclusionReason) -> Self {
        match reason {
            ExclusionReason::OutsideAuthorizedRoot
            | ExclusionReason::SiblingWorktreeLeak
            | ExclusionReason::NestedRepoBoundary => Self::ExcludedScope,
            ExclusionReason::PolicyDenied | ExclusionReason::IgnoredByDefault => {
                Self::ExcludedPermissionPolicy
            }
            ExclusionReason::SensitivityDenied => Self::ExcludedSensitivityPrivacy,
            ExclusionReason::GenerationStale | ExclusionReason::IntegrityMismatch => {
                Self::ExcludedStaleGeneration
            }
            ExclusionReason::SymlinkEscape
            | ExclusionReason::SymlinkCycle
            | ExclusionReason::TraversalBudgetExhausted
            | ExclusionReason::PathTraversal
            | ExclusionReason::MalformedIdentity
            | ExclusionReason::ExecutionForbidden => Self::ExcludedTraversalUnsafe,
            ExclusionReason::SourceUnavailable
            | ExclusionReason::Cancelled
            | ExclusionReason::Degraded => Self::SourceUnavailable,
            ExclusionReason::SelfClassifiedInstruction => Self::ExcludedPermissionPolicy,
        }
    }
}

/// One policy-safe candidate decision recorded in the trace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceEntry {
    /// Safe identifier — never a secret path/snippet when sensitivity requires redaction.
    pub candidate_id: String,
    pub included: bool,
    pub reason: TraceReason,
    pub authority: Option<AuthorityClass>,
    pub sensitivity: SensitivityClass,
    /// Never stores reconstructable secret payload.
    pub detail: Option<String>,
}

/// SelectionTrace explains inclusion/exclusion without secret retention.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionTrace {
    pub id: SelectionTraceId,
    pub bundle_id: Option<String>,
    pub entries: Vec<TraceEntry>,
    /// Most restrictive sensitivity across retained metadata (§23.27).
    pub effective_sensitivity: SensitivityClass,
    pub builder_version: String,
    pub selection_config_version: String,
}

impl SelectionTrace {
    pub fn new(builder_version: &str, selection_config_version: &str) -> Self {
        Self {
            id: SelectionTraceId(format!(
                "trace-{}",
                crate::digest::digest_bytes(
                    format!("{builder_version}|{selection_config_version}").as_bytes()
                )
                .hex()
            )),
            bundle_id: None,
            entries: Vec::new(),
            effective_sensitivity: SensitivityClass::Public,
            builder_version: builder_version.to_string(),
            selection_config_version: selection_config_version.to_string(),
        }
    }

    pub fn push(&mut self, entry: TraceEntry) {
        self.effective_sensitivity = self.effective_sensitivity.max(entry.sensitivity);
        self.entries.push(entry);
    }

    pub fn record_exclusion(
        &mut self,
        candidate_id: impl Into<String>,
        reason: TraceReason,
        sensitivity: SensitivityClass,
    ) {
        let candidate_id = candidate_id.into();
        let detail = if sensitivity >= SensitivityClass::Secret {
            None
        } else {
            Some(format!("{reason:?}"))
        };
        self.push(TraceEntry {
            candidate_id: if sensitivity >= SensitivityClass::Secret {
                redact_id(&candidate_id)
            } else {
                candidate_id
            },
            included: false,
            reason,
            authority: None,
            sensitivity,
            detail,
        });
    }

    pub fn record_inclusion(
        &mut self,
        item_id: &ContextItemId,
        reason: TraceReason,
        authority: AuthorityClass,
        sensitivity: SensitivityClass,
    ) {
        self.push(TraceEntry {
            candidate_id: if sensitivity >= SensitivityClass::Secret {
                redact_id(&item_id.0)
            } else {
                item_id.0.clone()
            },
            included: true,
            reason,
            authority: Some(authority),
            sensitivity,
            detail: None,
        });
    }

    /// True when no entry retains reconstructable secret payload bytes/paths.
    pub fn is_secret_safe(&self) -> bool {
        self.entries.iter().all(|e| {
            if e.sensitivity >= SensitivityClass::Secret {
                e.detail.is_none()
                    && !e.candidate_id.contains("secret")
                    && !e.candidate_id.contains(".env")
                    && !e.candidate_id.contains('/')
            } else {
                true
            }
        })
    }
}

fn redact_id(raw: &str) -> String {
    format!(
        "redacted:{}",
        crate::digest::digest_bytes(raw.as_bytes()).hex()
    )
}
