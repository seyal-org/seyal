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
    /// Hello evidence token that selects this principal (`cli`, `observer`, …).
    evidence_key: Vec<u8>,
}

impl PrincipalKind {
    pub fn code(self) -> u8 {
        match self {
            Self::FirstPartyCli => 1,
            Self::FirstPartySeyal => 2,
            Self::UserApprovedLocalClient => 3,
            Self::ManagedClient => 4,
        }
    }

    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::FirstPartyCli),
            2 => Some(Self::FirstPartySeyal),
            3 => Some(Self::UserApprovedLocalClient),
            4 => Some(Self::ManagedClient),
            _ => None,
        }
    }
}

impl PrincipalStatus {
    pub fn code(self) -> u8 {
        match self {
            Self::Active => 1,
            Self::Suspended => 2,
            Self::Revoked => 3,
        }
    }

    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Active),
            2 => Some(Self::Suspended),
            3 => Some(Self::Revoked),
            _ => None,
        }
    }
}

impl ClientScope {
    pub fn code(self) -> u8 {
        match self {
            Self::RunsCreate => 1,
            Self::RunsObserve => 2,
            Self::RunsInteract => 3,
            Self::RunsControl => 4,
        }
    }
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
    evidence_index: HashMap<Vec<u8>, ClientPrincipalId>,
}

/// Durable principal snapshot for store upsert (no pairing secrets).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DurablePrincipal {
    pub id: ClientPrincipalId,
    pub kind: PrincipalKind,
    pub status: PrincipalStatus,
    pub scopes: BTreeSet<ClientScope>,
    pub evidence_key: Vec<u8>,
}

impl AuthorizationRepository {
    pub fn register_principal(
        &mut self,
        kind: PrincipalKind,
        scopes: impl IntoIterator<Item = ClientScope>,
    ) -> ClientPrincipalId {
        let id = ClientPrincipalId::new();
        let evidence_key = match kind {
            PrincipalKind::FirstPartyCli => b"cli".to_vec(),
            PrincipalKind::FirstPartySeyal => b"seyal".to_vec(),
            PrincipalKind::ManagedClient => b"observer".to_vec(),
            PrincipalKind::UserApprovedLocalClient => {
                let mut key = b"approved:".to_vec();
                key.extend_from_slice(&id.to_bytes());
                key
            }
        };
        self.insert_principal(id, kind, scopes, evidence_key);
        id
    }

    pub fn register_principal_with_evidence(
        &mut self,
        kind: PrincipalKind,
        scopes: impl IntoIterator<Item = ClientScope>,
        evidence_key: Vec<u8>,
    ) -> ClientPrincipalId {
        let id = ClientPrincipalId::new();
        self.insert_principal(id, kind, scopes, evidence_key);
        id
    }

    fn insert_principal(
        &mut self,
        id: ClientPrincipalId,
        kind: PrincipalKind,
        scopes: impl IntoIterator<Item = ClientScope>,
        evidence_key: Vec<u8>,
    ) {
        self.principals.insert(
            id,
            Principal {
                kind,
                status: PrincipalStatus::Active,
                scopes: scopes.into_iter().collect(),
                allowed_runs: BTreeSet::new(),
                evidence_key: evidence_key.clone(),
            },
        );
        self.evidence_index.insert(evidence_key, id);
    }

    pub fn load_durable_principal(
        &mut self,
        row: DurablePrincipal,
    ) -> Result<(), AuthorizationError> {
        if self.principals.contains_key(&row.id) {
            return Err(AuthorizationError::Malformed);
        }
        self.principals.insert(
            row.id,
            Principal {
                kind: row.kind,
                status: row.status,
                scopes: row.scopes,
                allowed_runs: BTreeSet::new(),
                evidence_key: row.evidence_key.clone(),
            },
        );
        self.evidence_index.insert(row.evidence_key, row.id);
        Ok(())
    }

    pub fn durable_principal(
        &self,
        id: ClientPrincipalId,
    ) -> Result<DurablePrincipal, AuthorizationError> {
        let principal = self
            .principals
            .get(&id)
            .ok_or(AuthorizationError::UnknownPrincipal)?;
        Ok(DurablePrincipal {
            id,
            kind: principal.kind,
            status: principal.status,
            scopes: principal.scopes.clone(),
            evidence_key: principal.evidence_key.clone(),
        })
    }

    pub fn principal_by_evidence_key(&self, evidence: &[u8]) -> Option<ClientPrincipalId> {
        let key = normalize_evidence_key(evidence);
        self.evidence_index.get(key).copied().or_else(|| {
            if key == b"cli" {
                self.evidence_index.get(b"cli".as_slice()).copied()
            } else {
                None
            }
        })
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

    /// Grant observe/control targets to every principal that holds `RunsObserve`.
    /// Used when a run becomes durable so a distinct observe-only principal can
    /// read the same backend-authoritative aggregate (SPEC-017 §5; matrix 16).
    pub fn allow_run_for_observers(&mut self, run_id: AgentRunId) {
        for principal in self.principals.values_mut() {
            if principal.scopes.contains(&ClientScope::RunsObserve) {
                principal.allowed_runs.insert(run_id);
            }
        }
    }

    /// Recognized Hello principal evidence tokens.
    /// Same-UID admission is never enough: unknown evidence fails closed.
    pub fn recognize_principal_evidence(evidence: &[u8]) -> Result<(), AuthorizationError> {
        let key = normalize_evidence_key(evidence);
        match key {
            b"cli" | b"observer" | b"seyal" => Ok(()),
            other if other.starts_with(b"approved") => Ok(()),
            _ => Err(AuthorizationError::TransportAdmissionIsNotAuthorization),
        }
    }

    /// Map Hello principal evidence to a registered principal.
    ///
    /// When `owner`/`observer` are provided they remain the AB-0 fallback for
    /// empty/`cli`/`observer` tokens. Durable loads prefer the evidence index.
    pub fn principal_for_evidence(
        &self,
        evidence: &[u8],
        owner: ClientPrincipalId,
        observer: ClientPrincipalId,
    ) -> Result<ClientPrincipalId, AuthorizationError> {
        Self::recognize_principal_evidence(evidence)?;
        if let Some(id) = self.principal_by_evidence_key(evidence) {
            return Ok(id);
        }
        match normalize_evidence_key(evidence) {
            b"cli" => Ok(owner),
            b"observer" => Ok(observer),
            _ => Err(AuthorizationError::TransportAdmissionIsNotAuthorization),
        }
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

    pub fn authorize_session(
        &self,
        session_id: ClientSessionId,
        backend_instance_id: BackendInstanceId,
        required_scope: ClientScope,
    ) -> Result<ClientPrincipalId, AuthorizationError> {
        let session = self.session(session_id, backend_instance_id)?;
        if !session.scopes.contains(&required_scope) {
            return Err(AuthorizationError::ScopeDenied);
        }
        Ok(session.principal_id)
    }

    pub fn resume_session(
        &self,
        session_id: ClientSessionId,
        backend_instance_id: BackendInstanceId,
    ) -> Result<(), AuthorizationError> {
        let _session = self.session(session_id, backend_instance_id)?;
        Ok(())
    }

    pub fn session_principal(
        &self,
        session_id: ClientSessionId,
        backend_instance_id: BackendInstanceId,
    ) -> Result<ClientPrincipalId, AuthorizationError> {
        Ok(self.session(session_id, backend_instance_id)?.principal_id)
    }

    fn session(
        &self,
        session_id: ClientSessionId,
        backend_instance_id: BackendInstanceId,
    ) -> Result<&Session, AuthorizationError> {
        let session = self
            .sessions
            .get(&session_id)
            .ok_or(AuthorizationError::UnknownSession)?;
        if session.backend_instance_id != backend_instance_id {
            return Err(AuthorizationError::StaleBackendInstance);
        }
        let principal = self
            .principals
            .get(&session.principal_id)
            .ok_or(AuthorizationError::UnknownPrincipal)?;
        if principal.status != PrincipalStatus::Active {
            return Err(AuthorizationError::PrincipalInactive);
        }
        Ok(session)
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
}

fn normalize_evidence_key(evidence: &[u8]) -> &[u8] {
    match evidence {
        [] => b"cli",
        other => other,
    }
}

impl AuthorizationRepository {
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
        repo.authorize_run(session, backend, ClientScope::RunsObserve, run)
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
}
