//! Bounds that the serial daemon must keep: idle, stalled writes, and replay size.

mod support;

use std::{
    thread,
    time::{Duration, Instant},
};

use seyal_agent_backend::{
    AgentDaemon, DaemonConfig, DaemonError, HostObservationKind, IntegrationConfig, ScriptStep,
};
use seyal_agent_core::WorkScopeKind;
use seyal_agent_protocol::{
    AggregateRef, Command, CommandError, CommandResult, ABSOLUTE_MAX_FRAME_SIZE,
};
use seyal_agent_store::AgentStore;
use support::*;

fn bounded_config(idle: Duration, write: Duration) -> DaemonConfig {
    DaemonConfig {
        session_idle_timeout: idle,
        session_write_timeout: write,
        ..DaemonConfig::default()
    }
}

#[test]
fn slow_subscriber_is_dropped_and_resyncs_from_cursor() {
    let dir = temp_dir("slow-sub");
    let mut script = vec![ScriptStep::Emit(HostObservationKind::Started)];
    for step in 0..128 {
        script.push(ScriptStep::Emit(HostObservationKind::Result(vec![
            (step % 251) as u8;
            1024
        ])));
    }
    let mut daemon = AgentDaemon::bind_integration_with(
        &dir,
        bounded_config(Duration::from_secs(30), Duration::from_millis(200)),
        IntegrationConfig {
            store_path: dir.join("agent.db"),
            script,
        },
    )
    .unwrap();
    let socket = daemon.socket_path();
    let creator = thread::spawn(move || {
        let mut client = TestClient::connect(&socket);
        let scope = client.create_work_scope(WorkScopeKind::Repository);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        let started = client.start_agent_run(attempt);
        (client.session_id, started.run_id, started.event_count)
    });
    daemon.serve_one().unwrap();
    let (session_id, run_id, event_count) = creator.join().unwrap();

    for _ in 0..3 {
        let socket = daemon.socket_path();
        let stalled = thread::spawn(move || {
            let mut client =
                TestClient::resume_limits(&socket, session_id, ABSOLUTE_MAX_FRAME_SIZE, 1024)
                    .unwrap();
            for _ in 0..64 {
                client.send_unanswered(&Command::Subscribe {
                    session_id,
                    aggregate: AggregateRef::AgentRun(run_id),
                    after: None,
                });
            }
            thread::sleep(Duration::from_secs(2));
        });
        let started = Instant::now();
        let result = daemon.serve_one();
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "stalled subscriber was not dropped"
        );
        assert_eq!(result, Err(DaemonError::TimedOut));
        let _ = stalled.join();
    }

    let socket = daemon.socket_path();
    let resumed = thread::spawn(move || {
        let mut client =
            TestClient::resume_limits(&socket, session_id, ABSOLUTE_MAX_FRAME_SIZE, 64).unwrap();
        client
            .replay_all(AggregateRef::AgentRun(run_id))
            .into_iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>()
    });
    daemon.serve_one().unwrap();
    let sequences = resumed.join().unwrap();
    assert_eq!(sequences, (1..=event_count).collect::<Vec<_>>());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn idle_authenticated_client_is_released_and_session_survives() {
    let dir = temp_dir("idle");
    let mut daemon = AgentDaemon::bind_integration_with(
        &dir,
        bounded_config(Duration::from_millis(200), Duration::from_secs(2)),
        IntegrationConfig {
            store_path: dir.join("agent.db"),
            script: vec![ScriptStep::Emit(HostObservationKind::Started)],
        },
    )
    .unwrap();
    let socket = daemon.socket_path();
    let idle = thread::spawn(move || {
        let client = TestClient::connect(&socket);
        thread::sleep(Duration::from_secs(1));
        client.session_id
    });
    let started = Instant::now();
    assert_eq!(daemon.serve_one(), Ok(()));
    assert!(
        started.elapsed() < Duration::from_millis(900),
        "idle client held the daemon"
    );
    let session_id = idle.join().unwrap();
    let socket = daemon.socket_path();
    let resumed = thread::spawn(move || {
        TestClient::resume(&socket, session_id).map(|client| client.session_id)
    });
    daemon.serve_one().unwrap();
    assert_eq!(resumed.join().unwrap(), Ok(session_id));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn subscribe_fails_explicitly_when_one_event_cannot_fit() {
    let dir = temp_dir("tiny-frame");
    let mut daemon = AgentDaemon::bind_integration(
        &dir,
        IntegrationConfig {
            store_path: dir.join("agent.db"),
            script: vec![
                ScriptStep::Emit(HostObservationKind::Started),
                ScriptStep::Emit(HostObservationKind::Result(vec![4; 2048])),
            ],
        },
    )
    .unwrap();
    let socket = daemon.socket_path();
    let client = thread::spawn(move || {
        let mut client = TestClient::connect_limits(&socket, 512, 32);
        let scope = client.create_work_scope(WorkScopeKind::Repository);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        let started = client.start_agent_run(attempt);
        let first = client.subscribe(AggregateRef::AgentRun(started.run_id), None);
        let next = match &first {
            CommandResult::Replay { events } => client.subscribe(
                AggregateRef::AgentRun(started.run_id),
                events.last().map(|event| event.sequence),
            ),
            other => panic!("first page: {other:?}"),
        };
        (first, next)
    });
    daemon.serve_one().unwrap();
    let (first, next) = client.join().unwrap();
    match first {
        CommandResult::Replay { events } => assert!(!events.is_empty()),
        other => panic!("first page: {other:?}"),
    }
    assert_eq!(next, CommandResult::Error(CommandError::Failed));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn unreplayable_observation_is_never_persisted() {
    let dir = temp_dir("too-big");
    let mut daemon = AgentDaemon::bind_integration(
        &dir,
        IntegrationConfig {
            store_path: dir.join("agent.db"),
            script: vec![
                ScriptStep::Emit(HostObservationKind::Started),
                ScriptStep::Emit(HostObservationKind::Result(vec![0; 65_520])),
            ],
        },
    )
    .unwrap();
    let socket = daemon.socket_path();
    let client = thread::spawn(move || {
        let mut client = TestClient::connect(&socket);
        let scope = client.create_work_scope(WorkScopeKind::Repository);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        let started = client.command(&Command::StartAgentRun {
            session_id: client.session_id,
            attempt_id: attempt,
        });
        (client.session_id, started)
    });
    daemon.serve_one().unwrap();
    let (_session, started) = client.join().unwrap();
    assert_eq!(started, CommandResult::Error(CommandError::Failed));
    drop(daemon);
    let store = AgentStore::open(dir.join("agent.db")).unwrap();
    let runs = store.agent_runs().unwrap();
    assert_eq!(runs.len(), 1);
    let events = store
        .replay_after(seyal_agent_store::AggregateId::AgentRun(runs[0].0), None)
        .unwrap();
    assert!(events.iter().all(|event| event.payload.len() < 65_520));
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(feature = "test-fault-injection")]
#[test]
fn repeated_store_faults_fail_bounded_and_recover() {
    let dir = temp_dir("repeat-fault");
    let mut daemon = AgentDaemon::bind_integration(
        &dir,
        IntegrationConfig {
            store_path: dir.join("agent.db"),
            script: vec![ScriptStep::Emit(HostObservationKind::Started)],
        },
    )
    .unwrap();
    daemon.fail_after_writes(0);
    let socket = daemon.socket_path();
    let client = thread::spawn(move || {
        let mut client = TestClient::connect(&socket);
        let mut results = Vec::new();
        for _ in 0..5 {
            results.push(client.command(&Command::CreateWorkScope {
                session_id: client.session_id,
                kind: WorkScopeKind::Repository,
            }));
        }
        results
    });
    daemon.serve_one().unwrap();
    let results = client.join().unwrap();
    assert_eq!(results.len(), 5);
    assert!(results
        .iter()
        .all(|result| *result == CommandResult::Error(CommandError::Failed)));
    daemon.fail_after_writes(u64::MAX);
    let socket = daemon.socket_path();
    let created = thread::spawn(move || {
        let mut client = TestClient::connect(&socket);
        client.create_work_scope(WorkScopeKind::Repository)
    });
    daemon.serve_one().unwrap();
    let _ = created.join().unwrap();
    drop(daemon);
    let scopes = AgentStore::open(dir.join("agent.db"))
        .unwrap()
        .work_scopes()
        .unwrap();
    assert_eq!(scopes.len(), 1);
    let _ = std::fs::remove_dir_all(dir);
}
