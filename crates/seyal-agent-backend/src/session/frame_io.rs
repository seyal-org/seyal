//! Session frame reads over an authenticated stream.

use seyal_agent_protocol::{accepted_body_len, decode_frame, Frame, FrameError};

pub(crate) enum SessionRead {
    Frame(Frame),
    Disconnected,
    Oversized,
    Malformed,
    TimedOut,
    Io,
}

pub(crate) fn read_session_frame(
    stream: &mut impl std::io::Read,
    max_frame_size: u32,
) -> SessionRead {
    let mut header = [0; 10];
    match stream.read_exact(&mut header) {
        Ok(()) => {}
        Err(error) if is_disconnect(&error) => return SessionRead::Disconnected,
        Err(error) => return map_read_error(error),
    }
    let body_len = match accepted_body_len(&header, max_frame_size) {
        Ok(len) => len,
        Err(FrameError::Oversized) => return SessionRead::Oversized,
        Err(_) => return SessionRead::Malformed,
    };
    let mut body = vec![0; body_len];
    if body_len > 0
        && let Err(error) = stream.read_exact(&mut body)
    {
        // Match header-path disconnect classification: peer drop mid-body is a
        // normal session end, not a daemon-fatal I/O fault.
        return if is_disconnect(&error) {
            SessionRead::Disconnected
        } else {
            map_read_error(error)
        };
    }
    let mut bytes = Vec::with_capacity(header.len() + body.len());
    bytes.extend_from_slice(&header);
    bytes.extend_from_slice(&body);
    match decode_frame(&bytes, max_frame_size) {
        Ok(frame) => SessionRead::Frame(frame),
        Err(_) => SessionRead::Malformed,
    }
}

fn is_disconnect(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::UnexpectedEof
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::BrokenPipe
    )
}

fn map_read_error(error: std::io::Error) -> SessionRead {
    if error.kind() == std::io::ErrorKind::TimedOut
        || error.kind() == std::io::ErrorKind::WouldBlock
    {
        SessionRead::TimedOut
    } else {
        SessionRead::Io
    }
}

#[cfg(test)]
mod tests {
    use super::{read_session_frame, SessionRead};
    use std::io::Cursor;

    #[test]
    fn peer_disconnect_mid_body_is_disconnected_not_io() {
        // Valid AGB1 header with body_len=8, but only 2 body bytes then EOF so
        // read_exact surfaces UnexpectedEof (same disconnect class as header).
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"AGB1");
        bytes.extend_from_slice(&4_u16.to_le_bytes()); // Command
        bytes.extend_from_slice(&8_u32.to_le_bytes());
        bytes.extend_from_slice(&[0xAB, 0xCD]);
        let mut stream = Cursor::new(bytes);
        assert!(matches!(
            read_session_frame(&mut stream, 4096),
            SessionRead::Disconnected
        ));
    }
}
