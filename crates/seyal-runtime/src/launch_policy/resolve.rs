//! Pure `EffectiveLaunchPolicy` resolver (no spawn, no PTY, no protocol).

use std::path::{Path, PathBuf};

use super::{
    argv::interactive_login_argv,
    env::{build_base_env, LocaleEnv},
    types::{
        AccountRecord, EffectiveLaunchPolicy, LaunchPolicyFailure, LaunchPolicyResolution,
        LaunchPolicyWarning, LaunchProfileIntent,
    },
    validate::{account_record_usable, PathProbe},
};

/// Platform safe-fallback program candidates (SPEC-023 §5.1 step 3).
pub const PLATFORM_SAFE_FALLBACKS: &[&str] = &["/bin/zsh", "/bin/bash", "/bin/sh"];

/// Inputs for one cold-path resolution. All filesystem and account data are
/// injected so unit tests never touch a live PTY or require a real passwd row.
pub struct ResolveInputs<'a> {
    pub intent: &'a LaunchProfileIntent,
    pub account: Option<&'a AccountRecord>,
    /// Runtime process `SHELL` when absolute (SPEC-023 §5.1 step 2).
    pub process_shell: Option<&'a Path>,
    /// Pre-validated Darwin per-user temp dir, or `None` to omit `TMPDIR`.
    pub tmpdir: Option<&'a Path>,
    pub locale: &'a dyn LocaleEnv,
    pub probe: &'a dyn PathProbe,
}

/// Resolve profile intent into a validated policy object or a typed failure.
///
/// Does not spawn a process, open a PTY, or consult OSC/Pane metadata.
pub fn resolve(inputs: ResolveInputs<'_>) -> Result<LaunchPolicyResolution, LaunchPolicyFailure> {
    let account = match inputs.account {
        Some(record) if account_record_usable(record.name(), record.home()) => record,
        _ => return Err(LaunchPolicyFailure::AccountRecordUnavailable),
    };

    let mut warnings = Vec::new();
    let mut shell_source_invalid = false;

    if let Some(configured) = inputs.intent.configured_shell.as_deref() {
        if inputs.probe.is_valid_shell_program(configured) {
            return finish(configured.to_path_buf(), account, inputs, warnings);
        }
        shell_source_invalid = true;
    }

    // §5.1 order: account-record shell → process SHELL → platform safe fallbacks.
    if !account.shell().as_os_str().is_empty() {
        if inputs.probe.is_valid_shell_program(account.shell()) {
            if shell_source_invalid {
                warnings.push(LaunchPolicyWarning::ConfiguredShellInvalid);
            }
            return finish(account.shell().to_path_buf(), account, inputs, warnings);
        }
        shell_source_invalid = true;
    } else {
        // Empty pw_shell is not AccountRecordUnavailable; fall through and warn
        // once a safe default is selected (SPEC-023 §9 / §12 item 15).
        shell_source_invalid = true;
    }

    if let Some(shell) = inputs.process_shell {
        if shell.is_absolute() && inputs.probe.is_valid_shell_program(shell) {
            if shell_source_invalid {
                warnings.push(LaunchPolicyWarning::ConfiguredShellInvalid);
            }
            return finish(shell.to_path_buf(), account, inputs, warnings);
        }
    }

    for candidate in PLATFORM_SAFE_FALLBACKS {
        let path = Path::new(candidate);
        if inputs.probe.is_valid_shell_program(path) {
            if shell_source_invalid {
                warnings.push(LaunchPolicyWarning::ConfiguredShellInvalid);
            }
            return finish(path.to_path_buf(), account, inputs, warnings);
        }
    }

    Err(LaunchPolicyFailure::ShellFallbackExhausted)
}

fn finish(
    program: PathBuf,
    account: &AccountRecord,
    inputs: ResolveInputs<'_>,
    mut warnings: Vec<LaunchPolicyWarning>,
) -> Result<LaunchPolicyResolution, LaunchPolicyFailure> {
    let cwd = resolve_cwd(account, inputs.intent, inputs.probe, &mut warnings)?;
    let argv = interactive_login_argv(&program);
    let env = build_base_env(
        account.name(),
        account.home(),
        &program,
        inputs.tmpdir,
        inputs.locale,
    );
    let policy = EffectiveLaunchPolicy::from_parts(
        program,
        argv,
        cwd,
        env,
        inputs.intent.capability_profile,
    );
    Ok(LaunchPolicyResolution { policy, warnings })
}

fn resolve_cwd(
    account: &AccountRecord,
    intent: &LaunchProfileIntent,
    probe: &dyn PathProbe,
    warnings: &mut Vec<LaunchPolicyWarning>,
) -> Result<PathBuf, LaunchPolicyFailure> {
    if let Some(override_cwd) = intent.cwd_override.as_deref() {
        if probe.is_valid_cwd(override_cwd) {
            return Ok(override_cwd.to_path_buf());
        }
        warnings.push(LaunchPolicyWarning::CwdOverrideInvalid);
    }

    if probe.is_valid_cwd(account.home()) {
        Ok(account.home().to_path_buf())
    } else {
        Err(LaunchPolicyFailure::CwdInvalid)
    }
}
