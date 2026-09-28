//! Runtime interactive create composition (ADR-020 §3.6 / SPEC-023 §4).
//!
//! Builds `CommandSpec` only through `EffectiveLaunchPolicy`. CapabilityPolicy
//! and ShellIntegrationPolicy apply afterward inside `Runtime::create_execution`
//! (or via [`apply_post_policy`] for tests that inspect the full child env).

use std::path::PathBuf;

use seyal_exec::CommandSpec;

use crate::{CapabilityPolicy, RuntimeError, ShellIntegrationPolicy};

use super::{
    account::lookup_effective_account_record,
    env::ProcessLocaleEnv,
    resolve::{resolve, ResolveInputs},
    tmpdir::darwin_user_temp_dir,
    types::{LaunchPolicyFailure, LaunchPolicyResolution, LaunchProfileIntent},
    validate::RealPathProbe,
};

/// Resolve profile `0` from the live effective-UID account and process state.
///
/// Does not spawn, open a PTY, or mutate the Runtime registry.
pub fn resolve_default_interactive() -> Result<LaunchPolicyResolution, LaunchPolicyFailure> {
    let account = lookup_effective_account_record();
    let process_shell_buf = std::env::var_os("SHELL").map(PathBuf::from);
    let process_shell = process_shell_buf
        .as_deref()
        .filter(|path| path.is_absolute());
    let tmpdir = darwin_user_temp_dir();
    let intent = LaunchProfileIntent::default_interactive();
    resolve(ResolveInputs {
        intent: &intent,
        account: account.as_ref(),
        process_shell,
        tmpdir: tmpdir.as_deref(),
        locale: &ProcessLocaleEnv,
        probe: &RealPathProbe,
    })
}

/// Pure policy → `CommandSpec` conversion (SPEC-023 §4 first stage).
pub fn command_spec_from_policy(resolution: &LaunchPolicyResolution) -> CommandSpec {
    resolution.policy.to_command_spec()
}

/// Apply CapabilityPolicy then ShellIntegrationPolicy when eligible.
///
/// Used by tests that assert the child key set. Production create applies the
/// same owners inside `Runtime::create_execution` after policy conversion.
pub fn apply_post_policy(
    command: CommandSpec,
    capability: &CapabilityPolicy,
    shell_integration: Option<&ShellIntegrationPolicy>,
) -> Result<CommandSpec, RuntimeError> {
    if !capability.is_available() {
        return Err(RuntimeError::LaunchPolicy(
            LaunchPolicyFailure::CapabilityUnavailable,
        ));
    }
    let command = capability.apply(command);
    #[cfg(target_os = "macos")]
    {
        if let Some(policy) = shell_integration
            && ShellIntegrationPolicy::supports(&command)
        {
            let (command, _nonce) = policy.apply(command)?;
            return Ok(command);
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = shell_integration;
    }
    Ok(command)
}
