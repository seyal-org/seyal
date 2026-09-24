//! Standalone Agent Backend client.
//!
//! This crate speaks the versioned local protocol and never links Terminal
//! Runtime, a PTY, or a renderer. It also never unlinks or replaces the daemon
//! endpoint; endpoint ownership stays with the backend.

#[allow(unsafe_code)]
mod peer;

use std::{
    fs,
    io::{self, Read, Write},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{FileTypeExt, MetadataExt},
            net::UnixStream,
        },
    },
    path::Path,
    time::Duration,
};

use seyal_agent_protocol::{
    accepted_body_len, decode_ack, decode_frame, decode_handshake_error, encode_hello, FrameKind,
    Hello, HelloAck, ABSOLUTE_MAX_FRAME_SIZE,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientError {
    UnsafeEndpoint,
    HandshakeRejected,
    Malformed,
    Oversized,
    TimedOut,
    Io,
}

pub fn handshake(socket_path: &Path, hello: &Hello) -> Result<HelloAck, ClientError> {
    let our_uid = current_uid().map_err(|_| ClientError::Io)?;
    verify_parent_directory(socket_path, our_uid)?;
    let meta = fs::symlink_metadata(socket_path).map_err(map_io)?;
    if meta.file_type().is_symlink()
        || !meta.file_type().is_socket()
        || meta.uid() != our_uid
        || meta.mode() & 0o077 != 0
    {
        return Err(ClientError::UnsafeEndpoint);
    }

    let mut stream = UnixStream::connect(socket_path).map_err(map_io)?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|_| ClientError::Io)?;
    peer::verify_same_user_peer(stream.as_raw_fd()).map_err(|_| ClientError::UnsafeEndpoint)?;
    let frame = encode_hello(hello, ABSOLUTE_MAX_FRAME_SIZE).map_err(|_| ClientError::Malformed)?;
    stream.write_all(&frame).map_err(map_io)?;
    let response = read_frame(&mut stream)?;
    match response.kind {
        FrameKind::HelloAck => decode_ack(&response.body).map_err(|_| ClientError::Malformed),
        FrameKind::HandshakeError => {
            let _ = decode_handshake_error(&response.body);
            Err(ClientError::HandshakeRejected)
        }
        FrameKind::Hello => Err(ClientError::Malformed),
    }
}

fn verify_parent_directory(socket_path: &Path, our_uid: u32) -> Result<(), ClientError> {
    let Some(parent) = socket_path.parent() else {
        return Err(ClientError::UnsafeEndpoint);
    };
    let meta = fs::symlink_metadata(parent).map_err(map_io)?;
    if meta.file_type().is_symlink()
        || !meta.is_dir()
        || meta.uid() != our_uid
        || meta.mode() & 0o077 != 0
    {
        return Err(ClientError::UnsafeEndpoint);
    }
    Ok(())
}

fn read_frame(stream: &mut UnixStream) -> Result<seyal_agent_protocol::Frame, ClientError> {
    let mut header = [0; 10];
    stream.read_exact(&mut header).map_err(map_io)?;
    let body_len =
        accepted_body_len(&header, ABSOLUTE_MAX_FRAME_SIZE).map_err(|error| match error {
            seyal_agent_protocol::FrameError::Oversized => ClientError::Oversized,
            _ => ClientError::Malformed,
        })?;
    let mut body = vec![0; body_len];
    if body_len > 0 {
        stream.read_exact(&mut body).map_err(map_io)?;
    }
    let mut bytes = header.to_vec();
    bytes.extend_from_slice(&body);
    decode_frame(&bytes, ABSOLUTE_MAX_FRAME_SIZE).map_err(|_| ClientError::Malformed)
}

fn current_uid() -> io::Result<u32> {
    // SAFETY: `geteuid` only reads the calling process credentials.
    #[allow(unsafe_code)]
    {
        Ok(unsafe { libc::geteuid() })
    }
}

fn map_io(error: io::Error) -> ClientError {
    if error.kind() == io::ErrorKind::TimedOut {
        ClientError::TimedOut
    } else {
        ClientError::Io
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use seyal_agent_protocol::{
        accepted_body_len, decode_frame, decode_hello, encode_ack, negotiate_hello,
        BackendInstanceId, ProtocolVersion, ABSOLUTE_MAX_FRAME_SIZE,
    };
    use std::{
        fs::File,
        os::unix::{fs::PermissionsExt, net::UnixListener},
        sync::atomic::{AtomicU64, Ordering},
        thread,
    };

    static NEXT_DIR: AtomicU64 = AtomicU64::new(1);

    fn hello() -> Hello {
        Hello {
            supported_versions: vec![ProtocolVersion::V1],
            max_frame_size: 2048,
            event_window: 16,
            client_principal_evidence: b"not-a-secret".to_vec(),
        }
    }

    #[test]
    fn standalone_client_completes_hello_without_removing_the_endpoint() {
        let dir = std::env::temp_dir().join(format!(
            "seyal-agent-client-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        let mut permissions = std::fs::metadata(&dir).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&dir, permissions).unwrap();
        let socket = dir.join("agent.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let mut socket_permissions = std::fs::metadata(&socket).unwrap().permissions();
        socket_permissions.set_mode(0o600);
        std::fs::set_permissions(&socket, socket_permissions).unwrap();

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut header = [0; 10];
            stream.read_exact(&mut header).unwrap();
            let body_len = accepted_body_len(&header, ABSOLUTE_MAX_FRAME_SIZE).unwrap();
            let mut body = vec![0; body_len];
            stream.read_exact(&mut body).unwrap();
            let mut bytes = header.to_vec();
            bytes.extend_from_slice(&body);
            let frame = decode_frame(&bytes, ABSOLUTE_MAX_FRAME_SIZE).unwrap();
            let hello = decode_hello(&frame.body).unwrap();
            let ack = negotiate_hello(&hello, BackendInstanceId::new(), 2048, 16).unwrap();
            let encoded = encode_ack(&ack, ABSOLUTE_MAX_FRAME_SIZE).unwrap();
            stream.write_all(&encoded).unwrap();
            ack.backend_instance_id
        });

        let ack = handshake(&socket, &hello()).unwrap();
        assert_eq!(ack.backend_instance_id, server.join().unwrap());
        assert!(socket.symlink_metadata().unwrap().file_type().is_socket());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn client_rejects_a_symlink_and_a_regular_file() {
        let dir = std::env::temp_dir().join(format!(
            "seyal-agent-client-link-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        let mut permissions = std::fs::metadata(&dir).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&dir, permissions).unwrap();
        let file = dir.join("plain");
        File::create(&file).unwrap();
        assert_eq!(handshake(&file, &hello()), Err(ClientError::UnsafeEndpoint));
        assert!(file.exists());

        let link = dir.join("link");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert_eq!(handshake(&link, &hello()), Err(ClientError::UnsafeEndpoint));
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn client_rejects_an_insecure_parent_directory() {
        let dir = std::env::temp_dir().join(format!(
            "seyal-agent-client-insecure-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        let mut permissions = std::fs::metadata(&dir).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&dir, permissions).unwrap();
        let socket = dir.join("agent.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        drop(listener);
        assert_eq!(
            handshake(&socket, &hello()),
            Err(ClientError::UnsafeEndpoint)
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
