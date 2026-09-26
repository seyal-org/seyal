use std::cell::Cell;

use crate::{
    local::{InputAdmissionFailure, ResizeFailure},
    ClientError,
};

use super::{
    identity_words, pending_clients, with_active_client, SeyalPass9DiagSnapshot,
    SeyalRecoveryResult, ACTIVE_HANDLE, CLIENTS, LAST_RECOVERY_RESULT,
};

#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_last_recovery_result() -> SeyalRecoveryResult {
    LAST_RECOVERY_RESULT.with(Cell::get)
}

/// Classify a failed bridge open into a `SeyalAppRecoveryOutcome` code.
/// Rust owns the failure_class → outcome mapping (ADR-015 / #1065).
#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_classify_open_result(failure_class: u8, retryable: u8) -> u32 {
    use crate::recovery::{classify_open_result, AttemptOutcome};
    match classify_open_result(failure_class, retryable != 0) {
        AttemptOutcome::EndpointMissing => 3,
        AttemptOutcome::Retryable => 4,
        AttemptOutcome::ControllerBusy => 5,
        AttemptOutcome::Blocked => 6,
        AttemptOutcome::Connected | AttemptOutcome::Opened { .. } => 6,
    }
}

/// Quiescent-only Pass 9 diagnostic. Callers must not invoke this from poll,
/// input, resize, or render hot paths.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_pass9_diag_snapshot() -> SeyalPass9DiagSnapshot {
    let live_handles = CLIENTS.with(|clients| clients.borrow().len() as u32);
    let pending_handles = pending_clients()
        .lock()
        .map(|pending| pending.len() as u32)
        .unwrap_or(0);
    let active_handle = ACTIVE_HANDLE.with(Cell::get);
    let Some(client) = with_active_client(|client| {
        let (runtime_id_low, runtime_id_high) = identity_words(client.runtime_id());
        let execution = client.execution_id().to_bytes();
        let attachment = client.attachment_id().to_bytes();
        SeyalPass9DiagSnapshot {
            connected: 1,
            reserved0: [0; 7],
            socket_fd: client.socket_fd(),
            live_handles,
            pending_handles,
            active_handle,
            runtime_id_low,
            runtime_id_high,
            execution_id_low: u64::from_le_bytes(execution[..8].try_into().unwrap()),
            execution_id_high: u64::from_le_bytes(execution[8..].try_into().unwrap()),
            attachment_id_low: u64::from_le_bytes(attachment[..8].try_into().unwrap()),
            attachment_id_high: u64::from_le_bytes(attachment[8..].try_into().unwrap()),
        }
    }) else {
        return SeyalPass9DiagSnapshot {
            live_handles,
            pending_handles,
            active_handle,
            ..SeyalPass9DiagSnapshot::empty()
        };
    };
    client
}

/// Non-secret presentation reason only; never returns rejected input content.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_input_failure() -> i32 {
    with_active_client(|client| match client.input_failure() {
        None => 0,
        Some(InputAdmissionFailure::ClientBackpressure) => 1,
        Some(InputAdmissionFailure::CommitTooLarge) => 2,
        Some(InputAdmissionFailure::LostController) => 3,
        Some(InputAdmissionFailure::Disconnected) => 4,
    })
    .unwrap_or(0)
}

/// Non-secret resize failure reason only. Runtime result codes retain their
/// SPEC-004 numeric value under the 100-series namespace.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_resize_failure() -> i32 {
    with_active_client(|client| match client.resize_failure() {
        None => 0,
        Some(ResizeFailure::ClientBackpressure) => 1,
        Some(ResizeFailure::Apply(error)) => 100 + error as i32,
        Some(ResizeFailure::Protocol) => 200,
        Some(ResizeFailure::Disconnected) => 201,
    })
    .unwrap_or(0)
}

pub(crate) fn error_code(error: ClientError) -> i32 {
    match error {
        ClientError::Discovery(_) => -2,
        ClientError::StartupDeadlineExceeded => -20,
        ClientError::Io => -3,
        ClientError::Protocol => -4,
        ClientError::UnsupportedDisplayCapability => -5,
        ClientError::NoRunningExecution => -6,
        ClientError::AmbiguousExecutions => -19,
        ClientError::InvalidAttachment => -7,
        ClientError::Display => -8,
        ClientError::Prepare => -9,
        ClientError::Disconnected => -10,
        ClientError::Capacity => -11,
        ClientError::UnsupportedInteractiveCapability => -12,
        ClientError::ClientBackpressure => -13,
        ClientError::CommitTooLarge => -14,
        ClientError::LostController => -15,
        ClientError::ResizeProtocolFailure => -16,
        ClientError::InvalidGeometry => -17,
        ClientError::BlockMetadataConflict => -18,
        ClientError::Server(code) => -1000 - i32::from(code as u16),
    }
}
