use std::{
    collections::{BTreeSet, HashMap},
    fmt,
};

use seyal_agent_core::{
    AdapterId, AgentRunId, BackendInstanceId, ClientPrincipalId, ClientSessionId,
};

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
    /// Per-adapter `adapter.execute` grants (SPEC-027 §7 step 5 / D3). Until
    /// #1191 pairing lands, only `FirstPartyCli`/`FirstPartySeyal` principals
    /// may hold any entry here; `grant_adapter_execute` enforces that.
    executable_adapters: BTreeSet<AdapterId>,
    /// Durable `admin.adapters` grant (SPEC-027 §5.2 / SPEC-017 §5). Required
    /// for first-party catalog install/enable through `IntegrationService`.
    /// Until #1191 pairing lands, only first-party principal kinds may hold it.
    admin_adapters: bool,
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
    SessionPrincipalMismatch,
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
                executable_adapters: BTreeSet::new(),
                admin_adapters: false,
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
                executable_adapters: BTreeSet::new(),
                admin_adapters: false,
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

    /// Grant `adapter.execute` for one adapter (D3 / SPEC-027 §7 step 5).
    /// Until #1191 pairing is Done, only first-party principal kinds are
    /// eligible; this never relaxes for `UserApprovedLocalClient` or
    /// `ManagedClient` regardless of caller intent.
    pub fn grant_adapter_execute(
        &mut self,
        id: ClientPrincipalId,
        adapter_id: AdapterId,
    ) -> Result<(), AuthorizationError> {
        let principal = self
            .principals
            .get_mut(&id)
            .ok_or(AuthorizationError::UnknownPrincipal)?;
        match principal.kind {
            PrincipalKind::FirstPartyCli | PrincipalKind::FirstPartySeyal => {
                principal.executable_adapters.insert(adapter_id);
                Ok(())
            }
            PrincipalKind::UserApprovedLocalClient | PrincipalKind::ManagedClient => {
                Err(AuthorizationError::ScopeEscalation)
            }
        }
    }

    /// Grant durable `admin.adapters` (SPEC-027 §5.2). First-party only until
    /// #1191 pairing; never a client Command.
    pub fn grant_admin_adapters(
        &mut self,
        id: ClientPrincipalId,
    ) -> Result<(), AuthorizationError> {
        let principal = self
            .principals
            .get_mut(&id)
            .ok_or(AuthorizationError::UnknownPrincipal)?;
        match principal.kind {
            PrincipalKind::FirstPartyCli | PrincipalKind::FirstPartySeyal => {
                principal.admin_adapters = true;
                Ok(())
            }
            PrincipalKind::UserApprovedLocalClient | PrincipalKind::ManagedClient => {
                Err(AuthorizationError::ScopeEscalation)
            }
        }
    }

    /// Require `admin.adapters` for catalog install/enable.
    pub fn authorize_admin_adapters(
        &self,
        id: ClientPrincipalId,
    ) -> Result<(), AuthorizationError> {
        let principal = self
            .principals
            .get(&id)
            .ok_or(AuthorizationError::UnknownPrincipal)?;
        if !principal.admin_adapters {
            return Err(AuthorizationError::TargetDenied);
        }
        Ok(())
    }

    /// `runs.create` and `adapter.execute` are independent grants (D3): a
    /// session with the former but not the latter must still be rejected.
    pub fn authorize_adapter_execute(
        &self,
        session_id: ClientSessionId,
        backend_instance_id: BackendInstanceId,
        adapter_id: AdapterId,
        caller: ClientPrincipalId,
    ) -> Result<(), AuthorizationError> {
        let session = self.session(session_id, backend_instance_id, caller)?;
        let principal = self
            .principals
            .get(&session.principal_id)
            .ok_or(AuthorizationError::UnknownPrincipal)?;
        if !principal.executable_adapters.contains(&adapter_id) {
            return Err(AuthorizationError::TargetDenied);
        }
        Ok(())
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
        caller: ClientPrincipalId,
    ) -> Result<ClientPrincipalId, AuthorizationError> {
        let session = self.session(session_id, backend_instance_id, caller)?;
        if !session.scopes.contains(&required_scope) {
            return Err(AuthorizationError::ScopeDenied);
        }
        Ok(session.principal_id)
    }

    pub fn resume_session(
        &self,
        session_id: ClientSessionId,
        backend_instance_id: BackendInstanceId,
        caller: ClientPrincipalId,
    ) -> Result<(), AuthorizationError> {
        let _session = self.session(session_id, backend_instance_id, caller)?;
        Ok(())
    }

    fn session(
        &self,
        session_id: ClientSessionId,
        backend_instance_id: BackendInstanceId,
        caller: ClientPrincipalId,
    ) -> Result<&Session, AuthorizationError> {
        let session = self
            .sessions
            .get(&session_id)
            .ok_or(AuthorizationError::UnknownSession)?;
        // Principal binding is decided before backend instance, scope, target,
        // control nonce, and principal status. A foreign session must be
        // indistinguishable from an id that was never issued.
        if session.principal_id != caller {
            return Err(AuthorizationError::SessionPrincipalMismatch);
        }
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
        caller: ClientPrincipalId,
    ) -> Result<(), AuthorizationError> {
        let session = self.session(session_id, backend_instance_id, caller)?;
        if !session.scopes.contains(&required_scope) {
            return Err(AuthorizationError::ScopeDenied);
        }
        let principal = self
            .principals
            .get(&session.principal_id)
            .ok_or(AuthorizationError::UnknownPrincipal)?;
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
        caller: ClientPrincipalId,
    ) -> Result<(), AuthorizationError> {
        self.authorize_run(
            session_id,
            backend_instance_id,
            ClientScope::RunsControl,
            run_id,
            caller,
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
mod tests;
