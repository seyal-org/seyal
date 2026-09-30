//! Runtime interactive create composition (ADR-020 §3.6 / SPEC-023 §4).
//!
//! Builds `CommandSpec` only through `EffectiveLaunchPolicy`. CapabilityPolicy
//! and ShellIntegrationPolicy apply afterward through [`compose_child_command`],
//! which is the sole composition authority used by `Runtime::create_execution`
//! and by tests that inspect the full child env.

use std::path::PathBuf;

use seyal_exec::{CommandSpec, ShellIntegrationToken};

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

/// Result of CapabilityPolicy + ShellIntegrationPolicy composition.
///
/// This is the sole post-policy composition authority (ADR-020 §3.1).
#[derive(Debug)]
pub struct ComposedChildCommand {
    pub command: CommandSpec,
    /// True when zsh shell-integration hooks were applied (macOS only).
    pub shell_integration_applied: bool,
    /// Nonce token when shell integration applied; `None` otherwise.
    pub shell_nonce: Option<ShellIntegrationToken>,
}

/// Apply CapabilityPolicy then ShellIntegrationPolicy when eligible.
///
/// Production `Runtime::create_execution` and compose tests both call this.
pub fn compose_child_command(
    command: CommandSpec,
    capability: &CapabilityPolicy,
    shell_integration: Option<&ShellIntegrationPolicy>,
) -> Result<ComposedChildCommand, RuntimeError> {
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
            let (command, nonce) = policy.apply(command)?;
            return Ok(ComposedChildCommand {
                command,
                shell_integration_applied: true,
                shell_nonce: Some(nonce),
            });
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = shell_integration;
    }
    Ok(ComposedChildCommand {
        command,
        shell_integration_applied: false,
        shell_nonce: None,
    })
}

/// Test helper: composition returning only the `CommandSpec`.
/// Kept private to macOS lib tests; `compose_child_command` remains the sole
/// public launch-composition authority.
#[cfg(all(test, target_os = "macos"))]
pub fn apply_post_policy(
    command: CommandSpec,
    capability: &CapabilityPolicy,
    shell_integration: Option<&ShellIntegrationPolicy>,
) -> Result<CommandSpec, RuntimeError> {
    Ok(compose_child_command(command, capability, shell_integration)?.command)
}
