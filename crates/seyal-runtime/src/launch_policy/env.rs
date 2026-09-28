//! Clear-then-allowlist environment builder (ADR-020 §3.6 / SPEC-023 §6).
//!
//! CapabilityPolicy and ShellIntegrationPolicy keys are *not* added here; those
//! owners apply after `EffectiveLaunchPolicy::to_command_spec`.

use std::{
    ffi::{OsStr, OsString},
    path::Path,
};

/// Default interactive `PATH` (SPEC-023 §6).
pub const DEFAULT_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

const LOCALE_MAX_BYTES: usize = 128;

/// Optional locale copy source (Runtime process env). Keys are requested by name.
pub trait LocaleEnv {
    fn get(&self, key: &str) -> Option<OsString>;
}

/// Empty locale source (helper-like process env with no locale keys).
#[derive(Clone, Copy, Debug, Default)]
pub struct EmptyLocaleEnv;

impl LocaleEnv for EmptyLocaleEnv {
    fn get(&self, _key: &str) -> Option<OsString> {
        None
    }
}

/// Reads locale keys from the current process environment.
#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessLocaleEnv;

impl LocaleEnv for ProcessLocaleEnv {
    fn get(&self, key: &str) -> Option<OsString> {
        std::env::var_os(key)
    }
}

fn locale_value_ok(value: &OsStr) -> bool {
    let bytes = value.as_encoded_bytes();
    if bytes.is_empty() || bytes.len() > LOCALE_MAX_BYTES {
        return false;
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.chars().all(|c| !c.is_control()),
        Err(_) => false,
    }
}

/// Build the base allowlisted child environment. Does not set TERM/TERMINFO /
/// ZDOTDIR / SEYAL_* carve-outs.
pub fn build_base_env(
    account_name: &OsStr,
    home: &Path,
    program: &Path,
    tmpdir: Option<&Path>,
    locale: &dyn LocaleEnv,
) -> Vec<(OsString, OsString)> {
    let mut env = Vec::with_capacity(8);
    env.push((OsString::from("HOME"), home.as_os_str().to_os_string()));
    env.push((OsString::from("USER"), account_name.to_os_string()));
    env.push((OsString::from("LOGNAME"), account_name.to_os_string()));
    env.push((OsString::from("SHELL"), program.as_os_str().to_os_string()));
    env.push((OsString::from("PATH"), OsString::from(DEFAULT_PATH)));
    if let Some(dir) = tmpdir {
        if dir.is_absolute() {
            env.push((OsString::from("TMPDIR"), dir.as_os_str().to_os_string()));
        }
    }
    for key in ["LANG", "LC_CTYPE"] {
        if let Some(value) = locale.get(key) {
            if locale_value_ok(&value) {
                env.push((OsString::from(key), value));
            }
        }
    }
    env
}
