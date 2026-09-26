//! Outbound control-write and resize-planner helpers for LocalDisplayClient.

use std::collections::VecDeque;

use seyal_runtime::local_ipc::framing::{ErrorCode, ErrorMessage, MessageType, TerminalKeyKind};

use super::{server_error, ClientError, GridGeometry, InputAdmissionFailure};

pub(crate) fn valid_terminal_key_request(kind: TerminalKeyKind, scalar: u32) -> bool {
    match kind {
        TerminalKeyKind::ControlAscii => matches!(scalar, 0x20 | 0x3f | 0x40 | 0x41..=0x5f),
        _ => scalar == 0,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutboundKind {
    Input,
    Paste,
    TerminalKey,
    HostSelection,
    HostSearch,
    TerminalKeyV2 {
        action_id: u32,
    },
    TerminalMouse {
        action_id: u32,
    },
    Resize {
        request_id: u64,
        geometry: GridGeometry,
    },
    Resync,
    ComposerCommand,
    HistoryRangeRequest,
}

#[derive(Debug)]
pub(crate) struct PendingControlWrite {
    pub(crate) bytes: Vec<u8>,
    pub(crate) offset: usize,
    pub(crate) kind: OutboundKind,
}

impl PendingControlWrite {
    pub(crate) fn new(bytes: Vec<u8>, kind: OutboundKind) -> Self {
        Self {
            bytes,
            offset: 0,
            kind,
        }
    }

    pub(crate) fn remaining(&self) -> &[u8] {
        &self.bytes[self.offset..]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResizePhase {
    QueuedNotStarted,
    Writing,
    SentWaitingResult,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ResizeRecord {
    pub(crate) request_id: u64,
    pub(crate) geometry: GridGeometry,
    pub(crate) phase: ResizePhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AppliedFence {
    pub(crate) request_id: u64,
    pub(crate) geometry: GridGeometry,
    pub(crate) applied_generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RetrySuppression {
    pub(crate) geometry: GridGeometry,
}

pub(crate) fn newest_pending_geometry(
    unresolved: &VecDeque<ResizeRecord>,
    applied_fence: Option<AppliedFence>,
) -> Option<GridGeometry> {
    let unresolved_latest = unresolved
        .iter()
        .max_by_key(|record| record.request_id)
        .map(|record| (record.request_id, record.geometry));
    let applied_latest = applied_fence.map(|fence| (fence.request_id, fence.geometry));
    match (unresolved_latest, applied_latest) {
        (Some(unresolved), Some(applied)) => Some(if unresolved.0 > applied.0 {
            unresolved.1
        } else {
            applied.1
        }),
        (Some(unresolved), None) => Some(unresolved.1),
        (None, Some(applied)) => Some(applied.1),
        (None, None) => None,
    }
}

pub(crate) fn resize_needs_mutation(
    desired: GridGeometry,
    committed: GridGeometry,
    newest_pending: Option<GridGeometry>,
) -> bool {
    if newest_pending == Some(desired) {
        return false;
    }
    !(newest_pending.is_none() && committed == desired)
}

pub(crate) fn classify_server_error(
    error: ErrorMessage,
) -> Result<Option<InputAdmissionFailure>, ClientError> {
    if error.error_code == ErrorCode::Backpressure as u16
        && (error.offending_message_type == MessageType::Input as u16
            || error.offending_message_type == MessageType::TerminalKey as u16
            || error.offending_message_type == MessageType::Paste as u16
            || error.offending_message_type == MessageType::HostSelection as u16
            || error.offending_message_type == MessageType::HostSearch as u16
            || error.offending_message_type == MessageType::TerminalMouse as u16)
    {
        return Ok(Some(InputAdmissionFailure::ClientBackpressure));
    }
    // History presentation capacity is a per-request refusal. Never tear down
    // a healthy attachment because a Block body exceeded the wire byte budget.
    if error.error_code == ErrorCode::CapacityExceeded as u16
        && error.offending_message_type == MessageType::HistoryRangeRequest as u16
    {
        return Ok(None);
    }
    // Expected host outcomes: no search match, yank with no/stale selection.
    // These must not tear down a healthy attachment.
    if error.error_code == ErrorCode::InvalidState as u16
        && (error.offending_message_type == MessageType::HostSearch as u16
            || error.offending_message_type == MessageType::HostSelection as u16)
    {
        return Ok(None);
    }
    Err(server_error(error.error_code))
}
