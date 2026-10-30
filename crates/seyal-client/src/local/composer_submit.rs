//! Composer command and session-consent submits. Relay lines are fenced to the
//! displayed target generation and do not allocate a Block.

use seyal_runtime::local_ipc::framing::{
    encode_frame, ComposerTargetMode, MessageType, SessionConsent, SessionConsentDecision,
};

use super::{
    input_resize::{InputAdmissionFailure, OutboundKind},
    ClientError, LocalDisplayClient,
};

impl LocalDisplayClient {
    pub fn submit_composer_command(&mut self, command: &str) -> Result<(), ClientError> {
        self.require_controller()?;
        if !self.command_blocks_supported {
            return Err(ClientError::UnsupportedInteractiveCapability);
        }
        let request_id = self.next_composer_request_id;
        self.next_composer_request_id = request_id.checked_add(1).unwrap_or(1);
        let command_ref = seyal_runtime::local_ipc::framing::ComposerCommandRef {
            attachment_id: self.attachment_id,
            request_id,
            command,
        };
        let payload = if self.session_bootstrap_negotiated {
            let generation = self.target_generation;
            if generation == 0 {
                return Err(ClientError::Protocol);
            }
            command_ref.encode_fenced(generation)
        } else {
            command_ref.encode()
        };
        let relay = self
            .composer_target
            .is_some_and(|target| target.mode == ComposerTargetMode::Relay);
        let frame = encode_frame(MessageType::ComposerCommand, &payload);
        if let Err(error) = self.admit_frame(frame, OutboundKind::ComposerCommand) {
            self.input_failure = Some(InputAdmissionFailure::ClientBackpressure);
            return Err(error);
        }
        self.pending_composer_requests.insert(request_id);
        if relay {
            self.pending_relay_requests.insert(request_id);
        }
        self.input_failure = None;
        let result = self.flush_control_write();
        if result.is_err() {
            self.pending_composer_requests.remove(&request_id);
            self.pending_relay_requests.remove(&request_id);
        }
        result
    }

    /// Explicit "Enable Blocks for this session" (§4.2). Runtime publishes a
    /// type 42 offer; Accept/Decline use `submit_session_consent`.
    pub fn request_session_offer(&mut self, host_block_id: u64) -> Result<(), ClientError> {
        self.require_controller()?;
        if !self.session_bootstrap_negotiated {
            return Err(ClientError::UnsupportedInteractiveCapability);
        }
        if host_block_id == 0 {
            return Err(ClientError::Protocol);
        }
        let consent = SessionConsent {
            attachment_id: self.attachment_id,
            execution_id: self.execution_id,
            host_block_id,
            offer_generation: 0,
            presentation_epoch: 0,
            decision: SessionConsentDecision::Request,
        };
        let frame = encode_frame(MessageType::SessionConsent, &consent.encode());
        self.admit_frame(frame, OutboundKind::SessionConsent)?;
        self.flush_control_write()
    }

    /// Echo the current `SessionOffer`. Decline leaves the relay path; Accept
    /// is fenced by Runtime and fails closed until the stager exists.
    pub fn submit_session_consent(
        &mut self,
        decision: SessionConsentDecision,
    ) -> Result<(), ClientError> {
        self.require_controller()?;
        if !self.session_bootstrap_negotiated {
            return Err(ClientError::UnsupportedInteractiveCapability);
        }
        if matches!(decision, SessionConsentDecision::Request) {
            return Err(ClientError::Protocol);
        }
        let Some(offer) = self.session_offer else {
            return Err(ClientError::Protocol);
        };
        let consent = SessionConsent {
            attachment_id: offer.attachment_id,
            execution_id: offer.execution_id,
            host_block_id: offer.host_block_id,
            offer_generation: offer.offer_generation,
            presentation_epoch: offer.presentation_epoch,
            decision,
        };
        let frame = encode_frame(MessageType::SessionConsent, &consent.encode());
        self.admit_frame(frame, OutboundKind::SessionConsent)?;
        self.flush_control_write()
    }
}
