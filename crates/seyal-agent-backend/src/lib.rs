//! Independent Agent Backend process/domain composition boundary.
//!
//! AB-0.1 deliberately contains no daemon lifecycle, IPC, provider, routing, or
//! persistence implementation. The pure AgentDomain remains the sole lifecycle
//! transition authority established by this slice.

pub use seyal_agent_core::{AgentDomain, DomainError};
pub use seyal_agent_protocol::ProtocolVersion;
pub use seyal_agent_store::{AgentRunId, AttemptId, WorkItemId, WorkScopeId};
