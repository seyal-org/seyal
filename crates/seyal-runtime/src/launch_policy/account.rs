//! Effective-UID account-record lookup for production composition.

use std::{ffi::OsString, path::PathBuf};

#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;

use super::types::AccountRecord;
use super::validate::account_record_usable;

/// Look up the effective user's POSIX account record.
///
/// Returns `None` when the lookup fails, yields no entry, or the name/home
/// fields are unusable. An empty `pw_shell` still returns `Some` — that case is
/// not `AccountRecordUnavailable` (SPEC-023 §5.1).
#[cfg(unix)]
pub fn lookup_effective_account_record() -> Option<AccountRecord> {
    lookup_account_record_for_uid(unsafe { libc::geteuid() })
}

#[cfg(not(unix))]
pub fn lookup_effective_account_record() -> Option<AccountRecord> {
    None
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn lookup_account_record_for_uid(uid: libc::uid_t) -> Option<AccountRecord> {
    // SAFETY: getpwuid_r writes into caller-owned buffers; the returned
    // `passwd` pointers alias those buffers and are copied out before return.
    unsafe {
        let mut pwd = std::mem::MaybeUninit::<libc::passwd>::uninit();
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let mut buf = vec![0u8; 4096];
        loop {
            let rc = libc::getpwuid_r(
                uid,
                pwd.as_mut_ptr(),
                buf.as_mut_ptr().cast(),
                buf.len(),
                &mut result,
            );
            if rc == libc::ERANGE {
                buf.resize(buf.len().saturating_mul(2).max(4096), 0);
                continue;
            }
            if rc != 0 || result.is_null() {
                return None;
            }
            break;
        }
        let pwd = pwd.assume_init();
        let name = c_str_to_os_string(pwd.pw_name)?;
        let home = PathBuf::from(c_str_to_os_string(pwd.pw_dir)?);
        let shell = match c_str_to_os_string(pwd.pw_shell) {
            Some(value) => PathBuf::from(value),
            None => PathBuf::new(),
        };
        if !account_record_usable(&name, &home) {
            return None;
        }
        Some(AccountRecord::new(name, home, shell))
    }
}

#[cfg(unix)]
#[allow(unsafe_code)]
unsafe fn c_str_to_os_string(ptr: *const libc::c_char) -> Option<OsString> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: caller guarantees `ptr` is a valid C string from getpwuid_r.
    let cstr = unsafe { std::ffi::CStr::from_ptr(ptr) };
    let bytes = cstr.to_bytes();
    if bytes.is_empty() {
        return Some(OsString::new());
    }
    Some(OsString::from_vec(bytes.to_vec()))
}
