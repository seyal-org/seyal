//! Cold-path `EffectiveLaunchPolicy` types and pure resolver (ADR-020 / SPEC-023).
//!
//! Ownership: Runtime composition builds this object. It does not own a PTY,
//! spawn a child, or edit the provisioning protocol.

#[allow(unsafe_code)]
mod account;
mod argv;
mod compose;
mod env;
mod resolve;
#[allow(unsafe_code)]
mod tmpdir;
mod types;
mod validate;
mod wire;

#[cfg(all(test, target_os = "macos"))]
mod compose_tests;
#[cfg(test)]
mod tests;

pub use account::lookup_effective_account_record;
pub use argv::{interactive_login_argv, ShellFamily};
#[cfg(all(test, target_os = "macos"))]
pub use compose::apply_post_policy;
pub use compose::{
    command_spec_from_policy, compose_child_command, resolve_default_interactive,
    ComposedChildCommand,
};

/// Serialize process-env reads/writes across launch-policy and shell-integration
/// tests. Parallel `--lib` harnesses must not race on `setenv`/`getenv`.
#[cfg(all(test, target_os = "macos"))]
pub(crate) fn process_env_test_lock() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}
pub use env::{EmptyLocaleEnv, LocaleEnv, ProcessLocaleEnv, DEFAULT_PATH};
pub use resolve::{resolve, ResolveInputs, PLATFORM_SAFE_FALLBACKS};
pub use tmpdir::darwin_user_temp_dir;
pub use types::{
    AccountRecord, CapabilityProfileId, EffectiveLaunchPolicy, LaunchPolicyFailure,
    LaunchPolicyResolution, LaunchPolicyWarning, LaunchProfileIntent,
};
pub use validate::{
    account_record_usable, is_valid_cwd, is_valid_shell_program, path_has_forbidden_chars,
    PathProbe, RealPathProbe,
};
pub use wire::{
    encode_created_warnings, encode_launch_policy_failure, CreateResultWire,
    InteractiveCreateOutcome, CREATED_RESULT_CODE,
};
