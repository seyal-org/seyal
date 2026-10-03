//! Cross-principal session use over a real daemon socket (AB-1.7 / #1194).
//!
//! Helpers stay in this file. Ordering uses bounded channels, not sleeps.

#![cfg(unix)]

mod support;

use std::{sync::mpsc, thread, time::Duration};

use seyal_agent_backend::{AgentDaemon, HostObservationKind, IntegrationConfig, ScriptStep};
use seyal_agent_core::WorkScopeKind;
use seyal_agent_protocol::{ClientSessionId, Command, CommandError, CommandResult};
use seyal_agent_store::AgentStore;

use support::{temp_dir, TestClient};

const BARRIER_TIMEOUT: Duration = Duration::from_secs(10);

fn script() -> Vec<ScriptStep> {
    vec![
        ScriptStep::Emit(HostObservationKind::Started),
        ScriptStep::Emit(HostObservationKind::KnownSuccess),
    ]
}

fn session_at_sequence(template: ClientSessionId, sequence: u64) -> ClientSessionId {
    let mut bytes = template.to_bytes();
    bytes[..8].copy_from_slice(&sequence.to_le_bytes());
    ClientSessionId::from_bytes(bytes)
}

#[test]
fn observer_connection_cannot_use_owner_session_while_both_live() {
    let dir = temp_dir("session-principal-both-live");
    let mut daemon = AgentDaemon::bind_integration_with_script(
        &dir,
        IntegrationConfig {
            store_path: dir.join("agent.db"),
        },
        script(),
    )
    .unwrap();
    let socket = daemon.socket_path();
    let (owner_open_tx, owner_open_rx) = mpsc::channel();
    let (observer_open_tx, observer_open_rx) = mpsc::channel();
    let (run_ready_tx, run_ready_rx) = mpsc::channel();
    let (observer_done_tx, observer_done_rx) = mpsc::channel();

    let owner_socket = socket.clone();
    let owner = thread::spawn(move || {
        let mut client = TestClient::connect(&owner_socket);
        owner_open_tx
            .send(client.session_id)
            .expect("barrier: owner session published");
        observer_open_rx
            .recv_timeout(BARRIER_TIMEOUT)
            .expect("barrier: observer session open");
        let scope = client.create_work_scope(WorkScopeKind::Repository);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        let started = client.start_agent_run(attempt);
        run_ready_tx
            .send(started.run_id)
            .expect("barrier: owner run published");
        observer_done_rx
            .recv_timeout(BARRIER_TIMEOUT)
            .expect("barrier: observer finished");
    });

    let observer_socket = socket.clone();
    let observer = thread::spawn(move || {
        let owner_session = owner_open_rx
            .recv_timeout(BARRIER_TIMEOUT)
            .expect("barrier: owner session open");
        let mut client = TestClient::connect_observer(&observer_socket);
        observer_open_tx
            .send(())
            .expect("barrier: observer session published");
        match client.command(&Command::CreateWorkScope {
            session_id: owner_session,
            kind: WorkScopeKind::AdHoc,
        }) {
            CommandResult::Error(CommandError::RejectedSession) => {}
            other => panic!("observer use of owner session: {other:?}"),
        }
        let run_id = run_ready_rx
            .recv_timeout(BARRIER_TIMEOUT)
            .expect("barrier: owner run ready");
        let _state = client.read_run(run_id);
        observer_done_tx
            .send(())
            .expect("barrier: observer done published");
    });

    let owner_worker = daemon.accept_and_spawn().expect("accept owner");
    let observer_worker = daemon.accept_and_spawn().expect("accept observer");
    owner.join().expect("owner thread");
    observer.join().expect("observer thread");
    assert!(owner_worker.join().unwrap().is_ok());
    assert!(observer_worker.join().unwrap().is_ok());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn derived_session_ids_grant_no_authority() {
    let dir = temp_dir("derived-session-ids");
    let store_path = dir.join("agent.db");
    let mut daemon = AgentDaemon::bind_integration_with_script(
        &dir,
        IntegrationConfig {
            store_path: store_path.clone(),
        },
        script(),
    )
    .unwrap();
    let socket = daemon.socket_path();
    let owner = thread::spawn(move || TestClient::connect(&socket).session_id);
    daemon.serve_one().unwrap();
    let owner_session = owner.join().unwrap();
    let before = AgentStore::open(&store_path)
        .unwrap()
        .work_scopes()
        .unwrap()
        .len();

    let socket = daemon.socket_path();
    let observer = thread::spawn(move || {
        let mut client = TestClient::connect_observer(&socket);
        let own = client.session_id;
        let sequence = u64::from_le_bytes(own.to_bytes()[..8].try_into().unwrap());
        let owner_in_window =
            (1..=64).any(|k| session_at_sequence(own, sequence.wrapping_sub(k)) == owner_session);
        assert!(
            owner_in_window,
            "owner session must fall in seq-1..=seq-64 of the observer session; owner={owner_session} observer={own}"
        );
        for k in 1..=64 {
            let derived = session_at_sequence(own, sequence.wrapping_sub(k));
            match client.command(&Command::CreateWorkScope {
                session_id: derived,
                kind: WorkScopeKind::AdHoc,
            }) {
                CommandResult::Error(CommandError::RejectedSession) => {}
                other => panic!("derived session k={k}: {other:?}"),
            }
        }
    });
    daemon.serve_one().unwrap();
    observer.join().unwrap();

    let after = AgentStore::open(&store_path)
        .unwrap()
        .work_scopes()
        .unwrap()
        .len();
    assert_eq!(
        after, before,
        "derived session ids must not create work scopes"
    );
    let _ = std::fs::remove_dir_all(dir);
}
