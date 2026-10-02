    use super::{
        canonical_control_socket_path, classify_connect_error, classify_discovery_error,
        extended_terminal_key_supported, hello_until, hello_until_with_legacy_key_fallback,
        requested_capabilities, ClientError, DiscoveryFailure,
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
        let error = hello_until(
            &mut rejected_client,
            true,
            true,
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
        let hello = hello_until(
            &mut fallback_client,
            true,
            true,
            false,
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
