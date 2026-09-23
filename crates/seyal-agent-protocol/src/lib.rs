//! Versioned local Agent Backend protocol value boundary.
//!
//! AB-0.2 extends the AB-0.1 value boundary with the pure Hello/HelloAck
//! negotiation model. Socket framing and daemon lifecycle remain backend-owned.

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hello {
    pub supported_versions: Vec<ProtocolVersion>,
    pub max_frame_size: u32,
    pub event_window: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HelloAck {
    pub selected_version: ProtocolVersion,
    pub backend_instance_id: BackendInstanceId,
    pub max_frame_size: u32,
    pub event_window: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandshakeError {
    InvalidLimit,
    NoCompatibleVersion,
}

pub fn negotiate_hello(
    hello: &Hello,
    backend_instance_id: BackendInstanceId,
    server_max_frame_size: u32,
    server_event_window: u32,
) -> Result<HelloAck, HandshakeError> {
    if hello.max_frame_size == 0
        || hello.event_window == 0
        || server_max_frame_size == 0
        || server_event_window == 0
    {
        return Err(HandshakeError::InvalidLimit);
    }

    let selected_version = hello
        .supported_versions
        .iter()
        .copied()
        .filter(|version| *version == ProtocolVersion::V1)
        .max()
        .ok_or(HandshakeError::NoCompatibleVersion)?;

    Ok(HelloAck {
        selected_version,
        backend_instance_id,
        max_frame_size: hello.max_frame_size.min(server_max_frame_size),
        event_window: hello.event_window.min(server_event_window),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_v1_is_explicit_and_stable() {
        assert_eq!(ProtocolVersion::V1.get(), 1);
    }

    #[test]
    fn hello_selects_a_compatible_version_and_narrows_limits() {
        let backend = BackendInstanceId::new();
        let hello = Hello {
            supported_versions: vec![ProtocolVersion::new(99), ProtocolVersion::V1],
            max_frame_size: 1024,
            event_window: 64,
        };

        let ack = negotiate_hello(&hello, backend, 512, 128).unwrap();

        assert_eq!(ack.selected_version, ProtocolVersion::V1);
        assert_eq!(ack.backend_instance_id, backend);
        assert_eq!(ack.max_frame_size, 512);
        assert_eq!(ack.event_window, 64);
    }

    #[test]
    fn incompatible_or_unbounded_hello_fails_closed() {
        let backend = BackendInstanceId::new();
        let incompatible = Hello {
            supported_versions: vec![ProtocolVersion::new(99)],
            max_frame_size: 1024,
            event_window: 64,
        };
        assert_eq!(
            negotiate_hello(&incompatible, backend, 1024, 64),
            Err(HandshakeError::NoCompatibleVersion)
        );

        let zero_limit = Hello {
            supported_versions: vec![ProtocolVersion::V1],
            max_frame_size: 0,
            event_window: 64,
        };
        assert_eq!(
            negotiate_hello(&zero_limit, backend, 1024, 64),
            Err(HandshakeError::InvalidLimit)
        );
    }
}
