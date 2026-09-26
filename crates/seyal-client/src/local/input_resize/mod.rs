use std::io::Write;

use seyal_runtime::local_ipc::framing::{
    encode_frame, ErrorCode, ErrorMessage, HostSearch, HostSelection, HostSelectionAction,
    InputRef, MessageType, ResizeRequest, ResizeResult, ResizeResultCode, Resync, Role,
    TerminalKey, TerminalKeyKind, TerminalKeyModifiers, TerminalKeyV2, TerminalKeyV2Event,
    TerminalKeyV2Kind, TerminalKeyV2Modifiers, TerminalMouse, TerminalMouseKind, MAX_INPUT_BYTES,
};

use super::{
    server_error, ClientError, LocalDisplayClient, MAX_OUTBOUND_WIRE_BYTES, MAX_UNRESOLVED_RESIZES,
};

mod geometry;
mod planner;

#[cfg(test)]
mod tests;

pub use geometry::{
    cell_from_point, derive_grid_geometry, GridGeometry, InputAdmissionFailure, ResizeFailure,
};
pub(crate) use planner::{
    classify_server_error, newest_pending_geometry, resize_needs_mutation,
    valid_terminal_key_request, AppliedFence, OutboundKind, PendingControlWrite, ResizePhase,
    ResizeRecord, RetrySuppression,
};

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
