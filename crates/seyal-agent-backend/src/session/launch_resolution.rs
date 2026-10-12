//! SPEC-027 §5/§6 LaunchDescriptor resolution.
//!
//! Builds the exact spawn input (program, argv, env, cwd) the Agent Backend
//! passes to `SessionExecutionHost::start` from a frozen adapter manifest
//! and the dispatching run's `WorkScope` kind. Never reads a client-supplied
//! argv/cwd/env field — `StartAgentRun` carries none (§5.1 fixture 5).

use std::path::{Path, PathBuf};

use seyal_agent_core::{AdapterId, AgentRunId, BindingGeneration, LaunchDescriptor, WorkScopeKind};
use seyal_agent_store::{CwdPolicy, LaunchDescriptorTemplate};

const WORK_SCOPE_ROOT_TOKEN: &str = "{work_scope_root}";
const ENV_RUN_ID: &str = "SEYAL_RUN_ID";
const ENV_BINDING_GENERATION: &str = "SEYAL_BINDING_GENERATION";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchResolutionFailure {
    /// SPEC-027 §6: `Repository`/`Project` cwd requires a durable bound root
    /// that still exists as a directory after canonicalization.
    MissingBoundRoot,
    /// `argv_template` names `{work_scope_root}` but `cwd_policy` is not
    /// `WorkScopeRoot` or no bound root is available (§5.1).
    UnresolvedWorkScopeToken,
    /// Expanded `{work_scope_root}` path left the canonical bound root
    /// (traversal or symlink-escape).
    EscapedBoundRoot,
    AdapterWorkDirUnavailable,
}

/// Per-adapter work directory, backend-owned, nested under the daemon's own
/// private runtime directory (never `$HOME`, never a client-supplied path).
pub fn adapter_work_dir(adapter_work_root: &Path, adapter_id: AdapterId) -> PathBuf {
    let bytes = adapter_id.to_bytes();
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        hex.push_str(&format!("{byte:02x}"));
    }
    adapter_work_root.join(hex)
}

fn canonical_existing_dir(path: &Path) -> Result<PathBuf, LaunchResolutionFailure> {
    let canonical = path
        .canonicalize()
        .map_err(|_| LaunchResolutionFailure::MissingBoundRoot)?;
    if !canonical.is_dir() {
        return Err(LaunchResolutionFailure::MissingBoundRoot);
    }
    Ok(canonical)
}

/// SPEC-027 §6 cwd table. `AdHoc` is always `AdapterWorkDir`. `HostBound`
/// uses a supplied binding when present. `Repository`/`Project` require one.
fn resolve_cwd(
    kind: WorkScopeKind,
    adapter_work_dir: &Path,
    bound_root: Option<&Path>,
) -> Result<PathBuf, LaunchResolutionFailure> {
    match kind {
        WorkScopeKind::Repository | WorkScopeKind::Project => {
            let Some(path) = bound_root else {
                return Err(LaunchResolutionFailure::MissingBoundRoot);
            };
            canonical_existing_dir(path)
        }
        WorkScopeKind::HostBound => match bound_root {
            Some(path) => canonical_existing_dir(path),
            None => Ok(adapter_work_dir.to_path_buf()),
        },
        WorkScopeKind::AdHoc => Ok(adapter_work_dir.to_path_buf()),
    }
}

fn path_is_within(root: &Path, candidate: &Path) -> bool {
    match (root.canonicalize(), candidate.canonicalize()) {
        (Ok(root), Ok(candidate)) => candidate.starts_with(&root),
        _ => false,
    }
}

fn expand_token(token: &str, bound_root: &Path) -> Result<String, LaunchResolutionFailure> {
    let rest = token
        .strip_prefix(WORK_SCOPE_ROOT_TOKEN)
        .ok_or(LaunchResolutionFailure::UnresolvedWorkScopeToken)?;
    let candidate = if rest.is_empty() {
        bound_root.to_path_buf()
    } else {
        let rest = rest.strip_prefix('/').unwrap_or(rest);
        bound_root.join(rest)
    };
    if !path_is_within(bound_root, &candidate) {
        return Err(LaunchResolutionFailure::EscapedBoundRoot);
    }
    let canonical = candidate
        .canonicalize()
        .map_err(|_| LaunchResolutionFailure::EscapedBoundRoot)?;
    Ok(canonical.to_string_lossy().into_owned())
}

fn expand_argv(
    argv_template: &[String],
    cwd_policy: CwdPolicy,
    bound_root: Option<&Path>,
) -> Result<Vec<String>, LaunchResolutionFailure> {
    let mut argv = Vec::with_capacity(argv_template.len());
    for token in argv_template {
        if token.contains(WORK_SCOPE_ROOT_TOKEN) {
            if cwd_policy != CwdPolicy::WorkScopeRoot {
                return Err(LaunchResolutionFailure::UnresolvedWorkScopeToken);
            }
            let Some(root) = bound_root else {
                return Err(LaunchResolutionFailure::UnresolvedWorkScopeToken);
            };
            argv.push(expand_token(token, root)?);
        } else {
            argv.push(token.clone());
        }
    }
    Ok(argv)
}

/// §5.1 env resolution: a clear + allowlist. Named entries are read only
/// through `lookup` — in production, the daemon process's own environment,
/// never the client's, since `StartAgentRun` carries no env field at all —
/// then backend-injected non-secret `SEYAL_*` identity refs are added
/// unconditionally. `lookup` is injectable so tests never have to mutate
/// real process environment state.
fn resolve_env(
    env_allowlist: &[String],
    run_id: AgentRunId,
    binding_generation: BindingGeneration,
    lookup: impl Fn(&str) -> Option<String>,
) -> Vec<(String, String)> {
    let mut env = Vec::with_capacity(env_allowlist.len() + 2);
    for name in env_allowlist {
        if name == ENV_RUN_ID || name == ENV_BINDING_GENERATION {
            continue;
        }
        if let Some(value) = lookup(name) {
            env.push((name.clone(), value));
        }
    }
    env.push((ENV_RUN_ID.to_string(), run_id.to_string()));
    env.push((
        ENV_BINDING_GENERATION.to_string(),
        binding_generation.get().to_string(),
    ));
    env
}

/// Resolves the exact [`LaunchDescriptor`] passed to
/// `SessionExecutionHost::start`, from the manifest frozen at
/// `adapter_manifest_generation` and the dispatching run's `WorkScope`
/// kind plus optional durable bound root. Creates `AdapterWorkDir` on disk
/// when cwd resolves to it.
pub fn resolve_launch_descriptor(
    launch: &LaunchDescriptorTemplate,
    kind: WorkScopeKind,
    adapter_work_root: &Path,
    adapter_id: AdapterId,
    run_id: AgentRunId,
    binding_generation: BindingGeneration,
    bound_root: Option<&Path>,
) -> Result<LaunchDescriptor, LaunchResolutionFailure> {
    let work_dir = adapter_work_dir(adapter_work_root, adapter_id);
    let cwd = resolve_cwd(kind, &work_dir, bound_root)?;
    if cwd == work_dir {
        std::fs::create_dir_all(&work_dir)
            .map_err(|_| LaunchResolutionFailure::AdapterWorkDirUnavailable)?;
    }
    let argv_root = match kind {
        WorkScopeKind::Repository | WorkScopeKind::Project => Some(cwd.as_path()),
        WorkScopeKind::HostBound if bound_root.is_some() => Some(cwd.as_path()),
        _ => None,
    };
    let argv = expand_argv(&launch.argv_template, launch.cwd_policy, argv_root)?;
    let env = resolve_env(&launch.env_allowlist, run_id, binding_generation, |name| {
        std::env::var(name).ok()
    });
    Ok(LaunchDescriptor {
        program: launch.program.clone(),
        argv,
        env,
        cwd,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(cwd_policy: CwdPolicy) -> LaunchDescriptorTemplate {
        LaunchDescriptorTemplate::new("/bin/echo", cwd_policy).with_argv(["hello"])
    }

    fn run_and_generation() -> (AgentRunId, BindingGeneration) {
        let mut domain = seyal_agent_core::AgentDomain::new();
        let scope = domain.create_work_scope(WorkScopeKind::AdHoc);
        let item = domain.create_work_item(scope).unwrap();
        let attempt = domain.create_attempt(item).unwrap();
        let run = domain.create_agent_run(attempt).unwrap();
        (run, domain.agent_run(run).unwrap().binding_generation())
    }

    fn resolve(
        launch: &LaunchDescriptorTemplate,
        kind: WorkScopeKind,
        adapter_work_root: &Path,
        adapter_id: AdapterId,
        run_id: AgentRunId,
        generation: BindingGeneration,
        bound: Option<&Path>,
    ) -> Result<LaunchDescriptor, LaunchResolutionFailure> {
        resolve_launch_descriptor(
            launch,
            kind,
            adapter_work_root,
            adapter_id,
            run_id,
            generation,
            bound,
        )
    }

    #[test]
    fn work_scope_stays_copy_without_a_path_field() {
        fn assert_copy<T: Copy>() {}
        assert_copy::<seyal_agent_core::WorkScope>();
    }

    #[test]
    fn adhoc_resolves_to_adapter_work_dir_and_creates_it() {
        let root = std::env::temp_dir().join(format!(
            "seyal-launch-resolution-adhoc-{}-{}",
            std::process::id(),
            AdapterId::new().to_bytes()[0]
        ));
        let adapter_id = AdapterId::new();
        let (run, generation) = run_and_generation();
        let descriptor = resolve(
            &template(CwdPolicy::AdapterWorkDir),
            WorkScopeKind::AdHoc,
            &root,
            adapter_id,
            run,
            generation,
            None,
        )
        .unwrap();
        assert_eq!(descriptor.cwd, adapter_work_dir(&root, adapter_id));
        assert!(descriptor.cwd.is_dir());
        assert_eq!(descriptor.program, "/bin/echo");
        assert_eq!(descriptor.argv, vec!["hello".to_string()]);
        assert!(descriptor
            .env
            .iter()
            .any(|(name, value)| name == ENV_RUN_ID && value == &run.to_string()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn host_bound_without_a_binding_falls_back_to_adapter_work_dir() {
        let root = std::env::temp_dir().join(format!(
            "seyal-launch-resolution-hostbound-{}",
            std::process::id()
        ));
        let adapter_id = AdapterId::new();
        let (run, generation) = run_and_generation();
        let descriptor = resolve(
            &template(CwdPolicy::AdapterWorkDir),
            WorkScopeKind::HostBound,
            &root,
            adapter_id,
            run,
            generation,
            None,
        )
        .unwrap();
        assert_eq!(descriptor.cwd, adapter_work_dir(&root, adapter_id));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn repository_without_bindings_fails_closed_missing_root() {
        let adapter_id = AdapterId::new();
        let (run, generation) = run_and_generation();
        assert_eq!(
            resolve(
                &template(CwdPolicy::WorkScopeRoot),
                WorkScopeKind::Repository,
                Path::new("/tmp/unused"),
                adapter_id,
                run,
                generation,
                None,
            ),
            Err(LaunchResolutionFailure::MissingBoundRoot)
        );
    }

    #[test]
    fn project_without_bindings_fails_closed_missing_root() {
        let adapter_id = AdapterId::new();
        let (run, generation) = run_and_generation();
        assert_eq!(
            resolve(
                &template(CwdPolicy::WorkScopeRoot),
                WorkScopeKind::Project,
                Path::new("/tmp/unused"),
                adapter_id,
                run,
                generation,
                None,
            ),
            Err(LaunchResolutionFailure::MissingBoundRoot)
        );
    }

    #[test]
    fn work_scope_root_token_in_argv_fails_closed_without_a_binding() {
        let adapter_id = AdapterId::new();
        let (run, generation) = run_and_generation();
        let launch = LaunchDescriptorTemplate::new("/bin/echo", CwdPolicy::WorkScopeRoot)
            .with_argv(["{work_scope_root}"]);
        assert_eq!(
            resolve(
                &launch,
                WorkScopeKind::AdHoc,
                Path::new("/tmp/unused-root"),
                adapter_id,
                run,
                generation,
                None,
            ),
            Err(LaunchResolutionFailure::UnresolvedWorkScopeToken)
        );
    }

    #[test]
    fn repository_bound_root_is_launch_cwd_and_expands_the_token() {
        let bound =
            std::env::temp_dir().join(format!("seyal-launch-bound-root-{}", std::process::id()));
        std::fs::create_dir_all(&bound).unwrap();
        let adapter_id = AdapterId::new();
        let (run, generation) = run_and_generation();
        let launch = LaunchDescriptorTemplate::new("/bin/echo", CwdPolicy::WorkScopeRoot)
            .with_argv(["{work_scope_root}"]);
        let descriptor = resolve(
            &launch,
            WorkScopeKind::Repository,
            Path::new("/tmp/unused-adapter"),
            adapter_id,
            run,
            generation,
            Some(&bound),
        )
        .unwrap();
        let canonical = bound.canonicalize().unwrap();
        assert_eq!(descriptor.cwd, canonical);
        assert_eq!(
            descriptor.argv,
            vec![canonical.to_string_lossy().into_owned()]
        );
        let _ = std::fs::remove_dir_all(&bound);
    }

    #[test]
    fn deleted_bound_root_fails_closed_at_dispatch() {
        let bound =
            std::env::temp_dir().join(format!("seyal-launch-deleted-root-{}", std::process::id()));
        std::fs::create_dir_all(&bound).unwrap();
        std::fs::remove_dir_all(&bound).unwrap();
        let adapter_id = AdapterId::new();
        let (run, generation) = run_and_generation();
        assert_eq!(
            resolve(
                &template(CwdPolicy::WorkScopeRoot),
                WorkScopeKind::Repository,
                Path::new("/tmp/unused"),
                adapter_id,
                run,
                generation,
                Some(&bound),
            ),
            Err(LaunchResolutionFailure::MissingBoundRoot)
        );
    }

    #[test]
    fn argv_traversal_from_bound_root_is_rejected() {
        let bound = std::env::temp_dir().join(format!(
            "seyal-launch-traversal-root-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&bound).unwrap();
        let adapter_id = AdapterId::new();
        let (run, generation) = run_and_generation();
        let launch = LaunchDescriptorTemplate::new("/bin/echo", CwdPolicy::WorkScopeRoot)
            .with_argv(["{work_scope_root}/../escape"]);
        assert_eq!(
            resolve(
                &launch,
                WorkScopeKind::Repository,
                Path::new("/tmp/unused"),
                adapter_id,
                run,
                generation,
                Some(&bound),
            ),
            Err(LaunchResolutionFailure::EscapedBoundRoot)
        );
        let _ = std::fs::remove_dir_all(&bound);
    }

    #[cfg(unix)]
    #[test]
    fn argv_symlink_escape_from_bound_root_is_rejected() {
        let parent =
            std::env::temp_dir().join(format!("seyal-launch-symlink-{}", std::process::id()));
        let bound = parent.join("root");
        let outside = parent.join("outside");
        std::fs::create_dir_all(&bound).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, bound.join("escape")).unwrap();
        let adapter_id = AdapterId::new();
        let (run, generation) = run_and_generation();
        let launch = LaunchDescriptorTemplate::new("/bin/echo", CwdPolicy::WorkScopeRoot)
            .with_argv(["{work_scope_root}/escape"]);
        assert_eq!(
            resolve(
                &launch,
                WorkScopeKind::Repository,
                Path::new("/tmp/unused"),
                adapter_id,
                run,
                generation,
                Some(&bound),
            ),
            Err(LaunchResolutionFailure::EscapedBoundRoot)
        );
        let _ = std::fs::remove_dir_all(&parent);
    }

    #[test]
    fn env_allowlist_is_looked_up_only_through_the_injected_backend_source() {
        let (run, generation) = run_and_generation();
        let lookup = |name: &str| {
            (name == "SEYAL_TEST_LAUNCH_RESOLUTION_VAR").then(|| "backend-policy-value".into())
        };
        let env = resolve_env(
            &[
                "SEYAL_TEST_LAUNCH_RESOLUTION_VAR".to_string(),
                ENV_RUN_ID.to_string(),
            ],
            run,
            generation,
            lookup,
        );
        assert!(env.contains(&(
            "SEYAL_TEST_LAUNCH_RESOLUTION_VAR".to_string(),
            "backend-policy-value".to_string()
        )));
        assert_eq!(env.iter().filter(|(name, _)| name == ENV_RUN_ID).count(), 1);
        assert!(env.contains(&(ENV_RUN_ID.to_string(), run.to_string())));
    }
}
