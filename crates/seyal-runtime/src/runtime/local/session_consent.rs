//! Controller `SessionConsent` (type 43). Decline stays on the relay path.
//! Accept arms and then fails closed until the Slice B stager can write.

use crate::local_ipc::{
    attachment::AttachmentError,
    framing::{self, ErrorCode, MessageType, CAP_COMMAND_BLOCKS},
};

use super::super::Runtime;

impl Runtime {
    pub(super) fn handle_session_consent(&mut self, token: u64, payload: &[u8]) {
        let Ok(consent) = framing::SessionConsent::decode(payload) else {
            self.send_error(
                token,
                ErrorCode::MalformedPayload,
                MessageType::SessionConsent as u16,
            );
            return;
        };
        let negotiated = self.local_ipc.as_ref().is_some_and(|state| {
            state.connections.get(&token).is_some_and(|meta| {
                meta.client_capabilities & framing::CAP_SESSION_BOOTSTRAP != 0
                    && meta.client_capabilities & CAP_COMMAND_BLOCKS != 0
            })
        });
        if !negotiated {
            self.send_error(
                token,
                ErrorCode::PermissionDenied,
                MessageType::SessionConsent as u16,
            );
            return;
        }
        let execution_id = match self.local_ipc.as_ref().map(|state| {
            state
                .attachments
                .authorize_mutation(token, consent.attachment_id)
        }) {
            Some(Ok(id)) if id == consent.execution_id => id,
            Some(Ok(_)) | Some(Err(AttachmentError::PermissionDenied)) => {
                self.send_error(
                    token,
                    ErrorCode::PermissionDenied,
                    MessageType::SessionConsent as u16,
                );
                return;
            }
            _ => {
                self.send_error(
                    token,
                    ErrorCode::StaleIdentity,
                    MessageType::SessionConsent as u16,
                );
                return;
            }
        };
        let controller = self.local_ipc.as_ref().is_some_and(|state| {
            state.attachments.role_of(consent.attachment_id).ok() == Some(framing::Role::Controller)
        });
        if !controller {
            self.send_error(
                token,
                ErrorCode::PermissionDenied,
                MessageType::SessionConsent as u16,
            );
            return;
        }
        if self
            .apply_session_consent(
                execution_id,
                consent.host_block_id,
                consent.offer_generation,
                consent.presentation_epoch,
                matches!(consent.decision, framing::SessionConsentDecision::Accept),
            )
            .is_err()
        {
            self.send_error(
                token,
                ErrorCode::InvalidState,
                MessageType::SessionConsent as u16,
            );
        }
    }
}
