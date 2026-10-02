//! Per-connection handshake and session serve for serial and concurrent
//! accept paths.

use std::{
    io::Write,
    os::unix::net::UnixStream,
    sync::{Arc, Mutex},
    time::Duration,
};

use seyal_agent_core::{BackendInstanceId, ClientPrincipalId};
use seyal_agent_protocol::{
    decode_hello, encode_ack, encode_handshake_error, negotiate_hello, FrameKind, HandshakeError,
    Hello, HelloAck,
};

use super::DaemonError;

/// Daemon limits a connection's handshake and session are served under.
#[derive(Clone, Copy)]
pub(super) struct Handshake {
    pub(super) instance_id: BackendInstanceId,
    pub(super) max_frame_size: u32,
    pub(super) event_window: u32,
    pub(super) session_idle_timeout: Duration,
    pub(super) session_write_timeout: Duration,
}

/// Read Hello from an admitted peer, validate its evidence, and negotiate
/// limits. Rejections are reported to the peer before returning. Does not
/// write HelloAck so callers can bind a principal first.
pub(super) fn negotiate(
    stream: &mut UnixStream,
    handshake: Handshake,
) -> Result<(Hello, HelloAck), DaemonError> {
    let frame = super::read_one_frame(stream, handshake.max_frame_size)?;
    if frame.kind != FrameKind::Hello {
        return Err(DaemonError::Malformed);
    }
    let hello = decode_hello(&frame.body).map_err(|_| DaemonError::Malformed)?;
    if crate::AuthorizationRepository::recognize_principal_evidence(
        &hello.client_principal_evidence,
    )
    .is_err()
    {
        return Err(reject(
            stream,
            HandshakeError::Malformed,
            handshake.max_frame_size,
        ));
    }
    match negotiate_hello(
        &hello,
        handshake.instance_id,
        handshake.max_frame_size,
        handshake.event_window,
    ) {
        Ok(ack) => Ok((hello, ack)),
        Err(error) => Err(reject(stream, error, handshake.max_frame_size)),
    }
}

pub(super) fn write_ack(
    stream: &mut UnixStream,
    ack: &HelloAck,
    max_frame_size: u32,
) -> Result<(), DaemonError> {
    let bytes = encode_ack(ack, max_frame_size).map_err(|_| DaemonError::Malformed)?;
    stream.write_all(&bytes).map_err(super::map_io)
}

/// Complete the handshake for an admitted peer, bind its connection-scoped
/// principal, then serve its session until it disconnects.
pub(super) fn handshake_and_serve(
    service: &Arc<Mutex<crate::session::IntegrationService>>,
    stream: &mut UnixStream,
    handshake: Handshake,
) -> Result<(), DaemonError> {
    let (hello, ack) = negotiate(stream, handshake)?;
    let resolved = service
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .resolve_connection_principal(&hello.client_principal_evidence);
    let Ok(principal) = resolved else {
        return Err(reject(
            stream,
            HandshakeError::Malformed,
            handshake.max_frame_size,
        ));
    };
    write_ack(stream, &ack, handshake.max_frame_size)?;
    stream
        .set_read_timeout(Some(handshake.session_idle_timeout))
        .map_err(|_| DaemonError::Io)?;
    stream
        .set_write_timeout(Some(handshake.session_write_timeout))
        .map_err(|_| DaemonError::Io)?;
    serve_session_locked(
        service,
        stream,
        principal,
        ack.max_frame_size,
        ack.event_window,
    )
}

fn reject(stream: &mut UnixStream, error: HandshakeError, max_frame_size: u32) -> DaemonError {
    match encode_handshake_error(error, max_frame_size) {
        Ok(bytes) => {
            let _ = stream.write_all(&bytes);
            DaemonError::Handshake(error)
        }
        Err(_) => DaemonError::Malformed,
    }
}

fn serve_session_locked(
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
