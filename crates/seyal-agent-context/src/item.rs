//! Provenance-bound `ContextItem` identity (SPEC-013 §4).

use std::path::PathBuf;

use crate::digest::IntegrityDigest;
use crate::provenance::{ObjectIdentity, SourceProvenance};
use crate::source::{AuthorityClass, SensitivityClass, SourceClass};

/// Stable identity for one materialized context contribution.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContextItemId(pub String);

impl ContextItemId {
    pub fn from_parts(
        repository_id: &str,
        worktree_id: &str,
        relative: &str,
        fingerprint_hex: &str,
        range: Option<(u64, u64)>,
    ) -> Self {
        let range_part = match range {
            Some((start, end)) => format!("@{start}:{end}"),
            None => String::new(),
        };
        Self(format!(
            "{repository_id}|{worktree_id}|{relative}|{fingerprint_hex}{range_part}"
        ))
    }
}

/// Byte/token range identity for chunked optional sources (SPEC-013 §15 / §23.25).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ContentRange {
    pub start_byte: u64,
    pub end_byte: u64,
}

/// Derived, versioned representation of one eligible source contribution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextItem {
    pub id: ContextItemId,
    pub source_class: SourceClass,
    pub authority: AuthorityClass,
    pub sensitivity: SensitivityClass,
    pub provenance: SourceProvenance,
    pub content_fingerprint: IntegrityDigest,
    pub estimated_tokens: u64,
    /// Selected payload bytes (policy-retained; redacted when undispatchable).
    pub payload: Vec<u8>,
    pub range: Option<ContentRange>,
    pub object_identity: Option<ObjectIdentity>,
    pub mandatory: bool,
    pub builder_version: String,
}

impl ContextItem {
    pub fn with_range(mut self, range: ContentRange) -> Self {
        let sliced = if (range.end_byte as usize) <= self.payload.len()
            && (range.start_byte as usize) <= self.payload.len()
        {
            self.payload[range.start_byte as usize..range.end_byte as usize].to_vec()
        } else {
            self.payload.clone()
        };
        self.estimated_tokens = estimate_tokens(&sliced);
        self.payload = sliced;
        self.range = Some(range);
        self.id = ContextItemId::from_parts(
            &self.provenance.repository_id.0,
            &self.provenance.worktree_id.0,
            &self.provenance.relative_path.to_string_lossy(),
            &self.content_fingerprint.hex(),
            Some((range.start_byte, range.end_byte)),
        );
        self
    }

    pub fn relative_display(&self) -> PathBuf {
        self.provenance.relative_path.clone()
    }
}

/// Rough consumer-facing token estimate (bytes/4, minimum 1 when non-empty).
pub fn estimate_tokens(bytes: &[u8]) -> u64 {
    if bytes.is_empty() {
        0
    } else {
        (bytes.len() as u64).div_ceil(4)
    }
}

/// Map source class to default authority (SPEC-013 §7) before relevance.
pub fn authority_for_source(class: SourceClass) -> AuthorityClass {
    match class {
        SourceClass::NormativeInstruction => AuthorityClass::NormativeInstruction,
        SourceClass::RepositoryFile | SourceClass::WorktreeFile | SourceClass::GitState => {
            AuthorityClass::RepositoryWorktreeTruth
        }
        SourceClass::UserPinnedContext => AuthorityClass::ExactTaskPin,
        SourceClass::ArtifactRef => AuthorityClass::RunEvidence,
        SourceClass::LspDocumentResult | SourceClass::SymbolOrIndexResult => {
            AuthorityClass::LexicalRelevance
        }
        SourceClass::TypedExternalSource => AuthorityClass::LexicalRelevance,
    }
}
