//! Agent-domain persistence ownership boundary.
//!
//! AB-0.1 does not choose a database, event schema, snapshot strategy, or replay
//! implementation. This crate exists so AB-0.3 can add durable storage without
//! moving persistence into the domain or Terminal Runtime.

pub use seyal_agent_core::{AgentRunId, AttemptId, WorkItemId, WorkScopeId};
