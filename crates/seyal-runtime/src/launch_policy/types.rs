//! Portable launch-policy types (ADR-020 §3.2 / §3.10, SPEC-023 §4 / §9).

use std::{
    ffi::OsString,
    fmt,
    path::{Path, PathBuf},
};

use seyal_exec::CommandSpec;

/// Capability profile selector carried on the policy object. CapabilityPolicy
/// (ADR-008) owns TERM/TERMINFO application after conversion to `CommandSpec`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CapabilityProfileId(u32);

impl CapabilityProfileId {
    /// M001 bundled terminfo profile (`seyal-m001`).
    pub const M001: Self = Self(0);

    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u32 {
        self.0
    }
}

impl fmt::Debug for CapabilityProfileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("CapabilityProfileId").field(&self.0).finish()
    }
}

/// Runtime-local intent derived from a launch-profile selector (profile `0`
/// today) plus optional cold `[shell]` config (L4). Named profiles may extend
/// fields later; they do not put paths on the provisioning wire (ADR-017).
#[derive(Clone)]
pub struct LaunchProfileIntent {
    /// Optional configured shell override from `[shell].program`.
    pub configured_shell: Option<PathBuf>,
    /// Optional explicit CWD override from `[shell].cwd`.
    pub cwd_override: Option<PathBuf>,
    /// Login interactive when true (default). `false` requests non-login
    /// interactive argv for families that support a login flag. `/bin/sh` and
    /// unknown families remain non-login regardless (SPEC-023 §5.2).
    pub login: bool,
    pub capability_profile: CapabilityProfileId,
}

impl LaunchProfileIntent {
    /// Profile `0` default interactive intent (ADR-020 §3.9 / SPEC-023 §10).
    pub fn default_interactive() -> Self {
        Self {
            configured_shell: None,
            cwd_override: None,
            login: true,
            capability_profile: CapabilityProfileId::M001,
        }
    }
}

impl fmt::Debug for LaunchProfileIntent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LaunchProfileIntent")
            .field("has_configured_shell", &self.configured_shell.is_some())
            .field("has_cwd_override", &self.cwd_override.is_some())
            .field("login", &self.login)
            .field("capability_profile", &self.capability_profile)
            .finish_non_exhaustive()
    }
}

/// Effective UID POSIX account fields that launch policy may consume.
/// Runtime-process `HOME`/`USER`/`LOGNAME` are never substituted for these.
#[derive(Clone)]
pub struct AccountRecord {
    pub name: OsString,
    pub home: PathBuf,
    /// Account shell path; empty means "no shell field" (not a lookup failure).
    pub shell: PathBuf,
}

impl AccountRecord {
    pub fn new(
        name: impl Into<OsString>,
        home: impl Into<PathBuf>,
        shell: impl Into<PathBuf>,
    ) -> Self {
        Self {
            name: name.into(),
            home: home.into(),
            shell: shell.into(),
        }
    }

    pub fn name(&self) -> &std::ffi::OsStr {
        &self.name
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn shell(&self) -> &Path {
        &self.shell
    }
}

impl fmt::Debug for AccountRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccountRecord")
            .field("name_len", &self.name.len())
            .field("home_is_absolute", &self.home.is_absolute())
            .field("shell_is_empty", &self.shell.as_os_str().is_empty())
            .finish_non_exhaustive()
    }
}

/// Validated cold-path spawn inputs. Constructed only inside Runtime.
#[derive(Clone)]
pub struct EffectiveLaunchPolicy {
    program: PathBuf,
    argv: Vec<OsString>,
    cwd: PathBuf,
    clear_environment: bool,
    env: Vec<(OsString, OsString)>,
    capability_profile: CapabilityProfileId,
}

impl EffectiveLaunchPolicy {
    pub fn program(&self) -> &Path {
        &self.program
    }

    pub fn argv(&self) -> &[OsString] {
        &self.argv
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn clear_environment(&self) -> bool {
        self.clear_environment
    }

    pub fn env(&self) -> &[(OsString, OsString)] {
        &self.env
    }

    pub fn capability_profile(&self) -> CapabilityProfileId {
        self.capability_profile
    }

    /// Pure conversion preceding CapabilityPolicy / ShellIntegrationPolicy.
    pub fn to_command_spec(&self) -> CommandSpec {
        let mut spec = CommandSpec::new(&self.program)
            .args(self.argv.iter().cloned())
            .current_dir(&self.cwd);
        if self.clear_environment {
            spec = spec.clear_environment();
        }
        for (key, value) in &self.env {
            spec = spec.env(key, value);
        }
        spec
    }

    pub(super) fn from_parts(
        program: PathBuf,
        argv: Vec<OsString>,
        cwd: PathBuf,
        env: Vec<(OsString, OsString)>,
        capability_profile: CapabilityProfileId,
    ) -> Self {
        Self {
            program,
            argv,
            cwd,
            clear_environment: true,
            env,
            capability_profile,
        }
    }
}

impl fmt::Debug for EffectiveLaunchPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EffectiveLaunchPolicy")
            .field("arg_count", &self.argv.len())
            .field("clear_environment", &self.clear_environment)
            .field("environment_override_count", &self.env.len())
            .field("capability_profile", &self.capability_profile)
            .finish_non_exhaustive()
    }
}

/// Pre-spawn failure: nothing is published and no child starts.
/// Carries no path or env bytes (SPEC-023 §9).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum LaunchPolicyFailure {
    AccountRecordUnavailable,
    ShellFallbackExhausted,
    CwdInvalid,
    /// Reserved for CapabilityPolicy apply failures (ADR-008); not produced by
    /// the pure resolver itself.
    CapabilityUnavailable,
}

impl fmt::Debug for LaunchPolicyFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for LaunchPolicyFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::error::Error for LaunchPolicyFailure {}

impl LaunchPolicyFailure {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AccountRecordUnavailable => "AccountRecordUnavailable",
            Self::ShellFallbackExhausted => "ShellFallbackExhausted",
            Self::CwdInvalid => "CwdInvalid",
            Self::CapabilityUnavailable => "CapabilityUnavailable",
        }
    }

    /// Post-L0 `detail_code` values on `17 LaunchPolicyRejected` (SPEC-023 §9).
    pub const fn detail_code(self) -> u32 {
        match self {
            Self::AccountRecordUnavailable => 1,
            Self::ShellFallbackExhausted => 2,
            Self::CwdInvalid => 3,
            Self::CapabilityUnavailable => 4,
        }
    }
}

/// Spawn succeeded with a bounded, non-secret warning. Disjoint from failures.
/// Never carries the rejected path.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum LaunchPolicyWarning {
    ConfiguredShellInvalid,
    CwdOverrideInvalid,
}

impl fmt::Debug for LaunchPolicyWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for LaunchPolicyWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl LaunchPolicyWarning {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ConfiguredShellInvalid => "ConfiguredShellInvalid",
            Self::CwdOverrideInvalid => "CwdOverrideInvalid",
        }
    }

    /// `Created.detail_code` bit for this warning (SPEC-004 §18.3 / ADR-020 §3.10).
    pub const fn created_detail_bit(self) -> u32 {
        match self {
            Self::ConfiguredShellInvalid => 1 << 0,
            Self::CwdOverrideInvalid => 1 << 1,
        }
    }
}

/// Successful resolution: policy object plus zero or more warnings.
#[derive(Clone, Debug)]
pub struct LaunchPolicyResolution {
    pub policy: EffectiveLaunchPolicy,
    pub warnings: Vec<LaunchPolicyWarning>,
}
