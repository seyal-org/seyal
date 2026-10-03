//! Independent Agent Backend process/domain composition boundary.
//!
//! The `seyal-agent-backend` binary is the production daemon entry. Library
//! code owns the per-user Unix-domain daemon, Hello/HelloAck handshake,
//! principal/session authorization fencing, the deterministic provider-free
//! FakeExecutionHost fixture, and StandaloneProcessHost (SPEC-018 §2).
//! `AgentDomain` remains the only lifecycle transition authority.

mod auth;
#[cfg(unix)]
mod daemon;
#[cfg(unix)]
mod endpoint;
mod execution_host;
mod observation;
#[cfg(unix)]
#[allow(unsafe_code)]
mod peer;
#[cfg(unix)]
mod session;
mod standalone_process_host;

pub use auth::{
    AuthorizationError, AuthorizationRepository, ClientScope, DurablePrincipal, PairingCredential,
    PrincipalKind, PrincipalStatus,
};
#[cfg(unix)]
pub use daemon::{connect_hello, AgentDaemon, DaemonConfig, DaemonError, DaemonSample, ServeExit};
pub use execution_host::{
    FakeExecutionHost, HostObservation, HostObservationKind, ScriptError, ScriptStep,
};
pub use observation::{
    parse_script, ObservationAuthority, ObserveError, RunLiveness, WorkItemOutcome,
};
#[cfg(unix)]
pub use session::{IntegrationConfig, IntegrationService};
pub use seyal_agent_core::{AgentDomain, DomainError, ExecutionHost, ExecutionHostKind};
pub use seyal_agent_protocol::ProtocolVersion;
pub use seyal_agent_store::{AgentRunId, AttemptId, WorkItemId, WorkScopeId};
pub use standalone_process_host::{HostError, StandaloneProcessConfig, StandaloneProcessHost};
