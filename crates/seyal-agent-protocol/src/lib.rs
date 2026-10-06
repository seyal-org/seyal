//! Versioned local Agent Backend protocol boundary.
//!
//! AB-0.2 owns Hello/HelloAck negotiation and the bounded frame codec.
//! V1 session commands are the closed post-hello catalog. Socket ownership,
//! daemon lifecycle, and authorization stay outside this crate.

mod command;
mod frame;
mod handshake;
mod presence;

pub use command::{
    decode_command, decode_result, encode_command, encode_result, AggregateRef, Command,
    CommandError, CommandResult, ReplayEvent, SnapshotView, REPLAY_EVENT_OVERHEAD,
    REPLAY_RESULT_OVERHEAD,
};
pub use frame::{
    accepted_body_len, decode_frame, encode_frame, push_untrusted, Frame, FrameError, FrameKind,
    ABSOLUTE_MAX_FRAME_SIZE,
};
pub use handshake::{
    decode_ack, decode_handshake_error, decode_hello, encode_ack, encode_handshake_error,
    encode_hello, negotiate_hello, negotiate_hello_with_capabilities, ExecutionHostKind,
    HandshakeError, Hello, HelloAck, ServerCapabilities, MAX_EVENT_WINDOW, MAX_PRINCIPAL_EVIDENCE,
    MAX_VERSIONS,
};
pub use presence::{
    decode_capability_advertisement, decode_negotiated_capability,
    decode_presence_capability_projection, decode_presence_observation,
    encode_capability_advertisement, encode_negotiated_capability,
    encode_presence_capability_projection, encode_presence_observation,
    MAX_NEGOTIATED_CAPABILITIES,
};
pub use seyal_agent_core::{decode_output_ref, encode_output_ref};
pub use seyal_agent_core::{
    AgentRunId, AttemptId, BackendInstanceId, BindingGeneration, ClientPrincipalId,
    ClientSessionId, ControlGeneration, FingerprintRef, OutputRef, OutputRefError,
    RetentionPolicyRef, RouteOfferingId, StreamKind, WorkItemId, WorkScopeId, OUTPUT_REF_KIND,
    OUTPUT_REF_LEN, RETENTION_POLICY_RETAINED_STREAM,
};
pub use seyal_agent_core::{
    CapabilityId, CapabilityInstallTrust, CapabilitySupport, ClaimMode, EnforcementClass,
    NegotiatedCapability, PresenceCapabilityProjection, PresenceError, PresenceObservation,
    PresenceSourceTier,
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
