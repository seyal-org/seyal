//! Independent Agent Backend process/domain composition boundary.
//!
//! AB-0.2 adds the per-user Unix-domain daemon and Hello/HelloAck handshake.
//! `AgentDomain` remains the only lifecycle transition authority.

#[cfg(unix)]
mod daemon;
#[cfg(unix)]
mod endpoint;
#[cfg(unix)]
#[allow(unsafe_code)]
mod peer;

#[cfg(unix)]
pub use daemon::{connect_hello, AgentDaemon, DaemonConfig, DaemonError, DaemonSample};
pub use seyal_agent_core::{AgentDomain, DomainError};
pub use seyal_agent_protocol::ProtocolVersion;
pub use seyal_agent_store::{AgentRunId, AttemptId, WorkItemId, WorkScopeId};
