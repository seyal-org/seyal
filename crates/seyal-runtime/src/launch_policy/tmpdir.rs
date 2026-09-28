//! Darwin per-user temporary directory for child `TMPDIR` (SPEC-023 §6).

use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

/// Resolve `confstr(_CS_DARWIN_USER_TEMP_DIR)` when it is an absolute existing
/// directory owned by the effective UID. Never copies Runtime-process `TMPDIR`.
#[cfg(target_os = "macos")]
pub fn darwin_user_temp_dir() -> Option<PathBuf> {
    let path = confstr_darwin_user_temp_dir()?;
    if is_valid_tmpdir(&path) {
        Some(path)
    } else {
        None
    }
}

#[cfg(not(target_os = "macos"))]
pub fn darwin_user_temp_dir() -> Option<PathBuf> {
    None
}

pub fn is_valid_tmpdir(path: &Path) -> bool {
    if !path.is_absolute() {
        return false;
    }
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_dir() {
        return false;
    }
    #[cfg(unix)]
    {
        // SAFETY: geteuid reads process credentials only.
        let uid = unsafe { libc::geteuid() };
        metadata.uid() == uid
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn confstr_darwin_user_temp_dir() -> Option<PathBuf> {
    // SAFETY: standard two-call confstr pattern with owned storage.
    let required =
        unsafe { libc::confstr(libc::_CS_DARWIN_USER_TEMP_DIR, std::ptr::null_mut(), 0) };
    if required == 0 {
        return None;
    }
    let mut buffer = vec![0u8; required];
    // SAFETY: buffer length matches the size reported above.
    let written = unsafe {
        libc::confstr(
            libc::_CS_DARWIN_USER_TEMP_DIR,
            buffer.as_mut_ptr().cast::<libc::c_char>(),
            buffer.len(),
        )
    };
    if written == 0 || written > buffer.len() {
        return None;
    }
    if let Some(nul_index) = buffer.iter().position(|&b| b == 0) {
        buffer.truncate(nul_index);
    }
    if buffer.is_empty() {
        return None;
    }
    Some(PathBuf::from(std::ffi::OsString::from_vec(buffer)))
}
