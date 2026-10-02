//! Thin C ABI for Rust-owned launch-policy product copy (ADR-015 / #1119).
//!
//! Native hosts render the borrowed UTF-8; they must not invent failure text
//! or decide which result/detail codes are failures vs warnings.

use std::ptr;
use std::slice;

use crate::launch_policy_ux::{
    launch_policy_copies, launch_policy_failure_copy, launch_policy_warning_copy,
};

/// Borrowed UTF-8 product copy. Pointer is to static storage and remains valid
/// for the process lifetime.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalLaunchPolicyCopy {
    pub text: *const u8,
    pub text_len: u32,
    pub reserved: u32,
}

fn borrowed(text: &str) -> SeyalLaunchPolicyCopy {
    SeyalLaunchPolicyCopy {
        text: text.as_ptr(),
        text_len: text.len() as u32,
        reserved: 0,
    }
}

fn empty() -> SeyalLaunchPolicyCopy {
    SeyalLaunchPolicyCopy {
        text: ptr::null(),
        text_len: 0,
        reserved: 0,
    }
}

/// Fixed non-secret failure copy for create-result `result_code` / `detail_code`.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_launch_policy_failure_copy(
    result_code: u16,
    detail_code: u32,
) -> SeyalLaunchPolicyCopy {
    borrowed(launch_policy_failure_copy(result_code, detail_code))
}

/// Fixed non-secret warning copy for `Created.detail_code` bit index 0 or 1.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_launch_policy_warning_copy(bit_index: u32) -> SeyalLaunchPolicyCopy {
    match launch_policy_warning_copy(bit_index) {
        Some(text) => borrowed(text),
        None => empty(),
    }
}

/// Fill `out` with Rust-selected product copies for a create-result pair.
///
/// Returns the number of copies written (capped by `capacity`). When `out` is
/// null, returns the full selected count without writing. Success (`result_code
/// == 0`) yields warning copies; any other result yields the failure copy.
/// Native hosts must only iterate what this returns.
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)] // C ABI out-buffer; callers own the slots.
pub extern "C" fn seyal_launch_policy_copies(
    result_code: u16,
    detail_code: u32,
    out: *mut SeyalLaunchPolicyCopy,
    capacity: u32,
) -> u32 {
    let copies = launch_policy_copies(result_code, detail_code);
    let total = copies.len() as u32;
    if out.is_null() || capacity == 0 {
        return total;
    }
    let n = copies.len().min(capacity as usize);
    let slots = unsafe { slice::from_raw_parts_mut(out, n) };
    for (slot, text) in slots.iter_mut().zip(copies.iter()) {
        *slot = borrowed(text);
    }
    n as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use seyal_protocol::framing::ErrorCode;
    use std::slice;

    fn as_str(copy: SeyalLaunchPolicyCopy) -> String {
        if copy.text.is_null() || copy.text_len == 0 {
            return String::new();
        }
        let bytes = unsafe { slice::from_raw_parts(copy.text, copy.text_len as usize) };
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[test]
    fn ffi_surfaces_rust_owned_failure_and_warning_copy() {
        let failure = seyal_launch_policy_failure_copy(ErrorCode::LaunchPolicyRejected as u16, 2);
        assert_eq!(as_str(failure), "Shell unavailable");
        let warning = seyal_launch_policy_warning_copy(0);
        assert!(as_str(warning).contains("default shell"));
        assert_eq!(as_str(seyal_launch_policy_warning_copy(9)), "");
    }

    #[test]
    fn ffi_copies_selects_from_result_pair_without_host_bit_knowledge() {
        assert_eq!(
            seyal_launch_policy_copies(
                ErrorCode::LaunchPolicyRejected as u16,
                2,
                ptr::null_mut(),
                0
            ),
            1
        );
        let mut buf = [empty(); 4];
        let n = seyal_launch_policy_copies(0, 0b11 | (1 << 7), buf.as_mut_ptr(), buf.len() as u32);
        assert_eq!(n, 2);
        assert!(as_str(buf[0]).contains("default shell"));
        assert!(as_str(buf[1]).contains("home directory"));
        assert_eq!(
            seyal_launch_policy_copies(0, 1 << 7, buf.as_mut_ptr(), buf.len() as u32),
            0
        );
    }
}
