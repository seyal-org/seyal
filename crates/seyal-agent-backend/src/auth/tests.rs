use super::*;

#[test]
fn session_scopes_can_narrow_but_never_widen_principal_grant() {
    let mut repo = AuthorizationRepository::default();
    let principal = repo.register_principal(
        PrincipalKind::UserApprovedLocalClient,
        [ClientScope::RunsObserve],
    );
    let backend = BackendInstanceId::new();

    assert!(repo
        .open_session(principal, backend, [ClientScope::RunsObserve])
        .is_ok());
    assert_eq!(
        repo.open_session(principal, backend, [ClientScope::RunsControl]),
        Err(AuthorizationError::ScopeEscalation)
    );
}

#[test]
fn observe_only_session_cannot_control_and_exact_target_is_required() {
    let mut repo = AuthorizationRepository::default();
    let principal = repo.register_principal(
        PrincipalKind::UserApprovedLocalClient,
        [ClientScope::RunsObserve, ClientScope::RunsControl],
    );
    let allowed = AgentRunId::new();
    let foreign = AgentRunId::new();
    repo.allow_run(principal, allowed).unwrap();
    let backend = BackendInstanceId::new();
    let session = repo
        .open_session(principal, backend, [ClientScope::RunsObserve])
        .unwrap();

    assert_eq!(
        repo.authorize_run(
            session,
            backend,
            ClientScope::RunsControl,
            allowed,
            principal
        ),
        Err(AuthorizationError::ScopeDenied)
    );
    assert_eq!(
        repo.authorize_run(
            session,
            backend,
            ClientScope::RunsObserve,
            foreign,
            principal
        ),
        Err(AuthorizationError::TargetDenied)
    );
    assert!(repo
        .authorize_run(
            session,
            backend,
            ClientScope::RunsObserve,
            allowed,
            principal
        )
        .is_ok());
}

#[test]
fn backend_restart_fences_old_session_and_revocation_is_immediate() {
    let mut repo = AuthorizationRepository::default();
    let principal =
        repo.register_principal(PrincipalKind::FirstPartyCli, [ClientScope::RunsControl]);
    let run = AgentRunId::new();
    repo.allow_run(principal, run).unwrap();

    let first_backend = BackendInstanceId::new();
    let session = repo
        .open_session(principal, first_backend, [ClientScope::RunsControl])
        .unwrap();
    let restarted_backend = BackendInstanceId::new();

    assert_eq!(
        repo.authorize_run(
            session,
            restarted_backend,
            ClientScope::RunsControl,
            run,
            principal
        ),
        Err(AuthorizationError::StaleBackendInstance)
    );

    repo.set_principal_status(principal, PrincipalStatus::Revoked)
        .unwrap();
    assert_eq!(
        repo.authorize_run(
            session,
            first_backend,
            ClientScope::RunsControl,
            run,
            principal
        ),
        Err(AuthorizationError::PrincipalInactive)
    );
}

#[test]
fn one_client_cannot_control_another_run_and_replay_fails_closed() {
    let mut repo = AuthorizationRepository::default();
    let client_a =
        repo.register_principal(PrincipalKind::FirstPartyCli, [ClientScope::RunsControl]);
    let client_b = repo.register_principal(
        PrincipalKind::UserApprovedLocalClient,
        [ClientScope::RunsControl],
    );
    let run_a = AgentRunId::new();
    let run_b = AgentRunId::new();
    repo.allow_run(client_a, run_a).unwrap();
    repo.allow_run(client_b, run_b).unwrap();
    let backend = BackendInstanceId::new();
    let session_a = repo
        .open_session(client_a, backend, [ClientScope::RunsControl])
        .unwrap();

    assert_eq!(
        repo.authorize_control(session_a, backend, run_b, 1, client_a),
        Err(AuthorizationError::TargetDenied)
    );
    repo.authorize_control(session_a, backend, run_a, 1, client_a)
        .unwrap();
    assert_eq!(
        repo.authorize_control(session_a, backend, run_a, 1, client_a),
        Err(AuthorizationError::ReplayedRequest)
    );
}

#[test]
fn hello_evidence_selects_distinct_principals_and_rejects_unknown() {
    let mut repo = AuthorizationRepository::default();
    let owner = repo.register_principal(
        PrincipalKind::FirstPartyCli,
        [
            ClientScope::RunsCreate,
            ClientScope::RunsObserve,
            ClientScope::RunsControl,
        ],
    );
    let observer =
        repo.register_principal(PrincipalKind::ManagedClient, [ClientScope::RunsObserve]);
    assert_eq!(repo.principal_for_evidence(&[], owner, observer), Ok(owner));
    assert_eq!(
        repo.principal_for_evidence(b"cli", owner, observer),
        Ok(owner)
    );
    assert_eq!(
        repo.principal_for_evidence(b"observer", owner, observer),
        Ok(observer)
    );
    assert_eq!(
        AuthorizationRepository::recognize_principal_evidence(b"unpaired"),
        Err(AuthorizationError::TransportAdmissionIsNotAuthorization)
    );
    let run = AgentRunId::new();
    repo.allow_run_for_observers(run);
    let backend = BackendInstanceId::new();
    let session = repo
        .open_session(observer, backend, [ClientScope::RunsObserve])
        .unwrap();
    repo.authorize_run(session, backend, ClientScope::RunsObserve, run, observer)
        .unwrap();
    assert_eq!(
        repo.open_session(
            observer,
            backend,
            [ClientScope::RunsCreate, ClientScope::RunsObserve]
        ),
        Err(AuthorizationError::ScopeEscalation)
    );
}

#[test]
fn same_uid_and_repository_content_do_not_grant_authority() {
    let mut repo = AuthorizationRepository::default();
    assert_eq!(
        repo.authorize_local_uid(501, ClientScope::RunsControl),
        Err(AuthorizationError::TransportAdmissionIsNotAuthorization)
    );
    assert_eq!(
        repo.register_principal_from_repository(b"{\"scope\":\"runs.control\"}"),
        Err(AuthorizationError::RepositoryCannotRegisterPrincipal)
    );
    let managed = repo.register_principal(PrincipalKind::ManagedClient, []);
    assert_eq!(
        repo.principal_kind(managed),
        Some(PrincipalKind::ManagedClient)
    );
    let backend = BackendInstanceId::new();
    assert_eq!(
        repo.open_session(managed, backend, [ClientScope::RunsObserve]),
        Err(AuthorizationError::ScopeEscalation)
    );
}

#[test]
fn suspended_principal_is_denied_and_scope_bytes_fail_closed() {
    let mut repo = AuthorizationRepository::default();
    let principal =
        repo.register_principal(PrincipalKind::FirstPartySeyal, [ClientScope::RunsObserve]);
    repo.set_principal_status(principal, PrincipalStatus::Suspended)
        .unwrap();
    assert_eq!(
        repo.open_session(
            principal,
            BackendInstanceId::new(),
            [ClientScope::RunsObserve]
        ),
        Err(AuthorizationError::PrincipalInactive)
    );
    let mut state = 0x1234_u64;
    for _ in 0..256 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let byte = (state >> 33) as u8;
        match ClientScope::decode(byte) {
            Ok(scope) => {
                assert!((1..=4).contains(&byte) && format!("{scope:?}").starts_with("Runs"))
            }
            Err(AuthorizationError::Malformed) => assert!(byte == 0 || byte > 4),
            Err(other) => panic!("unexpected {other:?}"),
        }
    }
    let secret = PairingCredential::new("super-secret-token");
    let rendered = format!("{secret:?}");
    assert!(!rendered.contains("super-secret-token"));
    assert!(secret.event_payload().is_empty());
}

#[test]
fn foreign_principal_cannot_use_another_principals_session() {
    let mut repo = AuthorizationRepository::default();
    let owner = repo.register_principal(
        PrincipalKind::FirstPartyCli,
        [
            ClientScope::RunsCreate,
            ClientScope::RunsObserve,
            ClientScope::RunsControl,
        ],
    );
    let observer = repo.register_principal(
        PrincipalKind::ManagedClient,
        [ClientScope::RunsObserve, ClientScope::RunsControl],
    );
    let run = AgentRunId::new();
    repo.allow_run(owner, run).unwrap();
    repo.allow_run(observer, run).unwrap();
    let backend = BackendInstanceId::new();
    let session = repo
        .open_session(
            owner,
            backend,
            [
                ClientScope::RunsCreate,
                ClientScope::RunsObserve,
                ClientScope::RunsControl,
            ],
        )
        .unwrap();

    assert_eq!(
        repo.authorize_session(session, backend, ClientScope::RunsCreate, observer),
        Err(AuthorizationError::SessionPrincipalMismatch)
    );
    assert_eq!(
        repo.authorize_run(session, backend, ClientScope::RunsObserve, run, observer),
        Err(AuthorizationError::SessionPrincipalMismatch)
    );
    assert_eq!(
        repo.authorize_run(session, backend, ClientScope::RunsControl, run, observer),
        Err(AuthorizationError::SessionPrincipalMismatch)
    );
    assert_eq!(
        repo.authorize_control(session, backend, run, 1, observer),
        Err(AuthorizationError::SessionPrincipalMismatch)
    );
    assert_eq!(
        repo.resume_session(session, backend, observer),
        Err(AuthorizationError::SessionPrincipalMismatch)
    );
    repo.authorize_control(session, backend, run, 1, owner)
        .unwrap();
}

#[test]
fn foreign_session_rejected_before_scope_target_and_status() {
    let mut repo = AuthorizationRepository::default();
    let owner = repo.register_principal(
        PrincipalKind::FirstPartyCli,
        [
            ClientScope::RunsCreate,
            ClientScope::RunsObserve,
            ClientScope::RunsControl,
        ],
    );
    let observer =
        repo.register_principal(PrincipalKind::ManagedClient, [ClientScope::RunsObserve]);
    let foreign_run = AgentRunId::new();
    let backend = BackendInstanceId::new();
    let other_backend = BackendInstanceId::new();
    let session = repo
        .open_session(owner, backend, [ClientScope::RunsObserve])
        .unwrap();
    repo.set_principal_status(owner, PrincipalStatus::Revoked)
        .unwrap();

    assert_eq!(
        repo.authorize_run(
            session,
            other_backend,
            ClientScope::RunsControl,
            foreign_run,
            observer
        ),
        Err(AuthorizationError::SessionPrincipalMismatch)
    );
    assert_eq!(
        repo.authorize_control(session, other_backend, foreign_run, 99, observer),
        Err(AuthorizationError::SessionPrincipalMismatch)
    );
    assert_eq!(
        repo.authorize_session(session, other_backend, ClientScope::RunsCreate, observer),
        Err(AuthorizationError::SessionPrincipalMismatch)
    );
    assert_eq!(
        repo.resume_session(session, other_backend, observer),
        Err(AuthorizationError::SessionPrincipalMismatch)
    );
    assert_eq!(
        repo.authorize_run(
            session,
            backend,
            ClientScope::RunsObserve,
            foreign_run,
            owner
        ),
        Err(AuthorizationError::PrincipalInactive)
    );
}

#[test]
fn session_is_principal_bound_not_connection_bound() {
    let mut repo = AuthorizationRepository::default();
    let owner = repo.register_principal(
        PrincipalKind::FirstPartyCli,
        [
            ClientScope::RunsCreate,
            ClientScope::RunsObserve,
            ClientScope::RunsControl,
        ],
    );
    let run = AgentRunId::new();
    repo.allow_run(owner, run).unwrap();
    let backend = BackendInstanceId::new();
    let session = repo
        .open_session(
            owner,
            backend,
            [
                ClientScope::RunsCreate,
                ClientScope::RunsObserve,
                ClientScope::RunsControl,
            ],
        )
        .unwrap();
    let caller_on_another_connection = owner;
    assert_eq!(
        repo.authorize_session(
            session,
            backend,
            ClientScope::RunsCreate,
            caller_on_another_connection
        ),
        Ok(owner)
    );
    repo.authorize_run(
        session,
        backend,
        ClientScope::RunsObserve,
        run,
        caller_on_another_connection,
    )
    .unwrap();
    repo.resume_session(session, backend, caller_on_another_connection)
        .unwrap();
    repo.authorize_control(session, backend, run, 1, caller_on_another_connection)
        .unwrap();
}

#[test]
fn admin_adapters_grant_is_first_party_only_and_required() {
    let mut repo = AuthorizationRepository::default();
    let owner = repo.register_principal(
        PrincipalKind::FirstPartyCli,
        [ClientScope::RunsCreate, ClientScope::RunsObserve],
    );
    let managed = repo.register_principal(PrincipalKind::ManagedClient, [ClientScope::RunsObserve]);
    assert_eq!(
        repo.authorize_admin_adapters(owner),
        Err(AuthorizationError::TargetDenied)
    );
    repo.grant_admin_adapters(owner).unwrap();
    assert_eq!(repo.authorize_admin_adapters(owner), Ok(()));
    assert_eq!(
        repo.grant_admin_adapters(managed),
        Err(AuthorizationError::ScopeEscalation)
    );
}
