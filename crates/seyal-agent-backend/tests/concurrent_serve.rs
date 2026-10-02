//! Concurrent multi-client serve (AB-1.3 / #1179).
//!
//! Two live authorized peers against one daemon incarnation; observer cannot
//! escalate; peer fault does not stall the other connection.

#![cfg(unix)]

mod support;

use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

use seyal_agent_backend::{
    AgentDaemon, DaemonConfig, HostObservationKind, IntegrationConfig, ScriptStep,
};
use seyal_agent_core::WorkScopeKind;
use seyal_agent_protocol::{AggregateRef, Command, CommandError, CommandResult};
use seyal_agent_store::{AgentStore, AggregateId, AggregateSequence};

use support::{temp_dir, TestClient};

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
    let mut daemon = AgentDaemon::bind_integration(
        &dir,
        IntegrationConfig {
            store_path: dir.join("agent.db"),
            script: script(),
        },
    )
    .unwrap();
    let socket = daemon.socket_path();

    let owner_socket = socket.clone();
    let owner = thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        let mut client = TestClient::connect(&owner_socket);
        let scope = client.create_work_scope(WorkScopeKind::Repository);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        let started = client.start_agent_run(attempt);
        let replay = client.replay_all(AggregateRef::AgentRun(started.run_id));
        (started.run_id, started.event_count, replay.len())
    });

    let observer_socket = socket.clone();
    let observer = thread::spawn(move || {
        thread::sleep(Duration::from_millis(40));
        let mut client = TestClient::connect_observer(&observer_socket);
        match client.command(&Command::CreateWorkScope {
            session_id: client.session_id,
            kind: WorkScopeKind::AdHoc,
        }) {
            CommandResult::Error(CommandError::Denied) => {}
            other => panic!("observer create must be denied: {other:?}"),
        }
        client.session_id
    });

    let h1 = daemon.accept_and_spawn().expect("spawn first peer worker");
    let h2 = daemon.accept_and_spawn().expect("spawn second peer worker");

    let (run_id, event_count, replay_len) = owner.join().unwrap();
    let _ = observer.join().unwrap();
    assert!(h1.join().unwrap().is_ok());
    assert!(h2.join().unwrap().is_ok());
    assert_eq!(replay_len as u64, event_count);

    let observe = thread::spawn({
        let socket = socket.clone();
        move || {
            thread::sleep(Duration::from_millis(10));
            let mut client = TestClient::connect_observer(&socket);
            client.replay_all(AggregateRef::AgentRun(run_id)).len() as u64
        }
    });
    daemon.serve_one().unwrap();
    assert_eq!(observe.join().unwrap(), event_count);

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn observer_cannot_escalate_while_owner_is_live() {
    let dir = temp_dir("concurrent-no-escalate");
    let mut daemon = AgentDaemon::bind_integration(
        &dir,
        IntegrationConfig {
            store_path: dir.join("agent.db"),
            script: script(),
        },
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
    let dir = temp_dir("concurrent-peer-drop");
    let mut daemon = AgentDaemon::bind_integration(
        &dir,
        IntegrationConfig {
            store_path: dir.join("agent.db"),
            script: script(),
        },
    )
    .unwrap();
    let socket = daemon.socket_path();

    let owner_socket = socket.clone();
    let owner = thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        let mut client = TestClient::connect(&owner_socket);
        let scope = client.create_work_scope(WorkScopeKind::Repository);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        let started = client.start_agent_run(attempt);
        thread::sleep(Duration::from_millis(80));
        let snap = client.snapshot(AggregateRef::AgentRun(started.run_id));
        assert_eq!(snap.incorporated_through, started.event_count);
        started.event_count
    });

    let drop_socket = socket.clone();
    let dropper = thread::spawn(move || {
        thread::sleep(Duration::from_millis(40));
        let client = TestClient::connect_observer(&drop_socket);
        drop(client);
    });

    let h1 = daemon.accept_and_spawn().unwrap();
    let h2 = daemon.accept_and_spawn().unwrap();
    dropper.join().unwrap();
    let event_count = owner.join().unwrap();
    assert!(event_count > 0);
    assert!(h1.join().unwrap().is_ok());
    let peer = h2.join().unwrap();
    assert!(peer.is_ok(), "observer disconnect is normal: {peer:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn repeated_peer_fault_does_not_stall_unrelated_connection() {
    let dir = temp_dir("concurrent-fault-n");
    let config = DaemonConfig {
        session_idle_timeout: Duration::from_secs(5),
        session_write_timeout: Duration::from_secs(2),
        ..DaemonConfig::default()
    };
    let mut daemon = AgentDaemon::bind_integration_with(
        &dir,
        config,
        IntegrationConfig {
            store_path: dir.join("agent.db"),
            script: script(),
        },
    )
    .unwrap();
    let socket = daemon.socket_path();
    let progress = Arc::new(AtomicU64::new(0));

    let owner_progress = Arc::clone(&progress);
    let owner_socket = socket.clone();
    let owner = thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        let mut client = TestClient::connect(&owner_socket);
        for i in 0..8 {
            let _ = client.create_work_scope(WorkScopeKind::AdHoc);
            owner_progress.store(i + 1, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(15));
        }
        owner_progress.load(Ordering::SeqCst)
    });

    let fault_socket = socket.clone();
    let fault = thread::spawn(move || {
        use std::io::Write;
        thread::sleep(Duration::from_millis(40));
        // One live peer that repeatedly sends malformed post-handshake frames.
        let mut client = TestClient::connect_observer(&fault_socket);
        for _ in 0..5 {
            let _ = client.stream.write_all(b"BAD!\x00\x00\x00\x00\x00\x00");
            thread::sleep(Duration::from_millis(10));
        }
        drop(client);
    });

    let h_owner = daemon.accept_and_spawn().unwrap();
    let h_fault = daemon.accept_and_spawn().unwrap();
    let seen = owner.join().unwrap();
    let _ = fault.join();
    assert_eq!(seen, 8, "owner must keep making progress under peer faults");
    assert!(h_owner.join().unwrap().is_ok());
    let fault_result = h_fault.join().unwrap();
    assert!(
        fault_result.is_ok()
            || fault_result
                .as_ref()
                .err()
                .is_some_and(|e| e.is_recoverable_client_fault()),
        "fault peer must not take down the daemon: {fault_result:?}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// AC3: two concurrent slow (bounded-window) subscribers both see explicit
/// HistoryGap after retention truncation under `accept_and_spawn`.
#[test]
fn dual_slow_subscribers_both_receive_history_gap_under_accept_and_spawn() {
    let dir = temp_dir("concurrent-dual-gap");
    let mut daemon = AgentDaemon::bind_integration(
        &dir,
        IntegrationConfig {
            store_path: dir.join("agent.db"),
            script: vec![
                ScriptStep::Emit(HostObservationKind::Started),
                ScriptStep::Emit(HostObservationKind::Progress { step: 1 }),
                ScriptStep::Emit(HostObservationKind::KnownSuccess),
            ],
        },
    )
    .unwrap();
    let socket = daemon.socket_path();

    let owner = thread::spawn({
        let socket = socket.clone();
        move || {
            thread::sleep(Duration::from_millis(20));
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

    let sub_a = thread::spawn({
        let socket = socket.clone();
        move || {
            thread::sleep(Duration::from_millis(20));
            let mut client = TestClient::connect_window(&socket, 1);
            let gap = client.subscribe(AggregateRef::AgentRun(run_id), None);
            let tail = client.subscribe(AggregateRef::AgentRun(run_id), Some(3));
            (gap, tail)
        }
    });
    let sub_b = thread::spawn({
        let socket = socket.clone();
        move || {
            thread::sleep(Duration::from_millis(40));
            let mut client = TestClient::connect_observer(&socket);
            let gap = client.subscribe(AggregateRef::AgentRun(run_id), None);
            let tail = client.subscribe(AggregateRef::AgentRun(run_id), Some(3));
            (gap, tail)
        }
    });

    let h1 = daemon.accept_and_spawn().unwrap();
    let h2 = daemon.accept_and_spawn().unwrap();
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
