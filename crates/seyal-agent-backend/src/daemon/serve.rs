//! Per-connection session serve for serial and concurrent accept paths.

use std::{
    io::Write,
    os::unix::net::UnixStream,
    sync::{Arc, Mutex},
};

use seyal_agent_core::ClientPrincipalId;

use super::DaemonError;

/// Shared session loop used by serial `serve_one` and concurrent workers.
pub(super) fn serve_session_locked(
    service: &Arc<Mutex<crate::session::IntegrationService>>,
    stream: &mut UnixStream,
    principal: ClientPrincipalId,
    max_frame_size: u32,
    event_window: u32,
) -> Result<(), DaemonError> {
    loop {
        let frame = match crate::session::read_session_frame(stream, max_frame_size) {
            crate::session::SessionRead::Frame(frame) => frame,
            crate::session::SessionRead::Disconnected => return Ok(()),
            crate::session::SessionRead::Oversized => return Err(DaemonError::Oversized),
            crate::session::SessionRead::Malformed => return Err(DaemonError::Malformed),
            crate::session::SessionRead::TimedOut => return Ok(()),
            crate::session::SessionRead::Io => return Err(DaemonError::Io),
        };
        let response = {
            let mut guard = service
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match guard.handle(principal, frame, max_frame_size, event_window) {
                Ok(response) => response,
                Err(_) => seyal_agent_protocol::encode_result(
                    &seyal_agent_protocol::CommandResult::Error(
                        seyal_agent_protocol::CommandError::Failed,
                    ),
                    max_frame_size,
                )
                .map_err(|_| DaemonError::Unavailable)?,
            }
        };
        stream.write_all(&response).map_err(super::map_io)?;
    }
}
