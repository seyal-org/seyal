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
        return if is_disconnect(&error) {
            SessionRead::Io
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
