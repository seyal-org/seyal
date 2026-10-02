//! Post-L0 create-result encoding for launch-policy outcomes (SPEC-023 §9 / §12.17).
//!
//! This stack does not yet carry `CreateExecutionResult` (message type 37). The
//! pure mapping below is the sole authority for `result_code` / `detail_code`
//! values and is applied on the interactive create path that exists today.
//! There is no parallel code-14 mapping.

use seyal_protocol::framing::ErrorCode;

use super::types::{LaunchPolicyFailure, LaunchPolicyWarning};
use crate::ExecutionId;

/// SPEC-004 §18.3 create-result fields without inventing message type 37.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreateResultWire {
    pub result_code: u16,
    pub detail_code: u32,
}

/// Successful interactive create plus post-L0 `Created.detail_code` warning bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InteractiveCreateOutcome {
    pub execution_id: ExecutionId,
    /// SPEC-004 §18.3 warning bitfield; create succeeded regardless of bits.
    pub detail_code: u32,
}

/// `Created` result code (SPEC-004 §18.3).
pub const CREATED_RESULT_CODE: u16 = 0;

/// Map a pre-spawn failure to `17 LaunchPolicyRejected` with §9 detail codes 1–4.
///
/// This is the only create-path encoding for [`LaunchPolicyFailure`]. It must
/// never return `14 InternalFailure`.
pub fn encode_launch_policy_failure(failure: LaunchPolicyFailure) -> CreateResultWire {
    CreateResultWire {
        result_code: ErrorCode::LaunchPolicyRejected as u16,
        detail_code: failure.detail_code(),
    }
}

/// Map success-after-fallback warnings to `Created.detail_code` bits (SPEC-004 §18.3).
///
/// Bit 0 = `ConfiguredShellInvalid`, bit 1 = `CwdOverrideInvalid`. Reserved bits
/// stay 0. A warning never becomes a create failure.
pub fn encode_created_warnings(warnings: &[LaunchPolicyWarning]) -> CreateResultWire {
    let mut detail_code = 0u32;
    for warning in warnings {
        detail_code |= warning.created_detail_bit();
    }
    CreateResultWire {
        result_code: CREATED_RESULT_CODE,
        detail_code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch_policy::types::{LaunchPolicyFailure, LaunchPolicyWarning};

    #[test]
    fn item_17_each_failure_maps_to_code_17_with_detail_1_through_4() {
        let cases = [
            (LaunchPolicyFailure::AccountRecordUnavailable, 1u32),
            (LaunchPolicyFailure::ShellFallbackExhausted, 2),
            (LaunchPolicyFailure::CwdInvalid, 3),
            (LaunchPolicyFailure::CapabilityUnavailable, 4),
        ];
        for (failure, detail) in cases {
            let wire = encode_launch_policy_failure(failure);
            assert_eq!(wire.result_code, ErrorCode::LaunchPolicyRejected as u16);
            assert_eq!(wire.detail_code, detail);
            assert_ne!(wire.result_code, ErrorCode::InternalFailure as u16);
        }
    }

    #[test]
    fn item_16_interim_code_14_mapping_is_absent() {
        for failure in [
            LaunchPolicyFailure::AccountRecordUnavailable,
            LaunchPolicyFailure::ShellFallbackExhausted,
            LaunchPolicyFailure::CwdInvalid,
            LaunchPolicyFailure::CapabilityUnavailable,
        ] {
            let wire = encode_launch_policy_failure(failure);
            assert_ne!(
                wire.result_code,
                ErrorCode::InternalFailure as u16,
                "L3 removes the interim LaunchPolicyFailure → 14 mapping"
            );
            assert_ne!(
                wire.detail_code, 0,
                "post-L0 failures carry §9 detail codes"
            );
        }
        let warned = encode_created_warnings(&[LaunchPolicyWarning::ConfiguredShellInvalid]);
        assert_eq!(warned.result_code, CREATED_RESULT_CODE);
        assert_ne!(
            warned.detail_code, 0,
            "post-L0 warnings are on Created.detail_code; interim left them off the wire"
        );
    }

    #[test]
    fn item_17_fallback_warnings_set_only_spec_bits() {
        let shell = encode_created_warnings(&[LaunchPolicyWarning::ConfiguredShellInvalid]);
        assert_eq!(shell.result_code, CREATED_RESULT_CODE);
        assert_eq!(shell.detail_code, 1 << 0);

        let cwd = encode_created_warnings(&[LaunchPolicyWarning::CwdOverrideInvalid]);
        assert_eq!(cwd.result_code, CREATED_RESULT_CODE);
        assert_eq!(cwd.detail_code, 1 << 1);

        let both = encode_created_warnings(&[
            LaunchPolicyWarning::ConfiguredShellInvalid,
            LaunchPolicyWarning::CwdOverrideInvalid,
        ]);
        assert_eq!(both.result_code, CREATED_RESULT_CODE);
        assert_eq!(both.detail_code, (1 << 0) | (1 << 1));
        assert_eq!(both.detail_code & !0b11, 0, "reserved bits must stay 0");
    }

    #[test]
    fn wire_encoding_carries_no_path_or_env_bytes() {
        for failure in [
            LaunchPolicyFailure::AccountRecordUnavailable,
            LaunchPolicyFailure::ShellFallbackExhausted,
            LaunchPolicyFailure::CwdInvalid,
            LaunchPolicyFailure::CapabilityUnavailable,
        ] {
            let wire = encode_launch_policy_failure(failure);
            let debug = format!("{wire:?}");
            assert!(!debug.contains('/'));
            assert!(!debug.contains('='));
            assert!(!debug.contains("HOME"));
            assert!(!debug.contains("PATH"));
        }
    }
}
