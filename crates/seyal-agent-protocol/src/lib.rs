//! Versioned local Agent Backend protocol boundary.
//!
//! AB-0.2 owns Hello/HelloAck negotiation and the bounded frame codec.
//! Socket ownership, daemon lifecycle, and authorization stay outside this crate.

mod frame;
mod handshake;

pub use frame::{
    accepted_body_len, decode_frame, encode_frame, push_untrusted, Frame, FrameError, FrameKind,
    ABSOLUTE_MAX_FRAME_SIZE,
};
pub use handshake::{
    decode_ack, decode_handshake_error, decode_hello, encode_ack, encode_handshake_error,
    encode_hello, negotiate_hello, HandshakeError, Hello, HelloAck, ServerCapabilities,
    MAX_EVENT_WINDOW, MAX_PRINCIPAL_EVIDENCE, MAX_VERSIONS,
};
pub use seyal_agent_core::{
    AgentRunId, AttemptId, BackendInstanceId, BindingGeneration, ClientPrincipalId,
    ClientSessionId, ControlGeneration, WorkItemId, WorkScopeId,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProtocolVersion(u16);

impl ProtocolVersion {
    pub const V1: Self = Self(1);

    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}
