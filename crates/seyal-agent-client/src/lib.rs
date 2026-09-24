//! Reusable client-side Agent Backend protocol boundary.
//!
//! AB-0.1 adds no socket or reconnect behavior. AB-0.2 will add transport while
//! keeping client code independent from Seyal Terminal Runtime and rendering.

pub use seyal_agent_core::{BackendInstanceId, ClientPrincipalId, ClientSessionId};
pub use seyal_agent_protocol::ProtocolVersion;
