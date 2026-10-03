//! Standalone client path over authenticated local IPC.
//!
//! The client crate cannot depend on the backend, and the backend cannot depend
//! on the client. This test speaks the same V1 command frames as `SessionClient`.

use std::{
    fs,
    io::Write,
    path::Path,
    thread,
    time::{Duration, Instant},
};

use seyal_agent_backend::{AgentDaemon, HostObservationKind, IntegrationConfig, ScriptStep};
use seyal_agent_core::{BindingGeneration, WorkScopeKind};
use seyal_agent_protocol::{
    encode_frame, encode_result, AgentRunId, AggregateRef, ClientSessionId, Command, CommandError,
    CommandResult, FrameKind, ABSOLUTE_MAX_FRAME_SIZE,
};
use seyal_agent_store::{AgentStore, AggregateId, AggregateSequence};

#[test]
fn standalone_path_survives_disconnect_and_restart() {
    let dir = std::env::temp_dir().join(format!(
        "seyal-agent-path-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let config_script = vec![
        ScriptStep::Emit(HostObservationKind::Started),
        ScriptStep::Emit(HostObservationKind::Progress { step: 1 }),
        ScriptStep::Emit(HostObservationKind::KnownSuccess),
    ];
    let config = IntegrationConfig {
        store_path: dir.join("agent.db"),
    };

    let mut daemon =
        AgentDaemon::bind_integration_with_script(&dir, config.clone(), config_script.clone())
            .unwrap();
    let first_instance = daemon.instance_id();
    let socket = daemon.socket_path();
    let socket_for_client = socket.clone();
    let client = thread::spawn(move || {
        let mut client = TestClient::connect(&socket_for_client);
        let scope = client.create_work_scope(WorkScopeKind::Repository);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        let started = client.start_agent_run(attempt);
        let snapshot = client.snapshot(AggregateRef::AgentRun(started.run_id));
        let replay = client.replay(AggregateRef::AgentRun(started.run_id));
        let liveness = client.read_run(started.run_id);
        (
            client.session_id,
            scope,
            item,
            attempt,
            started,
            snapshot,
            replay,
            liveness,
        )
    });
    daemon.serve_one().unwrap();
    let (session_id, scope, item, attempt, started, snapshot, replay, liveness) =
        client.join().unwrap();

    assert_eq!(started.binding_generation, 1);
    assert_eq!(started.control_generation, 1);
    assert_eq!(started.event_count, 4);
    assert_eq!(snapshot.incorporated_through, 4);
    assert_eq!(replay.len(), 4);
    assert_eq!(*replay.last().unwrap(), snapshot.incorporated_through);
    assert!(replay.windows(2).all(|pair| pair[1] == pair[0] + 1));
    assert_eq!(liveness.2, 2);
    assert!(socket.exists());

    let run_id = started.run_id;
    let socket_for_client = socket.clone();
    let resume_session = session_id;
    let resume_scope = scope;
    let resume_run = run_id;
    let resumed = thread::spawn(move || {
        let mut client = TestClient::resume(&socket_for_client, resume_session).unwrap();
        let snapshot = client.snapshot(AggregateRef::AgentRun(resume_run));
        let scope_events = client.replay(AggregateRef::WorkScope(resume_scope));
        (snapshot.incorporated_through, scope_events.len())
    });
    daemon.serve_one().unwrap();
    let (again, scope_events) = resumed.join().unwrap();
    assert_eq!(again, snapshot.incorporated_through);
    assert_eq!(scope_events, 1);
    assert_eq!(daemon.instance_id(), first_instance);

    daemon.abandon_as_crash();
    let mut restarted =
        AgentDaemon::bind_integration_with_script(&dir, config, config_script.clone()).unwrap();
    assert_ne!(restarted.instance_id(), first_instance);

    let socket = restarted.socket_path();
    let rejected = thread::spawn(move || TestClient::resume(&socket, session_id));
    restarted.serve_one().unwrap();
    match rejected.join().unwrap() {
        Err(CommandError::RejectedSession) => {}
        Ok(_) => panic!("old session was accepted after restart"),
        Err(other) => panic!("unexpected resume error: {other:?}"),
    }

    let socket = restarted.socket_path();
    let recovered = thread::spawn(move || {
        let mut client = TestClient::connect(&socket);
        let run = client.read_run(run_id);
        let snapshot = client.snapshot(AggregateRef::AgentRun(run_id));
        let replay_after_snapshot = client.subscribe(
            AggregateRef::AgentRun(run_id),
            Some(snapshot.incorporated_through),
        );
        let replay = client.replay(AggregateRef::AgentRun(run_id));
        let item_events = client.replay(AggregateRef::WorkItem(item));
        let attempt_events = client.replay(AggregateRef::Attempt(attempt));
        let pre_crash = client.check_generation(run_id, 1, 1);
        let stale_binding = client.check_generation(run_id, 3, 2);
        let stale_control = client.check_generation(run_id, 2, 3);
        let current = client.check_generation(run_id, 2, 2);
        (
            run,
            snapshot,
            replay_after_snapshot,
            replay.len(),
            item_events.len(),
            attempt_events.len(),
            pre_crash,
            stale_binding,
            stale_control,
            current,
        )
    });
    restarted.serve_one().unwrap();
    let (
        run,
        snapshot,
        replay_after_snapshot,
        events,
        items,
        attempts,
        pre_crash,
        stale_binding,
        stale_control,
        current,
    ) = recovered.join().unwrap();
    assert_eq!(run.0, 2, "recovery fences binding generation");
    assert_eq!(run.1, 2, "recovery fences control generation");
    assert_eq!(
        run.2, 2,
        "restart must honor committed terminal observations"
    );
    assert_eq!(
        snapshot.payload.first().copied(),
        Some(2),
        "GetSnapshot liveness must match ReadRun after restart"
    );
    assert_eq!(
        snapshot.payload.first().copied(),
        Some(run.2),
        "snapshot and ReadRun must agree on recovered liveness"
    );
    match replay_after_snapshot {
        CommandResult::Replay { events } => assert!(
            events.is_empty(),
            "snapshot+replay must not invent live progress after restart"
        ),
        other => panic!("unexpected replay after snapshot: {other:?}"),
    }
    // Full replay includes pre-crash observations plus the recovery fence event.
    assert_eq!(events, 5);
    assert_eq!(items, 1);
    assert_eq!(attempts, 1);
    assert_eq!(
        pre_crash,
        Err(CommandError::StaleBinding),
        "pre-crash generations must be denied after recovery fence"
    );
    assert_eq!(stale_binding, Err(CommandError::StaleBinding));
    assert_eq!(stale_control, Err(CommandError::StaleControl));
    assert_eq!(current, Ok(()));

    let manifest = include_str!("../Cargo.toml");
    for forbidden in ["seyal-runtime", "seyal-terminal", "seyal-render"] {
        assert!(!manifest.contains(forbidden), "{forbidden}");
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn recovery_without_terminal_observation_stays_unknown_not_fabricated_termination() {
    let dir = temp_dir("live-recover");
    let config_script = vec![
        ScriptStep::Emit(HostObservationKind::Started),
        ScriptStep::Emit(HostObservationKind::ObservationDisconnected),
    ];
    let config = IntegrationConfig {
        store_path: dir.join("agent.db"),
    };
    let mut daemon =
        AgentDaemon::bind_integration_with_script(&dir, config.clone(), config_script.clone())
            .unwrap();
    let socket = daemon.socket_path();
    let client = thread::spawn(move || {
        let mut client = TestClient::connect(&socket);
        let scope = client.create_work_scope(WorkScopeKind::AdHoc);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        let started = client.start_agent_run(attempt);
        let before = client.read_run(started.run_id);
        (started.run_id, before)
    });
    daemon.serve_one().unwrap();
    let (run_id, before) = client.join().unwrap();
    assert_eq!(before.2, 4, "pre-crash observation-lost");
    daemon.abandon_as_crash();

    let mut restarted =
        AgentDaemon::bind_integration_with_script(&dir, config, config_script.clone()).unwrap();
    let socket = restarted.socket_path();
    let recovered = thread::spawn(move || {
        let mut client = TestClient::connect(&socket);
        let run = client.read_run(run_id);
        let snapshot = client.snapshot(AggregateRef::AgentRun(run_id));
        (run, snapshot.payload)
    });
    restarted.serve_one().unwrap();
    let (run, snapshot_payload) = recovered.join().unwrap();
    assert_eq!(
        run.2, 3,
        "non-terminal recovery must stay UnknownAfterCrash"
    );
    assert_eq!(snapshot_payload.first().copied(), Some(3));
    assert_ne!(run.2, 2, "must not fabricate KnownTerminated");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn malformed_input_and_narrow_sessions_do_not_disturb_authority() {
    let dir = std::env::temp_dir().join(format!(
        "seyal-ag-m-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let config_script = vec![ScriptStep::Emit(HostObservationKind::Started)];
    let config = IntegrationConfig {
        store_path: dir.join("agent.db"),
    };
    let mut daemon =
        AgentDaemon::bind_integration_with_script(&dir, config.clone(), config_script.clone())
            .unwrap();
    let socket = daemon.socket_path();
    let socket_for_client = socket.clone();
    let opened = thread::spawn(move || {
        let mut client = TestClient::connect(&socket_for_client);
        let unknown = client.write_frame(
            &encode_frame(
                FrameKind::Command,
                &99_u16.to_le_bytes(),
                ABSOLUTE_MAX_FRAME_SIZE,
            )
            .unwrap(),
        );
        let bad_scope = client.command(&Command::OpenSession { scopes: vec![9] });
        let widened = client.command(&Command::OpenSession { scopes: vec![3] });
        let scope = client.create_work_scope(WorkScopeKind::Repository);
        let foreign = AggregateRef::AgentRun(AgentRunId::new());
        let snapshot = client.command(&Command::GetSnapshot {
            session_id: client.session_id,
            aggregate: foreign,
        });
        let replay = client.command(&Command::Subscribe {
            session_id: client.session_id,
            aggregate: foreign,
            after: None,
        });
        (
            client.session_id,
            unknown,
            bad_scope,
            widened,
            scope,
            snapshot,
            replay,
        )
    });
    daemon.serve_one().unwrap();
    let (session_id, unknown, bad_scope, widened, scope, snapshot, replay) = opened.join().unwrap();
    assert_eq!(unknown, CommandResult::Error(CommandError::Malformed));
    assert_eq!(bad_scope, CommandResult::Error(CommandError::Malformed));
    assert_eq!(widened, CommandResult::Error(CommandError::Denied));
    assert_eq!(snapshot, CommandResult::Error(CommandError::Denied));
    assert_eq!(replay, CommandResult::Error(CommandError::Denied));

    let socket = daemon.socket_path();
    let rejected = thread::spawn(move || TestClient::resume(&socket, ClientSessionId::new()));
    daemon.serve_one().unwrap();
    assert_eq!(
        rejected.join().unwrap().err(),
        Some(CommandError::RejectedSession)
    );

    let socket = daemon.socket_path();
    let narrowed = thread::spawn(move || {
        let mut client = TestClient::connect_with(&socket, vec![2]);
        let create = client.command(&Command::CreateWorkScope {
            session_id: client.session_id,
            kind: WorkScopeKind::Repository,
        });
        let control = client.check_generation(AgentRunId::new(), 1, 1);
        (create, control)
    });
    daemon.serve_one().unwrap();
    let (create, control) = narrowed.join().unwrap();
    assert_eq!(create, CommandResult::Error(CommandError::Denied));
    assert_eq!(control, Err(CommandError::Denied));

    let socket = daemon.socket_path();
    let broken = thread::spawn(move || {
        let mut stream = handshake(&socket);
        let mut header = [0_u8; 10];
        header[..4].copy_from_slice(b"BAD!");
        stream.write_all(&header).unwrap();
    });
    assert_eq!(
        daemon.serve_one(),
        Err(seyal_agent_backend::DaemonError::Malformed)
    );
    broken.join().unwrap();

    let socket = daemon.socket_path();
    let resume_session = session_id;
    let resume_scope = scope;
    let still_there = thread::spawn(move || {
        let mut client = TestClient::resume(&socket, resume_session).unwrap();
        client.replay(AggregateRef::WorkScope(resume_scope)).len()
    });
    daemon.serve_one().unwrap();
    assert_eq!(still_there.join().unwrap(), 1);

    daemon.abandon_as_crash();
    let mut restarted =
        AgentDaemon::bind_integration_with_script(&dir, config, config_script.clone()).unwrap();
    let socket = restarted.socket_path();
    let rejected = thread::spawn(move || TestClient::resume(&socket, session_id));
    restarted.serve_one().unwrap();
    assert_eq!(
        rejected.join().unwrap().err(),
        Some(CommandError::RejectedSession)
    );
    let socket = restarted.socket_path();
    let recovered = thread::spawn(move || {
        let mut client = TestClient::connect(&socket);
        client.replay(AggregateRef::WorkScope(scope)).len()
    });
    restarted.serve_one().unwrap();
    assert_eq!(recovered.join().unwrap(), 1);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn observers_keep_independent_sequences_and_ignore_duplicate_observations() {
    let dir = temp_dir("observers");
    let config_script = vec![
        ScriptStep::Emit(HostObservationKind::Started),
        ScriptStep::DuplicateLast,
        ScriptStep::Emit(HostObservationKind::Progress { step: 1 }),
        ScriptStep::Emit(HostObservationKind::KnownSuccess),
    ];
    let config = IntegrationConfig {
        store_path: dir.join("agent.db"),
    };
    let mut daemon =
        AgentDaemon::bind_integration_with_script(&dir, config, config_script.clone()).unwrap();
    let socket = daemon.socket_path();
    let client = thread::spawn(move || {
        let mut client = TestClient::connect(&socket);
        let scope = client.create_work_scope(WorkScopeKind::Repository);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        let started = client.start_agent_run(attempt);
        let run = client.replay(AggregateRef::AgentRun(started.run_id));
        let scope_events = client.replay(AggregateRef::WorkScope(scope));
        let item_events = client.replay(AggregateRef::WorkItem(item));
        let attempt_events = client.replay(AggregateRef::Attempt(attempt));
        (started, run, scope_events, item_events, attempt_events)
    });
    daemon.serve_one().unwrap();
    let (started, run, scope_events, item_events, attempt_events) = client.join().unwrap();
    assert_eq!(
        started.event_count, 4,
        "duplicate observation is not a second event"
    );
    assert_eq!(run, vec![1, 2, 3, 4]);
    assert_eq!(scope_events, vec![1]);
    assert_eq!(item_events, vec![1]);
    assert_eq!(attempt_events, vec![1]);

    let socket = daemon.socket_path();
    let run_id = started.run_id;
    let observer = thread::spawn(move || {
        let mut client = TestClient::connect_observer(&socket);
        let replay = client.replay(AggregateRef::AgentRun(run_id));
        let snapshot = client.snapshot(AggregateRef::AgentRun(run_id));
        let create = client.command(&Command::CreateWorkScope {
            session_id: client.session_id,
            kind: WorkScopeKind::AdHoc,
        });
        let control = client.check_generation(run_id, 1, 1);
        (replay, snapshot.incorporated_through, create, control)
    });
    daemon.serve_one().unwrap();
    let (replay, through, create, control) = observer.join().unwrap();
    assert_eq!(replay, run);
    assert_eq!(through, 4);
    assert_eq!(create, CommandResult::Error(CommandError::Denied));
    assert_eq!(control, Err(CommandError::Denied));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn unknown_hello_evidence_is_rejected_before_session() {
    let dir = temp_dir("bad-evidence");
    let config_script = vec![ScriptStep::Emit(HostObservationKind::Started)];
    let config = IntegrationConfig {
        store_path: dir.join("agent.db"),
    };
    let mut daemon =
        AgentDaemon::bind_integration_with_script(&dir, config, config_script.clone()).unwrap();
    let socket = daemon.socket_path();
    let client = thread::spawn(move || {
        let mut stream = std::os::unix::net::UnixStream::connect(&socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let hello = seyal_agent_protocol::Hello {
            supported_versions: vec![seyal_agent_protocol::ProtocolVersion::V1],
            max_frame_size: 4096,
            event_window: 32,
            client_principal_evidence: b"unpaired".to_vec(),
        };
        let frame = seyal_agent_protocol::encode_hello(&hello, ABSOLUTE_MAX_FRAME_SIZE).unwrap();
        stream.write_all(&frame).unwrap();
        read_frame(&mut stream).kind
    });
    assert_eq!(
        daemon.serve_one(),
        Err(seyal_agent_backend::DaemonError::Handshake(
            seyal_agent_protocol::HandshakeError::Malformed
        ))
    );
    assert_eq!(client.join().unwrap(), FrameKind::HandshakeError);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn observer_principal_cannot_escalate_or_resume_owner_session() {
    let dir = temp_dir("principal-fence");
    let config_script = vec![
        ScriptStep::Emit(HostObservationKind::Started),
        ScriptStep::Emit(HostObservationKind::KnownSuccess),
    ];
    let config = IntegrationConfig {
        store_path: dir.join("agent.db"),
    };
    let mut daemon =
        AgentDaemon::bind_integration_with_script(&dir, config, config_script.clone()).unwrap();
    let socket = daemon.socket_path();
    let owner = thread::spawn(move || {
        let mut client = TestClient::connect(&socket);
        let scope = client.create_work_scope(WorkScopeKind::Repository);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        let started = client.start_agent_run(attempt);
        (client.session_id, started.run_id, started.event_count)
    });
    daemon.serve_one().unwrap();
    let (owner_session, run_id, event_count) = owner.join().unwrap();

    let socket = daemon.socket_path();
    let escalate = thread::spawn(move || {
        let mut stream = handshake_with_evidence(&socket, 4096, 32, b"observer");
        round_trip(
            &mut stream,
            &Command::OpenSession {
                scopes: vec![1, 2, 4],
            },
        )
    });
    daemon.serve_one().unwrap();
    assert_eq!(
        escalate.join().unwrap(),
        CommandResult::Error(CommandError::Denied)
    );

    let socket = daemon.socket_path();
    let observe = thread::spawn(move || {
        let mut client = TestClient::connect_observer(&socket);
        client.replay(AggregateRef::AgentRun(run_id))
    });
    daemon.serve_one().unwrap();
    let sequences = observe.join().unwrap();
    assert_eq!(sequences.len() as u64, event_count);
    assert_eq!(sequences, (1..=event_count).collect::<Vec<_>>());

    let socket = daemon.socket_path();
    let cross = thread::spawn(move || {
        let mut stream = handshake_with_evidence(&socket, 4096, 32, b"observer");
        match round_trip(
            &mut stream,
            &Command::ResumeSession {
                session_id: owner_session,
            },
        ) {
            CommandResult::Error(error) => error,
            other => panic!("cross resume: {other:?}"),
        }
    });
    daemon.serve_one().unwrap();
    assert_eq!(cross.join().unwrap(), CommandError::RejectedSession);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn replay_window_is_bounded_and_truncation_is_a_history_gap() {
    let dir = temp_dir("gap");
    let config_script = vec![
        ScriptStep::Emit(HostObservationKind::Started),
        ScriptStep::Emit(HostObservationKind::Progress { step: 1 }),
        ScriptStep::Emit(HostObservationKind::KnownSuccess),
    ];
    let config = IntegrationConfig {
        store_path: dir.join("agent.db"),
    };
    let mut daemon =
        AgentDaemon::bind_integration_with_script(&dir, config, config_script.clone()).unwrap();
    let socket = daemon.socket_path();
    let client = thread::spawn(move || {
        let mut client = TestClient::connect_window(&socket, 1);
        let scope = client.create_work_scope(WorkScopeKind::Project);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        let started = client.start_agent_run(attempt);
        let first = client.subscribe(AggregateRef::AgentRun(started.run_id), None);
        let second = client.subscribe(AggregateRef::AgentRun(started.run_id), Some(1));
        (started.run_id, first, second)
    });
    daemon.serve_one().unwrap();
    let (run_id, first, second) = client.join().unwrap();
    assert_eq!(sequences(first), vec![1]);
    assert_eq!(sequences(second), vec![2]);

    let aggregate = AggregateId::AgentRun(run_id);
    AgentStore::open(dir.join("agent.db"))
        .unwrap()
        .drop_events_before(aggregate, AggregateSequence::from_raw(4).unwrap())
        .unwrap();

    let socket = daemon.socket_path();
    let reader = thread::spawn(move || {
        let mut client = TestClient::connect(&socket);
        let gap = client.subscribe(AggregateRef::AgentRun(run_id), None);
        let tail = client.subscribe(AggregateRef::AgentRun(run_id), Some(3));
        (gap, tail)
    });
    daemon.serve_one().unwrap();
    let (gap, tail) = reader.join().unwrap();
    assert_eq!(
        gap,
        CommandResult::Gap {
            requested_after: 1,
            earliest_available: 4,
            current_snapshot_sequence: Some(4),
        }
    );
    assert_eq!(sequences(tail), vec![4]);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn out_of_order_and_lost_observations_do_not_fabricate_termination() {
    let dir = temp_dir("order");
    let config_script = vec![ScriptStep::EmitExact {
        binding_generation: BindingGeneration::FIRST,
        ordinal: 3,
        kind: HostObservationKind::KnownSuccess,
    }];
    let config = IntegrationConfig {
        store_path: dir.join("agent.db"),
    };
    let mut daemon =
        AgentDaemon::bind_integration_with_script(&dir, config, config_script.clone()).unwrap();
    let socket = daemon.socket_path();
    let client = thread::spawn(move || {
        let mut client = TestClient::connect(&socket);
        let scope = client.create_work_scope(WorkScopeKind::AdHoc);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        client.command(&Command::StartAgentRun {
            session_id: client.session_id,
            attempt_id: attempt,
        })
    });
    daemon.serve_one().unwrap();
    assert_eq!(
        client.join().unwrap(),
        CommandResult::Error(CommandError::Failed)
    );
    let run_id = only_run(&dir);
    let socket = daemon.socket_path();
    let reader = thread::spawn(move || {
        let mut client = TestClient::connect(&socket);
        (
            client.read_run(run_id).2,
            client.replay(AggregateRef::AgentRun(run_id)),
        )
    });
    daemon.serve_one().unwrap();
    let (liveness, replay) = reader.join().unwrap();
    assert_eq!(liveness, 1);
    assert_eq!(replay, vec![1]);
    drop(daemon);

    let lost_dir = temp_dir("lost");
    let config_script = vec![
        ScriptStep::Emit(HostObservationKind::Started),
        ScriptStep::Emit(HostObservationKind::ObservationDisconnected),
    ];
    let config = IntegrationConfig {
        store_path: lost_dir.join("agent.db"),
    };
    let mut daemon =
        AgentDaemon::bind_integration_with_script(&lost_dir, config, config_script.clone())
            .unwrap();
    let socket = daemon.socket_path();
    let client = thread::spawn(move || {
        let mut client = TestClient::connect(&socket);
        let scope = client.create_work_scope(WorkScopeKind::AdHoc);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        let started = client.start_agent_run(attempt);
        client.read_run(started.run_id).2
    });
    daemon.serve_one().unwrap();
    assert_eq!(client.join().unwrap(), 4);
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(lost_dir);
}

#[cfg(feature = "test-fault-injection")]
#[test]
fn persistence_fault_before_commit_does_not_publish_success() {
    let dir = temp_dir("fault");
    let config_script = vec![ScriptStep::Emit(HostObservationKind::KnownSuccess)];
    let config = IntegrationConfig {
        store_path: dir.join("agent.db"),
    };
    let mut daemon =
        AgentDaemon::bind_integration_with_script(&dir, config, config_script.clone()).unwrap();
    daemon.fail_after_writes(4);
    let socket = daemon.socket_path();
    let client = thread::spawn(move || {
        let mut client = TestClient::connect(&socket);
        let scope = client.create_work_scope(WorkScopeKind::Repository);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        client.command(&Command::StartAgentRun {
            session_id: client.session_id,
            attempt_id: attempt,
        })
    });
    daemon.serve_one().unwrap();
    assert_eq!(
        client.join().unwrap(),
        CommandResult::Error(CommandError::Failed)
    );
    let run_id = only_run(&dir);
    assert_eq!(
        AgentStore::open(dir.join("agent.db"))
            .unwrap()
            .replay_after(AggregateId::AgentRun(run_id), None)
            .unwrap()
            .len(),
        1
    );
    let socket = daemon.socket_path();
    let reader = thread::spawn(move || {
        let mut client = TestClient::connect(&socket);
        client.read_run(run_id).2
    });
    daemon.serve_one().unwrap();
    assert_eq!(reader.join().unwrap(), 1);
    drop(daemon);

    let refused = temp_dir("refused");
    let config_script = vec![ScriptStep::Emit(HostObservationKind::Started)];
    let config = IntegrationConfig {
        store_path: refused.join("agent.db"),
    };
    let mut daemon =
        AgentDaemon::bind_integration_with_script(&refused, config, config_script.clone()).unwrap();
    daemon.fail_after_writes(0);
    let socket = daemon.socket_path();
    let client = thread::spawn(move || {
        let mut client = TestClient::connect(&socket);
        client.command(&Command::CreateWorkScope {
            session_id: client.session_id,
            kind: WorkScopeKind::Repository,
        })
    });
    daemon.serve_one().unwrap();
    assert_eq!(
        client.join().unwrap(),
        CommandResult::Error(CommandError::Failed)
    );
    assert!(AgentStore::open(refused.join("agent.db"))
        .unwrap()
        .work_scopes()
        .unwrap()
        .is_empty());

    let segment_fault = temp_dir("segment-fault");
    let config_script = vec![
        ScriptStep::Emit(HostObservationKind::Started),
        ScriptStep::Emit(HostObservationKind::Output(vec![2; 8192])),
    ];
    let config = IntegrationConfig {
        store_path: segment_fault.join("agent.db"),
    };
    let mut daemon =
        AgentDaemon::bind_integration_with_script(&segment_fault, config, config_script.clone())
            .unwrap();
    // create scope/item/attempt/run use writes; fault the output segment commit.
    daemon.fail_after_writes(5);
    let socket = daemon.socket_path();
    let client = thread::spawn(move || {
        let mut client = TestClient::connect(&socket);
        let scope = client.create_work_scope(WorkScopeKind::Repository);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        client.command(&Command::StartAgentRun {
            session_id: client.session_id,
            attempt_id: attempt,
        })
    });
    daemon.serve_one().unwrap();
    assert_eq!(
        client.join().unwrap(),
        CommandResult::Error(CommandError::Failed)
    );
    let store = AgentStore::open(segment_fault.join("agent.db")).unwrap();
    let run_id = store.agent_runs().unwrap()[0].0;
    assert_eq!(store.output_segment_count(run_id).unwrap(), 0);
    let events = store
        .replay_after(AggregateId::AgentRun(run_id), None)
        .unwrap();
    assert!(events
        .iter()
        .all(|event| seyal_agent_store::decode_output_ref(&event.payload).is_err()));
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(refused);
    let _ = fs::remove_dir_all(segment_fault);
}

#[test]
fn bind_integration_recovers_migrated_v2_orphan_run() {
    let dir = temp_dir("v2-orphan");
    let store_path = dir.join("agent.db");
    write_populated_v2_store(&store_path);
    let config_script = vec![ScriptStep::Emit(HostObservationKind::Started)];
    let config = IntegrationConfig {
        store_path: store_path.clone(),
    };
    let mut daemon =
        AgentDaemon::bind_integration_with_script(&dir, config, config_script.clone()).unwrap();
    let prior_run = AgentRunId::from_bytes([9u8; 16]);
    let socket = daemon.socket_path();
    let client = thread::spawn(move || {
        let mut client = TestClient::connect(&socket);
        let run = client.read_run(prior_run);
        let replay = client.replay(AggregateRef::AgentRun(prior_run));
        (run, replay)
    });
    daemon.serve_one().unwrap();
    let (run, replay) = client.join().unwrap();
    assert_eq!(run, (5, 6, 3));
    assert_eq!(replay, vec![1, 2]);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn high_volume_subscribe_fits_frame_and_continues() {
    // Output now collapses to one segment-ref event, so this row proves
    // byte-budget paging with many bounded Result payloads instead.
    const RESULTS: usize = 200;
    const RESULT_BYTES: usize = 400;
    let dir = temp_dir("volume-sub");
    let mut script = vec![ScriptStep::Emit(HostObservationKind::Started)];
    for step in 0..RESULTS {
        script.push(ScriptStep::Emit(HostObservationKind::Result(vec![
            (step % 251) as u8;
            RESULT_BYTES
        ])));
    }
    script.push(ScriptStep::Emit(HostObservationKind::KnownSuccess));
    let mut daemon = AgentDaemon::bind_integration_with_script(
        &dir,
        IntegrationConfig {
            store_path: dir.join("agent.db"),
        },
        script,
    )
    .unwrap();
    let socket = daemon.socket_path();
    let client = thread::spawn(move || {
        let mut client = TestClient::connect_limits(&socket, ABSOLUTE_MAX_FRAME_SIZE, 1024);
        let scope = client.create_work_scope(WorkScopeKind::Repository);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        let started = client.start_agent_run(attempt);
        let mut after = None;
        let mut sequences = Vec::new();
        let mut pages = 0_u32;
        loop {
            match client.subscribe(AggregateRef::AgentRun(started.run_id), after) {
                CommandResult::Replay { events } if events.is_empty() => break,
                CommandResult::Replay { events } => {
                    pages += 1;
                    assert!(!events.is_empty());
                    assert!(
                        events.len() < started.event_count as usize,
                        "byte budget must shrink the count window for high-volume runs"
                    );
                    let frame = encode_result(
                        &CommandResult::Replay {
                            events: events.clone(),
                        },
                        ABSOLUTE_MAX_FRAME_SIZE,
                    )
                    .expect("each page must encode under the negotiated frame size");
                    assert!(frame.len() as u32 <= ABSOLUTE_MAX_FRAME_SIZE);
                    after = Some(events.last().unwrap().sequence);
                    sequences.extend(events.into_iter().map(|event| event.sequence));
                }
                other => panic!("subscribe: {other:?}"),
            }
        }
        (started.event_count, pages, sequences)
    });
    daemon.serve_one().unwrap();
    let (event_count, pages, sequences) = client.join().unwrap();
    assert!(pages >= 2, "high-volume replay must page across frames");
    assert_eq!(sequences.len() as u64, event_count);
    assert!(sequences.windows(2).all(|pair| pair[1] == pair[0] + 1));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn high_volume_output_uses_segments_end_to_end() {
    const OUTPUT_BYTES: usize = 256 * 1024;
    let dir = temp_dir("segments-e2e");
    let config_script = vec![
        ScriptStep::Emit(HostObservationKind::Started),
        ScriptStep::Emit(HostObservationKind::Output(vec![9; OUTPUT_BYTES])),
        ScriptStep::Emit(HostObservationKind::KnownSuccess),
    ];
    let config = IntegrationConfig {
        store_path: dir.join("agent.db"),
    };
    let mut daemon =
        AgentDaemon::bind_integration_with_script(&dir, config, config_script.clone()).unwrap();
    let socket = daemon.socket_path();
    let client = thread::spawn(move || {
        let mut client = TestClient::connect(&socket);
        let scope = client.create_work_scope(WorkScopeKind::Repository);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        let started = client.start_agent_run(attempt);
        let events = client.replay_all(AggregateRef::AgentRun(started.run_id));
        (started, events)
    });
    daemon.serve_one().unwrap();
    let (started, events) = client.join().unwrap();
    // create-run + Started + one output ref + KnownSuccess
    assert_eq!(started.event_count, 4);
    assert_eq!(events.len(), 4);
    let output = events
        .iter()
        .find(|event| seyal_agent_store::decode_output_ref(&event.payload).is_ok())
        .expect("one output-ref event");
    let decoded = seyal_agent_store::decode_output_ref(&output.payload).unwrap();
    assert_eq!(decoded.first_segment_index, 0);
    assert_eq!(
        decoded.segment_count as usize,
        OUTPUT_BYTES / seyal_agent_store::OUTPUT_SEGMENT_LEN
    );
    assert_eq!(decoded.byte_length as usize, OUTPUT_BYTES);
    assert!(decoded.first_ordinal >= 1);
    assert!(decoded.last_ordinal >= decoded.first_ordinal);
    assert_eq!(
        decoded.retention_policy_ref,
        seyal_agent_store::RetentionPolicyRef::retained_stream()
    );
    assert!(matches!(
        decoded.fingerprint_ref,
        seyal_agent_store::FingerprintRef::PublicContentDigest(_)
    ));
    assert!(
        events
            .iter()
            .filter(|event| seyal_agent_store::decode_output_ref(&event.payload).is_ok())
            .count()
            == 1
    );
    let store = AgentStore::open(dir.join("agent.db")).unwrap();
    assert_eq!(
        store.output_segment_count(started.run_id).unwrap(),
        decoded.segment_count as u64
    );
    drop(daemon);
    drop(store);

    let reopened = AgentStore::open(dir.join("agent.db")).unwrap();
    assert_eq!(
        reopened.output_segment_count(started.run_id).unwrap(),
        decoded.segment_count as u64
    );
    let again = reopened
        .replay_after(AggregateId::AgentRun(started.run_id), None)
        .unwrap();
    assert_eq!(again.len(), 4);
    let _ = fs::remove_dir_all(dir);

    let tiny = temp_dir("segments-tiny");
    let mut one_byte = Vec::new();
    one_byte.push(ScriptStep::Emit(HostObservationKind::Started));
    for _ in 0..1000 {
        one_byte.push(ScriptStep::Emit(HostObservationKind::Output(vec![1])));
    }
    one_byte.push(ScriptStep::Emit(HostObservationKind::KnownSuccess));
    let config_script = one_byte;
    let config = IntegrationConfig {
        store_path: tiny.join("agent.db"),
    };
    let mut daemon =
        AgentDaemon::bind_integration_with_script(&tiny, config, config_script.clone()).unwrap();
    let socket = daemon.socket_path();
    let client = thread::spawn(move || {
        let mut client = TestClient::connect(&socket);
        let scope = client.create_work_scope(WorkScopeKind::Repository);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        let started = client.start_agent_run(attempt);
        let events = client.replay_all(AggregateRef::AgentRun(started.run_id));
        (started, events)
    });
    daemon.serve_one().unwrap();
    let (started, events) = client.join().unwrap();
    assert_eq!(started.event_count, 4);
    let output = events
        .iter()
        .find_map(|event| seyal_agent_store::decode_output_ref(&event.payload).ok())
        .expect("coalesced one-byte outputs");
    assert_eq!(output.segment_count, 1);
    assert_eq!(output.byte_length, 1000);
    assert_eq!(
        AgentStore::open(tiny.join("agent.db"))
            .unwrap()
            .output_segment_count(started.run_id)
            .unwrap(),
        1
    );
    let _ = fs::remove_dir_all(tiny);
}

#[test]
fn records_session_startup_throughput_reconnect_and_storage_growth() {
    const OUTPUT_BYTES: usize = 32 * 1024;
    let dir = temp_dir("measure");
    let config_script = vec![
        ScriptStep::Emit(HostObservationKind::Started),
        ScriptStep::Emit(HostObservationKind::Output(vec![7; OUTPUT_BYTES])),
    ];
    let config = IntegrationConfig {
        store_path: dir.join("agent.db"),
    };
    let started_bind = Instant::now();
    let mut daemon =
        AgentDaemon::bind_integration_with_script(&dir, config, config_script.clone()).unwrap();
    let startup = started_bind.elapsed();
    let bytes_before = AgentStore::open(dir.join("agent.db"))
        .unwrap()
        .database_bytes()
        .unwrap();
    let idle_rss_kib = resident_kib();
    let idle_cpu = cpu_percent();

    let socket = daemon.socket_path();
    let client = thread::spawn(move || {
        let mut client = TestClient::connect_limits(&socket, ABSOLUTE_MAX_FRAME_SIZE, 64);
        let scope = client.create_work_scope(WorkScopeKind::Repository);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        let append_started = Instant::now();
        let started = client.start_agent_run(attempt);
        let append = append_started.elapsed();
        let snapshot_started = Instant::now();
        let snapshot = client.snapshot(AggregateRef::AgentRun(started.run_id));
        let snapshot_latency = snapshot_started.elapsed();
        let replay_started = Instant::now();
        let replay = client.replay(AggregateRef::AgentRun(started.run_id));
        let replay_latency = replay_started.elapsed();
        (
            client.session_id,
            started,
            snapshot.incorporated_through,
            replay,
            append,
            snapshot_latency,
            replay_latency,
        )
    });
    daemon.serve_one().unwrap();
    let (session_id, started, through, replay, append, snapshot_latency, replay_latency) =
        client.join().unwrap();
    assert_eq!(started.event_count, 3);
    assert_eq!(replay.len(), 3);
    assert_eq!(through, 3);

    let socket = daemon.socket_path();
    let run_id = started.run_id;
    let event_count = started.event_count;
    let reconnect_started = Instant::now();
    let resumed = thread::spawn(move || {
        let mut client =
            TestClient::resume_limits(&socket, session_id, ABSOLUTE_MAX_FRAME_SIZE, 64).unwrap();
        let snapshot = client.snapshot(AggregateRef::AgentRun(run_id));
        let replay = client.replay(AggregateRef::AgentRun(run_id));
        (snapshot.incorporated_through, replay.len())
    });
    daemon.serve_one().unwrap();
    let reconnect = reconnect_started.elapsed();
    let (again, replay_len) = resumed.join().unwrap();
    assert_eq!(again, through);
    assert_eq!(replay_len, 3);

    let bytes_after = AgentStore::open(dir.join("agent.db"))
        .unwrap()
        .database_bytes()
        .unwrap();
    assert!(bytes_after > bytes_before);
    for (name, sample) in [
        ("startup", startup),
        ("append", append),
        ("snapshot", snapshot_latency),
        ("replay", replay_latency),
        ("reconnect", reconnect),
    ] {
        assert!(
            sample < Duration::from_secs(5),
            "{name} exceeded the hang ceiling: {sample:?}"
        );
    }
    if let Some(rss) = idle_rss_kib {
        assert!(rss < 512 * 1024, "process RSS ceiling exceeded: {rss} KiB");
    }
    let events_per_s = events_per_second(event_count, append);
    eprintln!(
        "ab-0.6 measurement performance_claim=false host_class={} os={} arch={} build_mode={} workload=output_32kib_plus_started run_count=1 percentile_method=single_sample startup_us={} idle_rss_kib={} idle_cpu={} append_us={} events={} events_per_s={} snapshot_us={} replay_us={} reconnect_us={} db_bytes_before={} db_bytes_after={}",
        host_class(),
        std::env::consts::OS,
        std::env::consts::ARCH,
        build_mode(),
        startup.as_micros(),
        idle_rss_kib.map(|value| value.to_string()).unwrap_or_else(|| "none".to_string()),
        idle_cpu.map(|value| value.to_string()).unwrap_or_else(|| "none".to_string()),
        append.as_micros(),
        event_count,
        events_per_s,
        snapshot_latency.as_micros(),
        replay_latency.as_micros(),
        reconnect.as_micros(),
        bytes_before,
        bytes_after
    );
    let _ = fs::remove_dir_all(dir);
}

mod support;
use support::*;

fn cpu_percent() -> Option<f64> {
    let output = std::process::Command::new("ps")
        .args(["-o", "pcpu=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    String::from_utf8(output.stdout).ok()?.trim().parse().ok()
}

fn write_populated_v2_store(path: &Path) {
    use rusqlite::{params, Connection};
    use std::os::unix::fs::DirBuilderExt;
    if let Some(parent) = path.parent() {
        match fs::symlink_metadata(parent) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::DirBuilder::new().mode(0o700).create(parent).unwrap();
            }
            Err(error) => panic!("create store parent: {error}"),
        }
    }
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE aggregate_event (
            aggregate_kind INTEGER NOT NULL,
            aggregate_id BLOB NOT NULL,
            sequence INTEGER NOT NULL,
            event_id INTEGER NOT NULL,
            kind INTEGER NOT NULL,
            payload BLOB NOT NULL,
            PRIMARY KEY (aggregate_kind, aggregate_id, sequence)
        );
        CREATE TABLE aggregate_snapshot (
            aggregate_kind INTEGER NOT NULL,
            aggregate_id BLOB NOT NULL,
            incorporated_through INTEGER NOT NULL,
            payload BLOB NOT NULL,
            PRIMARY KEY (aggregate_kind, aggregate_id)
        );
        CREATE TABLE aggregate_sequence_hwm (
            aggregate_kind INTEGER NOT NULL,
            aggregate_id BLOB NOT NULL,
            high_water INTEGER NOT NULL,
            PRIMARY KEY (aggregate_kind, aggregate_id)
        );
        CREATE TABLE output_segment (
            agent_run_id BLOB NOT NULL,
            segment_index INTEGER NOT NULL,
            payload BLOB NOT NULL,
            PRIMARY KEY (agent_run_id, segment_index)
        );
        CREATE TABLE agent_run (
            id BLOB PRIMARY KEY,
            attempt_id BLOB NOT NULL,
            binding_generation INTEGER NOT NULL,
            control_generation INTEGER NOT NULL,
            liveness TEXT NOT NULL CHECK (liveness = 'unknown')
        );",
    )
    .unwrap();
    let prior_run = [9u8; 16];
    let prior_attempt = [8u8; 16];
    conn.execute(
        "INSERT INTO agent_run (id, attempt_id, binding_generation, control_generation, liveness)
         VALUES (?1, ?2, 4, 5, 'unknown')",
        params![prior_run.to_vec(), prior_attempt.to_vec()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO aggregate_event
            (aggregate_kind, aggregate_id, sequence, event_id, kind, payload)
         VALUES (4, ?1, 1, 1, 9, ?2)",
        params![prior_run.to_vec(), b"kept".to_vec()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO aggregate_sequence_hwm (aggregate_kind, aggregate_id, high_water)
         VALUES (4, ?1, 1)",
        params![prior_run.to_vec()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO aggregate_snapshot
            (aggregate_kind, aggregate_id, incorporated_through, payload)
         VALUES (4, ?1, 1, ?2)",
        params![prior_run.to_vec(), b"snap".to_vec()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO output_segment (agent_run_id, segment_index, payload)
         VALUES (?1, 0, ?2)",
        params![prior_run.to_vec(), b"seg".to_vec()],
    )
    .unwrap();
    conn.pragma_update(None, "user_version", 2).unwrap();
}

fn only_run(dir: &Path) -> AgentRunId {
    let runs = AgentStore::open(dir.join("agent.db"))
        .unwrap()
        .agent_runs()
        .unwrap();
    assert_eq!(runs.len(), 1);
    runs[0].0
}
