//! Independent Agent Backend process/domain composition boundary.
//!
//! AB-0.2 owns the per-user Unix-domain daemon and Hello/HelloAck handshake.
//! AB-0.4 adds the in-memory authorization authority for principal/session
//! fencing. `AgentDomain` remains the only lifecycle transition authority.

mod auth;
#[cfg(unix)]
mod daemon;
#[cfg(unix)]
mod endpoint;
#[cfg(unix)]
#[allow(unsafe_code)]
mod peer;

pub use auth::{
    AuthorizationError, AuthorizationRepository, ClientScope, PrincipalKind, PrincipalStatus,
};
#[cfg(unix)]
pub use daemon::{connect_hello, AgentDaemon, DaemonConfig, DaemonError, DaemonSample};
pub use seyal_agent_core::{AgentDomain, DomainError};
pub use seyal_agent_protocol::ProtocolVersion;
pub use seyal_agent_store::{AgentRunId, AttemptId, WorkItemId, WorkScopeId};
