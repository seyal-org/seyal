//! Pure unit tests: argv tables, rejection predicates, warning-versus-failure.

use std::{
    collections::HashMap,
    ffi::OsString,
    path::{Path, PathBuf},
};

use super::*;

#[derive(Default)]
struct MapProbe {
    shells: HashMap<PathBuf, bool>,
    cwds: HashMap<PathBuf, bool>,
}

impl MapProbe {
    fn shell(mut self, path: impl Into<PathBuf>, ok: bool) -> Self {
        self.shells.insert(path.into(), ok);
        self
    }

    fn cwd(mut self, path: impl Into<PathBuf>, ok: bool) -> Self {
        self.cwds.insert(path.into(), ok);
        self
    }
}

impl PathProbe for MapProbe {
    fn is_valid_shell_program(&self, path: &Path) -> bool {
        self.shells.get(path).copied().unwrap_or(false)
    }

    fn is_valid_cwd(&self, path: &Path) -> bool {
        self.cwds.get(path).copied().unwrap_or(false)
    }
}

#[derive(Default)]
struct MapLocale {
    values: HashMap<&'static str, OsString>,
}

impl MapLocale {
    fn set(mut self, key: &'static str, value: impl Into<OsString>) -> Self {
        self.values.insert(key, value.into());
        self
    }
}

impl LocaleEnv for MapLocale {
    fn get(&self, key: &str) -> Option<OsString> {
        self.values.get(key).cloned()
    }
}

fn account(shell: &str) -> AccountRecord {
    AccountRecord::new("alice", "/Users/alice", shell)
}

fn resolve_with(
    intent: &LaunchProfileIntent,
    account: Option<&AccountRecord>,
    process_shell: Option<&Path>,
    probe: &MapProbe,
) -> Result<LaunchPolicyResolution, LaunchPolicyFailure> {
    resolve(ResolveInputs {
        intent,
        account,
        process_shell,
        tmpdir: None,
        locale: &EmptyLocaleEnv,
        probe,
    })
}

#[test]
fn account_record_shell_selected_when_valid() {
    let account = account("/bin/zsh");
    let probe = MapProbe::default()
        .shell("/bin/zsh", true)
        .cwd("/Users/alice", true);
    let out = resolve_with(
        &LaunchProfileIntent::default_interactive(),
        Some(&account),
        None,
        &probe,
    )
    .expect("resolve");
    assert_eq!(out.policy.program(), Path::new("/bin/zsh"));
    assert_eq!(
        out.policy.argv(),
        &[OsString::from("-l"), OsString::from("-i")]
    );
    assert_eq!(out.policy.cwd(), Path::new("/Users/alice"));
    assert!(out.warnings.is_empty());
    assert!(out.policy.clear_environment());
}

#[test]
fn invalid_configured_shell_falls_back_with_warning() {
    let account = account("/bin/zsh");
    let mut intent = LaunchProfileIntent::default_interactive();
    intent.configured_shell = Some(PathBuf::from("/no/such/shell"));
    let probe = MapProbe::default()
        .shell("/no/such/shell", false)
        .shell("/bin/zsh", true)
        .cwd("/Users/alice", true);
    let out = resolve_with(&intent, Some(&account), None, &probe).expect("resolve");
    assert_eq!(out.policy.program(), Path::new("/bin/zsh"));
    assert_eq!(
        out.warnings,
        vec![LaunchPolicyWarning::ConfiguredShellInvalid]
    );
}

#[test]
fn account_lookup_failure_is_account_record_unavailable() {
    let probe = MapProbe::default();
    let err = resolve_with(
        &LaunchProfileIntent::default_interactive(),
        None,
        None,
        &probe,
    )
    .expect_err("must fail");
    assert_eq!(err, LaunchPolicyFailure::AccountRecordUnavailable);
}

#[test]
fn empty_name_or_relative_home_is_account_record_unavailable() {
    let probe = MapProbe::default().cwd("/Users/alice", true);
    let empty_name = AccountRecord::new("", "/Users/alice", "/bin/zsh");
    assert_eq!(
        resolve_with(
            &LaunchProfileIntent::default_interactive(),
            Some(&empty_name),
            None,
            &probe,
        )
        .expect_err("empty name"),
        LaunchPolicyFailure::AccountRecordUnavailable
    );
    let relative_home = AccountRecord::new("alice", "relative", "/bin/zsh");
    assert_eq!(
        resolve_with(
            &LaunchProfileIntent::default_interactive(),
            Some(&relative_home),
            None,
            &probe,
        )
        .expect_err("relative home"),
        LaunchPolicyFailure::AccountRecordUnavailable
    );
}

#[test]
fn exhausted_fallbacks_fail_closed() {
    let account = account("/bad/shell");
    let probe = MapProbe::default().cwd("/Users/alice", true);
    let err = resolve_with(
        &LaunchProfileIntent::default_interactive(),
        Some(&account),
        Some(Path::new("/bad/process")),
        &probe,
    )
    .expect_err("exhausted");
    assert_eq!(err, LaunchPolicyFailure::ShellFallbackExhausted);
}

#[test]
fn login_argv_shapes_match_spec_tables() {
    assert_eq!(
        interactive_login_argv(Path::new("/bin/zsh")),
        vec![OsString::from("-l"), OsString::from("-i")]
    );
    assert_eq!(
        interactive_login_argv(Path::new("/bin/bash")),
        vec![OsString::from("-l"), OsString::from("-i")]
    );
    assert_eq!(
        interactive_login_argv(Path::new("/usr/local/bin/fish")),
        vec![OsString::from("-l"), OsString::from("-i")]
    );
    assert_eq!(
        interactive_login_argv(Path::new("/bin/sh")),
        vec![OsString::from("-i")]
    );

    let account = account("/bad");
    let probe = MapProbe::default()
        .shell("/bin/sh", true)
        .cwd("/Users/alice", true);
    let out = resolve_with(
        &LaunchProfileIntent::default_interactive(),
        Some(&account),
        None,
        &probe,
    )
    .expect("sh fallback");
    assert_eq!(out.policy.program(), Path::new("/bin/sh"));
    assert_eq!(out.policy.argv(), &[OsString::from("-i")]);
    assert_eq!(
        out.warnings,
        vec![LaunchPolicyWarning::ConfiguredShellInvalid]
    );
}

#[test]
fn default_cwd_is_home_invalid_override_warns_invalid_home_fails() {
    let account = account("/bin/zsh");
    let probe = MapProbe::default()
        .shell("/bin/zsh", true)
        .cwd("/Users/alice", true)
        .cwd("/tmp/bad", false);

    let ok = resolve_with(
        &LaunchProfileIntent::default_interactive(),
        Some(&account),
        None,
        &probe,
    )
    .expect("default cwd");
    assert_eq!(ok.policy.cwd(), Path::new("/Users/alice"));

    let mut intent = LaunchProfileIntent::default_interactive();
    intent.cwd_override = Some(PathBuf::from("/tmp/bad"));
    let warned = resolve_with(&intent, Some(&account), None, &probe).expect("override");
    assert_eq!(warned.policy.cwd(), Path::new("/Users/alice"));
    assert_eq!(
        warned.warnings,
        vec![LaunchPolicyWarning::CwdOverrideInvalid]
    );

    let probe_bad_home = MapProbe::default()
        .shell("/bin/zsh", true)
        .cwd("/Users/alice", false);
    let err = resolve_with(
        &LaunchProfileIntent::default_interactive(),
        Some(&account),
        None,
        &probe_bad_home,
    )
    .expect_err("bad home");
    assert_eq!(err, LaunchPolicyFailure::CwdInvalid);
}

#[test]
fn empty_account_shell_warns_configured_shell_invalid_on_safe_default() {
    let account = account("");
    let probe = MapProbe::default()
        .shell("/bin/zsh", true)
        .cwd("/Users/alice", true);
    let out = resolve_with(
        &LaunchProfileIntent::default_interactive(),
        Some(&account),
        None,
        &probe,
    )
    .expect("safe default");
    assert_eq!(out.policy.program(), Path::new("/bin/zsh"));
    assert_eq!(
        out.warnings,
        vec![LaunchPolicyWarning::ConfiguredShellInvalid]
    );
    // SPEC-023 §12 item 15: warning accompanies success; encode as Created bit 0.
    let wire = crate::launch_policy::encode_created_warnings(&out.warnings);
    assert_eq!(wire.result_code, crate::launch_policy::CREATED_RESULT_CODE);
    assert_eq!(wire.detail_code, 1 << 0);
}

#[test]
fn rejection_predicates_relative_and_control_chars() {
    assert!(!is_valid_shell_program(Path::new("zsh")));
    assert!(!is_valid_shell_program(Path::new("bin/zsh")));
    assert!(!is_valid_cwd(Path::new("relative")));
    assert!(!is_valid_cwd(Path::new("/bin/zsh\n")));
    assert!(path_has_forbidden_chars(Path::new("/bin/zsh\n")));
    assert!(path_has_forbidden_chars(Path::new("/Users/alice\0x")));
    assert!(!account_record_usable(
        std::ffi::OsStr::new(""),
        Path::new("/Users/a")
    ));
    assert!(!account_record_usable(
        std::ffi::OsStr::new("alice"),
        Path::new("home")
    ));
}

#[test]
fn warnings_and_failures_are_disjoint_types_without_path_bytes() {
    // Compile-time / API: failure and warning are separate enums.
    let failure = LaunchPolicyFailure::ShellFallbackExhausted;
    let warning = LaunchPolicyWarning::ConfiguredShellInvalid;
    assert_ne!(format!("{failure:?}"), format!("{warning:?}"));

    // Debug / Display carry only the class name — no path or env bytes.
    for failure in [
        LaunchPolicyFailure::AccountRecordUnavailable,
        LaunchPolicyFailure::ShellFallbackExhausted,
        LaunchPolicyFailure::CwdInvalid,
        LaunchPolicyFailure::CapabilityUnavailable,
    ] {
        let rendered = format!("{failure:?}");
        assert_eq!(rendered, failure.as_str());
        assert!(!rendered.contains('/'));
        assert!(!rendered.contains('='));
    }
    for warning in [
        LaunchPolicyWarning::ConfiguredShellInvalid,
        LaunchPolicyWarning::CwdOverrideInvalid,
    ] {
        let rendered = format!("{warning:?}");
        assert_eq!(rendered, warning.as_str());
        assert!(!rendered.contains('/'));
    }
}

#[test]
fn policy_debug_redacts_program_path_and_env() {
    let account = account("/bin/zsh");
    let probe = MapProbe::default()
        .shell("/bin/zsh", true)
        .cwd("/Users/alice", true);
    let locale = MapLocale::default().set("LANG", "en_US.UTF-8");
    let out = resolve(ResolveInputs {
        intent: &LaunchProfileIntent::default_interactive(),
        account: Some(&account),
        process_shell: None,
        tmpdir: None,
        locale: &locale,
        probe: &probe,
    })
    .expect("resolve");
    let debug = format!("{:?}", out.policy);
    assert!(debug.contains("EffectiveLaunchPolicy"));
    assert!(debug.contains("arg_count"));
    assert!(!debug.contains("/bin/zsh"));
    assert!(!debug.contains("/Users/alice"));
    assert!(!debug.contains("LANG"));
    assert!(!debug.contains("en_US"));
}

#[test]
fn process_shell_used_when_account_shell_invalid() {
    let account = account("/bad/shell");
    let probe = MapProbe::default()
        .shell("/bad/shell", false)
        .shell("/opt/homebrew/bin/zsh", true)
        .cwd("/Users/alice", true);
    let out = resolve_with(
        &LaunchProfileIntent::default_interactive(),
        Some(&account),
        Some(Path::new("/opt/homebrew/bin/zsh")),
        &probe,
    )
    .expect("process shell");
    assert_eq!(out.policy.program(), Path::new("/opt/homebrew/bin/zsh"));
    assert_eq!(
        out.warnings,
        vec![LaunchPolicyWarning::ConfiguredShellInvalid]
    );
}

#[test]
fn base_env_has_required_keys_and_optional_locale() {
    let account = account("/bin/zsh");
    let probe = MapProbe::default()
        .shell("/bin/zsh", true)
        .cwd("/Users/alice", true);
    let locale = MapLocale::default()
        .set("LANG", "C")
        .set("LC_CTYPE", "UTF-8")
        .set("LC_ALL", "should-not-copy");
    let out = resolve(ResolveInputs {
        intent: &LaunchProfileIntent::default_interactive(),
        account: Some(&account),
        process_shell: None,
        tmpdir: Some(Path::new("/var/folders/tmp")),
        locale: &locale,
        probe: &probe,
    })
    .expect("resolve");
    let keys: Vec<_> = out
        .policy
        .env()
        .iter()
        .map(|(k, _)| k.to_string_lossy().into_owned())
        .collect();
    assert!(keys.contains(&"HOME".into()));
    assert!(keys.contains(&"USER".into()));
    assert!(keys.contains(&"LOGNAME".into()));
    assert!(keys.contains(&"SHELL".into()));
    assert!(keys.contains(&"PATH".into()));
    assert!(keys.contains(&"TMPDIR".into()));
    assert!(keys.contains(&"LANG".into()));
    assert!(keys.contains(&"LC_CTYPE".into()));
    assert!(!keys.iter().any(|k| k == "LC_ALL"));
    assert!(!keys.iter().any(|k| k == "TERM"));
    assert!(!keys.iter().any(|k| k.starts_with("DYLD_")));
}

#[test]
fn to_command_spec_clears_environment() {
    let account = account("/bin/bash");
    let probe = MapProbe::default()
        .shell("/bin/bash", true)
        .cwd("/Users/alice", true);
    let out = resolve_with(
        &LaunchProfileIntent::default_interactive(),
        Some(&account),
        None,
        &probe,
    )
    .expect("resolve");
    let spec = out.policy.to_command_spec();
    let debug = format!("{spec:?}");
    assert!(debug.contains("clear_environment: true"));
    assert!(!debug.contains("/bin/bash"));
}
