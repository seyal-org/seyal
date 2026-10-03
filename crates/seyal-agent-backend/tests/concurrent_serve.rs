//! Concurrent multi-client serve (AB-1.3 / #1179).
//!
//! Two live authorized peers against one daemon incarnation; observer cannot
//! escalate; peer fault does not stall the other connection.

#![cfg(unix)]

mod support;

use std::{
    os::unix::net::UnixStream,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};

use seyal_agent_backend::{
    AgentDaemon, DaemonConfig, DaemonError, HostObservationKind, IntegrationConfig, ScriptStep,
};
use seyal_agent_core::WorkScopeKind;
use seyal_agent_protocol::{
    encode_command, encode_hello, AggregateRef, Command, CommandError, CommandResult, FrameKind,
    Hello, ProtocolVersion, ABSOLUTE_MAX_FRAME_SIZE,
};
use seyal_agent_store::{AgentStore, AggregateId, AggregateSequence};

use support::{temp_dir, TestClient};

const BARRIER: Duration = Duration::from_secs(5);

fn recv_barrier<T>(rx: &mpsc::Receiver<T>, timeout: Duration, barrier: &str) -> T {
    match rx.recv_timeout(timeout) {
        Ok(value) => value,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("barrier {barrier} timed out after {timeout:?}")
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => panic!("barrier {barrier} disconnected"),
    }
}

fn script() -> Vec<ScriptStep> {
    vec![
        ScriptStep::Emit(HostObservationKind::Started),
        ScriptStep::Emit(HostObservationKind::Output(vec![7; 64])),
        ScriptStep::Emit(HostObservationKind::KnownSuccess),
    ]
}

#[test]
fn two_live_peers_owner_and_observer_share_authoritative_run() {
    let dir = temp_dir("concurrent-two-live");
    let mut daemon = AgentDaemon::bind_integration_with_script(
        &dir,
        IntegrationConfig {
            store_path: dir.join("agent.db"),
        },
        script(),
    )
    .unwrap();
    let socket = daemon.socket_path();
    let (live_tx, live_rx) = mpsc::channel();
    let (go_owner_tx, go_owner_rx) = mpsc::channel();
    let (go_observer_tx, go_observer_rx) = mpsc::channel();
    let (run_tx, run_rx) = mpsc::channel();
    let (replayed_tx, replayed_rx) = mpsc::channel();

    let owner_live = live_tx.clone();
    let owner_socket = socket.clone();
    let owner = thread::spawn(move || {
        let mut client = TestClient::connect(&owner_socket);
        owner_live.send("owner").unwrap();
        recv_barrier(&go_owner_rx, BARRIER, "two-live-release-owner");
        let scope = client.create_work_scope(WorkScopeKind::Repository);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        let started = client.start_agent_run(attempt);
        run_tx.send((started.run_id, started.event_count)).unwrap();
        recv_barrier(
            &replayed_rx,
            BARRIER,
            "observer-replayed-before-owner-disconnect",
        );
        started.event_count
    });

    let observer_socket = socket.clone();
    let observer = thread::spawn(move || {
        let mut client = TestClient::connect_observer(&observer_socket);
        live_tx.send("observer").unwrap();
        recv_barrier(&go_observer_rx, BARRIER, "two-live-release-observer");
        match client.command(&Command::CreateWorkScope {
            session_id: client.session_id,
            kind: WorkScopeKind::AdHoc,
        }) {
            CommandResult::Error(CommandError::Denied) => {}
            other => panic!("observer create must be denied: {other:?}"),
        }
        let (run_id, event_count) = recv_barrier(&run_rx, BARRIER, "owner-run-started");
        let replay = client.replay_all(AggregateRef::AgentRun(run_id));
        assert_eq!(replay.len() as u64, event_count);
        replayed_tx.send(()).unwrap();
        replay.len()
    });

    let h1 = daemon.accept_and_spawn().expect("spawn first peer worker");
    let h2 = daemon.accept_and_spawn().expect("spawn second peer worker");
    let mut live = vec![
        recv_barrier(&live_rx, BARRIER, "two-live"),
        recv_barrier(&live_rx, BARRIER, "two-live"),
    ];
    live.sort_unstable();
    assert_eq!(live, vec!["observer", "owner"], "barrier two-live");
    go_owner_tx.send(()).unwrap();
    go_observer_tx.send(()).unwrap();

    let event_count = owner.join().unwrap();
    let replay_len = observer.join().unwrap();
    assert_eq!(replay_len as u64, event_count);
    assert!(h1.join().unwrap().is_ok());
    assert!(h2.join().unwrap().is_ok());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn observer_cannot_escalate_while_owner_is_live() {
    let dir = temp_dir("concurrent-no-escalate");
    let mut daemon = AgentDaemon::bind_integration_with_script(
        &dir,
        IntegrationConfig {
            store_path: dir.join("agent.db"),
        },
        script(),
    )
    .unwrap();
    let socket = daemon.socket_path();
    let owner_alive = Arc::new(AtomicBool::new(true));

    let owner_flag = Arc::clone(&owner_alive);
    let owner_socket = socket.clone();
    let owner = thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        let mut client = TestClient::connect(&owner_socket);
        let scope = client.create_work_scope(WorkScopeKind::Repository);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        let started = client.start_agent_run(attempt);
        while owner_flag.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(5));
        }
        started.run_id
    });

    let obs_socket = socket.clone();
    let observer = thread::spawn(move || {
        thread::sleep(Duration::from_millis(40));
        let mut client = TestClient::connect_observer(&obs_socket);
        match client.command(&Command::CreateWorkScope {
            session_id: client.session_id,
            kind: WorkScopeKind::AdHoc,
        }) {
            CommandResult::Error(CommandError::Denied) => {}
            other => panic!("create denied: {other:?}"),
        }
        match client.command(&Command::CheckGeneration {
            session_id: client.session_id,
            run_id: seyal_agent_core::AgentRunId::from_bytes([0; 16]),
            binding_generation: 1,
            control_generation: 1,
        }) {
            CommandResult::Error(CommandError::Denied) => {}
            other => panic!("control denied: {other:?}"),
        }
    });

    let h1 = daemon.accept_and_spawn().unwrap();
    let h2 = daemon.accept_and_spawn().unwrap();
    observer.join().unwrap();
    owner_alive.store(false, Ordering::SeqCst);
    let _ = owner.join().unwrap();
    assert!(h1.join().unwrap().is_ok());
    assert!(h2.join().unwrap().is_ok());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn peer_disconnect_leaves_other_connection_and_run_healthy() {
    peer_disconnect_variant("concurrent-peer-drop-idle", false);
    peer_disconnect_variant("concurrent-peer-drop-inflight", true);
}

fn peer_disconnect_variant(label: &str, response_in_flight: bool) {
    let dir = temp_dir(label);
    let mut daemon = AgentDaemon::bind_integration_with_script(
        &dir,
        IntegrationConfig {
            store_path: dir.join("agent.db"),
        },
        script(),
    )
    .unwrap();
    let socket = daemon.socket_path();
    let (live_tx, live_rx) = mpsc::channel();
    let (go_owner_tx, go_owner_rx) = mpsc::channel();
    let (go_drop_tx, go_drop_rx) = mpsc::channel();
    let (dropped_tx, dropped_rx) = mpsc::channel();

    let owner_live = live_tx.clone();
    let owner_socket = socket.clone();
    let owner = thread::spawn(move || {
        let mut client = TestClient::connect(&owner_socket);
        owner_live.send("owner").unwrap();
        recv_barrier(&go_owner_rx, BARRIER, "disconnect-release-owner");
        recv_barrier(&dropped_rx, BARRIER, "dropper-closed");
        let scope = client.create_work_scope(WorkScopeKind::Repository);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        let started = client.start_agent_run(attempt);
        let snap = client.snapshot(AggregateRef::AgentRun(started.run_id));
        assert_eq!(snap.incorporated_through, started.event_count);
        started.event_count
    });

    let drop_socket = socket.clone();
    let dropper = thread::spawn(move || {
        let mut client = TestClient::connect_observer(&drop_socket);
        live_tx.send("dropper").unwrap();
        recv_barrier(&go_drop_rx, BARRIER, "disconnect-release-dropper");
        if response_in_flight {
            client.send_unanswered(&Command::CreateWorkScope {
                session_id: client.session_id,
                kind: WorkScopeKind::AdHoc,
            });
        }
        drop(client);
        dropped_tx.send(()).unwrap();
    });

    let h1 = daemon.accept_and_spawn().unwrap();
    let h2 = daemon.accept_and_spawn().unwrap();
    let mut live = vec![
        recv_barrier(&live_rx, BARRIER, "disconnect-both-live"),
        recv_barrier(&live_rx, BARRIER, "disconnect-both-live"),
    ];
    live.sort_unstable();
    assert_eq!(
        live,
        vec!["dropper", "owner"],
        "barrier disconnect-both-live"
    );
    go_owner_tx.send(()).unwrap();
    go_drop_tx.send(()).unwrap();

    let event_count = owner.join().unwrap();
    dropper.join().unwrap();
    assert!(
        event_count > 0,
        "barrier {label}: owner run produced no events"
    );
    for (name, handle) in [("worker-a", h1), ("worker-b", h2)] {
        match handle.join().unwrap() {
            Ok(()) | Err(DaemonError::Io) => {}
            other => panic!("barrier {label}-{name}: fatal {other:?}"),
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// Peers that connect but never send Hello are accepted ahead of the owner.
/// Admission must not wait on their handshake reads (`read_timeout`), so the
/// owner is served promptly and each stalled worker ends as a client fault.
#[test]
fn stalled_pre_hello_peers_do_not_block_next_peer_admission() {
    let dir = temp_dir("concurrent-stalled-hello");
    let handshake_timeout = Duration::from_secs(5);
    let config = DaemonConfig {
        read_timeout: handshake_timeout,
        ..DaemonConfig::default()
    };
    let mut daemon = AgentDaemon::bind_integration_with_script_config(
        &dir,
        config,
        IntegrationConfig {
            store_path: dir.join("agent.db"),
        },
        script(),
    )
    .unwrap();
    let socket = daemon.socket_path();

    let stalled: Vec<UnixStream> = (0..3)
        .map(|_| UnixStream::connect(&socket).expect("stalled peer connects"))
        .collect();
    let owner_socket = socket.clone();
    let owner = thread::spawn(move || {
        let started = Instant::now();
        let mut client = TestClient::connect(&owner_socket);
        let _ = client.create_work_scope(WorkScopeKind::AdHoc);
        started.elapsed()
    });

    let stalled_workers: Vec<_> = (0..stalled.len())
        .map(|_| daemon.accept_and_spawn().expect("admit stalled peer"))
        .collect();
    let owner_worker = daemon.accept_and_spawn().expect("admit owner");
    let owner_elapsed = owner.join().unwrap();
    assert!(
        owner_elapsed < handshake_timeout,
        "owner admission waited on stalled Hello reads: {owner_elapsed:?}"
    );
    assert!(owner_worker.join().unwrap().is_ok());

    drop(stalled);
    for worker in stalled_workers {
        let result = worker.join().unwrap();
        assert!(
            result
                .as_ref()
                .err()
                .is_some_and(|error| error.is_recoverable_client_fault()),
            "stalled peer must end as a client fault: {result:?}"
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// AC3: two concurrent slow (bounded-window) subscribers both see explicit
/// HistoryGap after retention truncation under `accept_and_spawn`.
#[test]
fn dual_slow_subscribers_both_receive_history_gap_under_accept_and_spawn() {
    let dir = temp_dir("concurrent-dual-gap");
    let mut daemon = AgentDaemon::bind_integration_with_script(
        &dir,
        IntegrationConfig {
            store_path: dir.join("agent.db"),
        },
        vec![
            ScriptStep::Emit(HostObservationKind::Started),
            ScriptStep::Emit(HostObservationKind::Progress { step: 1 }),
            ScriptStep::Emit(HostObservationKind::KnownSuccess),
        ],
    )
    .unwrap();
    let socket = daemon.socket_path();

    let owner = thread::spawn({
        let socket = socket.clone();
        move || {
            let mut client = TestClient::connect_window(&socket, 1);
            let scope = client.create_work_scope(WorkScopeKind::Project);
            let item = client.create_work_item(scope);
            let attempt = client.create_attempt(item);
            let started = client.start_agent_run(attempt);
            // Bounded window: first page is a single event, proving pull is capped.
            let first = client.subscribe(AggregateRef::AgentRun(started.run_id), None);
            match first {
                CommandResult::Replay { ref events } => assert_eq!(events.len(), 1),
                other => panic!("bounded first page: {other:?}"),
            }
            started.run_id
        }
    });
    assert!(daemon.accept_and_spawn().unwrap().join().unwrap().is_ok());
    let run_id = owner.join().unwrap();

    AgentStore::open(dir.join("agent.db"))
        .unwrap()
        .drop_events_before(
            AggregateId::AgentRun(run_id),
            AggregateSequence::from_raw(4).unwrap(),
        )
        .unwrap();

    let expected_gap = CommandResult::Gap {
        requested_after: 1,
        earliest_available: 4,
        current_snapshot_sequence: Some(4),
    };

    let (live_tx, live_rx) = mpsc::channel();
    let (go_a_tx, go_a_rx) = mpsc::channel();
    let (go_b_tx, go_b_rx) = mpsc::channel();
    let sub_a = thread::spawn({
        let socket = socket.clone();
        let live_tx = live_tx.clone();
        move || {
            let mut client = TestClient::connect_window(&socket, 1);
            live_tx.send("window").unwrap();
            recv_barrier(&go_a_rx, BARRIER, "dual-gap-release-window");
            let gap = client.subscribe(AggregateRef::AgentRun(run_id), None);
            let tail = client.subscribe(AggregateRef::AgentRun(run_id), Some(3));
            (gap, tail)
        }
    });
    let sub_b = thread::spawn({
        let socket = socket.clone();
        move || {
            let mut client = TestClient::connect_observer(&socket);
            live_tx.send("observer").unwrap();
            recv_barrier(&go_b_rx, BARRIER, "dual-gap-release-observer");
            let gap = client.subscribe(AggregateRef::AgentRun(run_id), None);
            let tail = client.subscribe(AggregateRef::AgentRun(run_id), Some(3));
            (gap, tail)
        }
    });

    let h1 = daemon.accept_and_spawn().unwrap();
    let h2 = daemon.accept_and_spawn().unwrap();
    let mut live = vec![
        recv_barrier(&live_rx, BARRIER, "dual-gap-both-live"),
        recv_barrier(&live_rx, BARRIER, "dual-gap-both-live"),
    ];
    live.sort_unstable();
    assert_eq!(
        live,
        vec!["observer", "window"],
        "barrier dual-gap-both-live"
    );
    go_a_tx.send(()).unwrap();
    go_b_tx.send(()).unwrap();
    let (gap_a, tail_a) = sub_a.join().unwrap();
    let (gap_b, tail_b) = sub_b.join().unwrap();
    assert!(h1.join().unwrap().is_ok());
    assert!(h2.join().unwrap().is_ok());

    assert_eq!(gap_a, expected_gap, "slow owner-window peer must Gap");
    assert_eq!(gap_b, expected_gap, "concurrent observer peer must Gap");
    match (&tail_a, &tail_b) {
        (CommandResult::Replay { events: a }, CommandResult::Replay { events: b }) => {
            assert_eq!(a.iter().map(|e| e.sequence).collect::<Vec<_>>(), vec![4]);
            assert_eq!(b.iter().map(|e| e.sequence).collect::<Vec<_>>(), vec![4]);
        }
        other => panic!("both peers must read the retained tail: {other:?}"),
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// Peer A writes Subscribe frames until its own write blocks. That happens
/// only after the daemon stops reading because it is blocked writing A's
/// response, independent of the platform socket-buffer size. Peer B's
/// commands must each finish within 1s during that block, and A's worker
/// must end `Err(TimedOut)`.
#[test]
fn stalled_reader_does_not_stall_other_peer() {
    let dir = temp_dir("concurrent-stalled-reader");
    let config = DaemonConfig {
        session_write_timeout: Duration::from_secs(3),
        session_idle_timeout: Duration::from_secs(30),
        ..DaemonConfig::default()
    };
    let mut daemon = AgentDaemon::bind_integration_with_script_config(
        &dir,
        config,
        IntegrationConfig {
            store_path: dir.join("agent.db"),
        },
        vec![
            ScriptStep::Emit(HostObservationKind::Started),
            ScriptStep::Emit(HostObservationKind::Result(vec![9; 24 * 1024])),
        ],
    )
    .unwrap();
    let socket = daemon.socket_path();

    let setup_socket = socket.clone();
    let setup = thread::spawn(move || {
        let mut client = TestClient::connect(&setup_socket);
        let scope = client.create_work_scope(WorkScopeKind::Repository);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        client.start_agent_run(attempt).run_id
    });
    assert!(daemon.accept_and_spawn().unwrap().join().unwrap().is_ok());
    let run_id = setup.join().unwrap();

    let (live_tx, live_rx) = mpsc::channel();
    let (go_a_tx, go_a_rx) = mpsc::channel();
    let (go_b_tx, go_b_rx) = mpsc::channel();
    let (sent_tx, sent_rx) = mpsc::channel();
    let (probes_tx, probes_rx) = mpsc::channel();
    let (release_a_tx, release_a_rx) = mpsc::channel();

    let a_socket = socket.clone();
    let a_live = live_tx.clone();
    let stalled = thread::spawn(move || {
        let mut client = TestClient::connect_limits(&a_socket, ABSOLUTE_MAX_FRAME_SIZE, 32);
        a_live.send("stalled").unwrap();
        recv_barrier(&go_a_rx, BARRIER, "stalled-reader-release-a");
        let subscribe = encode_command(
            &Command::Subscribe {
                session_id: client.session_id,
                aggregate: AggregateRef::AgentRun(run_id),
                after: None,
            },
            ABSOLUTE_MAX_FRAME_SIZE,
        )
        .unwrap();
        client
            .stream
            .set_write_timeout(Some(Duration::from_millis(250)))
            .unwrap();
        let mut blocked = false;
        for _ in 0..100_000 {
            match std::io::Write::write_all(&mut client.stream, &subscribe) {
                Ok(()) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    blocked = true;
                    break;
                }
                Err(error) => panic!("barrier stalled-reader-fill: {error}"),
            }
        }
        assert!(blocked, "barrier stalled-reader-fill: daemon kept reading");
        sent_tx.send(()).unwrap();
        recv_barrier(
            &release_a_rx,
            Duration::from_secs(10),
            "stalled-reader-release",
        );
    });

    let b_socket = socket.clone();
    let prober = thread::spawn(move || {
        let mut client = TestClient::connect(&b_socket);
        client
            .stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        live_tx.send("prober").unwrap();
        recv_barrier(&go_b_rx, BARRIER, "stalled-reader-release-b");
        recv_barrier(&sent_rx, BARRIER, "stalled-subscribe-sent");
        let mut samples = Vec::new();
        for i in 0..3 {
            let started = Instant::now();
            let _ = client.create_work_scope(WorkScopeKind::AdHoc);
            let took = started.elapsed();
            assert!(
                took <= Duration::from_secs(1),
                "barrier stalled-reader-probe-{i} took {took:?}"
            );
            samples.push(took);
        }
        probes_tx.send(samples).unwrap();
    });

    let h1 = daemon.accept_and_spawn().unwrap();
    let h2 = daemon.accept_and_spawn().unwrap();
    let mut live = vec![
        recv_barrier(&live_rx, BARRIER, "stalled-reader-both-live"),
        recv_barrier(&live_rx, BARRIER, "stalled-reader-both-live"),
    ];
    live.sort_unstable();
    assert_eq!(
        live,
        vec!["prober", "stalled"],
        "barrier stalled-reader-both-live"
    );
    go_a_tx.send(()).unwrap();
    go_b_tx.send(()).unwrap();

    let samples = recv_barrier(&probes_rx, Duration::from_secs(5), "stalled-reader-probes");
    assert_eq!(samples.len(), 3);
    prober.join().unwrap();
    let mut results = vec![h1.join().unwrap(), h2.join().unwrap()];
    results.sort_by_key(|result| match result {
        Ok(()) => 0,
        Err(DaemonError::TimedOut) => 1,
        Err(_) => 2,
    });
    assert_eq!(
        results,
        vec![Ok(()), Err(DaemonError::TimedOut)],
        "barrier stalled-reader-results"
    );
    release_a_tx.send(()).unwrap();
    stalled.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

/// `serve_one` accepts the next peer only after the current session ends, so
/// two sessions cannot be open together.
#[test]
fn serial_serve_one_cannot_satisfy_two_live_rendezvous() {
    let dir = temp_dir("serial-rendezvous");
    let mut daemon = AgentDaemon::bind_integration_with_script(
        &dir,
        IntegrationConfig {
            store_path: dir.join("agent.db"),
        },
        script(),
    )
    .unwrap();
    let socket = daemon.socket_path();
    let (live_tx, live_rx) = mpsc::channel();
    let (release_a_tx, release_a_rx) = mpsc::channel();
    let (release_b_tx, release_b_rx) = mpsc::channel();

    let first_socket = socket.clone();
    let first_live = live_tx.clone();
    let first = thread::spawn(move || {
        hold_until_released(&first_socket, &first_live, &release_a_rx);
    });
    let second = thread::spawn(move || {
        hold_until_released(&socket, &live_tx, &release_b_rx);
    });
    let server = thread::spawn(move || daemon.serve_one());

    recv_barrier(&live_rx, BARRIER, "serial-first-open");
    match live_rx.recv_timeout(Duration::from_secs(1)) {
        Err(mpsc::RecvTimeoutError::Timeout) => {}
        Ok(()) => panic!("barrier two-live unexpectedly satisfied"),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("barrier two-live disconnected")
        }
    }
    release_a_tx.send(()).unwrap();
    release_b_tx.send(()).unwrap();
    assert!(server.join().unwrap().is_ok(), "barrier serial-serve-one");
    first.join().unwrap();
    second.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

fn hold_until_released(
    socket: &std::path::Path,
    live_tx: &mpsc::Sender<()>,
    release_rx: &mpsc::Receiver<()>,
) {
    if let Ok(stream) = try_open_session(socket) {
        live_tx.send(()).unwrap();
        let _ = release_rx.recv_timeout(Duration::from_secs(5));
        drop(stream);
    }
}

fn try_open_session(path: &std::path::Path) -> Result<UnixStream, String> {
    use std::io::Write;

    let mut stream = UnixStream::connect(path).map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| error.to_string())?;
    let hello = Hello {
        supported_versions: vec![ProtocolVersion::V1],
        max_frame_size: 4096,
        event_window: 32,
        client_principal_evidence: Vec::new(),
    };
    let hello_bytes =
        encode_hello(&hello, ABSOLUTE_MAX_FRAME_SIZE).map_err(|_| "encode hello".to_string())?;
    stream
        .write_all(&hello_bytes)
        .map_err(|error| error.to_string())?;
    read_kind(&mut stream)?;
    let open = encode_command(
        &Command::OpenSession {
            scopes: vec![1, 2, 4],
        },
        ABSOLUTE_MAX_FRAME_SIZE,
    )
    .map_err(|_| "encode open".to_string())?;
    stream.write_all(&open).map_err(|error| error.to_string())?;
    if read_kind(&mut stream)? != FrameKind::Result {
        return Err("open session was not a result".to_string());
    }
    Ok(stream)
}

fn read_kind(stream: &mut UnixStream) -> Result<FrameKind, String> {
    use std::io::Read;

    let mut header = [0_u8; 10];
    stream
        .read_exact(&mut header)
        .map_err(|error| error.to_string())?;
    let body_len = seyal_agent_protocol::accepted_body_len(&header, ABSOLUTE_MAX_FRAME_SIZE)
        .map_err(|_| "bad header".to_string())?;
    let mut body = vec![0_u8; body_len];
    if body_len > 0 {
        stream
            .read_exact(&mut body)
            .map_err(|error| error.to_string())?;
    }
    let mut bytes = header.to_vec();
    bytes.extend_from_slice(&body);
    seyal_agent_protocol::decode_frame(&bytes, ABSOLUTE_MAX_FRAME_SIZE)
        .map(|frame| frame.kind)
        .map_err(|_| "decode".to_string())
}
