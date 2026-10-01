//! Terminate-accept handshake on the already-attached display socket.
//!
//! Poll may have left a partial frame in the client buffer. New bytes are
//! appended after that tail so the display parser stays aligned.

use std::time::Instant;

use seyal_runtime::local_ipc::framing::{
    ErrorMessage, FrameHeader, MessageType, HEADER_LEN, MAX_FRAME_PAYLOAD,
};
use seyal_runtime::ExecutionId;

use super::{discovery, server_error, ClientError, LocalDisplayClient, MAX_BUFFERED_BYTES};

/// `Some((message_type, payload_start, frame_end))` when `buf[offset..]` holds
/// one whole frame. `Ok(None)` when the tail is still short.
fn next_complete_frame(
    buf: &[u8],
    offset: usize,
) -> Result<Option<(u16, usize, usize)>, ClientError> {
    let available = buf.len().saturating_sub(offset);
    if available < HEADER_LEN {
        return Ok(None);
    }
    let header = FrameHeader::decode(&buf[offset..offset + HEADER_LEN])
        .map_err(|_| ClientError::Protocol)?;
    let payload_len = header.payload_len as usize;
    if payload_len > MAX_FRAME_PAYLOAD as usize {
        return Err(ClientError::Protocol);
    }
    let total = HEADER_LEN
        .checked_add(payload_len)
        .ok_or(ClientError::Capacity)?;
    if available < total {
        return Ok(None);
    }
    Ok(Some((
        header.message_type,
        offset + HEADER_LEN,
        offset + total,
    )))
}

/// How many more bytes are required before the frame that starts at `offset`
/// is complete. The caller appends them after the existing tail.
fn bytes_until_frame_complete(buf: &[u8], offset: usize) -> Result<usize, ClientError> {
    let available = buf.len().saturating_sub(offset);
    if available < HEADER_LEN {
        return Ok(HEADER_LEN - available);
    }
    let header = FrameHeader::decode(&buf[offset..offset + HEADER_LEN])
        .map_err(|_| ClientError::Protocol)?;
    let payload_len = header.payload_len as usize;
    if payload_len > MAX_FRAME_PAYLOAD as usize {
        return Err(ClientError::Protocol);
    }
    let total = HEADER_LEN
        .checked_add(payload_len)
        .ok_or(ClientError::Capacity)?;
    Ok(total.saturating_sub(available))
}

impl LocalDisplayClient {
    /// Read until the type-35 echo or an `Error` for that request.
    ///
    /// Unrelated frames, including `Error` for some other message, stay in the
    /// buffer for the normal poll path. One wait, no retry of the write.
    pub(super) fn read_terminate_acceptance(
        &mut self,
        execution: ExecutionId,
        deadline: Instant,
    ) -> Result<(), ClientError> {
        let expected = execution.to_bytes();
        let terminate_type = MessageType::TerminateExecution as u16;
        loop {
            if Instant::now() > deadline {
                return Err(ClientError::Io);
            }
            self.compact_buffer();
            let mut offset = 0;
            while let Some((message_type, payload_start, frame_end)) =
                next_complete_frame(&self.buffered, offset)?
            {
                let payload = &self.buffered[payload_start..frame_end];
                match MessageType::from_u16(message_type) {
                    Some(MessageType::TerminateExecution) => {
                        let matches = payload == expected.as_slice();
                        self.buffered.drain(offset..frame_end);
                        return if matches {
                            Ok(())
                        } else {
                            Err(ClientError::Protocol)
                        };
                    }
                    Some(MessageType::Error) => {
                        let error =
                            ErrorMessage::decode(payload).map_err(|_| ClientError::Protocol)?;
                        if error.offending_message_type == terminate_type {
                            self.buffered.drain(offset..frame_end);
                            return Err(server_error(error.error_code));
                        }
                        offset = frame_end;
                    }
                    _ => offset = frame_end,
                }
            }
            let missing = bytes_until_frame_complete(&self.buffered, offset)?;
            self.append_exact_until(missing, deadline)?;
        }
    }

    fn append_exact_until(&mut self, len: usize, deadline: Instant) -> Result<(), ClientError> {
        if len == 0 {
            return Err(ClientError::Protocol);
        }
        let live = self.buffered.len().saturating_sub(self.read_offset);
        if live
            .checked_add(len)
            .is_none_or(|total| total > MAX_BUFFERED_BYTES)
        {
            return Err(ClientError::Capacity);
        }
        let mut buf = vec![0u8; len];
        discovery::read_exact_until(&mut self.stream, &mut buf, deadline)?;
        self.buffered.extend_from_slice(&buf);
        Ok(())
    }
}
