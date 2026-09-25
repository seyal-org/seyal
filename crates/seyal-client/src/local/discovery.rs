use std::{
    io::{Read, Write},
    os::{fd::AsRawFd, unix::net::UnixStream},
    path::Path,
    time::{Duration, Instant},
};

use seyal_runtime::{
    local_ipc::{
        discovery::{
            control_socket_path, ensure_verified_runtime_dir, resolved_runtime_dir, DiscoveryError,
        },
        framing::{
            encode_frame, ClientHello, ErrorCode, ErrorMessage, MessageType, ServerHello,
            CAP_BINARY_DISPLAY, CAP_COMMAND_BLOCKS, CAP_CORRELATED_RESIZE,
            CAP_EXTENDED_TERMINAL_KEY, CAP_GRAPHEME_DISPLAY, CAP_SEMANTIC_TERMINAL_KEY,
            CAP_VIEWPORT_LINE_IDS,
        },
    },
    pass8::CAP_BLOCK_METADATA,
};

use super::{server_error, ClientError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscoveryFailure {
    /// The verified canonical endpoint does not exist. This is the sole
    /// discovery result that may claim the one helper-launch action for an
    /// episode.
    EndpointMissing,
    /// A canonical endpoint exists but is not accepting connections yet. The
    /// client retries the canonical path and never repairs or launches solely
    /// because of this observation.
    ConnectionRefused,
    /// The endpoint changed state while a connection was being attempted.
    /// This remains a bounded canonical-path retry, not evidence to create a
    /// competing Runtime.
    EndpointDisappeared,
    /// Directory/socket metadata or ownership failed same-user trust checks.
    /// This is terminal for the episode and must never trigger repair.
    UntrustedEndpoint,
    /// The canonical runtime location cannot be derived or represented safely.
    /// This is terminal for the episode and must never trigger a helper launch.
    InvalidPath,
}

pub(crate) fn connect_stream_until(
    path: &Path,
    deadline: Instant,
) -> Result<UnixStream, ClientError> {
    startup_remaining(deadline)?;
    let stream = UnixStream::connect(path).map_err(classify_connect_error)?;
    configure_startup_timeout(&stream, deadline)?;
    Ok(stream)
}

pub(crate) fn startup_remaining(deadline: Instant) -> Result<Duration, ClientError> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or(ClientError::StartupDeadlineExceeded)
}

pub(crate) fn configure_startup_timeout(
    stream: &UnixStream,
    deadline: Instant,
) -> Result<(), ClientError> {
    startup_remaining(deadline)?;
    stream.set_nonblocking(true).map_err(|_| ClientError::Io)
}

#[derive(Clone, Copy)]
enum StartupWaitInterest {
    Readable,
    Writable,
}

/// Minimal POSIX `poll(2)` surface for startup waits. Kept local so the
/// portable `seyal-client` dependency frontier stays
/// `seyal-protocol` + `seyal-render` only (no `libc` Cargo dep).
#[repr(C)]
struct StartupPollFd {
    fd: std::os::raw::c_int,
    events: i16,
    revents: i16,
}

const STARTUP_POLLIN: i16 = 0x0001;
const STARTUP_POLLOUT: i16 = 0x0004;
const STARTUP_POLLERR: i16 = 0x0008;
const STARTUP_POLLHUP: i16 = 0x0010;
const STARTUP_POLLNVAL: i16 = 0x0020;

#[allow(unsafe_code)]
unsafe extern "C" {
    /// POSIX poll; `nfds` is `nfds_t` (unsigned int on Darwin, unsigned long on
    /// glibc). Passing `1` is ABI-safe for both register widths.
    fn poll(
        fds: *mut StartupPollFd,
        nfds: std::os::raw::c_ulong,
        timeout: std::os::raw::c_int,
    ) -> std::os::raw::c_int;
}

/// Block until the peer is ready for the requested interest or the startup
/// deadline elapses. Uses `poll(2)` so a stalled peer cannot pin a core and we
/// do not invent sleep backoff that either burns CPU or inflates reconnect.
fn wait_startup_peer(
    stream: &UnixStream,
    deadline: Instant,
    interest: StartupWaitInterest,
) -> Result<(), ClientError> {
    let events = match interest {
        StartupWaitInterest::Readable => STARTUP_POLLIN,
        StartupWaitInterest::Writable => STARTUP_POLLOUT,
    };
    loop {
        let remaining = startup_remaining(deadline)?;
        // poll(2) timeout is whole milliseconds; keep at least 1ms so a sub-ms
        // remainder still parks in the kernel instead of busy-spinning.
        let timeout_ms = i32::try_from(remaining.as_millis().max(1)).unwrap_or(i32::MAX);
        let mut fds = [StartupPollFd {
            fd: stream.as_raw_fd(),
            events,
            revents: 0,
        }];
        // SAFETY: one stack-local pollfd; owned stream fd remains live for the call.
        let rc = {
            #[allow(unsafe_code)]
            unsafe {
                poll(fds.as_mut_ptr(), 1, timeout_ms)
            }
        };
        match rc {
            -1 => {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(ClientError::Io);
            }
            0 => return Err(ClientError::StartupDeadlineExceeded),
            _ => {
                let revents = fds[0].revents;
                if revents & (STARTUP_POLLERR | STARTUP_POLLHUP | STARTUP_POLLNVAL) != 0 {
                    // Let the subsequent read/write surface the concrete I/O error.
                    return Ok(());
                }
                if revents & events != 0 {
                    return Ok(());
                }
                // Spurious wake with no matching interest; retry while deadline remains.
            }
        }
    }
}

pub(crate) fn read_exact_until(
    stream: &mut UnixStream,
    buffer: &mut [u8],
    deadline: Instant,
) -> Result<(), ClientError> {
    let mut offset = 0;
    while offset < buffer.len() {
        configure_startup_timeout(stream, deadline)?;
        match stream.read(&mut buffer[offset..]) {
            Ok(0) => return Err(ClientError::Io),
            Ok(read) => {
                offset += read;
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                wait_startup_peer(stream, deadline, StartupWaitInterest::Readable)?;
            }
            Err(_) => return Err(ClientError::Io),
        }
    }
    Ok(())
}

pub(crate) fn canonical_control_socket_path() -> Result<std::path::PathBuf, ClientError> {
    let runtime_dir = resolved_runtime_dir().map_err(classify_discovery_error)?;
    ensure_verified_runtime_dir(&runtime_dir).map_err(classify_discovery_error)?;
    control_socket_path(&runtime_dir).map_err(classify_discovery_error)
}

/// Preserve discovery/trust distinctions through the client boundary. The
/// Swift recovery coordinator may launch only after `EndpointMissing`; it must
/// not turn an insecure path or an unready canonical listener into a launch or
/// repair action.
pub(crate) fn classify_discovery_error(error: DiscoveryError) -> ClientError {
    let failure = match error {
        DiscoveryError::NotADirectory
        | DiscoveryError::NotOwnedByEffectiveUser
        | DiscoveryError::GroupOrWorldWritable
        | DiscoveryError::ActiveEndpoint => DiscoveryFailure::UntrustedEndpoint,
        DiscoveryError::ConfstrFailed
        | DiscoveryError::PathTooLongForSocket
        | DiscoveryError::InvalidExplicitRuntimeDir
        | DiscoveryError::Io(_) => DiscoveryFailure::InvalidPath,
    };
    ClientError::Discovery(failure)
}

/// Discovery is allowed to retry only when the endpoint is not currently
/// usable. Preserve all other I/O failures as hard failures so the recovery
/// coordinator cannot turn permission, descriptor, or local resource errors
/// into an unbounded helper-launch loop.
pub(crate) fn classify_connect_error(error: std::io::Error) -> ClientError {
    match error.kind() {
        std::io::ErrorKind::NotFound => ClientError::Discovery(DiscoveryFailure::EndpointMissing),
        std::io::ErrorKind::ConnectionRefused => {
            ClientError::Discovery(DiscoveryFailure::ConnectionRefused)
        }
        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::NotConnected => {
            ClientError::Discovery(DiscoveryFailure::EndpointDisappeared)
        }
        _ => ClientError::Io,
    }
}

pub(crate) fn requested_capabilities(
    request_block_metadata: bool,
    request_extended_terminal_key: bool,
) -> u32 {
    requested_capabilities_with(
        request_block_metadata,
        request_extended_terminal_key,
        true,
    )
}

pub(crate) fn requested_capabilities_with(
    request_block_metadata: bool,
    request_extended_terminal_key: bool,
    request_viewport_line_ids: bool,
) -> u32 {
    CAP_COMMAND_BLOCKS
        | CAP_GRAPHEME_DISPLAY
        | if request_viewport_line_ids {
            CAP_VIEWPORT_LINE_IDS
        } else {
            0
        }
        | if request_extended_terminal_key {
            CAP_EXTENDED_TERMINAL_KEY
        } else {
            0
        }
        | if request_block_metadata {
            CAP_BLOCK_METADATA
        } else {
            0
        }
}

pub(crate) fn extended_terminal_key_supported(server_capabilities: u32) -> bool {
    server_capabilities & CAP_EXTENDED_TERMINAL_KEY != 0
}

pub(crate) fn hello_until(
    stream: &mut UnixStream,
    interactive: bool,
    request_block_metadata: bool,
    request_extended_terminal_key: bool,
    deadline: Instant,
) -> Result<ServerHello, ClientError> {
    hello_until_with(
        stream,
        interactive,
        request_block_metadata,
        request_extended_terminal_key,
        true,
        deadline,
    )
}

pub(crate) fn hello_until_with(
    stream: &mut UnixStream,
    interactive: bool,
    request_block_metadata: bool,
    request_extended_terminal_key: bool,
    request_viewport_line_ids: bool,
    deadline: Instant,
) -> Result<ServerHello, ClientError> {
    let client_capabilities = requested_capabilities_with(
        request_block_metadata,
        request_extended_terminal_key,
        request_viewport_line_ids,
    );
    send_control_until(
        stream,
        MessageType::ClientHello,
        &ClientHello {
            client_capabilities,
        }
        .encode(),
        deadline,
    )?;
    let (kind, payload) = super::attach::read_blocking_frame_until(stream, deadline)?;
    if kind == MessageType::Error {
        let error = ErrorMessage::decode(&payload).map_err(|_| ClientError::Protocol)?;
        return Err(server_error(error.error_code));
    }
    if kind != MessageType::ServerHello {
        return Err(ClientError::Protocol);
    }
    let hello = ServerHello::decode(&payload).map_err(|_| ClientError::Protocol)?;
    if hello.server_capabilities & CAP_BINARY_DISPLAY == 0 {
        return Err(ClientError::UnsupportedDisplayCapability);
    }
    if interactive
        && (hello.server_capabilities & CAP_SEMANTIC_TERMINAL_KEY == 0
            || hello.server_capabilities & CAP_CORRELATED_RESIZE == 0)
    {
        return Err(ClientError::UnsupportedInteractiveCapability);
    }
    Ok(hello)
}

/// New clients advertise newer optional bits; older Runtimes reject unknown
/// ClientHello bits as `MalformedPayload` instead of ignoring them.
///
/// Ordered reconnect fallback (bounded; each step at most once):
/// 1. Full advertise (viewport LineIds + extended key).
/// 2. Drop `CAP_VIEWPORT_LINE_IDS` (pre-#865 Runtime allowlist).
/// 3. Also drop `CAP_EXTENDED_TERMINAL_KEY` (SPEC-006 §21.5 / pre-V2).
pub(crate) fn hello_until_with_legacy_key_fallback(
    stream: &mut UnixStream,
    mut reconnect: impl FnMut() -> Result<UnixStream, ClientError>,
    interactive: bool,
    request_block_metadata: bool,
    deadline: Instant,
) -> Result<ServerHello, ClientError> {
    match hello_until_with(
        stream,
        interactive,
        request_block_metadata,
        true,
        true,
        deadline,
    ) {
        Ok(hello) => Ok(hello),
        Err(ClientError::Server(ErrorCode::MalformedPayload)) => {
            *stream = reconnect()?;
            match hello_until_with(
                stream,
                interactive,
                request_block_metadata,
                true,
                false,
                deadline,
            ) {
                Ok(hello) => Ok(hello),
                Err(ClientError::Server(ErrorCode::MalformedPayload)) => {
                    *stream = reconnect()?;
                    hello_until_with(
                        stream,
                        interactive,
                        request_block_metadata,
                        false,
                        false,
                        deadline,
                    )
                }
                Err(error) => Err(error),
            }
        }
        Err(error) => Err(error),
    }
}

pub(crate) fn send_control_until(
    stream: &mut UnixStream,
    message_type: MessageType,
    payload: &[u8],
    deadline: Instant,
) -> Result<(), ClientError> {
    configure_startup_timeout(stream, deadline)?;
    let frame = encode_frame(message_type, payload);
    let mut offset = 0;
    while offset < frame.len() {
        match stream.write(&frame[offset..]) {
            Ok(0) => return Err(ClientError::Io),
            Ok(written) => {
                offset += written;
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                wait_startup_peer(stream, deadline, StartupWaitInterest::Writable)?;
            }
            Err(_) => return Err(ClientError::Io),
        }
    }
    Ok(())
}

#[cfg(test)]
mod connect_error_tests {
    use super::{
        canonical_control_socket_path, classify_connect_error, classify_discovery_error,
        extended_terminal_key_supported, hello_until, hello_until_with,
        hello_until_with_legacy_key_fallback, requested_capabilities, ClientError, DiscoveryFailure,
    };
    use seyal_runtime::local_ipc::{discovery::DiscoveryError, framing::*};
    use std::{
        io::{self, Read, Write},
        os::unix::net::UnixStream,
        path::PathBuf,
        time::{Duration, Instant},
    };

    #[test]
    fn old_server_hello_without_extended_key_capability_disables_v2() {
        let (mut client, mut server) = UnixStream::pair().expect("unix stream pair");
        let server_thread = std::thread::spawn(move || {
            let mut header = [0_u8; HEADER_LEN];
            server.read_exact(&mut header).expect("client hello header");
            let payload_len = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
            let mut payload = vec![0_u8; payload_len];
            server
                .read_exact(&mut payload)
                .expect("client hello payload");
            let hello = ServerHello {
                runtime_id: 1,
                server_capabilities: CAP_BINARY_DISPLAY
                    | CAP_SEMANTIC_TERMINAL_KEY
                    | CAP_CORRELATED_RESIZE,
                max_frame_payload: MAX_FRAME_PAYLOAD,
                max_input_payload: 65_536,
            };
            server
                .write_all(&encode_frame(MessageType::ServerHello, &hello.encode()))
                .expect("server hello");
        });

        let hello = hello_until(
            &mut client,
            true,
            true,
            true,
            Instant::now() + Duration::from_secs(1),
        )
        .expect("old server hello remains usable");
        assert!(!extended_terminal_key_supported(hello.server_capabilities));
        server_thread.join().expect("server thread");
    }

    fn pre_v2_runtime_hello(mut server: UnixStream, accept_without_v2: bool) {
        let mut header = [0_u8; HEADER_LEN];
        server.read_exact(&mut header).expect("client hello header");
        let payload_len = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
        let mut payload = vec![0_u8; payload_len];
        server
            .read_exact(&mut payload)
            .expect("client hello payload");
        let hello = ClientHello::decode(&payload).expect("client hello");
        // Pre-V2 allowlist: reject any bit outside the M001 known set, including
        // both CAP_EXTENDED_TERMINAL_KEY and CAP_VIEWPORT_LINE_IDS.
        let unknown = hello.client_capabilities
            & !(CAP_COMMAND_BLOCKS
                | seyal_runtime::pass8::CAP_BLOCK_METADATA
                | CAP_GRAPHEME_DISPLAY
                | CAP_BINARY_DISPLAY
                | CAP_SEMANTIC_TERMINAL_KEY
                | CAP_CORRELATED_RESIZE);
        if unknown != 0 {
            let error = ErrorMessage {
                error_code: ErrorCode::MalformedPayload as u16,
                offending_message_type: MessageType::ClientHello as u16,
                detail_code: 0,
            };
            server
                .write_all(&encode_frame(MessageType::Error, &error.encode()))
                .expect("reject unknown capability bit");
            return;
        }
        assert!(
            accept_without_v2,
            "legacy fallback must omit CAP_EXTENDED_TERMINAL_KEY and CAP_VIEWPORT_LINE_IDS"
        );
        assert_eq!(hello.client_capabilities & CAP_EXTENDED_TERMINAL_KEY, 0);
        assert_eq!(hello.client_capabilities & CAP_VIEWPORT_LINE_IDS, 0);
        let hello = ServerHello {
            runtime_id: 1,
            server_capabilities: CAP_BINARY_DISPLAY
                | CAP_SEMANTIC_TERMINAL_KEY
                | CAP_CORRELATED_RESIZE,
            max_frame_payload: MAX_FRAME_PAYLOAD,
            max_input_payload: 65_536,
        };
        server
            .write_all(&encode_frame(MessageType::ServerHello, &hello.encode()))
            .expect("pre-v2 server hello");
    }

    /// Pre-#865 Runtime: knows extended key, rejects CAP_VIEWPORT_LINE_IDS.
    fn pre_viewport_line_ids_runtime_hello(mut server: UnixStream, accept_without_line_ids: bool) {
        let mut header = [0_u8; HEADER_LEN];
        server.read_exact(&mut header).expect("client hello header");
        let payload_len = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
        let mut payload = vec![0_u8; payload_len];
        server
            .read_exact(&mut payload)
            .expect("client hello payload");
        let hello = ClientHello::decode(&payload).expect("client hello");
        let unknown = hello.client_capabilities
            & !(CAP_COMMAND_BLOCKS
                | seyal_runtime::pass8::CAP_BLOCK_METADATA
                | CAP_GRAPHEME_DISPLAY
                | CAP_EXTENDED_TERMINAL_KEY);
        if unknown != 0 {
            let error = ErrorMessage {
                error_code: ErrorCode::MalformedPayload as u16,
                offending_message_type: MessageType::ClientHello as u16,
                detail_code: 0,
            };
            server
                .write_all(&encode_frame(MessageType::Error, &error.encode()))
                .expect("reject CAP_VIEWPORT_LINE_IDS");
            return;
        }
        assert!(
            accept_without_line_ids,
            "viewport LineIds fallback must omit CAP_VIEWPORT_LINE_IDS"
        );
        assert_eq!(hello.client_capabilities & CAP_VIEWPORT_LINE_IDS, 0);
        let hello = ServerHello {
            runtime_id: 1,
            server_capabilities: CAP_BINARY_DISPLAY
                | CAP_SEMANTIC_TERMINAL_KEY
                | CAP_CORRELATED_RESIZE
                | CAP_EXTENDED_TERMINAL_KEY
                | CAP_COMMAND_BLOCKS
                | CAP_GRAPHEME_DISPLAY,
            max_frame_payload: MAX_FRAME_PAYLOAD,
            max_input_payload: 65_536,
        };
        server
            .write_all(&encode_frame(MessageType::ServerHello, &hello.encode()))
            .expect("pre-viewport-line-ids server hello");
    }

    #[test]
    fn requested_capabilities_can_omit_extended_key_for_old_runtimes() {
        let with_v2 = requested_capabilities(true, true);
        let without_v2 = requested_capabilities(true, false);
        assert_ne!(with_v2 & CAP_EXTENDED_TERMINAL_KEY, 0);
        assert_eq!(without_v2 & CAP_EXTENDED_TERMINAL_KEY, 0);
        assert_ne!(without_v2 & CAP_COMMAND_BLOCKS, 0);
        assert_ne!(without_v2 & CAP_GRAPHEME_DISPLAY, 0);
        assert_ne!(without_v2 & CAP_VIEWPORT_LINE_IDS, 0);
        assert_ne!(without_v2 & seyal_runtime::pass8::CAP_BLOCK_METADATA, 0);
    }

    #[test]
    fn requested_capabilities_can_omit_viewport_line_ids() {
        let with = super::requested_capabilities_with(true, true, true);
        let without = super::requested_capabilities_with(true, true, false);
        assert_ne!(with & CAP_VIEWPORT_LINE_IDS, 0);
        assert_eq!(without & CAP_VIEWPORT_LINE_IDS, 0);
        assert_ne!(without & CAP_EXTENDED_TERMINAL_KEY, 0);
    }

    #[test]
    fn pre_v2_runtime_rejects_extended_key_hello_and_accepts_m001_fallback() {
        let (mut rejected_client, rejected_server) = UnixStream::pair().expect("unix stream pair");
        let rejector = std::thread::spawn(move || pre_v2_runtime_hello(rejected_server, false));
        let error = hello_until(
            &mut rejected_client,
            true,
            true,
            true,
            Instant::now() + Duration::from_secs(1),
        )
        .expect_err("pre-v2 Runtime rejects unknown ClientHello bits");
        assert_eq!(error, ClientError::Server(ErrorCode::MalformedPayload));
        rejector.join().expect("rejector thread");

        let (mut fallback_client, fallback_server) = UnixStream::pair().expect("unix stream pair");
        let acceptor = std::thread::spawn(move || pre_v2_runtime_hello(fallback_server, true));
        let hello = hello_until_with(
            &mut fallback_client,
            true,
            true,
            false,
            false,
            Instant::now() + Duration::from_secs(1),
        )
        .expect("M001 hello is accepted by a pre-v2 Runtime");
        assert!(!extended_terminal_key_supported(hello.server_capabilities));
        acceptor.join().expect("acceptor thread");
    }

    #[test]
    fn hello_fallback_reconnects_when_old_runtime_rejects_newer_capabilities() {
        // Pre-V2 path needs two reconnects: drop VIEWPORT_LINE_IDS, then drop EXTENDED_KEY.
        let (mut client, first_server) = UnixStream::pair().expect("first pair");
        let (second_client, second_server) = UnixStream::pair().expect("second pair");
        let (third_client, third_server) = UnixStream::pair().expect("third pair");
        let rejector = std::thread::spawn(move || pre_v2_runtime_hello(first_server, false));
        let rejector2 = std::thread::spawn(move || pre_v2_runtime_hello(second_server, false));
        let acceptor = std::thread::spawn(move || pre_v2_runtime_hello(third_server, true));
        let mut fallbacks = vec![second_client, third_client].into_iter();
        let hello = hello_until_with_legacy_key_fallback(
            &mut client,
            || fallbacks.next().ok_or(ClientError::Io),
            true,
            true,
            Instant::now() + Duration::from_secs(1),
        )
        .expect("SPEC-006 §21.5 new client may use M001 on an old server");
        assert!(!extended_terminal_key_supported(hello.server_capabilities));
        rejector.join().expect("rejector thread");
        rejector2.join().expect("rejector2 thread");
        acceptor.join().expect("acceptor thread");
    }

    #[test]
    fn hello_fallback_drops_viewport_line_ids_against_pre_865_runtime() {
        let (mut client, first_server) = UnixStream::pair().expect("first pair");
        let (fallback_client, second_server) = UnixStream::pair().expect("second pair");
        let rejector =
            std::thread::spawn(move || pre_viewport_line_ids_runtime_hello(first_server, false));
        let acceptor =
            std::thread::spawn(move || pre_viewport_line_ids_runtime_hello(second_server, true));
        let mut fallbacks = Some(fallback_client);
        let hello = hello_until_with_legacy_key_fallback(
            &mut client,
            || fallbacks.take().ok_or(ClientError::Io),
            true,
            true,
            Instant::now() + Duration::from_secs(1),
        )
        .expect("pre-#865 Runtime accepts hello without CAP_VIEWPORT_LINE_IDS");
        assert!(extended_terminal_key_supported(hello.server_capabilities));
        rejector.join().expect("rejector thread");
        acceptor.join().expect("acceptor thread");
    }

    #[test]
    fn endpoint_absence_refusal_and_disappearance_remain_distinct() {
        assert_eq!(
            classify_connect_error(io::Error::from(io::ErrorKind::NotFound)),
            ClientError::Discovery(DiscoveryFailure::EndpointMissing)
        );
        assert_eq!(
            classify_connect_error(io::Error::from(io::ErrorKind::ConnectionRefused)),
            ClientError::Discovery(DiscoveryFailure::ConnectionRefused)
        );
        for kind in [io::ErrorKind::ConnectionReset, io::ErrorKind::NotConnected] {
            assert_eq!(
                classify_connect_error(io::Error::from(kind)),
                ClientError::Discovery(DiscoveryFailure::EndpointDisappeared)
            );
        }
    }

    #[test]
    fn insecure_or_invalid_discovery_preconditions_fail_closed() {
        for error in [
            DiscoveryError::NotADirectory,
            DiscoveryError::NotOwnedByEffectiveUser,
            DiscoveryError::GroupOrWorldWritable,
        ] {
            assert_eq!(
                classify_discovery_error(error),
                ClientError::Discovery(DiscoveryFailure::UntrustedEndpoint)
            );
        }
        for error in [
            DiscoveryError::ConfstrFailed,
            DiscoveryError::PathTooLongForSocket,
            DiscoveryError::InvalidExplicitRuntimeDir,
        ] {
            assert_eq!(
                classify_discovery_error(error),
                ClientError::Discovery(DiscoveryFailure::InvalidPath)
            );
        }
    }

    #[test]
    fn unrelated_connect_errors_remain_non_discovery_io_failures() {
        for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::Other] {
            assert_eq!(
                classify_connect_error(io::Error::from(kind)),
                ClientError::Io
            );
        }
    }

    #[test]
    fn explicit_runtime_dir_redirects_canonical_socket_without_touching_production() {
        let _lock = seyal_runtime::runtime_dir::override_test_lock();
        seyal_runtime::runtime_dir::reset_explicit_runtime_dir();
        let isolated = PathBuf::from(format!(
            "/tmp/s860c-{}-{:x}",
            std::process::id() % 100_000,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                % 0xFFFF
        ));
        seyal_runtime::runtime_dir::set_explicit_runtime_dir(isolated.clone()).unwrap();
        let path = canonical_control_socket_path().expect("isolated socket path");
        assert_eq!(
            path,
            seyal_runtime::runtime_dir::control_socket_leaf(&isolated)
        );
        seyal_runtime::runtime_dir::reset_explicit_runtime_dir();
        let restored = canonical_control_socket_path().expect("canonical socket path");
        assert!(restored.ends_with("seyal-runtime/control.sock"));
        assert_ne!(restored, path);
    }
}
