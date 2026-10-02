use super::*;
use seyal_agent_core::{AttemptId, WorkItemId, WorkScopeId};
use seyal_agent_protocol::BackendInstanceId;
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
