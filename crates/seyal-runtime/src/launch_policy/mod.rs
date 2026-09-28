//! Cold-path `EffectiveLaunchPolicy` types and pure resolver (ADR-020 / SPEC-023).
//!
//! Ownership: Runtime composition builds this object. It does not own a PTY,
//! spawn a child, or edit the provisioning protocol.

#[allow(unsafe_code)]
mod account;
mod argv;
mod env;
mod resolve;
mod types;
mod validate;

#[cfg(test)]
mod tests;

pub use account::lookup_effective_account_record;
pub use argv::{interactive_login_argv, ShellFamily};
pub use env::{EmptyLocaleEnv, LocaleEnv, ProcessLocaleEnv, DEFAULT_PATH};
pub use resolve::{resolve, ResolveInputs, PLATFORM_SAFE_FALLBACKS};
pub use types::{
    AccountRecord, CapabilityProfileId, EffectiveLaunchPolicy, LaunchPolicyFailure,
    LaunchPolicyResolution, LaunchPolicyWarning, LaunchProfileIntent,
};
pub use validate::{
    account_record_usable, is_valid_cwd, is_valid_shell_program, path_has_forbidden_chars,
    PathProbe, RealPathProbe,
};
