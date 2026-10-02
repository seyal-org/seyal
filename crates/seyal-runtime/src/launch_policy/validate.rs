//! Shell program and CWD validation predicates (SPEC-023 §5.3 / §7).

use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// True when the path string contains NUL or an ASCII control character.
pub fn path_has_forbidden_chars(path: &Path) -> bool {
    path.as_os_str()
        .as_encoded_bytes()
        .iter()
        .any(|&b| b == 0 || b < 0x20)
}

/// SPEC-023 §5.3: absolute, existing regular file, executable, no NUL/control.
pub fn is_valid_shell_program(path: &Path) -> bool {
    if !path.is_absolute() || path_has_forbidden_chars(path) {
        return false;
    }
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// SPEC-023 §7: absolute existing directory, no NUL/control.
pub fn is_valid_cwd(path: &Path) -> bool {
    if !path.is_absolute() || path_has_forbidden_chars(path) {
        return false;
    }
    path.is_dir()
}

/// Account-record precondition (SPEC-023 §5.1): non-empty name and absolute home.
pub fn account_record_usable(name: &std::ffi::OsStr, home: &Path) -> bool {
    !name.is_empty() && home.is_absolute() && !path_has_forbidden_chars(home)
}

/// Injectable filesystem / permission probes for pure unit tests.
pub trait PathProbe {
    fn is_valid_shell_program(&self, path: &Path) -> bool;
    fn is_valid_cwd(&self, path: &Path) -> bool;
}

/// Production probe that uses the real filesystem.
#[derive(Clone, Copy, Debug, Default)]
pub struct RealPathProbe;

impl PathProbe for RealPathProbe {
    fn is_valid_shell_program(&self, path: &Path) -> bool {
        is_valid_shell_program(path)
    }

    fn is_valid_cwd(&self, path: &Path) -> bool {
        is_valid_cwd(path)
    }
}
