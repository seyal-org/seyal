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
            CAP_EXECUTION_PROVISIONING, CAP_EXTENDED_TERMINAL_KEY, CAP_GRAPHEME_DISPLAY,
            CAP_SEMANTIC_TERMINAL_KEY, CAP_VIEWPORT_LINE_IDS,
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
    request_viewport_line_ids: bool,
    request_execution_provisioning: bool,
) -> u32 {
    CAP_COMMAND_BLOCKS
        | CAP_GRAPHEME_DISPLAY
        | if request_execution_provisioning {
            CAP_EXECUTION_PROVISIONING
        } else {
            0
        }
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
    request_viewport_line_ids: bool,
    request_execution_provisioning: bool,
    deadline: Instant,
) -> Result<ServerHello, ClientError> {
    let client_capabilities = requested_capabilities(
        request_block_metadata,
        request_extended_terminal_key,
        request_viewport_line_ids,
        request_execution_provisioning,
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
        // Retry the hello fallback only for a ClientHello the client encoded.
        // Any other MalformedPayload is a protocol failure, not a probe.
        if ErrorCode::from_u16(error.error_code) == Some(ErrorCode::MalformedPayload)
            && error.offending_message_type != MessageType::ClientHello as u16
        {
            return Err(ClientError::Protocol);
        }
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
/// 1. Full advertise (viewport LineIds + extended key + execution provisioning).
/// 2. Drop `CAP_VIEWPORT_LINE_IDS` (pre-#865 Runtime; still advertises provisioning).
/// 3. Drop extended key + provisioning (SPEC-006 §21.5 / pre-V2 M001 allowlist).
pub(crate) fn hello_until_with_legacy_key_fallback(
    stream: &mut UnixStream,
    mut reconnect: impl FnMut() -> Result<UnixStream, ClientError>,
    interactive: bool,
    request_block_metadata: bool,
    deadline: Instant,
) -> Result<ServerHello, ClientError> {
    match hello_until(
        stream,
        interactive,
        request_block_metadata,
        true,
        true,
        true,
        deadline,
    ) {
        Ok(hello) => Ok(hello),
        Err(ClientError::Server(ErrorCode::MalformedPayload)) => {
            *stream = reconnect()?;
            match hello_until(
                stream,
                interactive,
                request_block_metadata,
                true,
                false,
                true,
                deadline,
            ) {
                Ok(hello) => Ok(hello),
                Err(ClientError::Server(ErrorCode::MalformedPayload)) => {
                    *stream = reconnect()?;
                    hello_until(
                        stream,
                        interactive,
                        request_block_metadata,
                        false,
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
#[path = "discovery_connect_tests.rs"]
mod connect_error_tests;
