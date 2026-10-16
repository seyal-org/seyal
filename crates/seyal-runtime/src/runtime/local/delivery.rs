//! SPEC-004 §19 per-attachment delivery suspend/resume (W5).

use crate::{
    local_ipc::{
        attachment::{AttachmentError, DeliveryState},
        connection::ConnectionState as LocalIpcConnState,
        framing::{
            ErrorCode, MessageType, ResumeDelivery, SuspendDelivery,
            CAP_ATTACHMENT_DELIVERY_CONTROL,
        },
    },
    AttachmentId,
};

use super::super::Runtime;

impl Runtime {
    pub(super) fn handle_suspend_delivery(&mut self, token: u64, payload: &[u8]) {
        self.handle_delivery_control(token, payload, MessageType::SuspendDelivery);
    }

    pub(super) fn handle_resume_delivery(&mut self, token: u64, payload: &[u8]) {
        self.handle_delivery_control(token, payload, MessageType::ResumeDelivery);
    }

    fn handle_delivery_control(&mut self, token: u64, payload: &[u8], kind: MessageType) {
        if !self.connection_has_delivery_control(token) {
            self.send_error(token, ErrorCode::UnknownMessage, kind as u16);
            return;
        }
        let current = self
            .local_ipc
            .as_ref()
            .and_then(|state| state.server.state_of(token));
        if current != Some(LocalIpcConnState::Attached) {
            self.send_error(token, ErrorCode::InvalidState, kind as u16);
            return;
        }
        let attachment_id = match decode_delivery_id(kind, payload) {
            Ok(id) => id,
            Err(()) => {
                self.send_error(token, ErrorCode::MalformedPayload, kind as u16);
                return;
            }
        };
        if attachment_id == AttachmentId::from_bytes([0; 16]) {
            self.send_error(token, ErrorCode::InvalidAttachment, kind as u16);
            return;
        }
        let lookup = self.local_ipc.as_ref().map(|state| {
            state
                .attachments
                .execution_for_connection(token, attachment_id)
        });
        match lookup {
            Some(Ok(_)) => {}
            Some(Err(AttachmentError::StaleIdentity))
            | Some(Err(AttachmentError::UnknownAttachment)) => {
                self.send_error(token, ErrorCode::StaleIdentity, kind as u16);
                return;
            }
            _ => {
                self.send_error(token, ErrorCode::StaleIdentity, kind as u16);
                return;
            }
        }

        match kind {
            MessageType::SuspendDelivery => {
                let previous = self.local_ipc.as_mut().and_then(|state| {
                    state
                        .attachments
                        .set_delivery(attachment_id, DeliveryState::Suspended)
                        .ok()
                });
                if previous == Some(DeliveryState::Delivering)
                    && let Some(state) = self.local_ipc.as_mut()
                {
                    state.server.drop_not_yet_started_presentation(token);
                    // Resume may have queued recovery in this same input batch.
                    // Leave its queue node stale so publication cannot revive a
                    // suspended attachment after this control has been handled.
                    state.pending_resync_set.remove(&token);
                }
            }
            MessageType::ResumeDelivery => {
                let conflicts_with_delivering_execution =
                    self.local_ipc.as_ref().is_some_and(|state| {
                        state
                            .attachments
                            .delivering_on_connection(token)
                            .iter()
                            .any(|(current_id, _)| *current_id != attachment_id)
                    });
                if conflicts_with_delivering_execution {
                    self.send_error(token, ErrorCode::InvalidState, kind as u16);
                    return;
                }
                let previous = self.local_ipc.as_mut().and_then(|state| {
                    state
                        .attachments
                        .set_delivery(attachment_id, DeliveryState::Delivering)
                        .ok()
                });
                if previous == Some(DeliveryState::Suspended) {
                    if let Some(meta) = self
                        .local_ipc
                        .as_mut()
                        .and_then(|state| state.connections.get_mut(&token))
                    {
                        meta.attachment = Some(attachment_id);
                    }
                    self.schedule_snapshot_recovery(token);
                }
            }
            _ => unreachable!("delivery control kinds only"),
        }
    }

    fn connection_has_delivery_control(&self, token: u64) -> bool {
        self.local_ipc
            .as_ref()
            .and_then(|state| state.connections.get(&token))
            .is_some_and(|meta| meta.client_capabilities & CAP_ATTACHMENT_DELIVERY_CONTROL != 0)
    }
}

fn decode_delivery_id(kind: MessageType, payload: &[u8]) -> Result<AttachmentId, ()> {
    match kind {
        MessageType::SuspendDelivery => SuspendDelivery::decode(payload)
            .map(|msg| msg.attachment_id)
            .map_err(|_| ()),
        MessageType::ResumeDelivery => ResumeDelivery::decode(payload)
            .map(|msg| msg.attachment_id)
            .map_err(|_| ()),
        _ => Err(()),
    }
}
