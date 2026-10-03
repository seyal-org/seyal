//! Thin C ABI for Rust-owned launch-policy product copy (ADR-015 / #1119).
//!
//! Native hosts render the borrowed UTF-8; they must not invent failure text.

use std::ptr;

use crate::launch_policy_ux::{launch_policy_failure_copy, launch_policy_warning_copy};

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
}
