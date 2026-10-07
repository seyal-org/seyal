#![cfg(all(target_os = "macos", feature = "test-fault-injection"))]
#![allow(unsafe_code)]

use std::{
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, PermissionsExt},
        net::UnixStream,
    },
    path::PathBuf,
    time::{Duration, Instant},
};

use seyal_exec::{CommandSpec, WindowSize};
use seyal_runtime::{
    local_ipc::{
        connection::MAX_CONNECTIONS,
        discovery::CONTROL_SOCKET_NAME,
        framing::{
            encode_frame, Attach, Attached, ClientHello, FrameHeader, MessageType, ResumeDelivery,
            Role, ServerHello, SuspendDelivery, CAP_ATTACHMENT_DELIVERY_CONTROL, HEADER_LEN,
        },
    },
    test_fault::{self, FaultPoint},
    ExecutionId, LocalIpcMode, Runtime, RuntimeConfig,
};

// `cargo test`'s default runner executes every `#[test]` in this file as a
// thread within one shared process (unlike separate test *files*, which run
// as separate processes). Several tests below assert on process-wide
// descriptor counts (`fd_count()`) to prove no FD/socket leak, which is only
// meaningful if no sibling test is concurrently opening/closing descriptors
// of its own. Serializing the whole file is the smallest fix: it changes
// nothing about what any individual test exercises or asserts, it only
// removes cross-test interference in a shared process-global measurement.
static TEST_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn serialized() -> std::sync::MutexGuard<'static, ()> {
    TEST_SERIAL
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

fn config() -> RuntimeConfig {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let suffix = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut config = RuntimeConfig::m001().unwrap();
    config.singleton_path = std::env::temp_dir().join(format!("s5fi-{suffix:x}.lock"));
    config.local_ipc = LocalIpcMode::Enabled {
        runtime_dir_override: Some(std::env::temp_dir().join(format!("s5fid-{suffix:x}"))),
    };
    config
}
fn pump(runtime: &mut Runtime) {
    runtime.poll_once(Some(Duration::from_millis(5))).unwrap();
}
fn connect(runtime: &mut Runtime) -> UnixStream {
    let stream = UnixStream::connect(runtime.local_ipc_socket_path().unwrap()).unwrap();
    stream.set_nonblocking(true).unwrap();
    pump(runtime);
    stream
}
fn send(runtime: &mut Runtime, stream: &mut UnixStream, kind: MessageType, payload: &[u8]) {
    stream.write_all(&encode_frame(kind, payload)).unwrap();
    pump(runtime);
}
fn frame(runtime: &mut Runtime, stream: &mut UnixStream) -> Option<(MessageType, Vec<u8>)> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut buffer = Vec::new();
    loop {
        let mut chunk = [0u8; 8192];
        match stream.read(&mut chunk) {
            Ok(0) => return None,
            Ok(n) => buffer.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => pump(runtime),
            Err(_) => return None,
        };
        if buffer.len() >= HEADER_LEN {
            let h = FrameHeader::decode(&buffer[..HEADER_LEN]).unwrap();
            let total = HEADER_LEN + h.payload_len as usize;
            if buffer.len() >= total {
                return Some((
                    MessageType::from_u16(h.message_type).unwrap(),
                    buffer[HEADER_LEN..total].to_vec(),
                ));
            }
        }
        assert!(Instant::now() < deadline, "frame timed out");
    }
}
fn hello(runtime: &mut Runtime, stream: &mut UnixStream) {
    send(
        runtime,
        stream,
        MessageType::ClientHello,
        &ClientHello {
            client_capabilities: 0,
        }
        .encode(),
    );
    let (kind, payload) = frame(runtime, stream).unwrap();
    assert_eq!(kind, MessageType::ServerHello);
    ServerHello::decode(&payload).unwrap();
}
fn attach(runtime: &mut Runtime, stream: &mut UnixStream, execution_id: ExecutionId) {
    send(
        runtime,
        stream,
        MessageType::Attach,
        &Attach {
            execution_id,
            requested_role: Role::Controller,
        }
        .encode(),
    );
}
fn no_authority(runtime: &Runtime, execution_id: ExecutionId) {
    assert_eq!(runtime.lookup(execution_id).unwrap().attachment_count, 0);
}

// Workstream G (Pass 5.1 failure-injection/resource-cleanup audit) note on
// "listen/setup failure where separately injectable": on this platform
// abstraction it is not separately injectable. `LocalIpcServer::bind` binds
// through `std::os::unix::net::UnixListener::bind`, which performs the
// bind(2) and listen(2) syscalls as one atomic standard-library operation
// with a single combined `io::Result`; there is no public seam between the
// two steps to fail independently, and there is no observable intermediate
// state to assert against. Introducing one would require replacing the safe
// std listener with hand-rolled libc socket/bind/listen calls purely to
// serve a test, which is a larger production-code change than the "smallest
// seam necessary" rule in AGENTS.md permits for a non-production need. The
// real bind(2) failure path (which also exercises the shared
// discovery/permission-verification code that listen() would run behind) is
// covered below by `socket_bind_failure_propagates_a_clean_error_and_releases_the_singleton`.

fn fd_count() -> usize {
    // Avoid /dev/fd enumeration changing the count while it is measured.
    (0..1024)
        .filter(|fd| {
            // SAFETY: F_GETFD only inspects the integer descriptor; invalid
            // descriptors are reported with EBADF and are not modified.
            (unsafe { libc::fcntl(*fd, libc::F_GETFD) }) >= 0
        })
        .count()
}

/// Sends `Attach` and reads back the mandatory `Attached` response,
/// confirming the attachment was fully admitted and the response fully
/// delivered/read -- i.e. the connection is past the handshake window, not
/// mid-transaction. (Deliberately does not also drain the trailing
/// `DisplaySnapshot` batch: this file's minimal `frame()` helper allocates a
/// fresh read buffer per call rather than retaining a per-connection
/// buffer, so it cannot safely split multiple frames that arrive
/// coalesced in one `read()`. `local_ipc_ctrunc.rs`/`local_ipc_protocol.rs`
/// use a `Harness` with a persistent per-client buffer for that; reusing
/// their heavier harness here would be more machinery than this scenario
/// needs. Leaving the snapshot bytes unread on a since-dropped socket is
/// itself an ordinary, non-abnormal close on a Unix-domain socket.)
fn attach_and_confirm(
    runtime: &mut Runtime,
    stream: &mut UnixStream,
    execution_id: ExecutionId,
) -> Attached {
    attach(runtime, stream, execution_id);
    let (kind, payload) = frame(runtime, stream).expect("Attached");
    assert_eq!(kind, MessageType::Attached);
    Attached::decode(&payload).unwrap()
}

fn unique_registration_suffix() -> u64 {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Builds a Runtime configuration together with the local-IPC runtime
/// directory it will bind in, so a failed `Runtime::new` attempt and a
/// subsequent recovery attempt can be proven to target the exact same
/// filesystem locations (no fresh path is substituted to dodge the failure).
fn registration_config() -> (RuntimeConfig, PathBuf) {
    let suffix = unique_registration_suffix();
    let runtime_dir = std::env::temp_dir().join(format!("s5fi-reg-dir-{suffix:x}"));
    let mut config = RuntimeConfig::m001().unwrap();
    config.singleton_path = std::env::temp_dir().join(format!("s5fi-reg-{suffix:x}.lock"));
    config.local_ipc = LocalIpcMode::Enabled {
        runtime_dir_override: Some(runtime_dir.clone()),
    };
    (config, runtime_dir)
}

#[test]
fn attach_admission_failure_publishes_no_authority_and_fresh_controller_recovers() {
    let _guard = serialized();
    let mut runtime = Runtime::new(config()).unwrap();
    let execution_id = runtime
        .create_execution(
            CommandSpec::new("/bin/cat"),
            WindowSize::new(80, 24, 0, 0).unwrap(),
        )
        .unwrap();
    let mut failed = connect(&mut runtime);
    hello(&mut runtime, &mut failed);
    test_fault::fail_next(FaultPoint::AttachAdmission);
    attach(&mut runtime, &mut failed, execution_id);
    for _ in 0..4 {
        pump(&mut runtime);
    }
    no_authority(&runtime, execution_id);

    let mut fresh = connect(&mut runtime);
    hello(&mut runtime, &mut fresh);
    attach(&mut runtime, &mut fresh, execution_id);
    let (kind, payload) = frame(&mut runtime, &mut fresh).expect("Attached");
    assert_eq!(kind, MessageType::Attached);
    assert_eq!(
        Attached::decode(&payload).unwrap().execution_id,
        execution_id
    );
    assert_eq!(runtime.lookup(execution_id).unwrap().attachment_count, 1);
    drop(fresh);
    drop(failed);
    for _ in 0..8 {
        pump(&mut runtime);
    }
    no_authority(&runtime, execution_id);
    runtime.begin_shutdown().unwrap();
    runtime
        .run_until_empty(Instant::now() + Duration::from_secs(3))
        .unwrap();
}

#[test]
fn writable_flush_failure_reclaims_published_authority() {
    let _guard = serialized();
    let mut runtime = Runtime::new(config()).unwrap();
    let execution_id = runtime
        .create_execution(
            CommandSpec::new("/bin/cat"),
            WindowSize::new(80, 24, 0, 0).unwrap(),
        )
        .unwrap();
    let mut client = connect(&mut runtime);
    hello(&mut runtime, &mut client);
    test_fault::fail_next(FaultPoint::AttachFlush);
    attach(&mut runtime, &mut client, execution_id);
    for _ in 0..16 {
        pump(&mut runtime);
    }
    no_authority(&runtime, execution_id);
}

#[test]
fn socket_bind_failure_propagates_a_clean_error_and_releases_the_singleton() {
    let _guard = serialized();
    let baseline_fds = fd_count();
    let (config, runtime_dir) = registration_config();
    let singleton_path = config.singleton_path.clone();

    // Pre-create the runtime directory without write permission. It still
    // passes discovery's ownership/mode verification (0o500 is neither
    // group- nor world-writable, and it is owned by the effective user), but
    // the kernel cannot create the control-socket file inside it, so
    // `UnixListener::bind` fails with a real EACCES rather than a synthetic
    // fault.
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(false).mode(0o500);
    builder.create(&runtime_dir).unwrap();

    assert!(
        Runtime::new(config).is_err(),
        "bind must fail against a read-only runtime directory"
    );

    let socket_path = runtime_dir.join(CONTROL_SOCKET_NAME);
    assert!(
        !socket_path.exists(),
        "no socket file should exist after a failed bind"
    );

    std::fs::set_permissions(&runtime_dir, std::fs::Permissions::from_mode(0o700)).unwrap();

    // The singleton guard was released via Drop when `Runtime::new` returned
    // Err, so a fresh attempt at the exact same singleton path and runtime
    // directory must succeed without contention.
    let mut recovered = RuntimeConfig::m001().unwrap();
    recovered.singleton_path = singleton_path;
    recovered.local_ipc = LocalIpcMode::Enabled {
        runtime_dir_override: Some(runtime_dir.clone()),
    };
    let mut runtime =
        Runtime::new(recovered).expect("Runtime recovers once the permission is fixed");
    runtime.begin_shutdown().unwrap();
    runtime
        .run_until_empty(Instant::now() + Duration::from_secs(3))
        .unwrap();
    drop(runtime);
    std::fs::remove_dir_all(&runtime_dir).ok();

    assert_eq!(
        fd_count(),
        baseline_fds,
        "a failed bind must not leak the listener descriptor"
    );
}

#[test]
fn accept_readiness_failure_does_not_corrupt_listener_or_pending_connection() {
    let _guard = serialized();
    let baseline_fds = fd_count();
    {
        let mut runtime = Runtime::new(config()).unwrap();
        let execution_id = runtime
            .create_execution(
                CommandSpec::new("/bin/cat"),
                WindowSize::new(80, 24, 0, 0).unwrap(),
            )
            .unwrap();

        // Arm the fault, then connect: the listener becomes readable and the
        // very first readiness poll must observe the injected accept error
        // rather than a real accept(2) failure (see FaultPoint::AcceptReady
        // doc: production accept(2) failure is not deterministically
        // reproducible without perturbing process-wide FD limits shared with
        // concurrently running tests).
        test_fault::fail_next(FaultPoint::AcceptReady);
        let path = runtime.local_ipc_socket_path().unwrap().to_path_buf();
        let mut client = UnixStream::connect(&path).unwrap();
        client.set_nonblocking(true).unwrap();

        let first_poll = runtime.poll_once(Some(Duration::from_millis(200)));
        assert!(
            first_poll.is_err(),
            "the injected accept readiness failure must propagate"
        );

        // The fault is one-shot and the listener registration is untouched:
        // ordinary polling now accepts the still-pending connection.
        for _ in 0..8 {
            pump(&mut runtime);
        }
        hello(&mut runtime, &mut client);
        attach(&mut runtime, &mut client, execution_id);
        let (kind, payload) = frame(&mut runtime, &mut client).expect("Attached");
        assert_eq!(kind, MessageType::Attached);
        assert_eq!(
            Attached::decode(&payload).unwrap().execution_id,
            execution_id
        );
        assert_eq!(runtime.lookup(execution_id).unwrap().attachment_count, 1);

        drop(client);
        for _ in 0..8 {
            pump(&mut runtime);
        }
        no_authority(&runtime, execution_id);
        runtime.begin_shutdown().unwrap();
        runtime
            .run_until_empty(Instant::now() + Duration::from_secs(3))
            .unwrap();
    }
    assert_eq!(
        fd_count(),
        baseline_fds,
        "a transient accept failure must not leak descriptors"
    );
}

#[test]
fn listener_reactor_registration_failure_cleans_up_socket_and_releases_singleton() {
    let _guard = serialized();
    let baseline_fds = fd_count();
    let (config, runtime_dir) = registration_config();
    let singleton_path = config.singleton_path.clone();

    test_fault::fail_next(FaultPoint::ListenerReactorRegistration);
    assert!(
        Runtime::new(config).is_err(),
        "injected listener reactor registration failure must propagate"
    );

    let socket_path = runtime_dir.join(CONTROL_SOCKET_NAME);
    assert!(
        !socket_path.exists(),
        "listener socket must not survive an injected registration failure"
    );

    // The singleton guard was released via Drop when `Runtime::new` returned
    // Err, so a fresh Runtime can bind at the exact same paths immediately.
    let mut recovered = RuntimeConfig::m001().unwrap();
    recovered.singleton_path = singleton_path;
    recovered.local_ipc = LocalIpcMode::Enabled {
        runtime_dir_override: Some(runtime_dir.clone()),
    };
    let mut runtime = Runtime::new(recovered).expect("Runtime recovers after injected failure");
    runtime.begin_shutdown().unwrap();
    runtime
        .run_until_empty(Instant::now() + Duration::from_secs(3))
        .unwrap();
    drop(runtime);
    std::fs::remove_dir_all(&runtime_dir).ok();

    assert_eq!(
        fd_count(),
        baseline_fds,
        "an injected registration failure must not leak the listener descriptor"
    );
}

#[test]
fn connection_reactor_registration_failure_closes_connection_without_leaking_capacity() {
    let _guard = serialized();
    let baseline_fds = fd_count();
    {
        let mut runtime = Runtime::new(config()).unwrap();
        let execution_id = runtime
            .create_execution(
                CommandSpec::new("/bin/cat"),
                WindowSize::new(80, 24, 0, 0).unwrap(),
            )
            .unwrap();

        test_fault::fail_next(FaultPoint::ConnectionReactorRegistration);
        let mut failed = connect(&mut runtime);

        // The injected failure means the connection was never registered
        // with the reactor and was closed immediately after accept; no
        // ServerHello is ever produced and the socket observes EOF.
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let mut probe = [0u8; 16];
            match failed.read(&mut probe) {
                Ok(0) => break,
                Ok(_) => {
                    panic!("a connection that failed reactor registration must never be serviced")
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => pump(&mut runtime),
                Err(error) => panic!("unexpected read error: {error}"),
            }
            assert!(
                Instant::now() < deadline,
                "connection was never closed after the injected registration failure"
            );
        }
        drop(failed);

        // The listener and its slot accounting remain healthy: a fresh
        // connection attaches normally afterward.
        let mut fresh = connect(&mut runtime);
        hello(&mut runtime, &mut fresh);
        attach(&mut runtime, &mut fresh, execution_id);
        let (kind, payload) = frame(&mut runtime, &mut fresh).expect("Attached");
        assert_eq!(kind, MessageType::Attached);
        assert_eq!(
            Attached::decode(&payload).unwrap().execution_id,
            execution_id
        );
        assert_eq!(runtime.lookup(execution_id).unwrap().attachment_count, 1);

        drop(fresh);
        for _ in 0..8 {
            pump(&mut runtime);
        }
        no_authority(&runtime, execution_id);
        runtime.begin_shutdown().unwrap();
        runtime
            .run_until_empty(Instant::now() + Duration::from_secs(3))
            .unwrap();
    }
    assert_eq!(
        fd_count(),
        baseline_fds,
        "descriptor count must return to baseline"
    );
}

#[test]
fn client_disconnect_during_attach_releases_authority_without_stale_state() {
    let _guard = serialized();
    let baseline_fds = fd_count();
    {
        let mut runtime = Runtime::new(config()).unwrap();
        let execution_id = runtime
            .create_execution(
                CommandSpec::new("/bin/cat"),
                WindowSize::new(80, 24, 0, 0).unwrap(),
            )
            .unwrap();
        let mut client = connect(&mut runtime);
        hello(&mut runtime, &mut client);

        // Write the Attach frame directly and do not read any response. A
        // single pump admits the attachment (proven by attachment_count)
        // while the Attached/snapshot response is still sitting unread in
        // the outbound queue -- this is the real "mid-handshake" window.
        client
            .write_all(&encode_frame(
                MessageType::Attach,
                &Attach {
                    execution_id,
                    requested_role: Role::Controller,
                }
                .encode(),
            ))
            .unwrap();
        pump(&mut runtime);
        assert_eq!(
            runtime.lookup(execution_id).unwrap().attachment_count,
            1,
            "attach must be admitted before the disconnect is injected"
        );

        // The client vanishes mid-handshake, before ever reading Attached.
        drop(client);

        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            pump(&mut runtime);
            if runtime.lookup(execution_id).unwrap().attachment_count == 0 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "attachment authority never released after the client vanished mid-attach"
            );
        }
        no_authority(&runtime, execution_id);

        // A fresh controller must be able to acquire the lease immediately,
        // proving no stale attachment/lease/registry entry survived.
        let mut fresh = connect(&mut runtime);
        hello(&mut runtime, &mut fresh);
        attach(&mut runtime, &mut fresh, execution_id);
        let (kind, payload) = frame(&mut runtime, &mut fresh).expect("Attached");
        assert_eq!(kind, MessageType::Attached);
        assert_eq!(
            Attached::decode(&payload).unwrap().execution_id,
            execution_id
        );
        assert_eq!(runtime.lookup(execution_id).unwrap().attachment_count, 1);

        drop(fresh);
        for _ in 0..8 {
            pump(&mut runtime);
        }
        no_authority(&runtime, execution_id);
        runtime.begin_shutdown().unwrap();
        runtime
            .run_until_empty(Instant::now() + Duration::from_secs(3))
            .unwrap();
    }
    assert_eq!(
        fd_count(),
        baseline_fds,
        "a mid-attach disconnect must not leak descriptors"
    );
}

#[test]
fn clean_controller_disconnect_releases_lease_for_immediate_reacquisition() {
    let _guard = serialized();
    let baseline_fds = fd_count();
    {
        let mut runtime = Runtime::new(config()).unwrap();
        let execution_id = runtime
            .create_execution(
                CommandSpec::new("/bin/cat"),
                WindowSize::new(80, 24, 0, 0).unwrap(),
            )
            .unwrap();
        let mut controller = connect(&mut runtime);
        hello(&mut runtime, &mut controller);
        let _attached = attach_and_confirm(&mut runtime, &mut controller, execution_id);
        assert_eq!(runtime.lookup(execution_id).unwrap().attachment_count, 1);

        // A completely clean disconnect: no malformed data, no ancillary
        // payload, just the socket going away after the attach transaction
        // was fully admitted and its mandatory response read. This is
        // distinct from the MSG_CTRUNC/fatal ancillary-error regression in
        // local_ipc_ctrunc.rs.
        drop(controller);
        for _ in 0..8 {
            pump(&mut runtime);
        }
        no_authority(&runtime, execution_id);

        // The controller lease itself -- not merely an observer slot -- must
        // be immediately reacquirable; this would fail with ControllerBusy
        // if the prior lease had survived the clean disconnect.
        let mut successor = connect(&mut runtime);
        hello(&mut runtime, &mut successor);
        let attached = attach_and_confirm(&mut runtime, &mut successor, execution_id);
        assert_eq!(attached.execution_id, execution_id);
        assert_eq!(runtime.lookup(execution_id).unwrap().attachment_count, 1);

        drop(successor);
        for _ in 0..8 {
            pump(&mut runtime);
        }
        no_authority(&runtime, execution_id);
        runtime.begin_shutdown().unwrap();
        runtime
            .run_until_empty(Instant::now() + Duration::from_secs(3))
            .unwrap();
    }
    assert_eq!(
        fd_count(),
        baseline_fds,
        "a clean controller disconnect must not leak descriptors"
    );
}

#[test]
fn connection_capacity_is_fully_recovered_after_disconnecting_all_connections() {
    let _guard = serialized();
    let baseline_fds = fd_count();
    {
        let mut runtime = Runtime::new(config()).unwrap();

        for round in 0..2 {
            // Saturate the connection table at its documented capacity.
            let mut clients: Vec<UnixStream> = Vec::new();
            for _ in 0..MAX_CONNECTIONS {
                let mut client = connect(&mut runtime);
                hello(&mut runtime, &mut client);
                clients.push(client);
            }

            // The kernel listen backlog does not know about the
            // application-level cap, so one more connect(2) still succeeds,
            // but the server must never service it while at capacity: no
            // ServerHello is ever produced, and the accepted-but-refused
            // socket is closed without ever being registered.
            let path = runtime.local_ipc_socket_path().unwrap().to_path_buf();
            let mut overflow = UnixStream::connect(&path).unwrap();
            overflow.set_nonblocking(true).unwrap();
            overflow
                .write_all(&encode_frame(
                    MessageType::ClientHello,
                    &ClientHello {
                        client_capabilities: 0,
                    }
                    .encode(),
                ))
                .unwrap();

            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                let mut probe = [0u8; 16];
                match overflow.read(&mut probe) {
                    Ok(0) => break,
                    Ok(_) => {
                        panic!("round {round}: an over-capacity connection must never be serviced")
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        pump(&mut runtime)
                    }
                    Err(error) => panic!("round {round}: unexpected overflow read error: {error}"),
                }
                assert!(
                    Instant::now() < deadline,
                    "round {round}: overflow connection was never closed by capacity enforcement"
                );
            }
            drop(overflow);

            // Disconnecting every held connection must free the entire
            // table, not merely headroom for one more -- proven by
            // successfully saturating it again on the next round.
            clients.clear();
            for _ in 0..8 {
                pump(&mut runtime);
            }
        }

        // The table remains fully usable after two full saturate/drain
        // rounds: a controller can still attach and be cleanly torn down.
        let execution_id = runtime
            .create_execution(
                CommandSpec::new("/bin/cat"),
                WindowSize::new(80, 24, 0, 0).unwrap(),
            )
            .unwrap();
        let mut client = connect(&mut runtime);
        hello(&mut runtime, &mut client);
        attach(&mut runtime, &mut client, execution_id);
        let (kind, payload) = frame(&mut runtime, &mut client).expect("Attached");
        assert_eq!(kind, MessageType::Attached);
        assert_eq!(
            Attached::decode(&payload).unwrap().execution_id,
            execution_id
        );
        assert_eq!(runtime.lookup(execution_id).unwrap().attachment_count, 1);

        drop(client);
        for _ in 0..8 {
            pump(&mut runtime);
        }
        no_authority(&runtime, execution_id);
        runtime.begin_shutdown().unwrap();
        runtime
            .run_until_empty(Instant::now() + Duration::from_secs(3))
            .unwrap();
    }
    assert_eq!(
        fd_count(),
        baseline_fds,
        "connection saturate/drain cycling must not leak descriptors"
    );
}

#[test]
fn resume_resync_n_times_display_encode_fault_stays_bounded() {
    let _guard = serialized();
    let mut runtime = Runtime::new(config()).unwrap();
    let execution_id = runtime
        .create_execution(
            CommandSpec::new("/bin/cat"),
            WindowSize::new(80, 24, 0, 0).unwrap(),
        )
        .unwrap();
    let mut stream = connect(&mut runtime);
    send(
        &mut runtime,
        &mut stream,
        MessageType::ClientHello,
        &ClientHello {
            client_capabilities: CAP_ATTACHMENT_DELIVERY_CONTROL,
        }
        .encode(),
    );
    let (kind, _) = frame(&mut runtime, &mut stream).unwrap();
    assert_eq!(kind, MessageType::ServerHello);
    attach(&mut runtime, &mut stream, execution_id);
    let (kind, payload) = frame(&mut runtime, &mut stream).expect("Attached");
    assert_eq!(kind, MessageType::Attached);
    let attached = Attached::decode(&payload).unwrap();
    // Drain the attach snapshot so resume resync can encode.
    let drain_until = Instant::now() + Duration::from_millis(400);
    while Instant::now() < drain_until {
        pump(&mut runtime);
        let mut buf = [0u8; 8192];
        match stream.read(&mut buf) {
            Ok(0) | Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("{error}"),
        }
    }

    send(
        &mut runtime,
        &mut stream,
        MessageType::SuspendDelivery,
        &SuspendDelivery {
            attachment_id: attached.attachment_id,
        }
        .encode(),
    );
    test_fault::fail_times(FaultPoint::DisplayEncode, 5);
    send(
        &mut runtime,
        &mut stream,
        MessageType::ResumeDelivery,
        &ResumeDelivery {
            attachment_id: attached.attachment_id,
        }
        .encode(),
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    while test_fault::remaining(FaultPoint::DisplayEncode) > 0 && Instant::now() < deadline {
        pump(&mut runtime);
        let mut buf = [0u8; 8192];
        match stream.read(&mut buf) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("{error}"),
        }
    }
    assert_eq!(
        test_fault::remaining(FaultPoint::DisplayEncode),
        0,
        "bounded resync retries must consume the injected encode faults"
    );
    assert_eq!(runtime.lookup(execution_id).unwrap().attachment_count, 1);
    runtime.begin_shutdown().unwrap();
    runtime
        .run_until_empty(Instant::now() + Duration::from_secs(3))
        .unwrap();
}
