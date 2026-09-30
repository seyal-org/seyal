//! SPEC-023 §12 items 6–9 and 11–14 at the composition seam (L2).
#![allow(unsafe_code)]

use std::{
    collections::HashSet,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

use seyal_exec::CommandSpec;

use crate::{
    m001_term_name, CapabilityPolicy, ShellIntegrationPolicy, NONCE_FD_ENV, USER_ZDOTDIR_ENV,
};

use super::{
    apply_post_policy, command_spec_from_policy, resolve, resolve_default_interactive,
    AccountRecord, EmptyLocaleEnv, LaunchProfileIntent, LocaleEnv, PathProbe,
    ResolveInputs, DEFAULT_PATH,
};

#[derive(Default)]
struct MapProbe {
    shells: std::collections::HashMap<PathBuf, bool>,
    cwds: std::collections::HashMap<PathBuf, bool>,
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
    values: std::collections::HashMap<&'static str, OsString>,
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

fn env_keys(command: &CommandSpec) -> HashSet<String> {
    command
        .environment_overrides()
        .iter()
        .map(|(k, _)| k.to_string_lossy().into_owned())
        .collect()
}

fn env_get<'a>(command: &'a CommandSpec, key: &str) -> Option<&'a OsStr> {
    command
        .environment_overrides()
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_os_str())
}

fn materialize_shell_policy() -> (PathBuf, ShellIntegrationPolicy) {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        sync::atomic::{AtomicU64, Ordering},
    };
    static SEQ: AtomicU64 = AtomicU64::new(1);
    let dir = std::env::temp_dir().join(format!(
        "seyal-compose-si-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    let zshenv = dir.join(".zshenv");
    fs::write(&zshenv, ShellIntegrationPolicy::bundled_zshenv()).unwrap();
    fs::set_permissions(&zshenv, fs::Permissions::from_mode(0o600)).unwrap();
    let policy = ShellIntegrationPolicy::from_zdotdir(&dir).unwrap();
    (dir, policy)
}

/// §12 item 6: helper-like process env (empty locale) still resolves.
#[test]
fn helper_like_empty_locale_still_resolves() {
    let account = account("/bin/zsh");
    let probe = MapProbe::default()
        .shell("/bin/zsh", true)
        .cwd("/Users/alice", true);
    let out = resolve(ResolveInputs {
        intent: &LaunchProfileIntent::default_interactive(),
        account: Some(&account),
        process_shell: None,
        tmpdir: None,
        locale: &EmptyLocaleEnv,
        probe: &probe,
    })
    .expect("helper-like env");
    assert!(out.policy.env().iter().all(|(k, _)| {
        let key = k.to_string_lossy();
        key != "LANG" && key != "LC_CTYPE"
    }));
    let spec = command_spec_from_policy(&out);
    assert!(spec.clears_environment());
}

/// §12 item 7: poisoned parent keys absent; child key set is allowlist ∪ carve-outs.
#[test]
fn poisoned_parent_env_absent_and_key_set_exact_with_and_without_integration() {
    let zsh_record = account("/bin/zsh");
    let probe = MapProbe::default()
        .shell("/bin/zsh", true)
        .cwd("/Users/alice", true);
    let locale = MapLocale::default().set("LANG", "C");
    let out = resolve(ResolveInputs {
        intent: &LaunchProfileIntent::default_interactive(),
        account: Some(&zsh_record),
        process_shell: None,
        tmpdir: Some(Path::new("/var/folders/tmp")),
        locale: &locale,
        probe: &probe,
    })
    .expect("resolve");

    let capability = CapabilityPolicy::bundled().expect("capability");
    let zsh_base = command_spec_from_policy(&out);

    // Not eligible (bash program) — no shell-integration carve-outs.
    let bash_record = account("/bin/bash");
    let bash_probe = MapProbe::default()
        .shell("/bin/bash", true)
        .cwd("/Users/alice", true);
    let bash = resolve(ResolveInputs {
        intent: &LaunchProfileIntent::default_interactive(),
        account: Some(&bash_record),
        process_shell: None,
        tmpdir: Some(Path::new("/var/folders/tmp")),
        locale: &locale,
        probe: &bash_probe,
    })
    .expect("bash");
    let bash_spec =
        apply_post_policy(command_spec_from_policy(&bash), &capability, None).expect("apply");
    let keys = env_keys(&bash_spec);
    for forbidden in [
        "DYLD_INSERT_LIBRARIES",
        "SSH_AUTH_SOCK",
        "COLORTERM",
        "TERMINFO_DIRS",
    ] {
        assert!(!keys.contains(forbidden), "leaked {forbidden}");
    }
    assert!(keys.contains("HOME"));
    assert!(keys.contains("USER"));
    assert!(keys.contains("LOGNAME"));
    assert!(keys.contains("SHELL"));
    assert!(keys.contains("PATH"));
    assert!(keys.contains("TMPDIR"));
    assert!(keys.contains("LANG"));
    assert!(keys.contains("TERM"));
    assert!(keys.contains("TERMINFO"));
    assert!(!keys.contains("ZDOTDIR"));
    assert!(!keys.contains(USER_ZDOTDIR_ENV));
    assert!(!keys.contains(NONCE_FD_ENV));
    assert_eq!(env_get(&bash_spec, "PATH"), Some(OsStr::new(DEFAULT_PATH)));
    assert_eq!(
        env_get(&bash_spec, "TERM"),
        Some(OsStr::new(m001_term_name()))
    );

    // Eligible zsh — carve-outs present; SEYAL_USER_ZDOTDIR absent without process ZDOTDIR.
    let _guard = super::process_env_test_lock();
    let original = std::env::var_os("ZDOTDIR");
    // SAFETY: test holds process_env_test_lock; no concurrent env readers in this process.
    unsafe { std::env::remove_var("ZDOTDIR") };
    let (si_dir, si) = materialize_shell_policy();
    let zsh_spec = apply_post_policy(zsh_base, &capability, Some(&si)).expect("zsh apply");
    let keys = env_keys(&zsh_spec);
    assert!(keys.contains("ZDOTDIR"));
    assert!(keys.contains(NONCE_FD_ENV));
    assert!(!keys.contains(USER_ZDOTDIR_ENV));
    assert!(!keys.contains("DYLD_INSERT_LIBRARIES"));
    assert_eq!(zsh_spec.inherited_fd_count(), 1);

    // Eligible with valid process ZDOTDIR → SEYAL_USER_ZDOTDIR present.
    let user_zdot = si_dir.join("user");
    std::fs::create_dir_all(&user_zdot).unwrap();
    // SAFETY: test holds LOCK.
    unsafe { std::env::set_var("ZDOTDIR", &user_zdot) };
    let with_user = apply_post_policy(command_spec_from_policy(&out), &capability, Some(&si))
        .expect("with user zdotdir");
    assert_eq!(
        env_get(&with_user, USER_ZDOTDIR_ENV).map(PathBuf::from),
        Some(user_zdot)
    );

    // SAFETY: restore process env under LOCK.
    unsafe {
        match original {
            Some(value) => std::env::set_var("ZDOTDIR", value),
            None => std::env::remove_var("ZDOTDIR"),
        }
    }
    std::fs::remove_dir_all(si_dir).unwrap();
}

/// §12 item 8: TERM/TERMINFO present; COLORTERM and TERMINFO_DIRS absent.
#[test]
fn term_terminfo_present_colorterm_and_terminfo_dirs_absent() {
    let account = account("/bin/zsh");
    let probe = MapProbe::default()
        .shell("/bin/zsh", true)
        .cwd("/Users/alice", true);
    let out = resolve(ResolveInputs {
        intent: &LaunchProfileIntent::default_interactive(),
        account: Some(&account),
        process_shell: None,
        tmpdir: None,
        locale: &EmptyLocaleEnv,
        probe: &probe,
    })
    .expect("resolve");
    let capability = CapabilityPolicy::bundled().expect("capability");
    // Poison process TERMINFO_DIRS — must not appear on the child.
    let _guard = super::process_env_test_lock();
    let original = std::env::var_os("TERMINFO_DIRS");
    // SAFETY: test holds process_env_test_lock; no concurrent env readers in this process.
    unsafe { std::env::set_var("TERMINFO_DIRS", "/evil/terminfo") };
    let spec = apply_post_policy(command_spec_from_policy(&out), &capability, None).unwrap();
    assert_eq!(env_get(&spec, "TERM"), Some(OsStr::new(m001_term_name())));
    assert!(env_get(&spec, "TERMINFO").is_some());
    assert!(env_get(&spec, "COLORTERM").is_none());
    assert!(env_get(&spec, "TERMINFO_DIRS").is_none());
    // SAFETY: restore process env under LOCK.
    unsafe {
        match original {
            Some(value) => std::env::set_var("TERMINFO_DIRS", value),
            None => std::env::remove_var("TERMINFO_DIRS"),
        }
    }
}

/// §12 item 9: OSC 7 / Pane title are not resolution inputs.
#[test]
fn osc7_and_pane_title_cannot_steer_cwd_or_program() {
    // ResolveInputs has no OSC/Pane fields; cwd and program come only from
    // account/intent/process SHELL / safe fallbacks.
    let account = account("/bin/zsh");
    let probe = MapProbe::default()
        .shell("/bin/zsh", true)
        .cwd("/Users/alice", true);
    let out = resolve(ResolveInputs {
        intent: &LaunchProfileIntent::default_interactive(),
        account: Some(&account),
        process_shell: None,
        tmpdir: None,
        locale: &EmptyLocaleEnv,
        probe: &probe,
    })
    .expect("resolve");
    assert_eq!(out.policy.cwd(), Path::new("/Users/alice"));
    assert_eq!(out.policy.program(), Path::new("/bin/zsh"));
}

/// §12 items 11–12: unavailable CapabilityPolicy does not block compose;
/// interactive create owns the `CapabilityUnavailable` gate (see
/// `launch_policy_create::capability_unavailable_publishes_zero_executions`).
#[test]
fn capability_unavailable_still_composes_for_explicit_argv() {
    let dir = std::env::temp_dir().join(format!(
        "seyal-cap-missing-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let capability = CapabilityPolicy::from_terminfo_dir(&dir).unwrap();
    assert!(!capability.is_available());
    let account = account("/bin/zsh");
    let probe = MapProbe::default()
        .shell("/bin/zsh", true)
        .cwd("/Users/alice", true);
    let out = resolve(ResolveInputs {
        intent: &LaunchProfileIntent::default_interactive(),
        account: Some(&account),
        process_shell: None,
        tmpdir: None,
        locale: &EmptyLocaleEnv,
        probe: &probe,
    })
    .expect("resolve");
    let spec = apply_post_policy(command_spec_from_policy(&out), &capability, None)
        .expect("compose must not gate CapabilityUnavailable");
    assert_eq!(env_get(&spec, "TERM"), Some(OsStr::new(m001_term_name())));
    assert_eq!(env_get(&spec, "TERMINFO"), Some(dir.as_os_str()));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// §12 item 12: explicit developer argv CommandSpec remains a distinct bypass shape.
#[test]
fn developer_explicit_argv_is_not_profile_zero_command_spec() {
    let bypass = CommandSpec::new("/bin/sh").args(["-c", "true"]);
    assert!(!bypass.clears_environment());
    assert_eq!(bypass.program(), OsStr::new("/bin/sh"));
    assert_eq!(
        bypass.args_slice(),
        &[OsString::from("-c"), OsString::from("true")]
    );

    let account = account("/bin/zsh");
    let probe = MapProbe::default()
        .shell("/bin/zsh", true)
        .cwd("/Users/alice", true);
    let out = resolve(ResolveInputs {
        intent: &LaunchProfileIntent::default_interactive(),
        account: Some(&account),
        process_shell: None,
        tmpdir: None,
        locale: &EmptyLocaleEnv,
        probe: &probe,
    })
    .expect("resolve");
    let profile0 = command_spec_from_policy(&out);
    assert!(profile0.clears_environment());
    assert_ne!(bypass.program(), profile0.program());
}

/// §12 item 14: only LANG and LC_CTYPE copy; LC_ALL / LC_MESSAGES never.
#[test]
fn locale_copies_only_lang_and_lc_ctype() {
    let account = account("/bin/zsh");
    let probe = MapProbe::default()
        .shell("/bin/zsh", true)
        .cwd("/Users/alice", true);
    let locale = MapLocale::default()
        .set("LANG", "en_US.UTF-8")
        .set("LC_CTYPE", "UTF-8")
        .set("LC_ALL", "C")
        .set("LC_MESSAGES", "C");
    let out = resolve(ResolveInputs {
        intent: &LaunchProfileIntent::default_interactive(),
        account: Some(&account),
        process_shell: None,
        tmpdir: None,
        locale: &locale,
        probe: &probe,
    })
    .expect("resolve");
    let keys: HashSet<_> = out
        .policy
        .env()
        .iter()
        .map(|(k, _)| k.to_string_lossy().into_owned())
        .collect();
    assert!(keys.contains("LANG"));
    assert!(keys.contains("LC_CTYPE"));
    assert!(!keys.contains("LC_ALL"));
    assert!(!keys.contains("LC_MESSAGES"));
}

/// Live default interactive resolve succeeds under a normal developer machine.
#[test]
fn live_default_interactive_resolves_on_macos() {
    let out = resolve_default_interactive().expect("live account + shell fallback");
    assert!(out.policy.program().is_absolute());
    assert!(out.policy.cwd().is_absolute());
    assert!(out.policy.clear_environment());
}
