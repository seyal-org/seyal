//! Inbound read, frame validation, and FD-bearing receive handling.

use super::{
    LocalIpcServer, ServerEvent, MAX_FRAMES_PER_READINESS, MAX_RECEIVE_BUFFER_BYTES,
    READ_CHUNK_BYTES,
};
use crate::local_ipc::{
    fd_transfer::{self, RecvFd},
    framing::{FrameHeader, HEADER_LEN},
};
use std::io;
use std::os::fd::AsRawFd;

impl LocalIpcServer {
    pub fn service_read(&mut self, token: u64, hangup: bool) -> Vec<ServerEvent> {
        let mut events = Vec::new();
        let Some(connection) = self.connections.get_mut(&token) else {
            return events;
        };
        let mut chunk = [0u8; READ_CHUNK_BYTES];
        while events.len() < MAX_FRAMES_PER_READINESS {
            let remaining = MAX_RECEIVE_BUFFER_BYTES.saturating_sub(connection.read_buf.len());
            if remaining == 0 {
                events.push(ServerEvent::FramingError { token });
                self.close_with_event(token, &mut events);
                return events;
            }
            match fd_transfer::recv_with_fd(
                connection.stream.as_raw_fd(),
                &mut chunk[..READ_CHUNK_BYTES.min(remaining)],
            ) {
                Ok((0, RecvFd::None)) => {
                    self.close_with_event(token, &mut events);
                    return events;
                }
                Ok((count, RecvFd::None)) => {
                    connection.read_buf.extend_from_slice(&chunk[..count]);
                    if drain_frames(connection, token, &mut events).is_err() {
                        self.close_with_event(token, &mut events);
                        return events;
                    }
                }
                Ok((_count, RecvFd::One(fd))) => {
                    drop(fd);
                    events.push(ServerEvent::FramingError { token });
                    self.close_with_event(token, &mut events);
                    return events;
                }
                Ok((_count, RecvFd::Malformed)) => {
                    events.push(ServerEvent::FramingError { token });
                    self.close_with_event(token, &mut events);
                    return events;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    self.close_with_event(token, &mut events);
                    return events;
                }
            }
        }
        if hangup {
            if self
                .connections
                .get(&token)
                .is_some_and(|connection| !connection.read_buf.is_empty())
            {
                events.push(ServerEvent::FramingError { token });
            }
            self.close_with_event(token, &mut events);
        }
        events
    }
}

struct FramingCutError;

fn drain_frames(
    connection: &mut super::Connection,
    token: u64,
    events: &mut Vec<ServerEvent>,
) -> Result<(), FramingCutError> {
    while events.len() < MAX_FRAMES_PER_READINESS {
        if connection.read_buf.len() < HEADER_LEN {
            return Ok(());
        }
        let header = match FrameHeader::decode(&connection.read_buf[..HEADER_LEN]) {
            Ok(header) => header,
            Err(_) => {
                events.push(ServerEvent::FramingError { token });
                return Err(FramingCutError);
            }
        };
        let total = HEADER_LEN
            .checked_add(header.payload_len as usize)
            .ok_or(FramingCutError)?;
        if connection.read_buf.len() < total {
            return Ok(());
        }
        let payload = connection.read_buf[HEADER_LEN..total].to_vec();
        connection.read_buf.drain(..total);
        events.push(ServerEvent::Frame {
            token,
            message_type: header.message_type,
            payload,
        });
    }
    Ok(())
}
