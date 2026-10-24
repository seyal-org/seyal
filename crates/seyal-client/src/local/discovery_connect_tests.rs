use super::{
    canonical_control_socket_path, classify_connect_error, classify_discovery_error,
    extended_terminal_key_supported, hello_chain_until, hello_until_with_legacy_key_fallback,
    hello_with_capabilities_until, requested_capabilities, ClientError, DiscoveryFailure,
};
use seyal_runtime::local_ipc::framing::BlockTimelineSchema;
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

    let hello = hello_with_capabilities_until(
        &mut client,
        true,
        requested_capabilities(true, true, true, true),
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
    // CAP_EXTENDED_TERMINAL_KEY, CAP_VIEWPORT_LINE_IDS, and CAP_EXECUTION_PROVISIONING.
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
            "legacy fallback must omit CAP_EXTENDED_TERMINAL_KEY, CAP_VIEWPORT_LINE_IDS, and CAP_EXECUTION_PROVISIONING"
        );
    assert_eq!(hello.client_capabilities & CAP_EXTENDED_TERMINAL_KEY, 0);
    assert_eq!(hello.client_capabilities & CAP_VIEWPORT_LINE_IDS, 0);
    assert_eq!(hello.client_capabilities & CAP_EXECUTION_PROVISIONING, 0);
    let hello = ServerHello {
        runtime_id: 1,
        server_capabilities: CAP_BINARY_DISPLAY | CAP_SEMANTIC_TERMINAL_KEY | CAP_CORRELATED_RESIZE,
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
            | CAP_EXTENDED_TERMINAL_KEY
            | CAP_EXECUTION_PROVISIONING);
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
    let with_v2 = requested_capabilities(true, true, true, true);
    let without_v2 = requested_capabilities(true, false, true, true);
    assert_ne!(with_v2 & CAP_EXTENDED_TERMINAL_KEY, 0);
    assert_eq!(without_v2 & CAP_EXTENDED_TERMINAL_KEY, 0);
    assert_ne!(without_v2 & CAP_COMMAND_BLOCKS, 0);
    assert_ne!(without_v2 & CAP_GRAPHEME_DISPLAY, 0);
    assert_ne!(without_v2 & CAP_EXECUTION_PROVISIONING, 0);
    assert_ne!(with_v2 & CAP_EXECUTION_PROVISIONING, 0);
    assert_ne!(without_v2 & CAP_VIEWPORT_LINE_IDS, 0);
    assert_ne!(without_v2 & seyal_runtime::pass8::CAP_BLOCK_METADATA, 0);
    let m001 = requested_capabilities(true, false, false, false);
    assert_eq!(m001 & CAP_EXECUTION_PROVISIONING, 0);
    assert_eq!(m001 & CAP_EXTENDED_TERMINAL_KEY, 0);
    assert_eq!(m001 & CAP_VIEWPORT_LINE_IDS, 0);
}

#[test]
fn requested_capabilities_can_omit_viewport_line_ids() {
    let with = requested_capabilities(true, true, true, true);
    let without = requested_capabilities(true, true, false, true);
    assert_ne!(with & CAP_VIEWPORT_LINE_IDS, 0);
    assert_eq!(without & CAP_VIEWPORT_LINE_IDS, 0);
    assert_ne!(without & CAP_EXTENDED_TERMINAL_KEY, 0);
    assert_ne!(without & CAP_EXECUTION_PROVISIONING, 0);
}

#[test]
fn pre_v2_runtime_rejects_extended_key_hello_and_accepts_m001_fallback() {
    let (mut rejected_client, rejected_server) = UnixStream::pair().expect("unix stream pair");
    let rejector = std::thread::spawn(move || pre_v2_runtime_hello(rejected_server, false));
    let error = hello_with_capabilities_until(
        &mut rejected_client,
        true,
        requested_capabilities(true, true, true, true),
        Instant::now() + Duration::from_secs(1),
    )
    .expect_err("pre-v2 Runtime rejects unknown ClientHello bits");
    assert_eq!(error, ClientError::Server(ErrorCode::MalformedPayload));
    rejector.join().expect("rejector thread");

    let (mut fallback_client, fallback_server) = UnixStream::pair().expect("unix stream pair");
    let acceptor = std::thread::spawn(move || pre_v2_runtime_hello(fallback_server, true));
    let hello = hello_with_capabilities_until(
        &mut fallback_client,
        true,
        requested_capabilities(true, false, false, false),
        Instant::now() + Duration::from_secs(1),
    )
    .expect("M001 hello is accepted by a pre-v2 Runtime");
    assert!(!extended_terminal_key_supported(hello.server_capabilities));
    acceptor.join().expect("acceptor thread");
}

#[test]
fn hello_fallback_reconnects_when_old_runtime_rejects_newer_capabilities() {
    // Pre-V2 path needs three reconnects (#1355 chain): drop COMMAND_BLOCK_DURATION,
    // then VIEWPORT_LINE_IDS, then EXTENDED_KEY.
    let (mut client, first_server) = UnixStream::pair().expect("first pair");
    let (second_client, second_server) = UnixStream::pair().expect("second pair");
    let (third_client, third_server) = UnixStream::pair().expect("third pair");
    let (fourth_client, fourth_server) = UnixStream::pair().expect("fourth pair");
    let rejector = std::thread::spawn(move || pre_v2_runtime_hello(first_server, false));
    let rejector2 = std::thread::spawn(move || pre_v2_runtime_hello(second_server, false));
    let rejector3 = std::thread::spawn(move || pre_v2_runtime_hello(third_server, false));
    let acceptor = std::thread::spawn(move || pre_v2_runtime_hello(fourth_server, true));
    let mut fallbacks = vec![second_client, third_client, fourth_client].into_iter();
    let hello = hello_until_with_legacy_key_fallback(
        &mut client,
        || fallbacks.next().ok_or(ClientError::Io),
        true,
        true,
        Instant::now() + Duration::from_secs(1),
    )
    .expect("SPEC-006 §21.5 new client may use M001 on an old server");
    assert!(!extended_terminal_key_supported(
        hello.server.server_capabilities
    ));
    rejector.join().expect("rejector thread");
    rejector2.join().expect("rejector2 thread");
    rejector3.join().expect("rejector3 thread");
    acceptor.join().expect("acceptor thread");
}

#[test]
fn hello_fallback_drops_viewport_line_ids_against_pre_865_runtime() {
    // #1355 chain: bit 8 is dropped first, then bit 9.
    let (mut client, first_server) = UnixStream::pair().expect("first pair");
    let (second_client, second_server) = UnixStream::pair().expect("second pair");
    let (third_client, third_server) = UnixStream::pair().expect("third pair");
    let rejector =
        std::thread::spawn(move || pre_viewport_line_ids_runtime_hello(first_server, false));
    let rejector2 =
        std::thread::spawn(move || pre_viewport_line_ids_runtime_hello(second_server, false));
    let acceptor =
        std::thread::spawn(move || pre_viewport_line_ids_runtime_hello(third_server, true));
    let mut fallbacks = vec![second_client, third_client].into_iter();
    let hello = hello_until_with_legacy_key_fallback(
        &mut client,
        || fallbacks.next().ok_or(ClientError::Io),
        true,
        true,
        Instant::now() + Duration::from_secs(1),
    )
    .expect("pre-#865 Runtime accepts hello without CAP_VIEWPORT_LINE_IDS");
    assert!(extended_terminal_key_supported(
        hello.server.server_capabilities
    ));
    rejector.join().expect("rejector thread");
    rejector2.join().expect("rejector2 thread");
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

/// One mock Runtime connection: rejects any requested bit outside `known`
/// with `MalformedPayload` for ClientHello, otherwise accepts and advertises
/// `known`. Returns the ClientHello capabilities it received.
fn allowlist_runtime_hello(mut server: UnixStream, known: u32) -> u32 {
    let mut header = [0_u8; HEADER_LEN];
    server.read_exact(&mut header).expect("client hello header");
    let payload_len = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
    let mut payload = vec![0_u8; payload_len];
    server
        .read_exact(&mut payload)
        .expect("client hello payload");
    let requested = ClientHello::decode(&payload)
        .expect("client hello")
        .client_capabilities;
    if requested & !known != 0 {
        let error = ErrorMessage {
            error_code: ErrorCode::MalformedPayload as u16,
            offending_message_type: MessageType::ClientHello as u16,
            detail_code: 0,
        };
        server
            .write_all(&encode_frame(MessageType::Error, &error.encode()))
            .expect("reject unknown capability bit");
        return requested;
    }
    let hello = ServerHello {
        runtime_id: 1,
        server_capabilities: known,
        max_frame_payload: MAX_FRAME_PAYLOAD,
        max_input_payload: 65_536,
    };
    server
        .write_all(&encode_frame(MessageType::ServerHello, &hello.encode()))
        .expect("server hello");
    requested
}

const DISPLAY: u32 = CAP_BINARY_DISPLAY | CAP_SEMANTIC_TERMINAL_KEY | CAP_CORRELATED_RESIZE;
const M001_KNOWN: u32 =
    DISPLAY | CAP_COMMAND_BLOCKS | seyal_runtime::pass8::CAP_BLOCK_METADATA | CAP_GRAPHEME_DISPLAY;
const PRE_865_KNOWN: u32 = M001_KNOWN | CAP_EXTENDED_TERMINAL_KEY | CAP_EXECUTION_PROVISIONING;
const PRE_DURATION_KNOWN: u32 = PRE_865_KNOWN | CAP_VIEWPORT_LINE_IDS;
const DURATION_KNOWN: u32 = PRE_DURATION_KNOWN | CAP_COMMAND_BLOCK_DURATION;

/// Run the production fallback chain against `connections` mock Runtimes that
/// all know `known`. Returns the chain result and every ClientHello sent.
fn run_chain(
    known: u32,
    connections: usize,
    initial: Option<u32>,
) -> (Result<super::NegotiatedHello, ClientError>, Vec<u32>) {
    let mut clients = Vec::new();
    let mut servers = Vec::new();
    for _ in 0..connections {
        let (client, server) = UnixStream::pair().expect("pair");
        clients.push(client);
        servers.push(std::thread::spawn(move || {
            allowlist_runtime_hello(server, known)
        }));
    }
    let mut clients = clients.into_iter();
    let mut first = clients.next().expect("first connection");
    let deadline = Instant::now() + Duration::from_secs(1);
    let result = match initial {
        None => hello_until_with_legacy_key_fallback(
            &mut first,
            || clients.next().ok_or(ClientError::Io),
            true,
            true,
            deadline,
        ),
        Some(capabilities) => hello_chain_until(
            &mut first,
            || clients.next().ok_or(ClientError::Io),
            true,
            capabilities,
            deadline,
        ),
    };
    drop(first);
    drop(clients);
    let sent = servers
        .into_iter()
        .filter_map(|server| server.join().ok())
        .collect();
    (result, sent)
}

#[test]
fn duration_runtime_accepts_the_full_hello_in_one_attempt() {
    let (result, sent) = run_chain(DURATION_KNOWN, 1, None);
    let hello = result.expect("duration-capable Runtime accepts the full hello");
    assert_eq!(sent.len(), 1);
    for bit in [
        CAP_COMMAND_BLOCK_DURATION,
        CAP_VIEWPORT_LINE_IDS,
        CAP_EXTENDED_TERMINAL_KEY,
        CAP_COMMAND_BLOCKS,
    ] {
        assert_ne!(sent[0] & bit, 0, "step 1 requests bit {bit:#x}");
    }
    assert_eq!(
        hello.block_timeline_schema(),
        Some(BlockTimelineSchema::Duration)
    );
}

#[test]
fn runtime_without_duration_keeps_viewport_line_ids_and_extended_key() {
    // #1355 step 2: removes only bit 8 (the master Runtime before #1043).
    let (result, sent) = run_chain(PRE_DURATION_KNOWN, 2, None);
    let hello = result.expect("step 2 succeeds");
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1], sent[0] & !CAP_COMMAND_BLOCK_DURATION);
    assert_ne!(sent[1] & CAP_VIEWPORT_LINE_IDS, 0);
    assert_ne!(sent[1] & CAP_EXTENDED_TERMINAL_KEY, 0);
    assert!(extended_terminal_key_supported(
        hello.server.server_capabilities
    ));
    assert_eq!(
        hello.block_timeline_schema(),
        Some(BlockTimelineSchema::Legacy)
    );
}

#[test]
fn pre_865_runtime_lands_on_step_three() {
    let (result, sent) = run_chain(PRE_865_KNOWN, 3, None);
    result.expect("step 3 succeeds");
    assert_eq!(sent.len(), 3);
    assert_eq!(sent[2], sent[1] & !CAP_VIEWPORT_LINE_IDS);
    assert_ne!(sent[2] & CAP_EXTENDED_TERMINAL_KEY, 0);
}

#[test]
fn pre_v2_runtime_lands_on_step_four_within_four_attempts() {
    let (result, sent) = run_chain(M001_KNOWN, 4, None);
    let hello = result.expect("step 4 succeeds");
    assert_eq!(sent.len(), 4);
    assert_eq!(
        sent[3],
        sent[2] & !(CAP_EXTENDED_TERMINAL_KEY | CAP_EXECUTION_PROVISIONING)
    );
    assert_eq!(
        hello.block_timeline_schema(),
        Some(BlockTimelineSchema::Legacy)
    );
}

#[test]
fn exhausted_chain_returns_the_final_error_after_four_attempts() {
    // Rejects even the M001 set, so every step fails. Only four connections
    // exist: a fifth attempt would surface as `Io`, not the server error.
    let (result, sent) = run_chain(DISPLAY, 4, None);
    assert_eq!(
        result.expect_err("chain exhausts"),
        ClientError::Server(ErrorCode::MalformedPayload)
    );
    assert_eq!(sent.len(), 4, "at most four ClientHellos per chain");
}

#[test]
fn skipped_steps_do_not_consume_attempts() {
    // Bit 9 was never requested: removing it is skipped, so this chain sends
    // the initial hello, then without bit 8, then without the extended key.
    let initial = requested_capabilities(true, true, false, true) | CAP_COMMAND_BLOCK_DURATION;
    let (result, sent) = run_chain(M001_KNOWN, 3, Some(initial));
    result.expect("lands after the extended-key step");
    assert_eq!(sent.len(), 3);
    assert_eq!(sent[1], initial & !CAP_COMMAND_BLOCK_DURATION);
    assert_eq!(
        sent[2],
        sent[1] & !(CAP_EXTENDED_TERMINAL_KEY | CAP_EXECUTION_PROVISIONING)
    );
}

#[test]
fn unrelated_server_error_is_not_retried() {
    let (mut client, mut server) = UnixStream::pair().expect("pair");
    let server_thread = std::thread::spawn(move || {
        let mut header = [0_u8; HEADER_LEN];
        server.read_exact(&mut header).expect("client hello header");
        let payload_len = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
        let mut payload = vec![0_u8; payload_len];
        server
            .read_exact(&mut payload)
            .expect("client hello payload");
        let error = ErrorMessage {
            error_code: ErrorCode::PermissionDenied as u16,
            offending_message_type: MessageType::ClientHello as u16,
            detail_code: 0,
        };
        server
            .write_all(&encode_frame(MessageType::Error, &error.encode()))
            .expect("unrelated error");
    });
    let mut reconnects = 0;
    let error = hello_until_with_legacy_key_fallback(
        &mut client,
        || {
            reconnects += 1;
            Err(ClientError::Io)
        },
        true,
        true,
        Instant::now() + Duration::from_secs(1),
    )
    .expect_err("unrelated error is final");
    assert_eq!(error, ClientError::Server(ErrorCode::PermissionDenied));
    assert_eq!(reconnects, 0);
    server_thread.join().expect("server thread");
}

#[test]
fn transport_failure_during_fallback_is_not_retried() {
    let (mut client, server) = UnixStream::pair().expect("pair");
    let rejector = std::thread::spawn(move || allowlist_runtime_hello(server, PRE_DURATION_KNOWN));
    let mut reconnects = 0;
    let error = hello_until_with_legacy_key_fallback(
        &mut client,
        || {
            reconnects += 1;
            Err(ClientError::Discovery(
                DiscoveryFailure::EndpointDisappeared,
            ))
        },
        true,
        true,
        Instant::now() + Duration::from_secs(1),
    )
    .expect_err("reconnect failure is final");
    assert_eq!(
        error,
        ClientError::Discovery(DiscoveryFailure::EndpointDisappeared)
    );
    assert_eq!(reconnects, 1);
    rejector.join().expect("rejector thread");
}

#[test]
fn block_timeline_schema_is_the_intersection_of_request_and_server() {
    // A duration-capable Runtime advertises bit 8 to every client; a client
    // that did not request it must still decode the legacy schema.
    let server = ServerHello {
        runtime_id: 1,
        server_capabilities: DURATION_KNOWN,
        max_frame_payload: MAX_FRAME_PAYLOAD,
        max_input_payload: 65_536,
    };
    let without = super::NegotiatedHello {
        server,
        client_capabilities: requested_capabilities(true, true, true, true),
    };
    assert_eq!(
        without.block_timeline_schema(),
        Some(BlockTimelineSchema::Legacy)
    );
    let with = super::NegotiatedHello {
        server,
        client_capabilities: without.client_capabilities | CAP_COMMAND_BLOCK_DURATION,
    };
    assert_eq!(
        with.block_timeline_schema(),
        Some(BlockTimelineSchema::Duration)
    );
}
