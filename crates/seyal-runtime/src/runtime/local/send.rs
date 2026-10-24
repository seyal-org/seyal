use crate::{
    display::EncodedDisplayBatch,
    local_ipc::{
        framing::{
            self, ErrorCode, FrameHeader, MessageType, ResizeResult as WireResizeResult,
            ResizeResultCode, CAP_ATTACHMENT_DELIVERY_CONTROL,
        },
        recovery,
    },
};
use std::sync::Arc;

use super::super::Runtime;
use crate::AttachmentId;

impl Runtime {
    pub(super) fn send_mandatory_frame(&mut self, token: u64, bytes: Vec<u8>) -> bool {
        let queued = self
            .local_ipc
            .as_mut()
            .is_some_and(|state| state.server.enqueue_mandatory(token, bytes).is_ok());
        if !queued {
            self.close_local_connection(token);
            return false;
        }
        self.sync_local_writable(token)
    }

    pub(super) fn send_after_display_frame(&mut self, token: u64, bytes: Vec<u8>) -> bool {
        let bytes = match self.envelope_type_35(token, bytes) {
            Ok(bytes) => bytes,
            Err(()) => {
                self.close_local_connection(token);
                return false;
            }
        };
        let queued = self
            .local_ipc
            .as_mut()
            .is_some_and(|state| state.server.enqueue_after_display(token, bytes).is_ok());
        if !queued {
            self.close_local_connection(token);
            return false;
        }
        self.sync_local_writable(token)
    }

    pub(super) fn send_attachment_after_display_frame(
        &mut self,
        token: u64,
        attachment_id: AttachmentId,
        bytes: Vec<u8>,
    ) -> bool {
        let negotiated = self.connection_has_delivery_control(token);
        let sequence = negotiated
            .then(|| self.resume_sequence(token, attachment_id))
            .flatten();
        if negotiated && sequence.is_none() {
            return false;
        }
        self.send_attachment_after_display_frame_at_sequence(
            token,
            attachment_id,
            sequence.unwrap_or(0),
            bytes,
        )
    }

    /// Queue an un-enveloped frame behind one negotiated attachment's own
    /// presentation output. Lifecycle carries its ExecutionId directly; its
    /// transport ordering, rather than a presentation envelope, scopes it.
    pub(super) fn send_attachment_lifecycle_after_display_frame(
        &mut self,
        token: u64,
        attachment_id: AttachmentId,
        bytes: Vec<u8>,
    ) -> bool {
        let negotiated = self.connection_has_delivery_control(token);
        let queued = self.local_ipc.as_mut().is_some_and(|state| {
            if negotiated {
                state
                    .server
                    .enqueue_attachment_after_display(token, attachment_id, bytes)
                    .is_ok()
            } else {
                state.server.enqueue_after_display(token, bytes).is_ok()
            }
        });
        if !queued {
            self.close_local_connection(token);
            return false;
        }
        self.sync_local_writable(token)
    }

    pub(super) fn send_attachment_after_display_frame_at_sequence(
        &mut self,
        token: u64,
        attachment_id: AttachmentId,
        sequence: u64,
        bytes: Vec<u8>,
    ) -> bool {
        let negotiated = self.connection_has_delivery_control(token);
        let bytes = if negotiated {
            match framing::envelope_presentation_frame(&bytes, attachment_id, sequence) {
                Ok(bytes) => bytes,
                Err(_) => {
                    self.close_local_connection(token);
                    return false;
                }
            }
        } else {
            bytes
        };
        let queued = self.local_ipc.as_mut().is_some_and(|state| {
            if negotiated {
                state
                    .server
                    .enqueue_attachment_after_display(token, attachment_id, bytes)
                    .is_ok()
            } else {
                state.server.enqueue_after_display(token, bytes).is_ok()
            }
        });
        if !queued {
            self.close_local_connection(token);
            return false;
        }
        self.sync_local_writable(token)
    }

    pub(super) fn send_snapshot_batch(
        &mut self,
        token: u64,
        attachment_id: AttachmentId,
        batch: EncodedDisplayBatch,
    ) -> bool {
        let negotiated = self.connection_has_delivery_control(token);
        let sequence = self.resume_sequence(token, attachment_id).unwrap_or(0);
        let batch =
            match self.envelope_display_batch_for_attachment(token, attachment_id, sequence, batch)
            {
                Ok(batch) => batch,
                Err(()) => {
                    self.close_local_connection(token);
                    return false;
                }
            };
        let queued = self.local_ipc.as_mut().is_some_and(|state| {
            if negotiated {
                state
                    .server
                    .enqueue_attachment_snapshot(token, attachment_id, batch)
                    .is_ok()
            } else {
                state.server.enqueue_snapshot(token, batch).is_ok()
            }
        });
        if !queued {
            self.close_local_connection(token);
            return false;
        }
        self.sync_local_writable(token)
    }

    pub(super) fn enqueue_delta_batch(
        &mut self,
        token: u64,
        attachment_id: AttachmentId,
        delta: EncodedDisplayBatch,
    ) -> Option<crate::local_ipc::connection::DeltaEnqueueResult> {
        let negotiated = self.connection_has_delivery_control(token);
        let sequence = self.resume_sequence(token, attachment_id).unwrap_or(0);
        let delta =
            match self.envelope_display_batch_for_attachment(token, attachment_id, sequence, delta)
            {
                Ok(delta) => delta,
                Err(()) => {
                    self.close_local_connection(token);
                    return None;
                }
            };
        let result = self.local_ipc.as_mut().and_then(|state| {
            if negotiated {
                state
                    .server
                    .try_enqueue_attachment_delta(token, attachment_id, delta)
                    .ok()
            } else {
                state.server.try_enqueue_delta(token, delta).ok()
            }
        });
        if result.is_none() {
            self.close_local_connection(token);
        }
        result
    }

    pub(super) fn connection_has_delivery_control(&self, token: u64) -> bool {
        self.local_ipc
            .as_ref()
            .and_then(|state| state.connections.get(&token))
            .is_some_and(|meta| meta.client_capabilities & CAP_ATTACHMENT_DELIVERY_CONTROL != 0)
    }

    pub(super) fn resume_sequence(&self, token: u64, attachment_id: AttachmentId) -> Option<u64> {
        let state = self.local_ipc.as_ref()?;
        state
            .attachments
            .execution_for_connection(token, attachment_id)
            .ok()?;
        state.attachments.resume_sequence_of(attachment_id).ok()
    }

    pub(super) fn envelope_display_batch_for_attachment(
        &self,
        token: u64,
        attachment_id: crate::AttachmentId,
        resume_sequence: u64,
        batch: EncodedDisplayBatch,
    ) -> Result<EncodedDisplayBatch, ()> {
        let negotiated = self.local_ipc.as_ref().is_some_and(|state| {
            state
                .connections
                .get(&token)
                .is_some_and(|meta| meta.client_capabilities & CAP_ATTACHMENT_DELIVERY_CONTROL != 0)
        });
        if !negotiated {
            return Ok(batch);
        }
        envelope_display_batch(batch, attachment_id, resume_sequence)
    }

    fn presentation_identity(&self, token: u64) -> Option<(crate::AttachmentId, u64)> {
        let state = self.local_ipc.as_ref()?;
        let meta = state.connections.get(&token)?;
        if meta.client_capabilities & CAP_ATTACHMENT_DELIVERY_CONTROL == 0 {
            return None;
        }
        let attachment_id = meta.attachment?;
        state
            .attachments
            .execution_for_connection(token, attachment_id)
            .ok()?;
        let sequence = state.attachments.resume_sequence_of(attachment_id).ok()?;
        Some((attachment_id, sequence))
    }

    fn envelope_type_35(&self, token: u64, bytes: Vec<u8>) -> Result<Vec<u8>, ()> {
        let Some((attachment_id, sequence)) = self.presentation_identity(token) else {
            return Ok(bytes);
        };
        let header = FrameHeader::decode(&bytes).map_err(|_| ())?;
        if header.message_type != MessageType::ViewportLineIds as u16 {
            return Ok(bytes);
        }
        framing::envelope_presentation_frame(&bytes, attachment_id, sequence).map_err(|_| ())
    }

    pub(super) fn schedule_snapshot_recovery(&mut self, token: u64, attachment_id: AttachmentId) {
        let should_wake = if let Some(state) = self.local_ipc.as_mut() {
            if !state.connections.contains_key(&token) || !state.server.contains(token) {
                false
            } else if recovery::schedule_snapshot_recovery(
                &mut state.pending_resync,
                &mut state.pending_resync_set,
                token,
                attachment_id,
            ) {
                !state
                    .server
                    .has_attachment_snapshot_delivery(token, attachment_id)
            } else {
                false
            }
        } else {
            false
        };
        if should_wake {
            let _ = self.reactor.waker().wake();
        }
    }

    pub(super) fn send_error(&mut self, token: u64, code: ErrorCode, offending: u16) {
        self.send_error_detail(token, code, offending, 0);
    }

    pub(super) fn send_error_detail(
        &mut self,
        token: u64,
        code: ErrorCode,
        offending: u16,
        detail_code: u32,
    ) {
        let message = framing::ErrorMessage {
            error_code: code as u16,
            offending_message_type: offending,
            detail_code,
        };
        let _ = self.send_mandatory_frame(
            token,
            framing::encode_frame(MessageType::Error, &message.encode()),
        );
    }

    pub(super) fn send_resize_result(
        &mut self,
        token: u64,
        attachment_id: AttachmentId,
        request_id: u64,
        result_code: ResizeResultCode,
        applied_generation: u64,
    ) {
        let message = WireResizeResult {
            attachment_id,
            request_id,
            result_code,
            detail_code: 0,
            applied_generation,
        };
        let _ = self.send_mandatory_frame(
            token,
            framing::encode_frame(MessageType::ResizeResult, &message.encode()),
        );
    }
}

fn envelope_display_batch(
    batch: EncodedDisplayBatch,
    attachment_id: crate::AttachmentId,
    resume_sequence: u64,
) -> Result<EncodedDisplayBatch, ()> {
    let mut total_bytes = 0usize;
    let mut frames = Vec::with_capacity(batch.frames.len());
    for frame in batch.frames {
        let enveloped =
            framing::envelope_presentation_frame(&frame, attachment_id, resume_sequence)
                .map_err(|_| ())?;
        total_bytes = total_bytes.checked_add(enveloped.len()).ok_or(())?;
        frames.push(Arc::<[u8]>::from(enveloped));
    }
    Ok(EncodedDisplayBatch {
        kind: batch.kind,
        schema: batch.schema,
        generation: batch.generation,
        base_generation: batch.base_generation,
        rows: batch.rows,
        columns: batch.columns,
        frames,
        total_bytes,
    })
}
