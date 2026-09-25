//! Pure Agent Backend domain identities and lifecycle construction.
//!
//! This crate owns no daemon, transport, persistence engine, provider, PTY,
//! terminal state, renderer, AppKit/Metal integration, or commercial behavior.
//! It is the provider/harness-neutral domain foundation for AB-0.

mod domain;
mod identity;

pub use domain::{AgentDomain, AgentRun, Attempt, DomainError, WorkItem, WorkScope, WorkScopeKind};
pub use identity::{
    AgentRunId, AttemptId, BackendInstanceId, BindingGeneration, ClientPrincipalId,
    ClientSessionId, ControlGeneration, WorkItemId, WorkScopeId,
};
