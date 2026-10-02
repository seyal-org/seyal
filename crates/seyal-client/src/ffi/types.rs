use std::ptr;

use seyal_render::PreparedCell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct SeyalRecoveryResult {
    pub stage: u8,
    pub failure_class: u8,
    pub retryable: u8,
    pub connection_origin: u8,
    pub handle: u64,
    pub runtime_id_low: u64,
    pub runtime_id_high: u64,
    pub execution_id_low: u64,
    pub execution_id_high: u64,
    pub attachment_id_low: u64,
    pub attachment_id_high: u64,
}

impl SeyalRecoveryResult {
    pub(crate) const fn empty() -> Self {
        Self {
            stage: 0,
            failure_class: 0,
            retryable: 0,
            connection_origin: 0,
            handle: 0,
            runtime_id_low: 0,
            runtime_id_high: 0,
            execution_id_low: 0,
            execution_id_high: 0,
            attachment_id_low: 0,
            attachment_id_high: 0,
        }
    }
}

#[cfg(test)]
mod recovery_result_tests {
    use super::SeyalRecoveryResult;
    use crate::ffi::session::{
        classify_bridge_discovery_error, recovery_deadline, set_recovery_failure,
    };
    use crate::{ClientError, DiscoveryFailure};
    use seyal_runtime::local_ipc::{discovery::DiscoveryError, framing::ErrorCode};
    use std::io;

    #[test]
    fn retryable_discovery_failure_is_typed() {
        set_recovery_failure(ClientError::Discovery(DiscoveryFailure::EndpointMissing));
        let result = crate::ffi::LAST_RECOVERY_RESULT.with(std::cell::Cell::get);
        assert_eq!(result.failure_class, 1);
        assert_eq!(result.retryable, 1);
        assert_eq!(result.stage, 1);
    }

    #[test]
    fn refusal_retries_without_claiming_a_missing_endpoint_or_launch_path() {
        set_recovery_failure(ClientError::Discovery(DiscoveryFailure::ConnectionRefused));
        let refused = crate::ffi::LAST_RECOVERY_RESULT.with(std::cell::Cell::get);
        assert_eq!(refused.failure_class, 2);
        assert_eq!(refused.retryable, 1);

        set_recovery_failure(ClientError::Discovery(
            DiscoveryFailure::EndpointDisappeared,
        ));
        let disappeared = crate::ffi::LAST_RECOVERY_RESULT.with(std::cell::Cell::get);
        assert_eq!(disappeared.failure_class, 2);
        assert_eq!(disappeared.retryable, 1);
    }

    #[test]
    fn untrusted_or_invalid_endpoint_is_never_retryable_or_launchable() {
        for failure in [
            DiscoveryFailure::UntrustedEndpoint,
            DiscoveryFailure::InvalidPath,
        ] {
            set_recovery_failure(ClientError::Discovery(failure));
            let result = crate::ffi::LAST_RECOVERY_RESULT.with(std::cell::Cell::get);
            assert_eq!(result.failure_class, 4);
            assert_eq!(result.retryable, 0);
        }
    }

    #[test]
    fn security_and_controller_failures_are_not_retryable() {
        set_recovery_failure(ClientError::Protocol);
        let protocol = crate::ffi::LAST_RECOVERY_RESULT.with(std::cell::Cell::get);
        assert_eq!(protocol.failure_class, 4);
        assert_eq!(protocol.retryable, 0);

        set_recovery_failure(ClientError::Server(ErrorCode::ControllerBusy));
        let busy = crate::ffi::LAST_RECOVERY_RESULT.with(std::cell::Cell::get);
        assert_eq!(busy.failure_class, 3);
        assert_eq!(busy.retryable, 1);
        set_recovery_failure(ClientError::Server(ErrorCode::CapacityExceeded));
        let capacity = crate::ffi::LAST_RECOVERY_RESULT.with(std::cell::Cell::get);
        assert_eq!(capacity.failure_class, 4);
        assert_eq!(capacity.retryable, 0);
        set_recovery_failure(ClientError::StartupDeadlineExceeded);
        let deadline = crate::ffi::LAST_RECOVERY_RESULT.with(std::cell::Cell::get);
        assert_eq!(deadline.failure_class, 4);
        assert_eq!(deadline.retryable, 0);
        let _ = SeyalRecoveryResult::empty();
    }

    #[test]
    fn missing_pinned_execution_has_a_distinct_terminal_recovery_class() {
        set_recovery_failure(ClientError::NoRunningExecution);
        let result = crate::ffi::LAST_RECOVERY_RESULT.with(std::cell::Cell::get);
        assert_eq!(result.failure_class, 7);
        assert_eq!(result.retryable, 0);
    }

    #[test]
    fn zero_budget_and_insecure_leaf_classification_fail_closed() {
        assert_eq!(
            recovery_deadline(0),
            Err(ClientError::StartupDeadlineExceeded)
        );
        assert_eq!(
            classify_bridge_discovery_error(DiscoveryError::NotOwnedByEffectiveUser),
            ClientError::Discovery(DiscoveryFailure::UntrustedEndpoint)
        );
        assert_eq!(
            classify_bridge_discovery_error(DiscoveryError::Io(io::Error::from(
                io::ErrorKind::NotFound,
            ))),
            ClientError::Discovery(DiscoveryFailure::EndpointMissing)
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct SeyalPass9DiagSnapshot {
    pub connected: u8,
    pub reserved0: [u8; 7],
    pub socket_fd: i32,
    pub live_handles: u32,
    pub pending_handles: u32,
    pub active_handle: u64,
    pub runtime_id_low: u64,
    pub runtime_id_high: u64,
    pub execution_id_low: u64,
    pub execution_id_high: u64,
    pub attachment_id_low: u64,
    pub attachment_id_high: u64,
}

impl SeyalPass9DiagSnapshot {
    pub(crate) const fn empty() -> Self {
        Self {
            connected: 0,
            reserved0: [0; 7],
            socket_fd: -1,
            live_handles: 0,
            pending_handles: 0,
            active_handle: 0,
            runtime_id_low: 0,
            runtime_id_high: 0,
            execution_id_low: 0,
            execution_id_high: 0,
            attachment_id_low: 0,
            attachment_id_high: 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct SeyalPreparedFrame {
    pub cells: *const PreparedCell,
    pub cell_count: u32,
    pub generation: u64,
    pub rows: u16,
    pub columns: u16,
    pub cursor_row: u16,
    pub cursor_column: u16,
    pub cursor_visible: u8,
    pub alternate_screen: u8,
    pub full_rebuild: u8,
    pub reserved0: u8,
    pub rebuilt_row_count: u16,
    pub reserved1: u16,
    pub damage_word0: u64,
    pub damage_word1: u64,
    pub damage_word2: u64,
    pub damage_word3: u64,
    pub grapheme_utf8: *const u8,
    pub grapheme_utf8_len: u32,
    pub reserved2: u32,
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct SeyalExecutionBlockMetadata {
    pub block_id_low: u64,
    pub block_id_high: u64,
    pub revision: u64,
    pub start_line_id: u64,
    pub state: u8,
    pub reserved: [u8; 7],
}

impl SeyalExecutionBlockMetadata {
    pub(crate) const fn empty() -> Self {
        Self {
            block_id_low: 0,
            block_id_high: 0,
            revision: 0,
            start_line_id: 0,
            state: 0,
            reserved: [0; 7],
        }
    }
}

#[cfg(test)]
mod pass8_execution_block_abi_tests {
    use std::mem::{align_of, offset_of, size_of};

    use seyal_render::PreparedCell;
    use seyal_runtime::local_ipc::framing::HistoryCell;

    use super::{
        SeyalBlockRecord, SeyalExecutionBlockMetadata, SeyalHistoryCell, SeyalHistoryRow,
        SeyalPreparedFrame,
    };

    #[test]
    fn execution_block_metadata_c_abi_is_exactly_40_bytes() {
        assert_eq!(size_of::<SeyalExecutionBlockMetadata>(), 40);
        assert_eq!(align_of::<SeyalExecutionBlockMetadata>(), 8);
        assert_eq!(offset_of!(SeyalExecutionBlockMetadata, block_id_low), 0);
        assert_eq!(offset_of!(SeyalExecutionBlockMetadata, block_id_high), 8);
        assert_eq!(offset_of!(SeyalExecutionBlockMetadata, revision), 16);
        assert_eq!(offset_of!(SeyalExecutionBlockMetadata, start_line_id), 24);
        assert_eq!(offset_of!(SeyalExecutionBlockMetadata, state), 32);
        assert_eq!(offset_of!(SeyalExecutionBlockMetadata, reserved), 33);
    }

    #[test]
    fn prepared_cell_matches_seyal_bridge_h() {
        assert_eq!(size_of::<PreparedCell>(), 16);
        assert_eq!(align_of::<PreparedCell>(), 4);
        assert_eq!(offset_of!(PreparedCell, scalar), 0);
        assert_eq!(offset_of!(PreparedCell, foreground), 4);
        assert_eq!(offset_of!(PreparedCell, background), 8);
        assert_eq!(offset_of!(PreparedCell, flags), 12);
        assert_eq!(offset_of!(PreparedCell, reserved), 14);
    }

    #[test]
    fn history_cell_matches_seyal_history_cell_and_header() {
        assert_eq!(size_of::<HistoryCell>(), 16);
        assert_eq!(size_of::<SeyalHistoryCell>(), 16);
        assert_eq!(align_of::<HistoryCell>(), align_of::<SeyalHistoryCell>());
        assert_eq!(offset_of!(HistoryCell, scalar), 0);
        assert_eq!(offset_of!(HistoryCell, foreground), 4);
        assert_eq!(offset_of!(HistoryCell, background), 8);
        assert_eq!(offset_of!(HistoryCell, flags), 12);
        assert_eq!(offset_of!(HistoryCell, reserved), 14);
        assert_eq!(
            offset_of!(SeyalHistoryCell, reserved),
            offset_of!(HistoryCell, reserved)
        );
    }

    #[test]
    fn prepared_frame_matches_seyal_bridge_h() {
        assert_eq!(size_of::<SeyalPreparedFrame>(), 88);
        assert_eq!(align_of::<SeyalPreparedFrame>(), 8);
        assert_eq!(offset_of!(SeyalPreparedFrame, cells), 0);
        assert_eq!(offset_of!(SeyalPreparedFrame, cell_count), 8);
        assert_eq!(offset_of!(SeyalPreparedFrame, generation), 16);
        assert_eq!(offset_of!(SeyalPreparedFrame, rows), 24);
        assert_eq!(offset_of!(SeyalPreparedFrame, columns), 26);
        assert_eq!(offset_of!(SeyalPreparedFrame, cursor_row), 28);
        assert_eq!(offset_of!(SeyalPreparedFrame, cursor_column), 30);
        assert_eq!(offset_of!(SeyalPreparedFrame, cursor_visible), 32);
        assert_eq!(offset_of!(SeyalPreparedFrame, alternate_screen), 33);
        assert_eq!(offset_of!(SeyalPreparedFrame, full_rebuild), 34);
        assert_eq!(offset_of!(SeyalPreparedFrame, reserved0), 35);
        assert_eq!(offset_of!(SeyalPreparedFrame, rebuilt_row_count), 36);
        assert_eq!(offset_of!(SeyalPreparedFrame, reserved1), 38);
        assert_eq!(offset_of!(SeyalPreparedFrame, damage_word0), 40);
        assert_eq!(offset_of!(SeyalPreparedFrame, damage_word3), 64);
        assert_eq!(offset_of!(SeyalPreparedFrame, grapheme_utf8), 72);
        assert_eq!(offset_of!(SeyalPreparedFrame, grapheme_utf8_len), 80);
    }

    #[test]
    fn history_row_and_block_record_match_seyal_bridge_h() {
        assert_eq!(size_of::<SeyalHistoryRow>(), 24);
        assert_eq!(align_of::<SeyalHistoryRow>(), 8);
        assert_eq!(offset_of!(SeyalHistoryRow, line_id), 0);
        assert_eq!(offset_of!(SeyalHistoryRow, cells), 8);
        assert_eq!(offset_of!(SeyalHistoryRow, cell_count), 16);

        assert_eq!(size_of::<SeyalBlockRecord>(), 48);
        assert_eq!(align_of::<SeyalBlockRecord>(), 8);
        assert_eq!(offset_of!(SeyalBlockRecord, id), 0);
        assert_eq!(offset_of!(SeyalBlockRecord, start_line), 8);
        assert_eq!(offset_of!(SeyalBlockRecord, end_line), 16);
        assert_eq!(offset_of!(SeyalBlockRecord, state), 24);
        assert_eq!(offset_of!(SeyalBlockRecord, reserved), 25);
        assert_eq!(offset_of!(SeyalBlockRecord, exit_status), 28);
        assert_eq!(offset_of!(SeyalBlockRecord, command), 32);
        assert_eq!(offset_of!(SeyalBlockRecord, command_len), 40);
    }
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct SeyalBlockRecord {
    pub id: u64,
    pub start_line: u64,
    pub end_line: u64,
    pub state: u8,
    pub reserved: [u8; 3],
    pub exit_status: i32,
    pub command: *const u8,
    pub command_len: u32,
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct SeyalHistoryRow {
    pub line_id: u64,
    pub cells: *const SeyalHistoryCell,
    pub cell_count: u32,
}

/// A typed, immutable identity for one history response. Swift must carry the
/// Runtime pair together; start/end anchors are only a lookup hint and are not
/// sufficient to correlate responses when ranges overlap.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct SeyalHistoryRange {
    pub start_line: u64,
    pub end_line: u64,
    pub block_id: u64,
    pub request_id: u64,
    pub revision: u64,
    pub row_count: u32,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct SeyalHistorySidecar {
    pub bytes: *const u8,
    pub len: u32,
    pub reserved: u32,
}

impl SeyalHistorySidecar {
    pub(crate) const fn empty() -> Self {
        Self {
            bytes: ptr::null(),
            len: 0,
            reserved: 0,
        }
    }
}

impl SeyalHistoryRange {
    pub(crate) const fn empty() -> Self {
        Self {
            start_line: 0,
            end_line: 0,
            block_id: 0,
            request_id: 0,
            revision: 0,
            row_count: 0,
            reserved: 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct SeyalComposerResult {
    pub request_id: u64,
    pub block_id: u64,
    pub code: u8,
    pub reserved: [u8; 7],
}

impl SeyalComposerResult {
    pub(crate) const fn empty() -> Self {
        Self {
            request_id: 0,
            block_id: 0,
            code: 4,
            reserved: [0; 7],
        }
    }
}

/// Runtime-published composer eligibility for the active client (#978).
/// `eligibility` uses `SeyalAppComposerEligibility` (0 = none published yet).
/// `revision` is Runtime's monotonic fence; the host relays both unchanged
/// through `SEYAL_APP_ACTION_APPLY_COMPOSER_STATUS`.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct SeyalComposerStatus {
    pub revision: u64,
    pub eligibility: u8,
    pub reserved: [u8; 7],
}

impl SeyalComposerStatus {
    pub(crate) const fn empty() -> Self {
        Self {
            revision: 0,
            eligibility: 0,
            reserved: [0; 7],
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct SeyalHistoryCell {
    pub scalar: u32,
    pub foreground: u32,
    pub background: u32,
    pub flags: u16,
    pub reserved: u16,
}

impl SeyalHistoryRow {
    pub(crate) const fn empty() -> Self {
        Self {
            line_id: 0,
            cells: ptr::null(),
            cell_count: 0,
        }
    }
}

impl SeyalBlockRecord {
    pub(crate) const fn empty() -> Self {
        Self {
            id: 0,
            start_line: 0,
            end_line: 0,
            state: 0,
            reserved: [0; 3],
            exit_status: 0,
            command: ptr::null(),
            command_len: 0,
        }
    }
}

impl SeyalPreparedFrame {
    pub(crate) const fn empty() -> Self {
        Self {
            cells: ptr::null(),
            cell_count: 0,
            generation: 0,
            rows: 0,
            columns: 0,
            cursor_row: 0,
            cursor_column: 0,
            cursor_visible: 0,
            alternate_screen: 0,
            full_rebuild: 0,
            reserved0: 0,
            rebuilt_row_count: 0,
            reserved1: 0,
            damage_word0: 0,
            damage_word1: 0,
            damage_word2: 0,
            damage_word3: 0,
            grapheme_utf8: ptr::null(),
            grapheme_utf8_len: 0,
            reserved2: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct SeyalCopiedText {
    pub utf8: *const u8,
    pub len: u32,
    pub reserved: u32,
}

impl SeyalCopiedText {
    pub(crate) const fn empty() -> Self {
        Self {
            utf8: ptr::null(),
            len: 0,
            reserved: 0,
        }
    }
}
