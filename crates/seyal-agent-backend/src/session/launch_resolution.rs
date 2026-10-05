//! SPEC-027 §5/§6 LaunchDescriptor resolution.
//!
//! Builds the exact spawn input (program, argv, env, cwd) the Agent Backend
//! passes to `SessionExecutionHost::start` from a frozen adapter manifest
//! and the dispatching run's `WorkScope` kind. Never reads a client-supplied
//! argv/cwd/env field — `StartAgentRun` carries none (§5.1 fixture 5).

use std::path::{Path, PathBuf};

use seyal_agent_core::{AdapterId, AgentRunId, BindingGeneration, LaunchDescriptor, WorkScopeKind};
use seyal_agent_store::LaunchDescriptorTemplate;

const WORK_SCOPE_ROOT_TOKEN: &str = "{work_scope_root}";
const ENV_RUN_ID: &str = "SEYAL_RUN_ID";
const ENV_BINDING_GENERATION: &str = "SEYAL_BINDING_GENERATION";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchResolutionFailure {
    /// SPEC-027 §6: `Repository`/`Project` cwd is "bound root from
    /// `WorkScope.bindings`". No bindings subsystem exists in this
    /// codebase yet (tracked by a linked follow-up issue), so resolution
    /// fails closed rather than guessing a root or silently falling back
    /// to `AdapterWorkDir`.
    MissingBoundRoot,
    /// `argv_template` names `{work_scope_root}` but this manifest/
    /// WorkScope combination never resolves one (§5.1: the token is only
    /// expanded when `cwd_policy = WorkScopeRoot` and the binding is a
    /// repository/root — which this codebase cannot yet produce).
    UnresolvedWorkScopeToken,
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

/// SPEC-027 §6 cwd table, restricted to the kinds this codebase can resolve
/// without inventing a bindings subsystem: `Repository`/`Project` fail
/// closed on a missing bound root — exactly §6's own prescribed failure mode
/// (fixture 17's fail-closed sub-case; the happy-path "resolve to the bound
/// root, reject traversal/escape" half of fixture 17 needs a durable
/// `WorkScope.bindings` concept that does not exist yet, tracked by
/// https://github.com/seyal-org/seyal/issues/1226). `HostBound` (no
/// host-supplied root binding exists either) and `AdHoc` both resolve to
/// `AdapterWorkDir`.
fn resolve_cwd(
    kind: WorkScopeKind,
    adapter_work_dir: &Path,
) -> Result<PathBuf, LaunchResolutionFailure> {
    match kind {
        WorkScopeKind::Repository | WorkScopeKind::Project => {
            Err(LaunchResolutionFailure::MissingBoundRoot)
        }
        WorkScopeKind::HostBound | WorkScopeKind::AdHoc => Ok(adapter_work_dir.to_path_buf()),
    }
}

/// §5.1: `argv_template` entries are literals or the `{work_scope_root}`
/// token. No `WorkScope.bindings` subsystem exists, so a repository/root
/// work-scope root is never available to substitute; a manifest that
/// declares the token fails closed rather than spawning with a literal,
/// unexpanded placeholder.
fn expand_argv(argv_template: &[String]) -> Result<Vec<String>, LaunchResolutionFailure> {
    let mut argv = Vec::with_capacity(argv_template.len());
    for token in argv_template {
        if token == WORK_SCOPE_ROOT_TOKEN {
            return Err(LaunchResolutionFailure::UnresolvedWorkScopeToken);
        }
        argv.push(token.clone());
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
            // Backend-injected identity refs below always win; an operator
            // listing them in env_allowlist does not source them from the
            // daemon's own process environment instead.
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
/// kind. Creates `AdapterWorkDir` on disk when cwd resolves to it.
pub fn resolve_launch_descriptor(
    launch: &LaunchDescriptorTemplate,
    kind: WorkScopeKind,
    adapter_work_root: &Path,
    adapter_id: AdapterId,
    run_id: AgentRunId,
    binding_generation: BindingGeneration,
) -> Result<LaunchDescriptor, LaunchResolutionFailure> {
    let work_dir = adapter_work_dir(adapter_work_root, adapter_id);
    let cwd = resolve_cwd(kind, &work_dir)?;
    if cwd == work_dir {
        std::fs::create_dir_all(&work_dir)
            .map_err(|_| LaunchResolutionFailure::AdapterWorkDirUnavailable)?;
    }
    let argv = expand_argv(&launch.argv_template)?;
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
    use seyal_agent_store::CwdPolicy;

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

    #[test]
    fn adhoc_resolves_to_adapter_work_dir_and_creates_it() {
        let root = std::env::temp_dir().join(format!(
            "seyal-launch-resolution-adhoc-{}-{}",
            std::process::id(),
            AdapterId::new().to_bytes()[0]
        ));
        let adapter_id = AdapterId::new();
        let (run, generation) = run_and_generation();
        let descriptor = resolve_launch_descriptor(
            &template(CwdPolicy::AdapterWorkDir),
            WorkScopeKind::AdHoc,
            &root,
            adapter_id,
            run,
            generation,
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
        let descriptor = resolve_launch_descriptor(
            &template(CwdPolicy::AdapterWorkDir),
            WorkScopeKind::HostBound,
            &root,
            adapter_id,
            run,
            generation,
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
            resolve_launch_descriptor(
                &template(CwdPolicy::WorkScopeRoot),
                WorkScopeKind::Repository,
                Path::new("/tmp/unused"),
                adapter_id,
                run,
                generation,
            ),
            Err(LaunchResolutionFailure::MissingBoundRoot)
        );
    }

    #[test]
    fn project_without_bindings_fails_closed_missing_root() {
        let adapter_id = AdapterId::new();
        let (run, generation) = run_and_generation();
        assert_eq!(
            resolve_launch_descriptor(
                &template(CwdPolicy::WorkScopeRoot),
                WorkScopeKind::Project,
                Path::new("/tmp/unused"),
                adapter_id,
                run,
                generation,
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
            resolve_launch_descriptor(
                &launch,
                WorkScopeKind::AdHoc,
                Path::new("/tmp/unused-root"),
                adapter_id,
                run,
                generation,
            ),
            Err(LaunchResolutionFailure::UnresolvedWorkScopeToken)
        );
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
        // SEYAL_RUN_ID is backend-injected, not sourced through `lookup`,
        // even though it was also listed in env_allowlist.
        assert_eq!(env.iter().filter(|(name, _)| name == ENV_RUN_ID).count(), 1);
        assert!(env.contains(&(ENV_RUN_ID.to_string(), run.to_string())));
    }
}
