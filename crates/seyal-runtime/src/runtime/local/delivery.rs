//! SPEC-004 §19 per-attachment delivery suspend/resume (W5).

use crate::{
    local_ipc::{
        attachment::{AttachmentError, DeliveryState},
        connection::ConnectionState as LocalIpcConnState,
        framing::{ErrorCode, MessageType, ResumeDelivery, SuspendDelivery},
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
        let (attachment_id, resume_sequence) = match decode_delivery(kind, payload) {
            Ok(decoded) => decoded,
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
                    state
                        .server
                        .drop_attachment_presentation(token, attachment_id);
                    // Resume may have queued recovery in this same input batch.
                    // Leave its queue node stale so publication cannot revive a
                    // suspended attachment after this control has been handled.
                    state.pending_resync_set.remove(&(token, attachment_id));
                }
            }
            MessageType::ResumeDelivery => {
                let Some(sequence) = resume_sequence else {
                    unreachable!("ResumeDelivery decoding supplies its sequence")
                };
                let result = self
                    .local_ipc
                    .as_mut()
                    .map(|state| state.attachments.resume_delivery(attachment_id, sequence));
                match result {
                    Some(Ok(_previous)) => {
                        if let Some(state) = self.local_ipc.as_mut() {
                            if let Some(meta) = state.connections.get_mut(&token) {
                                meta.attachment = Some(attachment_id);
                            }
                            state
                                .server
                                .drop_attachment_presentation(token, attachment_id);
                            state.pending_resync_set.remove(&(token, attachment_id));
                        }
                        self.schedule_snapshot_recovery(token, attachment_id);
                    }
                    Some(Err(AttachmentError::InvalidResumeSequence)) => {
                        self.send_error(token, ErrorCode::InvalidState, kind as u16);
                    }
                    _ => self.send_error(token, ErrorCode::StaleIdentity, kind as u16),
                }
            }
            _ => unreachable!("delivery control kinds only"),
        }
    }
}

fn decode_delivery(kind: MessageType, payload: &[u8]) -> Result<(AttachmentId, Option<u64>), ()> {
    match kind {
        MessageType::SuspendDelivery => SuspendDelivery::decode(payload)
            .map(|msg| (msg.attachment_id, None))
            .map_err(|_| ()),
        MessageType::ResumeDelivery => ResumeDelivery::decode(payload)
            .map(|msg| (msg.attachment_id, Some(msg.resume_sequence)))
            .map_err(|_| ()),
        _ => Err(()),
    }
}
