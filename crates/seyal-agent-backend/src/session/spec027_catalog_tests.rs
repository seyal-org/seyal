//! SPEC-027 §11 fixtures 2, 3, 4, 7, 10 — adapter-catalog resolution and
//! `adapter.execute` authorization wired through the real `StartAgentRun`
//! dispatch path (not just the pure `core::routing` resolver in isolation).

use super::*;
use crate::{FakeExecutionHost, ScriptStep};
use seyal_agent_core::{AdapterId, AgentRunLifecycle, AttemptId, RouteOfferingId, SelectionKind};
use seyal_agent_protocol::{BackendInstanceId, Command, CommandError, CommandResult};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_STORE: AtomicU64 = AtomicU64::new(1);

fn temp_store() -> PathBuf {
    std::env::temp_dir().join(format!(
        "seyal-session-spec027-catalog-{}-{}-{}",
        std::process::id(),
        NEXT_STORE.fetch_add(1, Ordering::Relaxed),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn open_bare() -> (PathBuf, IntegrationService) {
    let dir = temp_store();
    std::fs::create_dir_all(&dir).unwrap();
    let config = IntegrationConfig {
        store_path: dir.join("agent.db"),
    };
    let mut service = IntegrationService::open(BackendInstanceId::new(), &config).expect("open");
    let mut host = FakeExecutionHost::new(1024).unwrap();
    host.set_script(vec![ScriptStep::Emit(HostObservationKind::Started)]);
    service.install_execution_host(Box::new(host));
    (dir, service)
}

fn seed_attempt(
    service: &mut IntegrationService,
    principal: ClientPrincipalId,
) -> (ClientSessionId, AttemptId) {
    let CommandResult::Opened { session_id } = service.dispatch(
        principal,
        Command::OpenSession {
            scopes: vec![1, 2, 4],
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    ) else {
        panic!("open session");
    };
    // AdHoc, not Repository: these fixtures exercise adapter-catalog
    // resolution/authorization (SPEC-027 §4/§7), not cwd (§6, fixtures
    // 17/18). Repository/Project cwd fails closed without a
    // `WorkScope.bindings` root (see `launch_resolution`), which would
    // mask the resolution/authorization behavior under test here.
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
    let CommandResult::Attempt { id: attempt } = service.dispatch(
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
    (session_id, attempt)
}

fn seed_attempt_with_scope_kind(
    service: &mut IntegrationService,
    principal: ClientPrincipalId,
    kind: WorkScopeKind,
) -> (ClientSessionId, AttemptId) {
    let CommandResult::Opened { session_id } = service.dispatch(
        principal,
        Command::OpenSession {
            scopes: vec![1, 2, 4],
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    ) else {
        panic!("open session");
    };
    let CommandResult::WorkScope { id: scope } = service.dispatch(
        principal,
        Command::CreateWorkScope { session_id, kind },
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
    let CommandResult::Attempt { id: attempt } = service.dispatch(
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
    (session_id, attempt)
}

fn install_adapter(
    service: &mut IntegrationService,
    enabled: bool,
) -> (AdapterId, RouteOfferingId) {
    let adapter_id = AdapterId::new();
    let launch = seyal_agent_store::LaunchDescriptorTemplate::new(
        "/bin/echo",
        seyal_agent_store::CwdPolicy::AdapterWorkDir,
    )
    .with_argv(["spec027-catalog-fixture"]);
    service
        .store
        .install_or_update_adapter(adapter_id, 0, enabled, &launch)
        .expect("install adapter");
    let offering_id = RouteOfferingId::new();
    service
        .store
        .add_route_offering(offering_id, adapter_id, false)
        .expect("add offering");
    (adapter_id, offering_id)
}

/// Fixture 2: pin of an enabled offering with a host composed produces the
/// same AgentRun as the unpinned path; `SelectionKind::Pinned` is recorded.
#[test]
fn fixture_02_pin_of_enabled_offering_starts_with_pinned_selection() {
    let (dir, mut service) = open_bare();
    let (adapter_id, offering_id) = install_adapter(&mut service, true);
    service
        .auth
        .grant_adapter_execute(service.owner_principal_id, adapter_id)
        .expect("grant");
    let principal = service.begin_connection(b"cli").unwrap();
    let (session_id, attempt_id) = seed_attempt(&mut service, principal);

    let started = service.dispatch(
        principal,
        Command::StartAgentRun {
            session_id,
            attempt_id,
            route_offering_id: Some(offering_id),
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    );
    assert!(
        matches!(started, CommandResult::Started { .. }),
        "pinned start: {started:?}"
    );
    let runs = service.store.agent_runs().unwrap();
    assert_eq!(runs.len(), 1, "exactly one AgentRun minted");
    let run_id = runs[0].0;
    let routing = service
        .authority
        .domain()
        .agent_run(run_id)
        .and_then(|run| run.routing_decision_ref())
        .and_then(|reference| service.authority.domain().routing_decision(reference))
        .expect("routing decision recorded");
    assert_eq!(routing.selection_kind, SelectionKind::Pinned);
    assert_eq!(routing.adapter_id, adapter_id);
    assert_eq!(routing.route_offering_id, offering_id);
    let _ = std::fs::remove_dir_all(dir);
}

/// Fixture 3: unpinned start with two eligible offerings has no ranking
/// (SPEC-027 §4.3) and must fail closed without minting an AgentRun.
#[test]
fn fixture_03_two_eligible_offerings_unpinned_is_unavailable_no_mint() {
    let (dir, mut service) = open_bare();
    let (adapter_a, _offering_a) = install_adapter(&mut service, true);
    let (adapter_b, _offering_b) = install_adapter(&mut service, true);
    service
        .auth
        .grant_adapter_execute(service.owner_principal_id, adapter_a)
        .expect("grant a");
    service
        .auth
        .grant_adapter_execute(service.owner_principal_id, adapter_b)
        .expect("grant b");
    let principal = service.begin_connection(b"cli").unwrap();
    let (session_id, attempt_id) = seed_attempt(&mut service, principal);

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
    assert!(service.store.agent_runs().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// Fixture 4: unpinned start with exactly one eligible offering resolves to
/// `SelectionKind::Singleton` and starts the same AgentRun.
#[test]
fn fixture_04_exactly_one_eligible_offering_unpinned_is_singleton() {
    let (dir, mut service) = open_bare();
    let (adapter_id, offering_id) = install_adapter(&mut service, true);
    service
        .auth
        .grant_adapter_execute(service.owner_principal_id, adapter_id)
        .expect("grant");
    let principal = service.begin_connection(b"cli").unwrap();
    let (session_id, attempt_id) = seed_attempt(&mut service, principal);

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
    assert!(matches!(started, CommandResult::Started { .. }));
    let runs = service.store.agent_runs().unwrap();
    assert_eq!(runs.len(), 1);
    let run_id = runs[0].0;
    let routing = service
        .authority
        .domain()
        .agent_run(run_id)
        .and_then(|run| run.routing_decision_ref())
        .and_then(|reference| service.authority.domain().routing_decision(reference))
        .expect("routing decision recorded");
    assert_eq!(routing.selection_kind, SelectionKind::Singleton);
    assert_eq!(routing.route_offering_id, offering_id);
    let _ = std::fs::remove_dir_all(dir);
}

/// Fixture 7: `runs.create` and `adapter.execute` are independent grants
/// (D3). A session that can create runs but was never granted execute on
/// the resolved adapter must be denied, with no AgentRun minted.
#[test]
fn fixture_07_runs_create_without_adapter_execute_is_denied_no_mint() {
    let (dir, mut service) = open_bare();
    let (_adapter_id, _offering_id) = install_adapter(&mut service, true);
    // Deliberately no `grant_adapter_execute` call.
    let principal = service.begin_connection(b"cli").unwrap();
    let (session_id, attempt_id) = seed_attempt(&mut service, principal);

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
        CommandResult::Error(CommandError::AdapterExecuteDenied)
    );
    assert!(service.store.agent_runs().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// Fixture 19: a host that proves it never started (typed not-started, no
/// side effects) demotes the same AgentRun `Dispatching -> Prepared` with a
/// freshly minted RoutingDecision instead of leaving it stuck or fabricating
/// a start, so a later retry has a clean lifecycle to resume from.
#[test]
fn fixture_19_pre_start_not_started_demotes_to_prepared_same_run() {
    let (dir, mut service) = open_bare();
    let (adapter_id, _offering_id) = install_adapter(&mut service, true);
    service
        .auth
        .grant_adapter_execute(service.owner_principal_id, adapter_id)
        .expect("grant");
    let mut host = FakeExecutionHost::new(1024).unwrap();
    host.force_not_started(crate::HostNotStartedReason::TtyRequired);
    service.install_execution_host(Box::new(host));
    let principal = service.begin_connection(b"cli").unwrap();
    let (session_id, attempt_id) = seed_attempt(&mut service, principal);

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
    let runs = service.store.agent_runs().unwrap();
    assert_eq!(
        runs.len(),
        1,
        "same AgentRun stays minted, not retried blind"
    );
    let run_id = runs[0].0;
    let run = service
        .authority
        .domain()
        .agent_run(run_id)
        .expect("run present");
    assert_eq!(run.lifecycle(), AgentRunLifecycle::Prepared);
    assert!(
        run.routing_decision_ref().is_some(),
        "a RoutingDecision was minted even for the not-started attempt"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// Fixture 10: pinning a disabled adapter's offering is rejected before any
/// mint, distinctly from the generic unavailable code.
#[test]
fn fixture_10_pin_of_disabled_adapter_is_adapter_not_enabled_no_mint() {
    let (dir, mut service) = open_bare();
    let (adapter_id, offering_id) = install_adapter(&mut service, false);
    service
        .auth
        .grant_adapter_execute(service.owner_principal_id, adapter_id)
        .expect("grant");
    let principal = service.begin_connection(b"cli").unwrap();
    let (session_id, attempt_id) = seed_attempt(&mut service, principal);

    let started = service.dispatch(
        principal,
        Command::StartAgentRun {
            session_id,
            attempt_id,
            route_offering_id: Some(offering_id),
        },
        32,
        ABSOLUTE_MAX_FRAME_SIZE,
    );
    assert_eq!(
        started,
        CommandResult::Error(CommandError::AdapterNotEnabled)
    );
    assert!(service.store.agent_runs().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// Fixture 17 (dispatch-level): a `Repository` WorkScope has no bound root
/// (no `WorkScope.bindings` subsystem exists), so SPEC-027 §6's own
/// prescribed failure mode applies — cwd resolution fails closed before any
/// mint, exactly like any other unavailable target. This exercises the real
/// `StartAgentRun` dispatch path, not just `launch_resolution`'s pure
/// function in isolation.
#[test]
fn fixture_17_repository_work_scope_has_no_bound_root_fails_closed_no_mint() {
    let (dir, mut service) = open_bare();
    let (adapter_id, _offering_id) = install_adapter(&mut service, true);
    service
        .auth
        .grant_adapter_execute(service.owner_principal_id, adapter_id)
        .expect("grant");
    let principal = service.begin_connection(b"cli").unwrap();
    let (session_id, attempt_id) =
        seed_attempt_with_scope_kind(&mut service, principal, WorkScopeKind::Repository);

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
        "cwd resolution fails before any AgentRun mint"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// Fixture 18 (dispatch-level): an `AdHoc` WorkScope resolves cwd to the
/// backend-owned `AdapterWorkDir` under the daemon's own data directory
/// (never `$HOME`, never a client-supplied path), and the real
/// `StartAgentRun` dispatch path actually creates that directory on disk
/// before the host is started — not just the pure `launch_resolution`
/// function in isolation.
#[test]
fn fixture_18_adhoc_work_scope_resolves_cwd_to_adapter_work_dir_on_disk() {
    let (dir, mut service) = open_bare();
    let (adapter_id, _offering_id) = install_adapter(&mut service, true);
    service
        .auth
        .grant_adapter_execute(service.owner_principal_id, adapter_id)
        .expect("grant");
    let principal = service.begin_connection(b"cli").unwrap();
    let (session_id, attempt_id) =
        seed_attempt_with_scope_kind(&mut service, principal, WorkScopeKind::AdHoc);

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
    assert!(
        matches!(started, CommandResult::Started { .. }),
        "adhoc start: {started:?}"
    );
    let expected_cwd =
        super::launch_resolution::adapter_work_dir(&service.adapter_work_root, adapter_id);
    assert!(
        expected_cwd.is_dir(),
        "expected AdapterWorkDir {expected_cwd:?} to exist on disk after dispatch"
    );
    let _ = std::fs::remove_dir_all(dir);
}
