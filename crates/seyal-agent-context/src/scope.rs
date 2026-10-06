//! Discovery scope and authorized roots (SPEC-013 §§5–6, 10).

use std::path::{Path, PathBuf};

use seyal_agent_core::WorkScopeId;

use crate::source::SensitivityClass;

/// Stable repository identity for provenance fencing.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RepositoryId(pub String);

/// Stable worktree identity (or authorized non-VCS root identity).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct WorktreeId(pub String);

/// One authorized filesystem/git root for discovery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorizedRoot {
    pub path: PathBuf,
    pub repository_id: RepositoryId,
    pub worktree_id: WorktreeId,
    /// When true, this root is an explicitly authorized external symlink target.
    pub external_authorized: bool,
}

impl AuthorizedRoot {
    pub fn new(
        path: impl Into<PathBuf>,
        repository_id: impl Into<String>,
        worktree_id: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            repository_id: RepositoryId(repository_id.into()),
            worktree_id: WorktreeId(worktree_id.into()),
            external_authorized: false,
        }
    }

    pub fn with_external(mut self, authorized: bool) -> Self {
        self.external_authorized = authorized;
        self
    }
}

/// Explicit build/discovery scope. Consumes existing WorkScope identity; does
/// not invent a second WorkspaceStore.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveryScope {
    pub work_scope_id: WorkScopeId,
    pub roots: Vec<AuthorizedRoot>,
    pub policy_generation: u64,
    pub privacy_generation: u64,
    pub source_generation: u64,
    /// Maximum sensitivity allowed for automatic discovery.
    pub max_sensitivity: SensitivityClass,
    /// Paths explicitly authorized despite ignore rules (still sensitivity-filtered).
    pub authorized_ignored: Vec<PathBuf>,
    /// Absolute paths of normative-instruction locations (authorized policy only).
    pub normative_instruction_paths: Vec<PathBuf>,
}

impl DiscoveryScope {
    pub fn new(work_scope_id: WorkScopeId, roots: Vec<AuthorizedRoot>) -> Self {
        Self {
            work_scope_id,
            roots,
            policy_generation: 1,
            privacy_generation: 1,
            source_generation: 1,
            max_sensitivity: SensitivityClass::Internal,
            authorized_ignored: Vec::new(),
            normative_instruction_paths: Vec::new(),
        }
    }

    pub fn with_generations(mut self, policy: u64, privacy: u64, source: u64) -> Self {
        self.policy_generation = policy;
        self.privacy_generation = privacy;
        self.source_generation = source;
        self
    }

    pub fn finds_root_for(&self, path: &Path) -> Option<&AuthorizedRoot> {
        self.roots.iter().find(|root| path.starts_with(&root.path))
    }

    pub fn is_authorized_ignored(&self, path: &Path) -> bool {
        self.authorized_ignored.iter().any(|p| p == path)
    }

    pub fn is_normative_instruction_path(&self, path: &Path) -> bool {
        self.normative_instruction_paths.iter().any(|p| p == path)
    }
}
