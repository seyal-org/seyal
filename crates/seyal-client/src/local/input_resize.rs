use std::{collections::VecDeque, io::Write};

use seyal_runtime::local_ipc::framing::{
    encode_frame, ErrorCode, ErrorMessage, HostSearch, HostSelection, HostSelectionAction,
    InputRef, MessageType, ResizeRequest, ResizeResult, ResizeResultCode, Resync, Role,
    TerminalKey, TerminalKeyKind, TerminalKeyModifiers, TerminalKeyV2, TerminalKeyV2Event,
    TerminalKeyV2Kind, TerminalKeyV2Modifiers, TerminalMouse, TerminalMouseKind, MAX_INPUT_BYTES,
};

use super::{
    server_error, ClientError, LocalDisplayClient, MAX_OUTBOUND_WIRE_BYTES, MAX_UNRESOLVED_RESIZES,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputAdmissionFailure {
    ClientBackpressure,
    CommitTooLarge,
    LostController,
    Disconnected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeFailure {
    ClientBackpressure,
    Apply(ErrorCode),
    Protocol,
    Disconnected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridGeometry {
    pub rows: u16,
    pub columns: u16,
}

pub fn derive_grid_geometry(
    viewport_width: f64,
    viewport_height: f64,
    horizontal_insets: f64,
    vertical_insets: f64,
    cell_width: f64,
    cell_height: f64,
) -> Option<GridGeometry> {
    let operands = [
        viewport_width,
        viewport_height,
        horizontal_insets,
        vertical_insets,
        cell_width,
        cell_height,
    ];
    if operands.iter().any(|value| !value.is_finite())
        || viewport_width < 0.0
        || viewport_height < 0.0
        || horizontal_insets < 0.0
        || vertical_insets < 0.0
        || cell_width <= 0.0
        || cell_height <= 0.0
    {
        return None;
    }

    let usable_width = viewport_width - horizontal_insets;
    let usable_height = viewport_height - vertical_insets;
    if !usable_width.is_finite()
        || !usable_height.is_finite()
        || usable_width <= 0.0
        || usable_height <= 0.0
    {
        return None;
    }

    let column_ratio = usable_width / cell_width;
    let row_ratio = usable_height / cell_height;
    if !column_ratio.is_finite()
        || !row_ratio.is_finite()
        || column_ratio <= 0.0
        || row_ratio <= 0.0
    {
        return None;
    }

    let columns = column_ratio.floor().clamp(1.0, 512.0) as u16;
    let rows = row_ratio.floor().clamp(1.0, 256.0) as u16;
    Some(GridGeometry { rows, columns })
}

#[allow(clippy::too_many_arguments)]
pub fn cell_from_point(
    pixel_x: f64,
    pixel_y_from_top: f64,
    viewport_width: f64,
    viewport_height: f64,
    horizontal_insets: f64,
    vertical_insets: f64,
    cell_width: f64,
    cell_height: f64,
) -> Option<(u16, u16)> {
    let geometry = derive_grid_geometry(
        viewport_width,
        viewport_height,
        horizontal_insets,
        vertical_insets,
        cell_width,
        cell_height,
    )?;
    if !pixel_x.is_finite() || !pixel_y_from_top.is_finite() {
        return None;
    }
    let x = pixel_x - horizontal_insets;
    let y = pixel_y_from_top - vertical_insets;
    if x < 0.0 || y < 0.0 {
        return None;
    }
    let col = (x / cell_width).floor();
    let row = (y / cell_height).floor();
    if !col.is_finite() || !row.is_finite() {
        return None;
    }
    let col = (col as u16).min(geometry.columns.saturating_sub(1));
    let row = (row as u16).min(geometry.rows.saturating_sub(1));
    Some((col, row))
}

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
    bytes: Vec<u8>,
    offset: usize,
    kind: OutboundKind,
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
    request_id: u64,
    geometry: GridGeometry,
    phase: ResizePhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AppliedFence {
    request_id: u64,
    geometry: GridGeometry,
    applied_generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RetrySuppression {
    geometry: GridGeometry,
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

impl LocalDisplayClient {
    pub fn input_failure(&self) -> Option<InputAdmissionFailure> {
        self.input_failure
    }

    pub fn resize_failure(&self) -> Option<ResizeFailure> {
        self.resize_failure
    }

    pub fn submit_composer_command(&mut self, command: &str) -> Result<(), ClientError> {
        self.require_controller()?;
        if !self.command_blocks_supported {
            return Err(ClientError::UnsupportedInteractiveCapability);
        }
        let request_id = self.next_composer_request_id;
        self.next_composer_request_id = request_id.checked_add(1).unwrap_or(1);
        let payload = seyal_runtime::local_ipc::framing::ComposerCommandRef {
            attachment_id: self.attachment_id,
            request_id,
            command,
        }
        .encode();
        let frame = encode_frame(MessageType::ComposerCommand, &payload);
        if let Err(error) = self.admit_frame(frame, OutboundKind::ComposerCommand) {
            self.input_failure = Some(InputAdmissionFailure::ClientBackpressure);
            return Err(error);
        }
        self.pending_composer_requests.insert(request_id);
        self.input_failure = None;
        let result = self.flush_control_write();
        if result.is_err() {
            self.pending_composer_requests.remove(&request_id);
        }
        result
    }

    pub fn outbound_wire_bytes(&self) -> usize {
        self.outbound_wire_bytes
    }

    pub fn submit_committed_text(&mut self, text: &str) -> Result<(), ClientError> {
        self.require_controller()?;
        let bytes = text.as_bytes();
        if bytes.is_empty() {
            return Ok(());
        }
        if bytes.len() > MAX_INPUT_BYTES as usize {
            self.input_failure = Some(InputAdmissionFailure::CommitTooLarge);
            return Err(ClientError::CommitTooLarge);
        }
        let payload = InputRef {
            attachment_id: self.attachment_id,
            bytes,
        }
        .encode();
        let frame = encode_frame(MessageType::Input, &payload);
        if let Err(error) = self.admit_frame(frame, OutboundKind::Input) {
            self.input_failure = Some(InputAdmissionFailure::ClientBackpressure);
            return Err(error);
        }
        self.input_failure = None;
        self.flush_control_write()
    }

    pub fn submit_paste(&mut self, bytes: &[u8]) -> Result<(), ClientError> {
        self.require_controller()?;
        if bytes.is_empty() || bytes.len() > MAX_INPUT_BYTES as usize {
            self.input_failure = Some(InputAdmissionFailure::CommitTooLarge);
            return Err(ClientError::CommitTooLarge);
        }
        let payload = InputRef {
            attachment_id: self.attachment_id,
            bytes,
        }
        .encode();
        let frame = encode_frame(MessageType::Paste, &payload);
        if let Err(error) = self.admit_frame(frame, OutboundKind::Paste) {
            self.input_failure = Some(InputAdmissionFailure::ClientBackpressure);
            return Err(error);
        }
        self.input_failure = None;
        self.flush_control_write()
    }

    pub fn submit_host_selection(
        &mut self,
        action: HostSelectionAction,
        kind: u8,
        start_col: u16,
        start_row: u16,
        end_col: u16,
        end_row: u16,
    ) -> Result<(), ClientError> {
        self.require_controller()?;
        let payload = HostSelection {
            attachment_id: self.attachment_id,
            action,
            kind,
            start_col,
            start_row,
            end_col,
            end_row,
        }
        .encode();
        let frame = encode_frame(MessageType::HostSelection, &payload);
        if let Err(error) = self.admit_frame(frame, OutboundKind::HostSelection) {
            self.input_failure = Some(InputAdmissionFailure::ClientBackpressure);
            return Err(error);
        }
        self.input_failure = None;
        self.flush_control_write()
    }

    pub fn submit_host_search(&mut self, needle: &str, forward: bool) -> Result<(), ClientError> {
        self.require_controller()?;
        if needle.len() > MAX_INPUT_BYTES as usize {
            self.input_failure = Some(InputAdmissionFailure::CommitTooLarge);
            return Err(ClientError::CommitTooLarge);
        }
        let payload = HostSearch {
            attachment_id: self.attachment_id,
            forward,
            needle,
        }
        .encode();
        let frame = encode_frame(MessageType::HostSearch, &payload);
        if let Err(error) = self.admit_frame(frame, OutboundKind::HostSearch) {
            self.input_failure = Some(InputAdmissionFailure::ClientBackpressure);
            return Err(error);
        }
        self.input_failure = None;
        self.flush_control_write()
    }

    pub fn copied_text(&self) -> Option<&[u8]> {
        if self.copied_text.is_empty() {
            None
        } else {
            Some(self.copied_text.as_slice())
        }
    }

    pub fn take_copied_text(&mut self) -> Option<Vec<u8>> {
        if self.copied_text.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.copied_text))
        }
    }

    pub fn submit_terminal_key(
        &mut self,
        kind: TerminalKeyKind,
        scalar: u32,
    ) -> Result<(), ClientError> {
        self.require_controller()?;
        if !valid_terminal_key_request(kind, scalar) {
            return Err(ClientError::Protocol);
        }
        let modifiers = if kind == TerminalKeyKind::ControlAscii {
            TerminalKeyModifiers::CONTROL
        } else {
            TerminalKeyModifiers::NONE
        };
        let payload = TerminalKey {
            attachment_id: self.attachment_id,
            kind,
            modifiers,
            scalar,
        }
        .encode();
        let frame = encode_frame(MessageType::TerminalKey, &payload);
        if let Err(error) = self.admit_frame(frame, OutboundKind::TerminalKey) {
            self.input_failure = Some(InputAdmissionFailure::ClientBackpressure);
            return Err(error);
        }
        self.input_failure = None;
        self.flush_control_write()
    }

    pub fn submit_terminal_key_v2(
        &mut self,
        kind: TerminalKeyV2Kind,
        modifiers: TerminalKeyV2Modifiers,
        value: u32,
        event: TerminalKeyV2Event,
        shifted_ascii: u32,
        action_id: u32,
    ) -> Result<(), ClientError> {
        self.require_controller()?;
        if !self.extended_terminal_key_supported {
            return Err(ClientError::UnsupportedInteractiveCapability);
        }
        if action_id == 0 {
            return Err(ClientError::Protocol);
        }
        if action_id <= self.last_admitted_v2_action_id {
            return Err(ClientError::Protocol);
        }
        let key = TerminalKeyV2 {
            attachment_id: self.attachment_id,
            kind,
            modifiers,
            value,
            event,
            shifted_ascii,
            action_id,
        };
        if key.validate().is_err() {
            return Err(ClientError::Protocol);
        }
        let payload = key.encode();
        let frame = encode_frame(MessageType::TerminalKeyV2, &payload);
        if let Err(error) = self.admit_frame(frame, OutboundKind::TerminalKeyV2 { action_id }) {
            self.input_failure = Some(InputAdmissionFailure::ClientBackpressure);
            return Err(error);
        }
        self.last_admitted_v2_action_id = action_id;
        self.input_failure = None;
        self.flush_control_write()
    }

    pub fn submit_terminal_mouse(
        &mut self,
        kind: TerminalMouseKind,
        button: u8,
        modifiers: TerminalKeyV2Modifiers,
        col: u16,
        row: u16,
        action_id: u32,
    ) -> Result<(), ClientError> {
        self.require_controller()?;
        if action_id == 0 || action_id <= self.last_admitted_mouse_action_id {
            return Err(ClientError::Protocol);
        }
        let event = TerminalMouse {
            attachment_id: self.attachment_id,
            action_id,
            kind,
            button,
            modifiers,
            col,
            row,
        };
        if event.validate().is_err() {
            return Err(ClientError::Protocol);
        }
        let payload = event.encode();
        let frame = encode_frame(MessageType::TerminalMouse, &payload);
        if let Err(error) = self.admit_frame(frame, OutboundKind::TerminalMouse { action_id }) {
            self.input_failure = Some(InputAdmissionFailure::ClientBackpressure);
            return Err(error);
        }
        self.last_admitted_mouse_action_id = action_id;
        self.input_failure = None;
        self.flush_control_write()
    }

    pub(crate) fn classify_incoming_error(
        &mut self,
        error: ErrorMessage,
    ) -> Result<Option<InputAdmissionFailure>, ClientError> {
        if error.offending_message_type == MessageType::TerminalKeyV2 as u16 {
            return self.classify_v2_error(error);
        }
        classify_server_error(error)
    }

    fn classify_v2_error(
        &mut self,
        error: ErrorMessage,
    ) -> Result<Option<InputAdmissionFailure>, ClientError> {
        let (disposition, highest) = crate::v2_error::classify_v2_incoming_error(
            error.error_code,
            error.detail_code,
            self.last_sent_v2_action_id,
            self.highest_v2_error_id,
        );
        self.highest_v2_error_id = highest;
        match disposition {
            crate::v2_error::V2IncomingDisposition::ClientBackpressure => {
                Ok(Some(InputAdmissionFailure::ClientBackpressure))
            }
            crate::v2_error::V2IncomingDisposition::LostController => {
                self.role = Role::Observer;
                Ok(Some(InputAdmissionFailure::LostController))
            }
            crate::v2_error::V2IncomingDisposition::Protocol => Err(ClientError::Protocol),
            crate::v2_error::V2IncomingDisposition::Fatal(code) => Err(ClientError::Server(code)),
        }
    }

    pub fn set_desired_geometry(&mut self, geometry: GridGeometry) -> Result<(), ClientError> {
        self.set_desired_geometry_for_layout(geometry, false)
    }

    pub fn set_desired_geometry_for_layout(
        &mut self,
        geometry: GridGeometry,
        meaningful_layout_epoch: bool,
    ) -> Result<(), ClientError> {
        if geometry.rows == 0
            || geometry.columns == 0
            || geometry.rows > 256
            || geometry.columns > 512
        {
            return Err(ClientError::InvalidGeometry);
        }
        self.require_controller()?;
        let geometry_changed = self.desired_geometry != Some(geometry);
        self.desired_geometry = Some(geometry);
        if geometry_changed
            || (meaningful_layout_epoch
                && self
                    .retry_suppression
                    .is_some_and(|suppressed| suppressed.geometry == geometry))
        {
            self.retry_suppression = None;
            self.resize_failure = None;
        }
        self.reconcile_resize()?;
        self.flush_control_write()
    }

    pub fn retry_resize(&mut self) -> Result<(), ClientError> {
        self.retry_suppression = None;
        self.resize_failure = None;
        self.reconcile_resize()?;
        self.flush_control_write()
    }

    pub(crate) fn require_controller(&mut self) -> Result<(), ClientError> {
        if self.role != Role::Controller {
            self.input_failure = Some(InputAdmissionFailure::LostController);
            return Err(ClientError::LostController);
        }
        Ok(())
    }

    pub(crate) fn admit_frame(
        &mut self,
        bytes: Vec<u8>,
        kind: OutboundKind,
    ) -> Result<(), ClientError> {
        #[cfg(feature = "benchmark-instrumentation")]
        let benchmark_input = matches!(
            kind,
            OutboundKind::Input | OutboundKind::TerminalKey | OutboundKind::TerminalKeyV2 { .. }
        );
        let next = self
            .outbound_wire_bytes
            .checked_add(bytes.len())
            .ok_or(ClientError::Capacity)?;
        if next > MAX_OUTBOUND_WIRE_BYTES {
            return Err(ClientError::ClientBackpressure);
        }
        self.outbound_wire_bytes = next;
        self.outbound
            .push_back(PendingControlWrite::new(bytes, kind));
        #[cfg(feature = "benchmark-instrumentation")]
        {
            crate::pass7_benchmark::observe_pass7_client_queue(self.outbound_wire_bytes);
            if benchmark_input {
                crate::pass7_benchmark::mark_pass7_client_admission(self.outbound_wire_bytes);
            }
        }
        Ok(())
    }

    /// Complete at most one nonblocking write attempt. Accepted wire bytes are
    /// decremented only as the socket actually accepts them; a partial frame is
    /// immutable and remains at the front of the FIFO until complete.
    pub fn flush_control_write(&mut self) -> Result<(), ClientError> {
        let (written, resize_phase, completed) = {
            let Some(front) = self.outbound.front_mut() else {
                return Ok(());
            };
            let remaining_len = front.remaining().len();
            if remaining_len == 0 {
                return Err(ClientError::Protocol);
            }
            match self.stream.write(front.remaining()) {
                Ok(0) => return Err(ClientError::Io),
                Ok(count) if count <= remaining_len => {
                    front.offset = front
                        .offset
                        .checked_add(count)
                        .ok_or(ClientError::Capacity)?;
                    let phase = match front.kind {
                        OutboundKind::Resize { request_id, .. } => Some((
                            request_id,
                            if count == remaining_len {
                                ResizePhase::SentWaitingResult
                            } else {
                                ResizePhase::Writing
                            },
                        )),
                        _ => None,
                    };
                    (count, phase, front.offset == front.bytes.len())
                }
                Ok(_) => return Err(ClientError::Io),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                Err(_) => {
                    self.input_failure = Some(InputAdmissionFailure::Disconnected);
                    self.resize_failure = Some(ResizeFailure::Disconnected);
                    return Err(ClientError::Io);
                }
            }
        };

        self.outbound_wire_bytes = self.outbound_wire_bytes.saturating_sub(written);
        if let Some((request_id, phase)) = resize_phase {
            self.set_resize_phase(request_id, phase)?;
        }
        if completed {
            let completed_v2 = match self.outbound.front().map(|pending| pending.kind) {
                Some(OutboundKind::TerminalKeyV2 { action_id }) => Some(action_id),
                _ => None,
            };
            #[cfg(feature = "benchmark-instrumentation")]
            let completed_input = self.outbound.front().is_some_and(|pending| {
                matches!(
                    pending.kind,
                    OutboundKind::Input
                        | OutboundKind::TerminalKey
                        | OutboundKind::TerminalKeyV2 { .. }
                )
            });
            self.outbound.pop_front();
            if let Some(action_id) = completed_v2 {
                self.last_sent_v2_action_id = action_id;
            }
            #[cfg(feature = "benchmark-instrumentation")]
            if completed_input {
                crate::pass7_benchmark::mark_pass7_client_socket_complete(self.outbound_wire_bytes);
            }
        }
        if self.input_failure == Some(InputAdmissionFailure::ClientBackpressure) {
            self.input_failure = None;
        }
        self.try_queue_resync()?;
        self.reconcile_resize()?;
        Ok(())
    }

    pub(crate) fn observe_projection(&mut self) -> Result<(), ClientError> {
        let geometry = GridGeometry {
            rows: self.cache.rows,
            columns: self.cache.columns,
        };
        self.committed_geometry = geometry;
        if let Some(fence) = self.applied_awaiting_projection
            && self.cache.generation >= fence.applied_generation
        {
            if self.cache.generation == fence.applied_generation && geometry != fence.geometry {
                self.resize_failure = Some(ResizeFailure::Protocol);
                return Err(ClientError::ResizeProtocolFailure);
            }
            self.applied_awaiting_projection = None;
        }
        if self.desired_geometry == Some(geometry) && self.applied_awaiting_projection.is_none() {
            self.resize_failure = None;
        }
        self.reconcile_resize()
    }

    pub(crate) fn accept_resize_result(&mut self, result: ResizeResult) -> Result<(), ClientError> {
        if result.attachment_id != self.attachment_id {
            self.resize_failure = Some(ResizeFailure::Protocol);
            return Err(ClientError::ResizeProtocolFailure);
        }
        let Some(index) = self
            .unresolved_resizes
            .iter()
            .position(|record| record.request_id == result.request_id)
        else {
            self.resize_failure = Some(ResizeFailure::Protocol);
            return Err(ClientError::ResizeProtocolFailure);
        };
        let record = self
            .unresolved_resizes
            .remove(index)
            .ok_or(ClientError::ResizeProtocolFailure)?;

        match result.result_code {
            ResizeResultCode::Applied => {
                let replace_fence = self
                    .applied_awaiting_projection
                    .is_none_or(|fence| result.request_id > fence.request_id);
                if replace_fence {
                    self.applied_awaiting_projection = Some(AppliedFence {
                        request_id: result.request_id,
                        geometry: record.geometry,
                        applied_generation: result.applied_generation,
                    });
                }
                self.retry_suppression = None;
                self.resize_failure = None;
            }
            ResizeResultCode::Error(error) => {
                self.resize_failure = Some(ResizeFailure::Apply(error));
                let newer_same_desired = self.unresolved_resizes.iter().any(|candidate| {
                    candidate.request_id > record.request_id
                        && Some(candidate.geometry) == self.desired_geometry
                });
                if !newer_same_desired && Some(record.geometry) == self.desired_geometry {
                    self.retry_suppression = Some(RetrySuppression {
                        geometry: record.geometry,
                    });
                }
                if matches!(
                    error,
                    ErrorCode::InvalidState
                        | ErrorCode::InvalidExecution
                        | ErrorCode::InvalidAttachment
                        | ErrorCode::StaleIdentity
                        | ErrorCode::PermissionDenied
                        | ErrorCode::ControllerBusy
                        | ErrorCode::UnsupportedVersion
                        | ErrorCode::UnknownMessage
                        | ErrorCode::MalformedPayload
                ) {
                    self.role = Role::Observer;
                    self.input_failure = Some(InputAdmissionFailure::LostController);
                }
            }
        }
        self.reconcile_resize()
    }

    fn set_resize_phase(&mut self, request_id: u64, phase: ResizePhase) -> Result<(), ClientError> {
        let record = self
            .unresolved_resizes
            .iter_mut()
            .find(|record| record.request_id == request_id)
            .ok_or(ClientError::ResizeProtocolFailure)?;
        record.phase = phase;
        Ok(())
    }

    pub(crate) fn reconcile_resize(&mut self) -> Result<(), ClientError> {
        let Some(desired) = self.desired_geometry else {
            return Ok(());
        };
        if self.role != Role::Controller {
            return Ok(());
        }
        if self
            .retry_suppression
            .is_some_and(|suppressed| suppressed.geometry == desired)
        {
            return Ok(());
        }

        let newest_pending =
            newest_pending_geometry(&self.unresolved_resizes, self.applied_awaiting_projection);
        if !resize_needs_mutation(desired, self.committed_geometry, newest_pending) {
            return Ok(());
        }

        if let Some(last) = self.outbound.back_mut()
            && last.offset == 0
            && let OutboundKind::Resize {
                request_id,
                geometry,
            } = &mut last.kind
        {
            let Some(record) = self
                .unresolved_resizes
                .iter_mut()
                .find(|record| record.request_id == *request_id)
            else {
                return Err(ClientError::ResizeProtocolFailure);
            };
            if record.phase != ResizePhase::QueuedNotStarted {
                return Err(ClientError::ResizeProtocolFailure);
            }
            let payload = ResizeRequest {
                attachment_id: self.attachment_id,
                request_id: *request_id,
                rows: desired.rows,
                columns: desired.columns,
            }
            .encode();
            let replacement = encode_frame(MessageType::ResizeRequest, &payload);
            if replacement.len() != last.bytes.len() {
                return Err(ClientError::Protocol);
            }
            last.bytes = replacement;
            *geometry = desired;
            record.geometry = desired;
            return Ok(());
        }

        if self.unresolved_resizes.len() >= MAX_UNRESOLVED_RESIZES {
            self.resize_failure = Some(ResizeFailure::ClientBackpressure);
            return Err(ClientError::ClientBackpressure);
        }

        let request_id = self.next_resize_request_id;
        if request_id == 0 {
            self.resize_failure = Some(ResizeFailure::Protocol);
            return Err(ClientError::ResizeProtocolFailure);
        }
        let payload = ResizeRequest {
            attachment_id: self.attachment_id,
            request_id,
            rows: desired.rows,
            columns: desired.columns,
        }
        .encode();
        let frame = encode_frame(MessageType::ResizeRequest, &payload);
        if let Err(error) = self.admit_frame(
            frame,
            OutboundKind::Resize {
                request_id,
                geometry: desired,
            },
        ) {
            if error == ClientError::ClientBackpressure {
                self.resize_failure = Some(ResizeFailure::ClientBackpressure);
            }
            return Err(error);
        }
        self.unresolved_resizes.push_back(ResizeRecord {
            request_id,
            geometry: desired,
            phase: ResizePhase::QueuedNotStarted,
        });
        self.next_resize_request_id = request_id.checked_add(1).unwrap_or(0);
        if self.resize_failure == Some(ResizeFailure::ClientBackpressure) {
            self.resize_failure = None;
        }
        Ok(())
    }

    pub(crate) fn request_resync(&mut self) -> Result<(), ClientError> {
        self.resync_needed = true;
        self.clear_viewport_line_ids();
        self.try_queue_resync()
    }

    pub(crate) fn try_queue_resync(&mut self) -> Result<(), ClientError> {
        if !self.resync_needed
            || self
                .outbound
                .iter()
                .any(|pending| pending.kind == OutboundKind::Resync)
        {
            return Ok(());
        }
        let frame = encode_frame(
            MessageType::Resync,
            &Resync {
                attachment_id: self.attachment_id,
            }
            .encode(),
        );
        match self.admit_frame(frame, OutboundKind::Resync) {
            Ok(()) => {
                self.resync_needed = false;
                self.flush_control_write()
            }
            Err(ClientError::ClientBackpressure) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use seyal_runtime::{
        local_ipc::framing::{ComposerResult, ComposerResultCode, TerminalKeyKind},
        AttachmentId,
    };

    fn geometry(rows: u16, columns: u16) -> GridGeometry {
        GridGeometry { rows, columns }
    }

    #[test]
    fn mouse_cell_uses_top_origin_and_clamps_to_grid() {
        assert_eq!(
            cell_from_point(15.0, 25.0, 80.0, 48.0, 0.0, 0.0, 8.0, 16.0),
            Some((1, 1))
        );
        assert_eq!(
            cell_from_point(79.0, 47.0, 80.0, 48.0, 0.0, 0.0, 8.0, 16.0),
            Some((9, 2))
        );
        assert_eq!(
            cell_from_point(-1.0, 0.0, 80.0, 48.0, 0.0, 0.0, 8.0, 16.0),
            None
        );
    }

    #[test]
    fn finite_geometry_validation_rejects_each_nonfinite_operand_and_clamps() {
        let base = [800.0, 600.0, 20.0, 20.0, 10.0, 20.0];
        for index in 0..base.len() {
            for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let mut values = base;
                values[index] = invalid;
                assert_eq!(
                    derive_grid_geometry(
                        values[0], values[1], values[2], values[3], values[4], values[5]
                    ),
                    None
                );
            }
        }

        for invalid in [
            [-1.0, 600.0, 20.0, 20.0, 10.0, 20.0],
            [800.0, -1.0, 20.0, 20.0, 10.0, 20.0],
            [800.0, 600.0, -1.0, 20.0, 10.0, 20.0],
            [800.0, 600.0, 20.0, -1.0, 10.0, 20.0],
            [800.0, 600.0, 20.0, 20.0, 0.0, 20.0],
            [800.0, 600.0, 20.0, 20.0, -1.0, 20.0],
            [800.0, 600.0, 20.0, 20.0, 10.0, 0.0],
            [800.0, 600.0, 20.0, 20.0, 10.0, -1.0],
        ] {
            assert_eq!(
                derive_grid_geometry(
                    invalid[0], invalid[1], invalid[2], invalid[3], invalid[4], invalid[5]
                ),
                None
            );
        }

        let smallest_positive = f64::from_bits(1);
        assert_eq!(
            derive_grid_geometry(1.0, 1.0, 0.0, 0.0, smallest_positive, 1.0),
            None
        );
        assert_eq!(
            derive_grid_geometry(1.0, 1.0, 0.0, 0.0, 1.0, smallest_positive),
            None
        );
        assert_eq!(
            derive_grid_geometry(0.1, 0.1, 0.0, 0.0, 10.0, 20.0),
            Some(GridGeometry {
                rows: 1,
                columns: 1
            })
        );
        assert_eq!(
            derive_grid_geometry(1.0e12, 1.0e12, 0.0, 0.0, 1.0, 1.0),
            Some(GridGeometry {
                rows: 256,
                columns: 512
            })
        );
    }

    #[test]
    fn newest_pending_resize_is_highest_request_id_across_result_and_unresolved_state() {
        let mut unresolved = VecDeque::from([ResizeRecord {
            request_id: 1,
            geometry: geometry(24, 80),
            phase: ResizePhase::SentWaitingResult,
        }]);
        let fence = AppliedFence {
            request_id: 2,
            geometry: geometry(30, 100),
            applied_generation: 9,
        };
        assert_eq!(
            newest_pending_geometry(&unresolved, Some(fence)),
            Some(geometry(30, 100))
        );

        unresolved.push_back(ResizeRecord {
            request_id: 3,
            geometry: geometry(24, 80),
            phase: ResizePhase::QueuedNotStarted,
        });
        assert_eq!(
            newest_pending_geometry(&unresolved, Some(fence)),
            Some(geometry(24, 80))
        );
    }

    #[test]
    fn committed_geometry_does_not_suppress_restore_when_newer_pending_resize_differs() {
        let committed = geometry(24, 80);
        let desired = geometry(24, 80);
        assert!(resize_needs_mutation(
            desired,
            committed,
            Some(geometry(30, 100))
        ));
        assert!(!resize_needs_mutation(desired, committed, None));
        assert!(!resize_needs_mutation(
            desired,
            geometry(30, 100),
            Some(desired)
        ));
    }

    #[test]
    fn applied_fence_is_pending_until_authoritative_generation_catches_up() {
        let desired = geometry(30, 100);
        let committed = geometry(24, 80);
        let fence = AppliedFence {
            request_id: 4,
            geometry: desired,
            applied_generation: 12,
        };

        // A successful result is still pending projection, so reconciliation
        // must not admit another request for the same target.
        assert!(!resize_needs_mutation(
            desired,
            committed,
            Some(fence.geometry)
        ));
        assert_eq!(
            newest_pending_geometry(&VecDeque::new(), Some(fence)),
            Some(desired)
        );
    }

    #[test]
    fn newer_unresolved_resize_remains_authoritative_over_older_applied_fence() {
        let older = AppliedFence {
            request_id: 4,
            geometry: geometry(30, 100),
            applied_generation: 12,
        };
        let newer = ResizeRecord {
            request_id: 5,
            geometry: geometry(40, 120),
            phase: ResizePhase::SentWaitingResult,
        };

        assert_eq!(
            newest_pending_geometry(&VecDeque::from([newer]), Some(older)),
            Some(geometry(40, 120))
        );
    }

    #[test]
    fn invalid_terminal_key_requests_fail_before_wire_encoding() {
        assert!(!valid_terminal_key_request(
            TerminalKeyKind::ControlAscii,
            'é' as u32
        ));
        assert!(!valid_terminal_key_request(TerminalKeyKind::ArrowUp, 1));
        assert!(valid_terminal_key_request(
            TerminalKeyKind::ControlAscii,
            '?' as u32
        ));
        assert!(valid_terminal_key_request(TerminalKeyKind::ArrowUp, 0));
    }

    #[test]
    fn runtime_input_backpressure_is_visible_without_fatal_disconnect() {
        let error = ErrorMessage {
            error_code: ErrorCode::Backpressure as u16,
            offending_message_type: MessageType::Input as u16,
            detail_code: 0,
        };
        assert_eq!(
            classify_server_error(error).unwrap(),
            Some(InputAdmissionFailure::ClientBackpressure)
        );
        let key_error = ErrorMessage {
            offending_message_type: MessageType::TerminalKey as u16,
            ..error
        };
        assert_eq!(
            classify_server_error(key_error).unwrap(),
            Some(InputAdmissionFailure::ClientBackpressure)
        );
        assert_eq!(
            classify_server_error(ErrorMessage {
                error_code: ErrorCode::InvalidExecution as u16,
                offending_message_type: MessageType::Input as u16,
                detail_code: 0,
            }),
            Err(ClientError::Server(ErrorCode::InvalidExecution))
        );
        assert_eq!(
            classify_server_error(ErrorMessage {
                error_code: ErrorCode::CapacityExceeded as u16,
                offending_message_type: MessageType::HistoryRangeRequest as u16,
                detail_code: 0,
            }),
            Ok(None)
        );
        assert_eq!(
            classify_server_error(ErrorMessage {
                error_code: ErrorCode::CapacityExceeded as u16,
                offending_message_type: MessageType::BlockTimeline as u16,
                detail_code: 0,
            }),
            Err(ClientError::Server(ErrorCode::CapacityExceeded))
        );
        assert_eq!(
            classify_server_error(ErrorMessage {
                error_code: ErrorCode::InvalidState as u16,
                offending_message_type: MessageType::HostSearch as u16,
                detail_code: 0,
            }),
            Ok(None)
        );
        assert_eq!(
            classify_server_error(ErrorMessage {
                error_code: ErrorCode::InvalidState as u16,
                offending_message_type: MessageType::HostSelection as u16,
                detail_code: 0,
            }),
            Ok(None)
        );
    }

    #[test]
    fn composer_result_quarantines_cross_attachment_and_unknown_requests() {
        let attachment = AttachmentId::from_bytes([7; 16]);
        let other_attachment = AttachmentId::from_bytes([8; 16]);
        let mut pending = std::collections::HashSet::new();
        pending.insert(41);

        let accepted = ComposerResult {
            attachment_id: attachment,
            code: ComposerResultCode::Accepted,
            block_id: 99,
            request_id: 41,
        };
        assert!(super::super::validate_composer_result(
            accepted, attachment, &pending
        ));
        assert!(!super::super::validate_composer_result(
            accepted,
            other_attachment,
            &pending
        ));
        assert!(!super::super::validate_composer_result(
            ComposerResult {
                request_id: 42,
                ..accepted
            },
            attachment,
            &pending
        ));
    }

    #[test]
    fn composer_result_requires_code_specific_block_identity() {
        let attachment = AttachmentId::from_bytes([9; 16]);
        let pending = std::collections::HashSet::from([5]);
        assert!(!super::super::validate_composer_result(
            ComposerResult {
                attachment_id: attachment,
                code: ComposerResultCode::Accepted,
                block_id: 0,
                request_id: 5,
            },
            attachment,
            &pending
        ));
        assert!(!super::super::validate_composer_result(
            ComposerResult {
                attachment_id: attachment,
                code: ComposerResultCode::Busy,
                block_id: 77,
                request_id: 5,
            },
            attachment,
            &pending
        ));
        assert!(super::super::validate_composer_result(
            ComposerResult {
                attachment_id: attachment,
                code: ComposerResultCode::Busy,
                block_id: 0,
                request_id: 5,
            },
            attachment,
            &pending
        ));
    }
}
