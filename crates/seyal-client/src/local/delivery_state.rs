//! Client-side filtering and recovery fences for attachment presentation frames.

use seyal_runtime::local_ipc::framing::decode_presentation_envelope as decode_envelope;

use super::{ClientError, LocalDisplayClient};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PresentationFrameDisposition {
    Legacy,
    Current,
    Discard,
    UnexpectedFuture,
}

impl PresentationFrameDisposition {
    pub(super) fn has_envelope(self) -> bool {
        matches!(self, Self::Current | Self::Discard | Self::UnexpectedFuture)
    }

    pub(super) fn must_discard(self) -> bool {
        matches!(self, Self::Discard | Self::UnexpectedFuture)
    }
}

impl LocalDisplayClient {
    /// Classify a complete frame without borrowing mutable client state across
    /// the read buffer. Future epochs are cleared by the caller after this ends.
    pub(super) fn classify_presentation_frame(
        &self,
        frame: &[u8],
        message_type: u16,
    ) -> Result<PresentationFrameDisposition, ClientError> {
        if !self.delivery_control_negotiated
            || !matches!(message_type, 12 | 13 | 21 | 26 | 27 | 28 | 35)
        {
            return Ok(PresentationFrameDisposition::Legacy);
        }
        let envelope = decode_envelope(frame).map_err(|_| ClientError::Protocol)?;
        let is_snapshot = matches!(message_type, 12 | 27);
        if envelope.attachment_id != self.attachment_id
            || envelope.resume_sequence < self.resume_sequence
            || self.delivery_suspended
            || (self.resume_fenced && !is_snapshot)
        {
            return Ok(PresentationFrameDisposition::Discard);
        }
        if envelope.resume_sequence > self.resume_sequence {
            return Ok(PresentationFrameDisposition::UnexpectedFuture);
        }
        Ok(PresentationFrameDisposition::Current)
    }
}
