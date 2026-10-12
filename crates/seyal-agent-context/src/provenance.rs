//! Filesystem/repository provenance and path-identity validation (SPEC-013 §10).

use std::path::{Component, Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

use crate::digest::IntegrityDigest;
use crate::scope::{RepositoryId, WorktreeId};
use crate::source::{ExclusionReason, SourceClass, VcsMembership};

/// Stable filesystem object identity when the platform provides one safely.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ObjectIdentity {
    pub device: u64,
    pub inode: u64,
}

impl ObjectIdentity {
    pub fn from_meta(meta: &std::fs::Metadata) -> Option<Self> {
        #[cfg(unix)]
        {
            Some(Self {
                device: meta.dev(),
                inode: meta.ino(),
            })
        }
        #[cfg(not(unix))]
        {
            let _ = meta;
            None
        }
    }
}

/// Provenance recorded for every discovered source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceProvenance {
    pub repository_id: RepositoryId,
    pub worktree_id: WorktreeId,
    pub authorized_root: PathBuf,
    pub relative_path: PathBuf,
    pub absolute_path: PathBuf,
    pub membership: VcsMembership,
    pub object_identity: Option<ObjectIdentity>,
    pub content_fingerprint: Option<IntegrityDigest>,
    pub symlink_link_path: Option<PathBuf>,
    pub symlink_target_path: Option<PathBuf>,
    pub nested_repository_id: Option<RepositoryId>,
    pub submodule_revision: Option<String>,
    pub submodule_dirty: Option<bool>,
    pub source_generation: u64,
    pub policy_generation: u64,
    pub privacy_generation: u64,
}

/// One discovery result before ranking (eligible or explicitly excluded).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredSource {
    pub source_class: SourceClass,
    pub provenance: SourceProvenance,
    pub exclusion: Option<ExclusionReason>,
}

/// Reject `..`, absolute escape, NUL, and empty path components (SPEC-013 §23.35).
pub fn validate_relative_identity(relative: &Path) -> Result<(), ExclusionReason> {
    if relative.as_os_str().is_empty() {
        return Err(ExclusionReason::MalformedIdentity);
    }
    let raw = relative.to_string_lossy();
    if raw.contains('\0') || raw.contains('\\') {
        return Err(ExclusionReason::PathTraversal);
    }
    for component in relative.components() {
        match component {
            Component::Normal(part) => {
                let s = part.to_string_lossy();
                if s.is_empty() || s == "." || s.contains('\0') {
                    return Err(ExclusionReason::MalformedIdentity);
                }
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(ExclusionReason::PathTraversal);
            }
        }
    }
    Ok(())
}

/// Ensure `candidate` is inside `root` after lexical join (no `..` escape).
pub fn join_under_root(root: &Path, relative: &Path) -> Result<PathBuf, ExclusionReason> {
    validate_relative_identity(relative)?;
    let mut normalized = if root.is_absolute() {
        PathBuf::from(Component::RootDir.as_os_str())
    } else {
        PathBuf::new()
    };
    for component in root.components() {
        match component {
            Component::RootDir | Component::Prefix(_) => {}
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(ExclusionReason::PathTraversal);
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    let root_norm = if root.is_absolute() && !normalized.is_absolute() {
        Path::new("/").join(&normalized)
    } else {
        normalized
    };
    let mut joined = root_norm.clone();
    for component in relative.components() {
        match component {
            Component::RootDir | Component::Prefix(_) | Component::ParentDir => {
                return Err(ExclusionReason::PathTraversal);
            }
            Component::CurDir => {}
            Component::Normal(part) => joined.push(part),
        }
    }
    if !joined.starts_with(&root_norm) && !path_is_under(root, &root.join(relative)) {
        return Err(ExclusionReason::PathTraversal);
    }
    Ok(root.join(relative))
}

fn path_is_under(root: &Path, candidate: &Path) -> bool {
    let mut root_components = root.components();
    let mut cand = candidate.components();
    loop {
        match (root_components.next(), cand.next()) {
            (None, _) => return true,
            (Some(_), None) => return false,
            (Some(a), Some(b)) if a == b => {}
            _ => return false,
        }
    }
}

/// Resolve symlink chain with hop bound; returns final path + hops used.
pub fn resolve_symlink_chain(
    start: &Path,
    max_hops: u32,
) -> Result<(PathBuf, u32), ExclusionReason> {
    let mut current = start.to_path_buf();
    let mut hops = 0u32;
    loop {
        let meta =
            std::fs::symlink_metadata(&current).map_err(|_| ExclusionReason::SourceUnavailable)?;
        if !meta.file_type().is_symlink() {
            return Ok((current, hops));
        }
        hops = hops.saturating_add(1);
        if hops > max_hops {
            return Err(ExclusionReason::TraversalBudgetExhausted);
        }
        let target =
            std::fs::read_link(&current).map_err(|_| ExclusionReason::SourceUnavailable)?;
        current = if target.is_absolute() {
            target
        } else {
            current
                .parent()
                .unwrap_or_else(|| Path::new("/"))
                .join(target)
        };
    }
}
