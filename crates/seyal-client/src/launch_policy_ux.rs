//! Portable launch-policy product UI copy (ADR-015 / ADR-020 §3.10 / SPEC-023 §9).
//!
//! Rust owns the fixed non-secret strings. Native hosts only render the borrowed
//! UTF-8 this module selects. Default copy never includes OS strerror, path,
//! argv, or environment values.

use seyal_protocol::framing::ErrorCode;

/// SPEC-004 §15 / §18.3 detail codes on `17 LaunchPolicyRejected`.
pub const DETAIL_ACCOUNT_RECORD_UNAVAILABLE: u32 = 1;
pub const DETAIL_SHELL_FALLBACK_EXHAUSTED: u32 = 2;
pub const DETAIL_CWD_INVALID: u32 = 3;
pub const DETAIL_CAPABILITY_UNAVAILABLE: u32 = 4;

/// `Created.detail_code` warning bits (SPEC-004 §18.3).
pub const WARN_CONFIGURED_SHELL_INVALID: u32 = 1 << 0;
pub const WARN_CWD_OVERRIDE_INVALID: u32 = 1 << 1;

const GENERIC_FAILURE: &str = "New terminal could not start";
const ACCOUNT_UNAVAILABLE: &str = "Account information unavailable";
const SHELL_UNAVAILABLE: &str = "Shell unavailable";
const CWD_UNAVAILABLE: &str = "Working directory unavailable";
const CAPABILITY_UNAVAILABLE: &str = "Terminal capability unavailable";
const SHELL_FALLBACK_WARNING: &str =
    "Using the default shell because the configured shell is invalid";
const CWD_FALLBACK_WARNING: &str =
    "Using the home directory because the working directory override is invalid";

/// Bounded failure copy for a create-result pair. Unknown codes stay generic.
pub fn launch_policy_failure_copy(result_code: u16, detail_code: u32) -> &'static str {
    if result_code != ErrorCode::LaunchPolicyRejected as u16 {
        return GENERIC_FAILURE;
    }
    match detail_code {
        DETAIL_ACCOUNT_RECORD_UNAVAILABLE => ACCOUNT_UNAVAILABLE,
        DETAIL_SHELL_FALLBACK_EXHAUSTED => SHELL_UNAVAILABLE,
        DETAIL_CWD_INVALID => CWD_UNAVAILABLE,
        DETAIL_CAPABILITY_UNAVAILABLE => CAPABILITY_UNAVAILABLE,
        _ => GENERIC_FAILURE,
    }
}

/// Bounded warning copy for one `Created.detail_code` bit index (0 or 1).
pub fn launch_policy_warning_copy(bit_index: u32) -> Option<&'static str> {
    match bit_index {
        0 => Some(SHELL_FALLBACK_WARNING),
        1 => Some(CWD_FALLBACK_WARNING),
        _ => None,
    }
}

/// All warning copies set in a `Created.detail_code` bitfield (reserved bits ignored).
pub fn launch_policy_warning_copies(detail_code: u32) -> Vec<&'static str> {
    let mut out = Vec::new();
    if detail_code & WARN_CONFIGURED_SHELL_INVALID != 0 {
        out.push(SHELL_FALLBACK_WARNING);
    }
    if detail_code & WARN_CWD_OVERRIDE_INVALID != 0 {
        out.push(CWD_FALLBACK_WARNING);
    }
    out
}

/// Selected product copies for a create-result pair (ADR-015).
///
/// `result_code == 0` (Created) yields warning copies from `detail_code`.
/// Any non-zero result yields the single failure copy. Native hosts must not
/// re-decide which codes or bits are failures vs warnings.
pub fn launch_policy_copies(result_code: u16, detail_code: u32) -> Vec<&'static str> {
    if result_code == 0 {
        launch_policy_warning_copies(detail_code)
    } else {
        vec![launch_policy_failure_copy(result_code, detail_code)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_failure_class_has_bounded_non_secret_copy() {
        let cases = [
            (DETAIL_ACCOUNT_RECORD_UNAVAILABLE, ACCOUNT_UNAVAILABLE),
            (DETAIL_SHELL_FALLBACK_EXHAUSTED, SHELL_UNAVAILABLE),
            (DETAIL_CWD_INVALID, CWD_UNAVAILABLE),
            (DETAIL_CAPABILITY_UNAVAILABLE, CAPABILITY_UNAVAILABLE),
        ];
        for (detail, expected) in cases {
            let copy = launch_policy_failure_copy(ErrorCode::LaunchPolicyRejected as u16, detail);
            assert_eq!(copy, expected);
            assert!(!copy.contains('/'));
            assert!(!copy.contains('='));
            assert!(!copy.contains("strerror"));
            assert!(!copy.contains("HOME"));
            assert!(!copy.contains("PATH"));
        }
    }

    #[test]
    fn unknown_detail_and_non_17_codes_are_generic() {
        assert_eq!(
            launch_policy_failure_copy(ErrorCode::LaunchPolicyRejected as u16, 99),
            GENERIC_FAILURE
        );
        assert_eq!(
            launch_policy_failure_copy(ErrorCode::InternalFailure as u16, 1),
            GENERIC_FAILURE
        );
        assert_eq!(
            launch_policy_failure_copy(15, 1),
            GENERIC_FAILURE,
            "codes 15/16 stay unknown on this stack"
        );
        assert_eq!(launch_policy_failure_copy(16, 1), GENERIC_FAILURE);
    }

    #[test]
    fn warnings_are_bounded_and_never_look_like_create_failure() {
        assert_eq!(launch_policy_warning_copy(0), Some(SHELL_FALLBACK_WARNING));
        assert_eq!(launch_policy_warning_copy(1), Some(CWD_FALLBACK_WARNING));
        assert_eq!(launch_policy_warning_copy(2), None);
        let copies = launch_policy_warning_copies(
            WARN_CONFIGURED_SHELL_INVALID | WARN_CWD_OVERRIDE_INVALID | (1 << 7),
        );
        assert_eq!(copies, vec![SHELL_FALLBACK_WARNING, CWD_FALLBACK_WARNING]);
        for copy in copies {
            assert!(!copy.contains('/'));
            assert_ne!(copy, GENERIC_FAILURE);
        }
    }

    #[test]
    fn copies_selects_failure_or_warnings_from_result_pair() {
        assert_eq!(
            launch_policy_copies(
                ErrorCode::LaunchPolicyRejected as u16,
                DETAIL_SHELL_FALLBACK_EXHAUSTED
            ),
            vec![SHELL_UNAVAILABLE]
        );
        assert_eq!(
            launch_policy_copies(0, WARN_CONFIGURED_SHELL_INVALID | (1 << 7)),
            vec![SHELL_FALLBACK_WARNING]
        );
        assert_eq!(
            launch_policy_copies(0, WARN_CONFIGURED_SHELL_INVALID | WARN_CWD_OVERRIDE_INVALID),
            vec![SHELL_FALLBACK_WARNING, CWD_FALLBACK_WARNING]
        );
        assert!(launch_policy_copies(0, 1 << 7).is_empty());
        assert_eq!(
            launch_policy_copies(ErrorCode::InternalFailure as u16, 1),
            vec![GENERIC_FAILURE]
        );
    }
}
