//! Bounded authorized filesystem discovery (SPEC-013 §§10, 18–23).

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use crate::budget::DiscoveryBudget;
use crate::caps::{
    MAX_SYMLINK_HOPS, MAX_TRAVERSAL_DEPTH, MAX_TRAVERSAL_ENTRIES, MAX_VISITED_IDENTITIES,
};
use crate::digest::{digest_bytes, digest_file};
use crate::git::{capture_git_snapshot, is_git_repository, is_nested_git_root, GitSnapshot};
use crate::ignore::IgnoreRules;
use crate::policy::apply_eligibility;
use crate::provenance::{
    join_under_root, resolve_symlink_chain, DiscoveredSource, ObjectIdentity, SourceProvenance,
};
use crate::scope::{AuthorizedRoot, DiscoveryScope, RepositoryId};
use crate::source::{DiscoveryHealth, ExclusionReason, SourceClass, VcsMembership};

#[derive(Clone, Debug)]
pub struct DiscoveryReport {
    pub sources: Vec<DiscoveredSource>,
    pub health: DiscoveryHealth,
    pub entries_visited: u64,
    pub visited_identities: u64,
    pub max_depth_seen: u32,
}

struct WalkState<'a> {
    scope: &'a DiscoveryScope,
    root: &'a AuthorizedRoot,
    ignore: IgnoreRules,
    git: GitSnapshot,
    budget: &'a DiscoveryBudget,
    visited: HashSet<ObjectIdentity>,
    lineage: HashSet<ObjectIdentity>,
    entries: u64,
    max_depth_seen: u32,
    sources: Vec<DiscoveredSource>,
    health: DiscoveryHealth,
}

/// Discover authorized sources under `scope` without executing content.
pub fn discover(scope: &DiscoveryScope, budget: &DiscoveryBudget) -> DiscoveryReport {
    let mut all = Vec::new();
    let mut health = DiscoveryHealth::Ok;
    let mut entries = 0u64;
    let mut visited_ids = 0u64;
    let mut max_depth = 0u32;

    for root in &scope.roots {
        if budget.is_cancelled() {
            health = DiscoveryHealth::Cancelled;
            break;
        }
        let ignore = IgnoreRules::load_from_repo(&root.path);
        let git = capture_git_snapshot(&root.path).unwrap_or_default();
        let mut state = WalkState {
            scope,
            root,
            ignore,
            git,
            budget,
            visited: HashSet::new(),
            lineage: HashSet::new(),
            entries: 0,
            max_depth_seen: 0,
            sources: Vec::new(),
            health: DiscoveryHealth::Ok,
        };
        // Enumerate the root itself as GitState when it is a repository.
        if is_git_repository(&root.path) {
            let git_state = make_source(
                &state,
                Path::new(""),
                &root.path,
                SourceClass::GitState,
                VcsMembership::Tracked,
                None,
                None,
                None,
            );
            state.sources.push(apply_eligibility(scope, git_state));
        }
        walk_dir(&mut state, &root.path, Path::new(""), 0);
        entries = entries.saturating_add(state.entries);
        visited_ids = visited_ids.saturating_add(state.visited.len() as u64);
        max_depth = max_depth.max(state.max_depth_seen);
        if state.health != DiscoveryHealth::Ok {
            health = state.health;
        }
        all.extend(state.sources);
    }

    // Deterministic order independent of directory enumeration (SPEC-013 §8).
    all.sort_by(|a, b| {
        (
            a.provenance.repository_id.0.as_str(),
            a.provenance.worktree_id.0.as_str(),
            a.provenance.relative_path.as_os_str(),
            a.source_class.code(),
        )
            .cmp(&(
                b.provenance.repository_id.0.as_str(),
                b.provenance.worktree_id.0.as_str(),
                b.provenance.relative_path.as_os_str(),
                b.source_class.code(),
            ))
    });

    DiscoveryReport {
        sources: all,
        health,
        entries_visited: entries,
        visited_identities: visited_ids,
        max_depth_seen: max_depth,
    }
}

fn walk_dir(state: &mut WalkState<'_>, abs: &Path, rel: &Path, depth: u32) {
    if state.budget.is_cancelled() {
        state.health = DiscoveryHealth::Cancelled;
        return;
    }
    if depth > MAX_TRAVERSAL_DEPTH {
        push_excluded(state, rel, abs, ExclusionReason::TraversalBudgetExhausted);
        return;
    }
    state.max_depth_seen = state.max_depth_seen.max(depth);

    let read_dir = match std::fs::read_dir(abs) {
        Ok(rd) => rd,
        Err(_) => {
            push_excluded(state, rel, abs, ExclusionReason::SourceUnavailable);
            return;
        }
    };

    // Sort entries for deterministic walk independent of FS readdir order.
    let mut entries: Vec<_> = read_dir.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        if state.budget.is_cancelled() {
            state.health = DiscoveryHealth::Cancelled;
            return;
        }
        if state.entries >= MAX_TRAVERSAL_ENTRIES {
            push_excluded(state, rel, abs, ExclusionReason::TraversalBudgetExhausted);
            state.health = DiscoveryHealth::Degraded;
            return;
        }
        state.entries = state.entries.saturating_add(1);

        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let child_abs = entry.path();
        let child_rel = if rel.as_os_str().is_empty() {
            PathBuf::from(&name)
        } else {
            rel.join(&name)
        };

        if let Err(reason) = join_under_root(&state.root.path, &child_rel) {
            push_excluded(state, &child_rel, &child_abs, reason);
            continue;
        }

        let meta = match std::fs::symlink_metadata(&child_abs) {
            Ok(m) => m,
            Err(_) => {
                push_excluded(
                    state,
                    &child_rel,
                    &child_abs,
                    ExclusionReason::SourceUnavailable,
                );
                continue;
            }
        };

        if meta.file_type().is_symlink() {
            handle_symlink(state, &child_abs, &child_rel, depth);
            continue;
        }

        if let Some(oid) = ObjectIdentity::from_meta(&meta) {
            if state.visited.len() as u64 >= MAX_VISITED_IDENTITIES {
                push_excluded(
                    state,
                    &child_rel,
                    &child_abs,
                    ExclusionReason::TraversalBudgetExhausted,
                );
                state.health = DiscoveryHealth::Degraded;
                return;
            }
            if !state.visited.insert(oid) {
                // Already visited as an object — do not re-enumerate.
                continue;
            }
        }

        if meta.is_dir() {
            // Nested independent repository: record distinct identity, do not flatten.
            if is_nested_git_root(&child_abs, &state.root.path) {
                let nested_id = RepositoryId(format!("nested:{}", child_rel.display()));
                let mut source = make_source(
                    state,
                    &child_rel,
                    &child_abs,
                    SourceClass::RepositoryFile,
                    VcsMembership::Tracked,
                    None,
                    None,
                    Some(nested_id.clone()),
                );
                source.provenance.nested_repository_id = Some(nested_id);
                // Capture nested HEAD as distinct revision authority.
                if let Ok(nested_git) = capture_git_snapshot(&child_abs) {
                    source.provenance.submodule_revision = nested_git.head;
                }
                state.sources.push(apply_eligibility(state.scope, source));
                continue;
            }

            let is_ignored = state.ignore.is_ignored(&child_abs, true)
                || state.git.membership_of(&child_rel) == VcsMembership::Ignored;
            if is_ignored && !state.scope.is_authorized_ignored(&child_abs) {
                let source = make_source(
                    state,
                    &child_rel,
                    &child_abs,
                    SourceClass::WorktreeFile,
                    VcsMembership::Ignored,
                    None,
                    None,
                    None,
                );
                state.sources.push(apply_eligibility(state.scope, source));
                continue;
            }
            walk_dir(state, &child_abs, &child_rel, depth.saturating_add(1));
            continue;
        }

        // Regular file — inspect without executing.
        let membership = {
            let from_git = state.git.membership_of(&child_rel);
            if from_git == VcsMembership::Untracked && state.ignore.is_ignored(&child_abs, false) {
                VcsMembership::Ignored
            } else {
                from_git
            }
        };

        let class = classify_file(state.scope, &child_abs, membership);
        let fingerprint = digest_file(&child_abs).ok();
        let mut source = make_source(
            state,
            &child_rel,
            &child_abs,
            class,
            membership,
            fingerprint,
            ObjectIdentity::from_meta(&meta),
            None,
        );
        if let Some((rev, dirty)) = state.git.submodules.get(&child_rel) {
            source.provenance.submodule_revision = Some(rev.clone());
            source.provenance.submodule_dirty = Some(*dirty);
        }
        // Instruction-shaped content cannot self-promote (SPEC-013 §23.34).
        if class != SourceClass::NormativeInstruction
            && let Ok(bytes) = std::fs::read(&child_abs)
            && looks_instruction_shaped(&bytes)
            && !state.scope.is_normative_instruction_path(&child_abs)
        {
            // Remain RepositoryFile/WorktreeFile — never NormativeInstruction.
            source.source_class = if membership == VcsMembership::Tracked {
                SourceClass::RepositoryFile
            } else {
                SourceClass::WorktreeFile
            };
        }
        state.sources.push(apply_eligibility(state.scope, source));
    }
}

fn handle_symlink(state: &mut WalkState<'_>, abs: &Path, rel: &Path, depth: u32) {
    let link_meta = match std::fs::symlink_metadata(abs) {
        Ok(m) => m,
        Err(_) => {
            push_excluded(state, rel, abs, ExclusionReason::SourceUnavailable);
            return;
        }
    };
    let link_oid = ObjectIdentity::from_meta(&link_meta);
    if let Some(oid) = link_oid {
        if state.lineage.contains(&oid) {
            push_excluded(state, rel, abs, ExclusionReason::SymlinkCycle);
            return;
        }
        if state.visited.len() as u64 >= MAX_VISITED_IDENTITIES {
            push_excluded(state, rel, abs, ExclusionReason::TraversalBudgetExhausted);
            return;
        }
        state.visited.insert(oid);
    }

    let (resolved, hops) = match resolve_symlink_chain(abs, MAX_SYMLINK_HOPS) {
        Ok(v) => v,
        Err(reason) => {
            push_excluded(state, rel, abs, reason);
            return;
        }
    };
    let _ = hops;

    // Authorize resolved target against authorized roots.
    let authorized = state.scope.roots.iter().any(|r| {
        resolved.starts_with(&r.path) || (r.external_authorized && resolved.starts_with(&r.path))
    }) || state.root.external_authorized && resolved.starts_with(&state.root.path);

    let inside_root = resolved.starts_with(&state.root.path);
    if !inside_root && !authorized {
        // Also allow explicit external roots listed on the scope.
        let external_ok = state
            .scope
            .roots
            .iter()
            .any(|r| r.external_authorized && resolved.starts_with(&r.path));
        if !external_ok {
            let mut source = make_source(
                state,
                rel,
                abs,
                SourceClass::WorktreeFile,
                VcsMembership::Untracked,
                None,
                link_oid,
                None,
            );
            source.provenance.symlink_link_path = Some(abs.to_path_buf());
            source.provenance.symlink_target_path = Some(resolved);
            source.exclusion = Some(ExclusionReason::SymlinkEscape);
            state.sources.push(source);
            return;
        }
    }

    // Bind read to the authorized object identity (prevent check-then-read swap).
    let target_meta = match std::fs::symlink_metadata(&resolved) {
        Ok(m) => m,
        Err(_) => {
            push_excluded(state, rel, abs, ExclusionReason::SourceUnavailable);
            return;
        }
    };
    let target_oid = ObjectIdentity::from_meta(&target_meta);
    if let (Some(expected), Some(_actual)) = (link_oid, target_oid)
        && let Ok(recheck) = std::fs::symlink_metadata(abs)
        && ObjectIdentity::from_meta(&recheck) != Some(expected)
    {
        push_excluded(state, rel, abs, ExclusionReason::SymlinkEscape);
        return;
    }

    if target_meta.is_dir() {
        if let Some(oid) = target_oid {
            if state.lineage.contains(&oid) || state.visited.contains(&oid) {
                push_excluded(state, rel, abs, ExclusionReason::SymlinkCycle);
                return;
            }
            state.lineage.insert(oid);
            state.visited.insert(oid);
        }
        let mut source = make_source(
            state,
            rel,
            abs,
            SourceClass::WorktreeFile,
            VcsMembership::Untracked,
            None,
            link_oid,
            None,
        );
        source.provenance.symlink_link_path = Some(abs.to_path_buf());
        source.provenance.symlink_target_path = Some(resolved.clone());
        state.sources.push(apply_eligibility(state.scope, source));
        walk_dir(state, &resolved, rel, depth.saturating_add(1));
        if let Some(oid) = target_oid {
            state.lineage.remove(&oid);
        }
        return;
    }

    // File symlink: fingerprint bound to authorized target identity.
    let fingerprint = digest_file(&resolved).ok();
    // Re-verify target identity after read authorization.
    if let Ok(after) = std::fs::symlink_metadata(&resolved)
        && ObjectIdentity::from_meta(&after) != target_oid
    {
        push_excluded(state, rel, abs, ExclusionReason::SymlinkEscape);
        return;
    }
    let mut source = make_source(
        state,
        rel,
        abs,
        SourceClass::WorktreeFile,
        state.git.membership_of(rel),
        fingerprint,
        target_oid.or(link_oid),
        None,
    );
    source.provenance.symlink_link_path = Some(abs.to_path_buf());
    source.provenance.symlink_target_path = Some(resolved);
    state.sources.push(apply_eligibility(state.scope, source));
}

fn classify_file(scope: &DiscoveryScope, abs: &Path, membership: VcsMembership) -> SourceClass {
    if scope.is_normative_instruction_path(abs) {
        return SourceClass::NormativeInstruction;
    }
    match membership {
        VcsMembership::Tracked => SourceClass::RepositoryFile,
        VcsMembership::Untracked | VcsMembership::Ignored | VcsMembership::Unknown => {
            SourceClass::WorktreeFile
        }
    }
}

fn looks_instruction_shaped(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    let lower = text.to_ascii_lowercase();
    lower.contains("you are a helpful assistant")
        || lower.contains("system prompt")
        || lower.contains("ignore previous instructions")
        || lower.contains("# normative instruction")
}

#[allow(clippy::too_many_arguments)]
fn make_source(
    state: &WalkState<'_>,
    rel: &Path,
    abs: &Path,
    class: SourceClass,
    membership: VcsMembership,
    fingerprint: Option<crate::digest::IntegrityDigest>,
    object_identity: Option<ObjectIdentity>,
    nested: Option<RepositoryId>,
) -> DiscoveredSource {
    DiscoveredSource {
        source_class: class,
        provenance: SourceProvenance {
            repository_id: state.root.repository_id.clone(),
            worktree_id: state.root.worktree_id.clone(),
            authorized_root: state.root.path.clone(),
            relative_path: rel.to_path_buf(),
            absolute_path: abs.to_path_buf(),
            membership,
            object_identity,
            content_fingerprint: fingerprint,
            symlink_link_path: None,
            symlink_target_path: None,
            nested_repository_id: nested,
            submodule_revision: state.git.submodules.get(rel).map(|(r, _)| r.clone()),
            submodule_dirty: state.git.submodules.get(rel).map(|(_, d)| *d),
            source_generation: state.scope.source_generation,
            policy_generation: state.scope.policy_generation,
            privacy_generation: state.scope.privacy_generation,
        },
        exclusion: None,
    }
}

fn push_excluded(state: &mut WalkState<'_>, rel: &Path, abs: &Path, reason: ExclusionReason) {
    let mut source = make_source(
        state,
        rel,
        abs,
        SourceClass::WorktreeFile,
        VcsMembership::Unknown,
        None,
        None,
        None,
    );
    source.exclusion = Some(reason);
    state.sources.push(source);
}

/// Property helper: reject malformed relative identities without walking.
pub fn reject_malformed_relative(relative: &str) -> Result<(), ExclusionReason> {
    join_under_root(Path::new("/authorized"), Path::new(relative)).map(|_| ())
}

/// Build a deterministic catalog fingerprint for generation fencing.
pub fn catalog_fingerprint(sources: &[DiscoveredSource]) -> crate::digest::IntegrityDigest {
    let mut map = BTreeMap::new();
    for s in sources {
        let key = format!(
            "{}|{}|{}|{:?}",
            s.provenance.repository_id.0,
            s.provenance.worktree_id.0,
            s.provenance.relative_path.display(),
            s.exclusion
        );
        let fp = s
            .provenance
            .content_fingerprint
            .map(|d| d.hex())
            .unwrap_or_default();
        map.insert(key, fp);
    }
    let mut bytes = Vec::new();
    for (k, v) in map {
        bytes.extend_from_slice(k.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(v.as_bytes());
        bytes.push(0);
    }
    digest_bytes(&bytes)
}
