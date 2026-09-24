//! Independent Agent Backend process/domain composition boundary.
//!
//! AB-0.2 owns the per-user Unix-domain daemon and Hello/HelloAck handshake.
//! AB-0.4 adds in-memory principal/session authorization fencing.
//! AB-0.5 adds a deterministic provider-free ExecutionHost fixture.
//! `AgentDomain` remains the only lifecycle transition authority.

mod auth;
#[cfg(unix)]
mod daemon;
#[cfg(unix)]
mod endpoint;
#[cfg(unix)]
#[allow(unsafe_code)]
mod peer;
mod execution_host;
mod observation;

pub use auth::{
    AuthorizationError, AuthorizationRepository, ClientScope, PairingCredential, PrincipalKind,
    PrincipalStatus,
};
#[cfg(unix)]
pub use daemon::{connect_hello, AgentDaemon, DaemonConfig, DaemonError, DaemonSample};
pub use execution_host::{
    FakeExecutionHost, HostObservation, HostObservationKind, ScriptError, ScriptStep,
};
pub use observation::{
    parse_script, ObservationAuthority, ObserveError, RunLiveness, WorkItemOutcome,
};
pub use seyal_agent_core::{AgentDomain, DomainError};
pub use seyal_agent_protocol::ProtocolVersion;
pub use seyal_agent_store::{AgentRunId, AttemptId, WorkItemId, WorkScopeId};
