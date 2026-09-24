use std::collections::{BTreeSet, HashMap};

use seyal_agent_core::{AgentRunId, BackendInstanceId, ClientPrincipalId, ClientSessionId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ClientScope {
    RunsCreate,
    RunsObserve,
    RunsInteract,
    RunsControl,
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
            },
        );
        Ok(id)
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
}
