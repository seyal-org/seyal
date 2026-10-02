use super::*;
use seyal_agent_core::{AttemptId, WorkItemId, WorkScopeId};
use seyal_agent_protocol::{BackendInstanceId, Command, CommandError, CommandResult};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_store() -> PathBuf {
    std::env::temp_dir().join(format!(
        "seyal-session-undo-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn mid_group_apply_conflict_undoes_prior_outputs() {
    let dir = temp_store();
    std::fs::create_dir_all(&dir).unwrap();
    let store_path = dir.join("agent.db");
    let config = IntegrationConfig {
        store_path: store_path.clone(),
        script: vec![ScriptStep::Emit(HostObservationKind::Started)],
    };
    let mut service =
        IntegrationService::open(BackendInstanceId::new(), &config).expect("open service");

    let scope = WorkScopeId::new();
    let item = WorkItemId::new();
    let attempt = AttemptId::new();
    let run = AgentRunId::new();
    let binding = BindingGeneration::FIRST;
    let control = ControlGeneration::FIRST;
    service
        .store
        .commit_work_scope(scope, WorkScopeKind::AdHoc.code())
        .unwrap();
    service
        .authority
        .restore_work_scope(scope, WorkScopeKind::AdHoc)
        .unwrap();
    service.store.commit_work_item(item, scope).unwrap();
    service.authority.restore_work_item(item, scope).unwrap();
    service.store.commit_attempt(attempt, item).unwrap();
    service.authority.restore_attempt(attempt, item).unwrap();
    service
        .store
        .mutate_agent_run_and_append(
            run,
            attempt,
            binding.get(),
            control.get(),
            1,
            &attempt.to_bytes(),
        )
        .unwrap();
    service
        .authority
        .restore_agent_run(run, attempt, binding, control)
        .unwrap();

    let group = vec![
        HostObservation {
            run_id: run,
            binding_generation: binding,
            ordinal: 1,
            kind: HostObservationKind::Output(vec![1, 2, 3]),
        },
        HostObservation {
            run_id: run,
            binding_generation: binding,
            ordinal: 1,
            kind: HostObservationKind::Output(vec![9, 9, 9]),
        },
    ];
    let err = service
        .commit_output_group(&group)
        .expect_err("conflicting second Output must fail");
    assert_eq!(err, CommandError::Failed);
    assert_eq!(
        service.authority.applied_count(),
        0,
        "prior Output apply must be undone so retry is not sticky"
    );
    assert_eq!(service.authority.effects_performed(), 0);
    assert!(service.authority.recorded_liveness(run).is_none());
    assert_eq!(
        AgentStore::open(&store_path)
            .unwrap()
            .replay_after(AggregateId::AgentRun(run), None)
            .unwrap()
            .len(),
        1,
        "failed group must not persist Output events"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(feature = "test-fault-injection")]
#[test]
fn start_agent_run_event_count_does_not_load_full_replay_payloads() {
    let dir = temp_store();
    std::fs::create_dir_all(&dir).unwrap();
    let store_path = dir.join("agent.db");
    const RESULTS: usize = 128;
    const RESULT_BYTES: usize = 2 * 1024;
    let mut script = vec![ScriptStep::Emit(HostObservationKind::Started)];
    for step in 0..RESULTS {
        script.push(ScriptStep::Emit(HostObservationKind::Result(vec![
            (step % 251) as u8;
            RESULT_BYTES
        ])));
    }
    script.push(ScriptStep::Emit(HostObservationKind::KnownSuccess));
    let config = IntegrationConfig {
        store_path: store_path.clone(),
        script,
    };
    let mut service =
        IntegrationService::open(BackendInstanceId::new(), &config).expect("open service");
    service.begin_connection(b"cli").expect("hello principal");

    let opened = service.dispatch(
        Command::OpenSession {
            scopes: vec![1, 2, 4],
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    );
    let CommandResult::Opened { session_id } = opened else {
        panic!("open session: {opened:?}");
    };
    let CommandResult::WorkScope { id: scope } = service.dispatch(
        Command::CreateWorkScope {
            session_id,
            kind: WorkScopeKind::Repository,
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    ) else {
        panic!("scope");
    };
    let CommandResult::WorkItem { id: item } = service.dispatch(
        Command::CreateWorkItem {
            session_id,
            work_scope_id: scope,
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    ) else {
        panic!("item");
    };
    let CommandResult::Attempt { id: attempt } = service.dispatch(
        Command::CreateAttempt {
            session_id,
            work_item_id: item,
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    ) else {
        panic!("attempt");
    };

    let _ = service.store.take_replay_payload_bytes_loaded();
    let started = service.dispatch(
        Command::StartAgentRun {
            session_id,
            attempt_id: attempt,
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    );
    let CommandResult::Started {
        event_count,
        run_id,
        ..
    } = started
    else {
        panic!("start: {started:?}");
    };
    let replay_bytes = service.store.take_replay_payload_bytes_loaded();
    assert_eq!(
        replay_bytes, 0,
        "start_agent_run must not load full replay payloads to count events"
    );
    assert_eq!(
        event_count,
        service
            .store
            .high_water(AggregateId::AgentRun(run_id))
            .unwrap()
    );
    assert!(event_count >= (RESULTS as u64) + 2);
    assert_eq!(
        service.authority.liveness(run_id),
        RunLiveness::KnownTerminated
    );

    // Snapshot + replay convergence and HistoryGap still hold for the run.
    let (position, _) = service
        .store
        .get_snapshot(AggregateId::AgentRun(run_id))
        .unwrap()
        .expect("snapshot");
    assert_eq!(position.incorporated_through.get(), event_count);
    let tail = service
        .store
        .replay_after(
            AggregateId::AgentRun(run_id),
            Some(position.incorporated_through),
        )
        .unwrap();
    assert!(tail.is_empty());

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn recovery_matrix_honors_terminal_and_keeps_live_unknown() {
    let dir = temp_store();
    std::fs::create_dir_all(&dir).unwrap();
    let terminal_path = dir.join("terminal.db");
    let live_path = dir.join("live.db");

    let terminal_config = IntegrationConfig {
        store_path: terminal_path.clone(),
        script: vec![
            ScriptStep::Emit(HostObservationKind::Started),
            ScriptStep::Emit(HostObservationKind::KnownSuccess),
        ],
    };
    let mut terminal =
        IntegrationService::open(BackendInstanceId::new(), &terminal_config).unwrap();
    terminal.begin_connection(b"cli").unwrap();
    let session = match terminal.dispatch(
        Command::OpenSession {
            scopes: vec![1, 2, 4],
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    ) {
        CommandResult::Opened { session_id } => session_id,
        other => panic!("{other:?}"),
    };
    let scope = match terminal.dispatch(
        Command::CreateWorkScope {
            session_id: session,
            kind: WorkScopeKind::AdHoc,
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    ) {
        CommandResult::WorkScope { id } => id,
        other => panic!("{other:?}"),
    };
    let item = match terminal.dispatch(
        Command::CreateWorkItem {
            session_id: session,
            work_scope_id: scope,
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    ) {
        CommandResult::WorkItem { id } => id,
        other => panic!("{other:?}"),
    };
    let attempt = match terminal.dispatch(
        Command::CreateAttempt {
            session_id: session,
            work_item_id: item,
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    ) {
        CommandResult::Attempt { id } => id,
        other => panic!("{other:?}"),
    };
    let run_id = match terminal.dispatch(
        Command::StartAgentRun {
            session_id: session,
            attempt_id: attempt,
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    ) {
        CommandResult::Started { run_id, .. } => run_id,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        terminal.authority.liveness(run_id),
        RunLiveness::KnownTerminated
    );
    drop(terminal);

    let recovered_terminal =
        IntegrationService::open(BackendInstanceId::new(), &terminal_config).unwrap();
    assert_eq!(
        recovered_terminal.authority.liveness(run_id),
        RunLiveness::KnownTerminated,
        "recovered liveness must honor committed terminal observations"
    );

    let live_config = IntegrationConfig {
        store_path: live_path,
        script: vec![ScriptStep::Emit(HostObservationKind::Started)],
    };
    let mut live = IntegrationService::open(BackendInstanceId::new(), &live_config).unwrap();
    live.begin_connection(b"cli").unwrap();
    let session = match live.dispatch(
        Command::OpenSession {
            scopes: vec![1, 2, 4],
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    ) {
        CommandResult::Opened { session_id } => session_id,
        other => panic!("{other:?}"),
    };
    let scope = match live.dispatch(
        Command::CreateWorkScope {
            session_id: session,
            kind: WorkScopeKind::AdHoc,
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    ) {
        CommandResult::WorkScope { id } => id,
        other => panic!("{other:?}"),
    };
    let item = match live.dispatch(
        Command::CreateWorkItem {
            session_id: session,
            work_scope_id: scope,
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    ) {
        CommandResult::WorkItem { id } => id,
        other => panic!("{other:?}"),
    };
    let attempt = match live.dispatch(
        Command::CreateAttempt {
            session_id: session,
            work_item_id: item,
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    ) {
        CommandResult::Attempt { id } => id,
        other => panic!("{other:?}"),
    };
    let live_run = match live.dispatch(
        Command::StartAgentRun {
            session_id: session,
            attempt_id: attempt,
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    ) {
        CommandResult::Started { run_id, .. } => run_id,
        other => panic!("{other:?}"),
    };
    drop(live);
    let recovered_live = IntegrationService::open(BackendInstanceId::new(), &live_config).unwrap();
    assert_eq!(
        recovered_live.authority.liveness(live_run),
        RunLiveness::UnknownAfterCrash,
        "live/non-terminal recovery must not fabricate termination"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn durable_principals_survive_restart_and_revoke_denies_after_reopen() {
    let dir = temp_store();
    std::fs::create_dir_all(&dir).unwrap();
    let store_path = dir.join("agent.db");
    let config = IntegrationConfig {
        store_path: store_path.clone(),
        script: vec![ScriptStep::Emit(HostObservationKind::Started)],
    };
    let first_instance = BackendInstanceId::new();
    let mut service = IntegrationService::open(first_instance, &config).expect("open");
    let owner = service.owner_principal_id();
    service.begin_connection(b"cli").unwrap();
    let session = match service.dispatch(
        Command::OpenSession {
            scopes: vec![1, 2, 4],
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    ) {
        CommandResult::Opened { session_id } => session_id,
        other => panic!("{other:?}"),
    };
    service
        .set_principal_status(owner, PrincipalStatus::Revoked)
        .unwrap();
    drop(service);

    let second_instance = BackendInstanceId::new();
    let mut restarted = IntegrationService::open(second_instance, &config).expect("reopen");
    // Prior process session cannot resume under a new BackendInstanceId.
    restarted.begin_connection(b"cli").unwrap();
    assert_eq!(
        restarted.dispatch(
            Command::ResumeSession {
                session_id: session
            },
            32,
            ABSOLUTE_MAX_FRAME_SIZE,
        ),
        CommandResult::Error(CommandError::RejectedSession)
    );
    // Revoked principal cannot open a fresh privileged session.
    assert_eq!(
        restarted.dispatch(
            Command::OpenSession {
                scopes: vec![1, 2, 4],
            },
            32,
            ABSOLUTE_MAX_FRAME_SIZE,
        ),
        CommandResult::Error(CommandError::Denied)
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn pairing_secret_never_enters_event_payload() {
    let secret = crate::PairingCredential::new("super-secret-pairing-material");
    assert_eq!(secret.event_payload(), b"");
    let rendered = format!("{secret:?}");
    assert!(!rendered.contains("super-secret"));
    assert!(rendered.contains("redacted"));
}
