//! Immutable ActionIntent fields and canonical digest (SPEC-016 §3 / SPEC-028 §5.1).

use crate::identity::{ActionId, AgentRunId};
use crate::memory::{PolicyError, RevocationFence};
use crate::routing::sha256::sha256;

pub const MAX_CAPABILITY_BYTES: usize = 64;
pub const MAX_RESOURCE_TYPE_BYTES: usize = 32;
pub const MAX_CANONICAL_INTENT_BYTES: usize = 64 * 1024;

/// Typed capability identity for a Seyal-controlled operation (SPEC-028 CapabilityRef).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CapabilityRef {
    bytes: Vec<u8>,
}

impl CapabilityRef {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Result<Self, ActionIntentError> {
        let bytes = bytes.into();
        if bytes.is_empty() || bytes.len() > MAX_CAPABILITY_BYTES {
            return Err(ActionIntentError::InvalidField);
        }
        Ok(Self { bytes })
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// SPEC-028 ResourceIdentityV1: type + canonical id + scope + version/fingerprint.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ResourceIdentity {
    resource_type: Vec<u8>,
    canonical_id: [u8; 16],
    scope: [u8; 16],
    version_or_fingerprint: [u8; 32],
}

impl ResourceIdentity {
    pub fn new(
        resource_type: impl Into<Vec<u8>>,
        canonical_id: [u8; 16],
        scope: [u8; 16],
        version_or_fingerprint: [u8; 32],
    ) -> Result<Self, ActionIntentError> {
        let resource_type = resource_type.into();
        if resource_type.is_empty() || resource_type.len() > MAX_RESOURCE_TYPE_BYTES {
            return Err(ActionIntentError::InvalidField);
        }
        Ok(Self {
            resource_type,
            canonical_id,
            scope,
            version_or_fingerprint,
        })
    }

    pub fn resource_type(&self) -> &[u8] {
        &self.resource_type
    }

    pub fn canonical_id(&self) -> [u8; 16] {
        self.canonical_id
    }

    pub fn scope(&self) -> [u8; 16] {
        self.scope
    }

    pub fn version_or_fingerprint(&self) -> [u8; 32] {
        self.version_or_fingerprint
    }
}

/// SHA-256 of normalized arguments (SPEC-016 §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ArgumentFingerprint(pub [u8; 32]);

impl ArgumentFingerprint {
    pub fn of(normalized_arguments: &[u8]) -> Self {
        Self(sha256(normalized_arguments))
    }
}

/// Effect-class identity recorded on the intent. Codes distinguish operations
/// for SPEC-016 §3.1 material change; recovery/replay rules remain SPEC-016.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum EffectClass {
    Pure = 1,
    FilesystemIsolated = 2,
    IdempotentExternal = 3,
    NonIdempotentExternal = 4,
    NonReplayable = 5,
}

impl EffectClass {
    pub const fn code(self) -> u8 {
        self as u8
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Pure),
            2 => Some(Self::FilesystemIsolated),
            3 => Some(Self::IdempotentExternal),
            4 => Some(Self::NonIdempotentExternal),
            5 => Some(Self::NonReplayable),
            _ => None,
        }
    }
}

/// Required authorization class (policy vs human approval). SPEC-028 `User | Policy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum AuthorizationClass {
    Policy = 1,
    HumanApproval = 2,
}

impl AuthorizationClass {
    pub const fn code(self) -> u8 {
        self as u8
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Policy),
            2 => Some(Self::HumanApproval),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RequestProvenance {
    AgentBackend = 1,
    Harness = 2,
    Client = 3,
}

impl RequestProvenance {
    pub const fn code(self) -> u8 {
        self as u8
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::AgentBackend),
            2 => Some(Self::Harness),
            3 => Some(Self::Client),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PrivacyDependencyId(pub [u8; 16]);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ExecutorCapabilityRef {
    pub identity: [u8; 16],
    pub version: u64,
}

/// Durable Action lifecycle (SPEC-016 §4). Canonical ActionIntent bytes always
/// encode [`Prepared`]; later states live on the store lifecycle column so the
/// identity digest cannot drift.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ActionLifecycle {
    Prepared = 1,
    Authorized = 2,
    Dispatching = 3,
    Succeeded = 4,
    FailedKnown = 5,
    EffectUnknown = 6,
    CancelledBeforeDispatch = 7,
    CancelledAfterDispatch = 8,
}

impl ActionLifecycle {
    pub const fn code(self) -> u8 {
        self as u8
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Prepared),
            2 => Some(Self::Authorized),
            3 => Some(Self::Dispatching),
            4 => Some(Self::Succeeded),
            5 => Some(Self::FailedKnown),
            6 => Some(Self::EffectUnknown),
            7 => Some(Self::CancelledBeforeDispatch),
            8 => Some(Self::CancelledAfterDispatch),
            _ => None,
        }
    }

    pub const fn is_known_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::FailedKnown | Self::CancelledBeforeDispatch
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionIntentError {
    InvalidField,
    Fence(PolicyError),
    CanonicalTooLarge,
    Malformed,
}

impl From<PolicyError> for ActionIntentError {
    fn from(value: PolicyError) -> Self {
        Self::Fence(value)
    }
}

/// Immutable ActionIntent (SPEC-016 §3). After preparation the stored record
/// is never edited; a material change mints a new [`ActionId`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionIntent {
    action_id: ActionId,
    agent_run_id: AgentRunId,
    capability: CapabilityRef,
    resource: ResourceIdentity,
    argument_fingerprint: ArgumentFingerprint,
    effect_class: EffectClass,
    policy_generation: u64,
    privacy_dependency: PrivacyDependencyId,
    revocation_fence: RevocationFence,
    provenance: RequestProvenance,
    authorization_class: AuthorizationClass,
    created_at_ms: u64,
    expires_at_ms: Option<u64>,
    executor: Option<ExecutorCapabilityRef>,
    lifecycle: ActionLifecycle,
}

impl ActionIntent {
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        action_id: ActionId,
        agent_run_id: AgentRunId,
        capability: CapabilityRef,
        resource: ResourceIdentity,
        argument_fingerprint: ArgumentFingerprint,
        effect_class: EffectClass,
        policy_generation: u64,
        privacy_dependency: PrivacyDependencyId,
        revocation_fence: RevocationFence,
        provenance: RequestProvenance,
        authorization_class: AuthorizationClass,
        created_at_ms: u64,
        expires_at_ms: Option<u64>,
        executor: Option<ExecutorCapabilityRef>,
    ) -> Result<Self, ActionIntentError> {
        if policy_generation == 0 {
            return Err(ActionIntentError::InvalidField);
        }
        if expires_at_ms.is_some_and(|expiry| expiry < created_at_ms) {
            return Err(ActionIntentError::InvalidField);
        }
        let intent = Self {
            action_id,
            agent_run_id,
            capability,
            resource,
            argument_fingerprint,
            effect_class,
            policy_generation,
            privacy_dependency,
            revocation_fence,
            provenance,
            authorization_class,
            created_at_ms,
            expires_at_ms,
            executor,
            lifecycle: ActionLifecycle::Prepared,
        };
        if intent.encode().len() > MAX_CANONICAL_INTENT_BYTES {
            return Err(ActionIntentError::CanonicalTooLarge);
        }
        Ok(intent)
    }

    /// SPEC-016 §3.1: material change mints a new ActionId and never edits `self`.
    #[allow(clippy::too_many_arguments)]
    pub fn material_successor(
        &self,
        capability: CapabilityRef,
        resource: ResourceIdentity,
        argument_fingerprint: ArgumentFingerprint,
        effect_class: EffectClass,
        policy_generation: u64,
        privacy_dependency: PrivacyDependencyId,
        revocation_fence: RevocationFence,
        authorization_class: AuthorizationClass,
        created_at_ms: u64,
        expires_at_ms: Option<u64>,
        executor: Option<ExecutorCapabilityRef>,
    ) -> Result<Self, ActionIntentError> {
        Self::prepare(
            ActionId::new(),
            self.agent_run_id,
            capability,
            resource,
            argument_fingerprint,
            effect_class,
            policy_generation,
            privacy_dependency,
            revocation_fence,
            self.provenance,
            authorization_class,
            created_at_ms,
            expires_at_ms,
            executor,
        )
    }

    pub fn action_id(&self) -> ActionId {
        self.action_id
    }

    pub fn agent_run_id(&self) -> AgentRunId {
        self.agent_run_id
    }

    pub fn capability(&self) -> &CapabilityRef {
        &self.capability
    }

    pub fn resource(&self) -> &ResourceIdentity {
        &self.resource
    }

    pub fn argument_fingerprint(&self) -> ArgumentFingerprint {
        self.argument_fingerprint
    }

    pub fn effect_class(&self) -> EffectClass {
        self.effect_class
    }

    pub fn policy_generation(&self) -> u64 {
        self.policy_generation
    }

    pub fn privacy_dependency(&self) -> PrivacyDependencyId {
        self.privacy_dependency
    }

    pub fn revocation_fence(&self) -> &RevocationFence {
        &self.revocation_fence
    }

    pub fn provenance(&self) -> RequestProvenance {
        self.provenance
    }

    pub fn authorization_class(&self) -> AuthorizationClass {
        self.authorization_class
    }

    pub fn created_at_ms(&self) -> u64 {
        self.created_at_ms
    }

    pub fn expires_at_ms(&self) -> Option<u64> {
        self.expires_at_ms
    }

    pub fn executor(&self) -> Option<ExecutorCapabilityRef> {
        self.executor
    }

    pub fn lifecycle(&self) -> ActionLifecycle {
        self.lifecycle
    }

    pub fn digest(&self) -> [u8; 32] {
        action_intent_digest(self)
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.action_id.to_bytes());
        out.extend_from_slice(&self.agent_run_id.to_bytes());
        put_bytes(&mut out, self.capability.as_bytes());
        put_bytes(&mut out, self.resource.resource_type());
        out.extend_from_slice(&self.resource.canonical_id());
        out.extend_from_slice(&self.resource.scope());
        out.extend_from_slice(&self.resource.version_or_fingerprint());
        out.extend_from_slice(&self.argument_fingerprint.0);
        out.push(self.effect_class.code());
        out.extend_from_slice(&self.policy_generation.to_le_bytes());
        out.extend_from_slice(&self.privacy_dependency.0);
        let fence = self.revocation_fence.encode();
        put_bytes(&mut out, &fence);
        out.push(self.provenance.code());
        out.push(self.authorization_class.code());
        out.extend_from_slice(&self.created_at_ms.to_le_bytes());
        match self.expires_at_ms {
            Some(ms) => {
                out.push(1);
                out.extend_from_slice(&ms.to_le_bytes());
            }
            None => out.push(0),
        }
        match self.executor {
            Some(exec) => {
                out.push(1);
                out.extend_from_slice(&exec.identity);
                out.extend_from_slice(&exec.version.to_le_bytes());
            }
            None => out.push(0),
        }
        // Identity freeze: mutable runtime lifecycle is never part of the digest.
        out.push(ActionLifecycle::Prepared.code());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ActionIntentError> {
        let mut offset = 0;
        let action_id = ActionId::from_bytes(take_array(&mut offset, bytes)?);
        let agent_run_id = AgentRunId::from_bytes(take_array(&mut offset, bytes)?);
        let capability = CapabilityRef::new(take_len_bytes(&mut offset, bytes)?)?;
        let resource_type = take_len_bytes(&mut offset, bytes)?;
        let canonical_id = take_array(&mut offset, bytes)?;
        let scope = take_array(&mut offset, bytes)?;
        let version = take_array(&mut offset, bytes)?;
        let resource = ResourceIdentity::new(resource_type, canonical_id, scope, version)?;
        let argument_fingerprint = ArgumentFingerprint(take_array(&mut offset, bytes)?);
        let effect_class = EffectClass::from_code(take_u8(&mut offset, bytes)?)
            .ok_or(ActionIntentError::Malformed)?;
        let policy_generation = u64::from_le_bytes(take_array(&mut offset, bytes)?);
        let privacy_dependency = PrivacyDependencyId(take_array(&mut offset, bytes)?);
        let fence_bytes = take_len_bytes(&mut offset, bytes)?;
        let revocation_fence =
            RevocationFence::decode(&fence_bytes).map_err(ActionIntentError::Fence)?;
        let provenance = RequestProvenance::from_code(take_u8(&mut offset, bytes)?)
            .ok_or(ActionIntentError::Malformed)?;
        let authorization_class = AuthorizationClass::from_code(take_u8(&mut offset, bytes)?)
            .ok_or(ActionIntentError::Malformed)?;
        let created_at_ms = u64::from_le_bytes(take_array(&mut offset, bytes)?);
        let expires_at_ms = match take_u8(&mut offset, bytes)? {
            0 => None,
            1 => Some(u64::from_le_bytes(take_array(&mut offset, bytes)?)),
            _ => return Err(ActionIntentError::Malformed),
        };
        let executor = match take_u8(&mut offset, bytes)? {
            0 => None,
            1 => Some(ExecutorCapabilityRef {
                identity: take_array(&mut offset, bytes)?,
                version: u64::from_le_bytes(take_array(&mut offset, bytes)?),
            }),
            _ => return Err(ActionIntentError::Malformed),
        };
        let lifecycle = ActionLifecycle::from_code(take_u8(&mut offset, bytes)?)
            .ok_or(ActionIntentError::Malformed)?;
        if offset != bytes.len() {
            return Err(ActionIntentError::Malformed);
        }
        if lifecycle != ActionLifecycle::Prepared {
            return Err(ActionIntentError::Malformed);
        }
        Self::prepare(
            action_id,
            agent_run_id,
            capability,
            resource,
            argument_fingerprint,
            effect_class,
            policy_generation,
            privacy_dependency,
            revocation_fence,
            provenance,
            authorization_class,
            created_at_ms,
            expires_at_ms,
            executor,
        )
    }
}

/// Canonical immutable ActionIntent digest (SPEC-016 §5 / SPEC-028 `action_intent_digest`).
pub fn action_intent_digest(intent: &ActionIntent) -> [u8; 32] {
    sha256(&intent.encode())
}

/// SPEC-016 §3.1 material fields. Policy/revocation *generation* snapshots are
/// provenance: advancing them does not rewrite an existing intent.
pub fn material_fields_changed(left: &ActionIntent, right: &ActionIntent) -> bool {
    left.capability != right.capability
        || left.resource != right.resource
        || left.argument_fingerprint != right.argument_fingerprint
        || left.effect_class != right.effect_class
        || left.privacy_dependency != right.privacy_dependency
        || left.authorization_class != right.authorization_class
}

fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
}

fn take_u8(offset: &mut usize, bytes: &[u8]) -> Result<u8, ActionIntentError> {
    if *offset >= bytes.len() {
        return Err(ActionIntentError::Malformed);
    }
    let value = bytes[*offset];
    *offset += 1;
    Ok(value)
}

fn take_array<const N: usize>(
    offset: &mut usize,
    bytes: &[u8],
) -> Result<[u8; N], ActionIntentError> {
    if bytes.len() < *offset + N {
        return Err(ActionIntentError::Malformed);
    }
    let mut out = [0u8; N];
    out.copy_from_slice(&bytes[*offset..*offset + N]);
    *offset += N;
    Ok(out)
}

fn take_len_bytes(offset: &mut usize, bytes: &[u8]) -> Result<Vec<u8>, ActionIntentError> {
    let len = u32::from_le_bytes(take_array(offset, bytes)?) as usize;
    if bytes.len() < *offset + len {
        return Err(ActionIntentError::Malformed);
    }
    let out = bytes[*offset..*offset + len].to_vec();
    *offset += len;
    Ok(out)
}
