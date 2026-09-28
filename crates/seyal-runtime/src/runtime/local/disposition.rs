//! SPEC-004 §18.4 / SPEC-003 §11 client-requested execution disposition (P4).

use crate::{
    local_ipc::{
        connection::ConnectionState as LocalIpcConnState,
        framing::{
            self, ErrorCode, MessageType, Role, TerminateExecutionRequest,
            TerminateExecutionResult, TerminateExecutionResultCode, CAP_EXECUTION_PROVISIONING,
        },
    },
    AttachmentId,
};

use super::super::Runtime;

impl Runtime {
    pub(super) fn handle_terminate_execution_request(&mut self, token: u64, payload: &[u8]) {
        // SPEC-004 §18.4 rule 1: capability before any other terminate admission.
        let negotiated = self.local_ipc.as_ref().is_some_and(|state| {
            state
                .connections
                .get(&token)
                .is_some_and(|meta| meta.client_capabilities & CAP_EXECUTION_PROVISIONING != 0)
        });
        if !negotiated {
            self.send_error(
                token,
                ErrorCode::UnknownMessage,
                MessageType::TerminateExecutionRequest as u16,
            );
            return;
        }

        // Rule 2: Attached only. Fail closed with the generic Error path so an
        // unattached peer cannot obtain a typed type-39 correlation.
        let Some(current_state) = self
            .local_ipc
            .as_ref()
            .and_then(|state| state.server.state_of(token))
        else {
            return;
        };
        if current_state != LocalIpcConnState::Attached {
            self.send_error(
                token,
                ErrorCode::InvalidState,
                MessageType::TerminateExecutionRequest as u16,
            );
            return;
        }

        // Rule 3: exact payload / reserved.
        let Ok(request) = TerminateExecutionRequest::decode(payload) else {
            self.send_error(
                token,
                ErrorCode::MalformedPayload,
                MessageType::TerminateExecutionRequest as u16,
            );
            return;
        };

        // Rule 4: shared connection-local nonzero/strictly-increasing request-id
        // space with create (§18.2). Advance only when accepted.
        let id_ok = self.local_ipc.as_mut().is_some_and(|state| {
            let Some(meta) = state.connections.get_mut(&token) else {
                return false;
            };
            if request.request_id <= meta.last_provisioning_request_id {
                false
            } else {
                meta.last_provisioning_request_id = request.request_id;
                true
            }
        });
        if !id_ok {
            self.send_terminate_execution_result(
                token,
                request.attachment_id,
                request.request_id,
                TerminateExecutionResultCode::Error(ErrorCode::MalformedPayload),
            );
            return;
        }

        // Rule 5: attachment_id is this connection's current live attachment.
        let current_attachment = self.local_ipc.as_ref().and_then(|state| {
            state
                .connections
                .get(&token)
                .and_then(|meta| meta.attachment)
        });
        if request.attachment_id.to_bytes() == [0; 16] {
            self.send_terminate_execution_result(
                token,
                request.attachment_id,
                request.request_id,
                TerminateExecutionResultCode::Error(ErrorCode::InvalidAttachment),
            );
            return;
        }
        let Some(live_attachment) = current_attachment else {
            // Attached state without a live attachment identity is not expected;
            // treat as InvalidState rather than inventing authority.
            self.send_terminate_execution_result(
                token,
                request.attachment_id,
                request.request_id,
                TerminateExecutionResultCode::Error(ErrorCode::InvalidState),
            );
            return;
        };
        if request.attachment_id != live_attachment {
            self.send_terminate_execution_result(
                token,
                request.attachment_id,
                request.request_id,
                TerminateExecutionResultCode::Error(ErrorCode::StaleIdentity),
            );
            return;
        }

        // Rule 6: execution_id matches that attachment's execution.
        let Some(attached_execution) = self
            .local_ipc
            .as_ref()
            .and_then(|state| state.attachments.execution_of(live_attachment).ok())
        else {
            self.send_terminate_execution_result(
                token,
                request.attachment_id,
                request.request_id,
                TerminateExecutionResultCode::Error(ErrorCode::StaleIdentity),
            );
            return;
        };
        if request.execution_id != attached_execution {
            self.send_terminate_execution_result(
                token,
                request.attachment_id,
                request.request_id,
                TerminateExecutionResultCode::Error(ErrorCode::StaleIdentity),
            );
            return;
        }

        // Rule 7: Controller lease only.
        let role = self
            .local_ipc
            .as_ref()
            .and_then(|state| state.attachments.role_of(live_attachment).ok());
        if role != Some(Role::Controller) {
            self.send_terminate_execution_result(
                token,
                request.attachment_id,
                request.request_id,
                TerminateExecutionResultCode::Error(ErrorCode::PermissionDenied),
            );
            return;
        }

        // Drive SPEC-003 §11 with Runtime's own configured TerminationPolicy.
        // `request_termination` is idempotent for already-terminating /
        // draining executions (no extra signal, no deadline reset).
        let _ = self.request_termination(attached_execution);

        self.send_terminate_execution_result(
            token,
            request.attachment_id,
            request.request_id,
            TerminateExecutionResultCode::TerminationRequested,
        );
    }

    fn send_terminate_execution_result(
        &mut self,
        token: u64,
        attachment_id: AttachmentId,
        request_id: u64,
        result_code: TerminateExecutionResultCode,
    ) {
        let message = TerminateExecutionResult {
            attachment_id,
            request_id,
            result_code,
            detail_code: 0,
        };
        let _ = self.send_mandatory_frame(
            token,
            framing::encode_frame(MessageType::TerminateExecutionResult, &message.encode()),
        );
    }
}
