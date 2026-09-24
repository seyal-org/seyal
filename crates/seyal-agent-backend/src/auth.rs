use std::{
    collections::{BTreeSet, HashMap},
    fmt,
};

use seyal_agent_core::{AgentRunId, BackendInstanceId, ClientPrincipalId, ClientSessionId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ClientScope {
    RunsCreate,
    RunsObserve,
    RunsInteract,
    RunsControl,
}

impl ClientScope {
    pub fn decode(byte: u8) -> Result<Self, AuthorizationError> {
        match byte {
            1 => Ok(Self::RunsCreate),
            2 => Ok(Self::RunsObserve),
            3 => Ok(Self::RunsInteract),
            4 => Ok(Self::RunsControl),
            _ => Err(AuthorizationError::Malformed),
        }
    }
}

/// Pairing material is never written into events or `Debug` output.
pub struct PairingCredential(String);

impl PairingCredential {
    pub fn new(secret: impl Into<String>) -> Self {
        Self(secret.into())
    }

    pub fn event_payload(&self) -> &'static [u8] {
        b""
    }
}

impl fmt::Debug for PairingCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let _secret_is_not_rendered = self.0.len();
        formatter.write_str("PairingCredential([redacted])")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrincipalKind {
    FirstPartyCli,
    FirstPartySeyal,
    UserApprovedLocalClient,
    ManagedClient,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrincipalStatus {
    Active,
    Suspended,
    Revoked,
}

#[derive(Clone, Debug)]
struct Principal {
    kind: PrincipalKind,
    status: PrincipalStatus,
    scopes: BTreeSet<ClientScope>,
    allowed_runs: BTreeSet<AgentRunId>,
}

#[derive(Clone, Debug)]
struct Session {
    principal_id: ClientPrincipalId,
    backend_instance_id: BackendInstanceId,
    scopes: BTreeSet<ClientScope>,
    next_control_nonce: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthorizationError {
    UnknownPrincipal,
    UnknownSession,
    PrincipalInactive,
    ScopeEscalation,
    ScopeDenied,
    TargetDenied,
    StaleBackendInstance,
    Malformed,
    ReplayedRequest,
    TransportAdmissionIsNotAuthorization,
    RepositoryCannotRegisterPrincipal,
}

#[derive(Default)]
pub struct AuthorizationRepository {
    principals: HashMap<ClientPrincipalId, Principal>,
    sessions: HashMap<ClientSessionId, Session>,
}

impl AuthorizationRepository {
    pub fn register_principal(
        &mut self,
        kind: PrincipalKind,
        scopes: impl IntoIterator<Item = ClientScope>,
    ) -> ClientPrincipalId {
        let id = ClientPrincipalId::new();
        self.principals.insert(
            id,
            Principal {
                kind,
                status: PrincipalStatus::Active,
                scopes: scopes.into_iter().collect(),
                allowed_runs: BTreeSet::new(),
            },
        );
        id
    }

    pub fn principal_kind(&self, id: ClientPrincipalId) -> Option<PrincipalKind> {
        self.principals.get(&id).map(|principal| principal.kind)
    }

    pub fn set_principal_status(
        &mut self,
        id: ClientPrincipalId,
        status: PrincipalStatus,
    ) -> Result<(), AuthorizationError> {
        let principal = self
            .principals
            .get_mut(&id)
            .ok_or(AuthorizationError::UnknownPrincipal)?;
        principal.status = status;
        Ok(())
    }

    pub fn allow_run(
        &mut self,
        id: ClientPrincipalId,
        run_id: AgentRunId,
    ) -> Result<(), AuthorizationError> {
        let principal = self
            .principals
            .get_mut(&id)
            .ok_or(AuthorizationError::UnknownPrincipal)?;
        principal.allowed_runs.insert(run_id);
        Ok(())
    }

    pub fn open_session(
        &mut self,
        principal_id: ClientPrincipalId,
        backend_instance_id: BackendInstanceId,
        requested_scopes: impl IntoIterator<Item = ClientScope>,
    ) -> Result<ClientSessionId, AuthorizationError> {
        let principal = self
            .principals
            .get(&principal_id)
            .ok_or(AuthorizationError::UnknownPrincipal)?;
        if principal.status != PrincipalStatus::Active {
            return Err(AuthorizationError::PrincipalInactive);
        }

        let scopes: BTreeSet<_> = requested_scopes.into_iter().collect();
        if !scopes.is_subset(&principal.scopes) {
            return Err(AuthorizationError::ScopeEscalation);
        }

        let id = ClientSessionId::new();
        self.sessions.insert(
            id,
            Session {
                principal_id,
                backend_instance_id,
                scopes,
                next_control_nonce: 1,
            },
        );
        Ok(id)
    }

    pub fn authorize_local_uid(
        &self,
        _uid: u32,
        _scope: ClientScope,
    ) -> Result<(), AuthorizationError> {
        Err(AuthorizationError::TransportAdmissionIsNotAuthorization)
    }

    pub fn register_principal_from_repository(
        &mut self,
        _manifest: &[u8],
    ) -> Result<ClientPrincipalId, AuthorizationError> {
        Err(AuthorizationError::RepositoryCannotRegisterPrincipal)
    }

    pub fn authorize_run(
        &self,
        session_id: ClientSessionId,
        backend_instance_id: BackendInstanceId,
        required_scope: ClientScope,
        run_id: AgentRunId,
    ) -> Result<(), AuthorizationError> {
        let session = self
            .sessions
            .get(&session_id)
            .ok_or(AuthorizationError::UnknownSession)?;
        if session.backend_instance_id != backend_instance_id {
            return Err(AuthorizationError::StaleBackendInstance);
        }
        if !session.scopes.contains(&required_scope) {
            return Err(AuthorizationError::ScopeDenied);
        }

        let principal = self
            .principals
            .get(&session.principal_id)
            .ok_or(AuthorizationError::UnknownPrincipal)?;
        if principal.status != PrincipalStatus::Active {
            return Err(AuthorizationError::PrincipalInactive);
        }
        if !principal.allowed_runs.contains(&run_id) {
            return Err(AuthorizationError::TargetDenied);
        }

        Ok(())
    }

    pub fn authorize_control(
        &mut self,
        session_id: ClientSessionId,
        backend_instance_id: BackendInstanceId,
        run_id: AgentRunId,
        nonce: u64,
    ) -> Result<(), AuthorizationError> {
        self.authorize_run(
            session_id,
            backend_instance_id,
            ClientScope::RunsControl,
            run_id,
        )?;
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or(AuthorizationError::UnknownSession)?;
        if nonce != session.next_control_nonce {
            return Err(AuthorizationError::ReplayedRequest);
        }
        session.next_control_nonce = session
            .next_control_nonce
            .checked_add(1)
            .ok_or(AuthorizationError::Malformed)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
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
            repo.authorize_run(session, backend, ClientScope::RunsControl, allowed),
            Err(AuthorizationError::ScopeDenied)
        );
        assert_eq!(
            repo.authorize_run(session, backend, ClientScope::RunsObserve, foreign),
            Err(AuthorizationError::TargetDenied)
        );
        assert!(repo
            .authorize_run(session, backend, ClientScope::RunsObserve, allowed)
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
            repo.authorize_run(session, restarted_backend, ClientScope::RunsControl, run),
            Err(AuthorizationError::StaleBackendInstance)
        );

        repo.set_principal_status(principal, PrincipalStatus::Revoked)
            .unwrap();
        assert_eq!(
            repo.authorize_run(session, first_backend, ClientScope::RunsControl, run),
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
            repo.authorize_control(session_a, backend, run_b, 1),
            Err(AuthorizationError::TargetDenied)
        );
        repo.authorize_control(session_a, backend, run_a, 1)
            .unwrap();
        assert_eq!(
            repo.authorize_control(session_a, backend, run_a, 1),
            Err(AuthorizationError::ReplayedRequest)
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
}
