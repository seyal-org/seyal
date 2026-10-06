//! Hostless StartAgentRun checks (AB-1.9 AC2/AC4) — no fixture-host required.

use super::*;
use seyal_agent_protocol::{
    BackendInstanceId, Command, CommandError, CommandResult, ABSOLUTE_MAX_FRAME_SIZE,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_STORE: AtomicU64 = AtomicU64::new(1);

fn temp_store() -> PathBuf {
    std::env::temp_dir().join(format!(
        "seyal-session-hostless-{}-{}-{}",
        std::process::id(),
        NEXT_STORE.fetch_add(1, Ordering::Relaxed),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn open_hostless() -> (PathBuf, IntegrationService, ClientPrincipalId) {
    let dir = temp_store();
    std::fs::create_dir_all(&dir).unwrap();
    let config = IntegrationConfig {
        store_path: dir.join("agent.db"),
    };
    let mut service =
        IntegrationService::open(BackendInstanceId::new(), &config).expect("open hostless");
    let principal = service.begin_connection(b"cli").expect("principal");
    (dir, service, principal)
}

fn seed_attempt(service: &mut IntegrationService, principal: ClientPrincipalId) -> ClientSessionId {
    let opened = service.dispatch(
        principal,
        Command::OpenSession {
            scopes: vec![1, 2, 4],
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    );
    let CommandResult::Opened { session_id } = opened else {
        panic!("open session: {opened:?}");
    };
    // AdHoc: these tests exercise hostless/host-dispatch gating (AB-1.9
    // AC2/AC4), not cwd (§6, fixtures 17/18). Repository/Project cwd fails
    // closed without a `WorkScope.bindings` root.
    let CommandResult::WorkScope { id: scope } = service.dispatch(
        principal,
        Command::CreateWorkScope {
            session_id,
            kind: WorkScopeKind::AdHoc,
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    ) else {
        panic!("scope");
    };
    let CommandResult::WorkItem { id: item } = service.dispatch(
        principal,
        Command::CreateWorkItem {
            session_id,
            work_scope_id: scope,
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    ) else {
        panic!("item");
    };
    let CommandResult::Attempt { .. } = service.dispatch(
        principal,
        Command::CreateAttempt {
            session_id,
            work_item_id: item,
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    ) else {
        panic!("attempt");
    };
    session_id
}

/// AB-1.9's hostless path (no composed host, no mint) is unchanged by
/// SPEC-027; only the wire error code changes from generic `Failed` to the
/// typed `ExecutionTargetUnavailable` (§8.1, §13, fixture 1).
#[test]
fn start_agent_run_without_host_fails_closed_with_no_agent_run() {
    let (dir, mut service, principal) = open_hostless();
    let session_id = seed_attempt(&mut service, principal);
    let attempt_id = service.store.attempts().unwrap()[0].0;

    let started = service.dispatch(
        principal,
        Command::StartAgentRun {
            session_id,
            attempt_id,
            route_offering_id: None,
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    );
    assert_eq!(
        started,
        CommandResult::Error(CommandError::ExecutionTargetUnavailable)
    );
    assert!(
        service.store.agent_runs().unwrap().is_empty(),
        "hostless ExecutionTargetUnavailable must not mint AgentRun"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn missing_attempt_is_not_found_not_failed_when_hostless() {
    let (dir, mut service, principal) = open_hostless();
    let session_id = seed_attempt(&mut service, principal);
    let missing = AttemptId::new();
    let result = service.dispatch(
        principal,
        Command::StartAgentRun {
            session_id,
            attempt_id: missing,
            route_offering_id: None,
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    );
    assert_eq!(result, CommandResult::Error(CommandError::NotFound));
    assert!(service.store.agent_runs().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn foreign_session_is_rejected_not_failed_when_hostless() {
    let (dir, mut service, principal) = open_hostless();
    let _ = seed_attempt(&mut service, principal);
    let foreign = ClientSessionId::new();
    let attempt_id = service.store.attempts().unwrap()[0].0;
    let result = service.dispatch(
        principal,
        Command::StartAgentRun {
            session_id: foreign,
            attempt_id,
            route_offering_id: None,
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    );
    assert_eq!(result, CommandResult::Error(CommandError::RejectedSession));
    assert!(service.store.agent_runs().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn prepare_action_persists_intent_without_starting_a_host() {
    use seyal_agent_core::{
        ActionId, ActionIntent, ArgumentFingerprint, AuthorizationClass, CapabilityRef,
        EffectClass, PrivacyDependencyId, RequestProvenance, ResourceIdentity, RevocationFence,
        RevocationFenceMember, RevocationGeneration, ScopeIdentity, ScopeKind,
    };

    let (dir, mut service, principal) = open_hostless();
    let _ = seed_attempt(&mut service, principal);
    let attempt_id = service.store.attempts().unwrap()[0].0;
    let run = AgentRunId::new();
    service
        .store
        .mutate_agent_run_and_append(run, attempt_id, 1, 1, 1, b"run")
        .unwrap();
    let intent = ActionIntent::prepare(
        ActionId::new(),
        run,
        CapabilityRef::new(b"fs.write").unwrap(),
        ResourceIdentity::new(b"file", [1; 16], [2; 16], [3; 32]).unwrap(),
        ArgumentFingerprint::of(b"mkdir"),
        EffectClass::NonReplayable,
        1,
        PrivacyDependencyId([9; 16]),
        RevocationFence::new(vec![RevocationFenceMember {
            scope: ScopeIdentity::new(ScopeKind::Workspace, [4; 16]),
            generation: RevocationGeneration::FIRST,
        }])
        .unwrap(),
        RequestProvenance::AgentBackend,
        AuthorizationClass::HumanApproval,
        1,
        None,
        None,
    )
    .unwrap();
    assert_eq!(
        service.prepare_action(&intent).unwrap(),
        seyal_agent_store::PrepareOutcome::NewlyPrepared
    );
    assert!(service.host.is_none());
    let loaded = service
        .store
        .actions()
        .get(intent.action_id())
        .unwrap()
        .unwrap();
    assert_eq!(loaded, intent);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn recover_action_does_not_start_a_host_and_never_retries_unknown_effects() {
    use seyal_agent_core::{
        ActionId, ActionIntent, ActionLifecycle, ArgumentFingerprint, AuthorizationClass,
        CapabilityRef, CrashBoundary, EffectClass, PrivacyDependencyId, RecoveryEvidence,
        RequestProvenance, ResourceIdentity, RevocationFence, RevocationFenceMember,
        RevocationGeneration, ScopeIdentity, ScopeKind,
    };

    let (dir, mut service, principal) = open_hostless();
    let _ = seed_attempt(&mut service, principal);
    let attempt_id = service.store.attempts().unwrap()[0].0;
    let run = AgentRunId::new();
    service
        .store
        .mutate_agent_run_and_append(run, attempt_id, 1, 1, 1, b"run")
        .unwrap();
    let action_id = ActionId::new();
    let intent = ActionIntent::prepare(
        action_id,
        run,
        CapabilityRef::new(b"fs.write").unwrap(),
        ResourceIdentity::new(b"file", [1; 16], [2; 16], [3; 32]).unwrap(),
        ArgumentFingerprint::of(b"rm -rf"),
        EffectClass::NonReplayable,
        1,
        PrivacyDependencyId([9; 16]),
        RevocationFence::new(vec![RevocationFenceMember {
            scope: ScopeIdentity::new(ScopeKind::Workspace, [4; 16]),
            generation: RevocationGeneration::FIRST,
        }])
        .unwrap(),
        RequestProvenance::AgentBackend,
        AuthorizationClass::HumanApproval,
        1,
        None,
        None,
    )
    .unwrap();
    service.prepare_action(&intent).unwrap();
    service
        .store
        .actions()
        .fixture_mark_authorized(action_id)
        .unwrap();
    service
        .store
        .actions()
        .fixture_mark_dispatching(action_id, 3)
        .unwrap();
    let decision = service
        .recover_action(
            action_id,
            CrashBoundary::AfterEffectBeforeResultPersist,
            &RecoveryEvidence::none(),
        )
        .unwrap();
    assert_eq!(decision.runtime.lifecycle, ActionLifecycle::EffectUnknown);
    assert!(!decision.may_retry_effect);
    assert!(service.host.is_none());
    let digest_before = intent.digest();
    let loaded = service.store.actions().get(action_id).unwrap().unwrap();
    assert_eq!(loaded.digest(), digest_before);
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(feature = "fixture-host")]
#[test]
fn start_agent_run_uses_injected_host_script_via_collect_observations() {
    use crate::{FakeExecutionHost, ScriptStep};

    let dir = temp_store();
    std::fs::create_dir_all(&dir).unwrap();
    let config = IntegrationConfig {
        store_path: dir.join("agent.db"),
    };
    let mut service = IntegrationService::open(BackendInstanceId::new(), &config).expect("open");
    // Distinct from any historical default (Started + 0x05 output bytes).
    let mut host = FakeExecutionHost::new(1024).unwrap();
    host.set_script(vec![
        ScriptStep::Emit(HostObservationKind::Started),
        ScriptStep::Emit(HostObservationKind::Progress { step: 7 }),
        ScriptStep::Emit(HostObservationKind::KnownSuccess),
    ]);
    service.install_execution_host(Box::new(host));
    service.install_default_adapter_catalog_for_tests();
    let principal = service.begin_connection(b"cli").unwrap();
    let session_id = seed_attempt(&mut service, principal);
    let attempt_id = service.store.attempts().unwrap()[0].0;
    let started = service.dispatch(
        principal,
        Command::StartAgentRun {
            session_id,
            attempt_id,
            route_offering_id: None,
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    );
    let CommandResult::Started { event_count, .. } = started else {
        panic!("expected Started via injected host: {started:?}");
    };
    // create-run append + three injected observations (lifecycle columns are
    // updated without extra outbox rows on the start path).
    assert_eq!(event_count, 4);
    assert_eq!(service.store.agent_runs().unwrap().len(), 1);
    assert_eq!(service.authority.applied_count(), 3);
    let _ = std::fs::remove_dir_all(dir);
}
