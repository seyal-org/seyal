//! Standalone client path over authenticated local IPC.
//!
//! The client crate cannot depend on the backend, and the backend cannot depend
//! on the client. This test speaks the same V1 command frames as `SessionClient`.

use std::{
    fs,
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::Path,
    thread,
};

use seyal_agent_backend::{AgentDaemon, HostObservationKind, IntegrationConfig, ScriptStep};
use seyal_agent_core::{BindingGeneration, WorkScopeKind};
use seyal_agent_protocol::{
    decode_frame, decode_result, encode_command, encode_frame, encode_hello, AgentRunId,
    AggregateRef, AttemptId, ClientSessionId, Command, CommandError, CommandResult, FrameKind,
    Hello, ProtocolVersion, WorkItemId, WorkScopeId, ABSOLUTE_MAX_FRAME_SIZE,
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
    let config = IntegrationConfig {
        store_path: dir.join("agent.db"),
        script: vec![
            ScriptStep::Emit(HostObservationKind::Started),
            ScriptStep::Emit(HostObservationKind::Progress { step: 1 }),
            ScriptStep::Emit(HostObservationKind::KnownSuccess),
        ],
    };

    let mut daemon = AgentDaemon::bind_integration(&dir, config.clone()).unwrap();
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
    let mut restarted = AgentDaemon::bind_integration(&dir, config).unwrap();
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
        let replay = client.replay(AggregateRef::AgentRun(run_id));
        let item_events = client.replay(AggregateRef::WorkItem(item));
        let attempt_events = client.replay(AggregateRef::Attempt(attempt));
        let stale_binding = client.check_generation(run_id, 2, 1);
        let stale_control = client.check_generation(run_id, 1, 2);
        let current = client.check_generation(run_id, 1, 1);
        (
            run,
            replay.len(),
            item_events.len(),
            attempt_events.len(),
            stale_binding,
            stale_control,
            current,
        )
    });
    restarted.serve_one().unwrap();
    let (run, events, items, attempts, stale_binding, stale_control, current) =
        recovered.join().unwrap();
    assert_eq!(run.0, 1);
    assert_eq!(run.1, 1);
    assert_eq!(run.2, 3, "restart classifies liveness as unknown");
    assert_eq!(events, 4);
    assert_eq!(items, 1);
    assert_eq!(attempts, 1);
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
fn malformed_input_and_narrow_sessions_do_not_disturb_authority() {
    let dir = std::env::temp_dir().join(format!(
        "seyal-ag-m-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let config = IntegrationConfig {
        store_path: dir.join("agent.db"),
        script: vec![ScriptStep::Emit(HostObservationKind::Started)],
    };
    let mut daemon = AgentDaemon::bind_integration(&dir, config.clone()).unwrap();
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
    let mut restarted = AgentDaemon::bind_integration(&dir, config).unwrap();
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
    let config = IntegrationConfig {
        store_path: dir.join("agent.db"),
        script: vec![
            ScriptStep::Emit(HostObservationKind::Started),
            ScriptStep::DuplicateLast,
            ScriptStep::Emit(HostObservationKind::Progress { step: 1 }),
            ScriptStep::Emit(HostObservationKind::KnownSuccess),
        ],
    };
    let mut daemon = AgentDaemon::bind_integration(&dir, config).unwrap();
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
        let mut client = TestClient::connect_with(&socket, vec![2]);
        let replay = client.replay(AggregateRef::AgentRun(run_id));
        let snapshot = client.snapshot(AggregateRef::AgentRun(run_id));
        let create = client.command(&Command::CreateWorkScope {
            session_id: client.session_id,
            kind: WorkScopeKind::AdHoc,
        });
        (replay, snapshot.incorporated_through, create)
    });
    daemon.serve_one().unwrap();
    let (replay, through, create) = observer.join().unwrap();
    assert_eq!(replay, run);
    assert_eq!(through, 4);
    assert_eq!(create, CommandResult::Error(CommandError::Denied));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn replay_window_is_bounded_and_truncation_is_a_history_gap() {
    let dir = temp_dir("gap");
    let config = IntegrationConfig {
        store_path: dir.join("agent.db"),
        script: vec![
            ScriptStep::Emit(HostObservationKind::Started),
            ScriptStep::Emit(HostObservationKind::Progress { step: 1 }),
            ScriptStep::Emit(HostObservationKind::KnownSuccess),
        ],
    };
    let mut daemon = AgentDaemon::bind_integration(&dir, config).unwrap();
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
    let config = IntegrationConfig {
        store_path: dir.join("agent.db"),
        script: vec![ScriptStep::EmitExact {
            binding_generation: BindingGeneration::FIRST,
            ordinal: 3,
            kind: HostObservationKind::KnownSuccess,
        }],
    };
    let mut daemon = AgentDaemon::bind_integration(&dir, config).unwrap();
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
    let config = IntegrationConfig {
        store_path: lost_dir.join("agent.db"),
        script: vec![
            ScriptStep::Emit(HostObservationKind::Started),
            ScriptStep::Emit(HostObservationKind::ObservationDisconnected),
        ],
    };
    let mut daemon = AgentDaemon::bind_integration(&lost_dir, config).unwrap();
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

#[test]
fn persistence_fault_before_commit_does_not_publish_success() {
    let dir = temp_dir("fault");
    let config = IntegrationConfig {
        store_path: dir.join("agent.db"),
        script: vec![ScriptStep::Emit(HostObservationKind::KnownSuccess)],
    };
    let mut daemon = AgentDaemon::bind_integration(&dir, config).unwrap();
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
    let config = IntegrationConfig {
        store_path: refused.join("agent.db"),
        script: vec![ScriptStep::Emit(HostObservationKind::Started)],
    };
    let mut daemon = AgentDaemon::bind_integration(&refused, config).unwrap();
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
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(refused);
}

fn temp_dir(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "seyal-q-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn only_run(dir: &Path) -> AgentRunId {
    let runs = AgentStore::open(dir.join("agent.db"))
        .unwrap()
        .agent_runs()
        .unwrap();
    assert_eq!(runs.len(), 1);
    runs[0].0
}

fn sequences(result: CommandResult) -> Vec<u64> {
    match result {
        CommandResult::Replay { events } => {
            events.into_iter().map(|event| event.sequence).collect()
        }
        other => panic!("replay: {other:?}"),
    }
}

struct TestClient {
    stream: UnixStream,
    session_id: ClientSessionId,
}

struct RunState {
    run_id: AgentRunId,
    binding_generation: u64,
    control_generation: u64,
    event_count: u64,
}

struct Snap {
    incorporated_through: u64,
}

impl TestClient {
    fn connect(path: &Path) -> Self {
        Self::connect_with(path, vec![1, 2, 4])
    }

    fn connect_window(path: &Path, event_window: u32) -> Self {
        let mut stream = handshake_with(path, event_window);
        let session_id = match round_trip(
            &mut stream,
            &Command::OpenSession {
                scopes: vec![1, 2, 4],
            },
        ) {
            CommandResult::Opened { session_id } => session_id,
            other => panic!("open session: {other:?}"),
        };
        Self { stream, session_id }
    }

    fn connect_with(path: &Path, scopes: Vec<u8>) -> Self {
        let mut stream = handshake(path);
        let session_id = match round_trip(&mut stream, &Command::OpenSession { scopes }) {
            CommandResult::Opened { session_id } => session_id,
            other => panic!("open session: {other:?}"),
        };
        Self { stream, session_id }
    }

    fn command(&mut self, command: &Command) -> CommandResult {
        round_trip(&mut self.stream, command)
    }

    fn write_frame(&mut self, frame: &[u8]) -> CommandResult {
        self.stream.write_all(frame).unwrap();
        let response = read_frame(&mut self.stream);
        assert_eq!(response.kind, FrameKind::Result);
        decode_result(&response.body).unwrap()
    }

    fn resume(path: &Path, session_id: ClientSessionId) -> Result<Self, CommandError> {
        let mut stream = handshake(path);
        match round_trip(&mut stream, &Command::ResumeSession { session_id }) {
            CommandResult::Resumed => Ok(Self { stream, session_id }),
            CommandResult::Error(error) => Err(error),
            other => panic!("resume: {other:?}"),
        }
    }

    fn create_work_scope(&mut self, kind: WorkScopeKind) -> WorkScopeId {
        match round_trip(
            &mut self.stream,
            &Command::CreateWorkScope {
                session_id: self.session_id,
                kind,
            },
        ) {
            CommandResult::WorkScope { id } => id,
            other => panic!("scope: {other:?}"),
        }
    }

    fn create_work_item(&mut self, work_scope_id: WorkScopeId) -> WorkItemId {
        match round_trip(
            &mut self.stream,
            &Command::CreateWorkItem {
                session_id: self.session_id,
                work_scope_id,
            },
        ) {
            CommandResult::WorkItem { id } => id,
            other => panic!("item: {other:?}"),
        }
    }

    fn create_attempt(&mut self, work_item_id: WorkItemId) -> AttemptId {
        match round_trip(
            &mut self.stream,
            &Command::CreateAttempt {
                session_id: self.session_id,
                work_item_id,
            },
        ) {
            CommandResult::Attempt { id } => id,
            other => panic!("attempt: {other:?}"),
        }
    }

    fn start_agent_run(&mut self, attempt_id: AttemptId) -> RunState {
        match round_trip(
            &mut self.stream,
            &Command::StartAgentRun {
                session_id: self.session_id,
                attempt_id,
            },
        ) {
            CommandResult::Started {
                run_id,
                binding_generation,
                control_generation,
                event_count,
            } => RunState {
                run_id,
                binding_generation,
                control_generation,
                event_count,
            },
            other => panic!("start: {other:?}"),
        }
    }

    fn snapshot(&mut self, aggregate: AggregateRef) -> Snap {
        match round_trip(
            &mut self.stream,
            &Command::GetSnapshot {
                session_id: self.session_id,
                aggregate,
            },
        ) {
            CommandResult::Snapshot { view: Some(view) } => Snap {
                incorporated_through: view.incorporated_through,
            },
            other => panic!("snapshot: {other:?}"),
        }
    }

    fn replay(&mut self, aggregate: AggregateRef) -> Vec<u64> {
        sequences(self.subscribe(aggregate, None))
    }

    fn subscribe(&mut self, aggregate: AggregateRef, after: Option<u64>) -> CommandResult {
        round_trip(
            &mut self.stream,
            &Command::Subscribe {
                session_id: self.session_id,
                aggregate,
                after,
            },
        )
    }

    fn read_run(&mut self, run_id: AgentRunId) -> (u64, u64, u8) {
        match round_trip(
            &mut self.stream,
            &Command::ReadRun {
                session_id: self.session_id,
                run_id,
            },
        ) {
            CommandResult::Run {
                binding_generation,
                control_generation,
                liveness,
            } => (binding_generation, control_generation, liveness),
            other => panic!("read: {other:?}"),
        }
    }

    fn check_generation(
        &mut self,
        run_id: AgentRunId,
        binding_generation: u64,
        control_generation: u64,
    ) -> Result<(), CommandError> {
        match round_trip(
            &mut self.stream,
            &Command::CheckGeneration {
                session_id: self.session_id,
                run_id,
                binding_generation,
                control_generation,
            },
        ) {
            CommandResult::GenerationOk => Ok(()),
            CommandResult::Error(error) => Err(error),
            other => panic!("generation: {other:?}"),
        }
    }
}

fn handshake(path: &Path) -> UnixStream {
    handshake_with(path, 32)
}

fn handshake_with(path: &Path, event_window: u32) -> UnixStream {
    let mut stream = UnixStream::connect(path).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    let hello = Hello {
        supported_versions: vec![ProtocolVersion::V1],
        max_frame_size: 4096,
        event_window,
        client_principal_evidence: Vec::new(),
    };
    let frame = encode_hello(&hello, ABSOLUTE_MAX_FRAME_SIZE).unwrap();
    stream.write_all(&frame).unwrap();
    let response = read_frame(&mut stream);
    assert_eq!(response.kind, FrameKind::HelloAck);
    stream
}

fn round_trip(stream: &mut UnixStream, command: &Command) -> CommandResult {
    let frame = encode_command(command, ABSOLUTE_MAX_FRAME_SIZE).unwrap();
    stream.write_all(&frame).unwrap();
    let response = read_frame(stream);
    assert_eq!(response.kind, FrameKind::Result);
    decode_result(&response.body).unwrap()
}

fn read_frame(stream: &mut UnixStream) -> seyal_agent_protocol::Frame {
    let mut header = [0; 10];
    stream.read_exact(&mut header).unwrap();
    let body_len =
        seyal_agent_protocol::accepted_body_len(&header, ABSOLUTE_MAX_FRAME_SIZE).unwrap();
    let mut body = vec![0; body_len];
    if body_len > 0 {
        stream.read_exact(&mut body).unwrap();
    }
    let mut bytes = header.to_vec();
    bytes.extend_from_slice(&body);
    decode_frame(&bytes, ABSOLUTE_MAX_FRAME_SIZE).unwrap()
}
