use crate::{BackendInstanceId, FrameError, ProtocolVersion};

pub const MAX_VERSIONS: usize = 8;
pub const MAX_PRINCIPAL_EVIDENCE: usize = 256;
pub const MAX_EVENT_WINDOW: u32 = 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hello {
    pub supported_versions: Vec<ProtocolVersion>,
    pub max_frame_size: u32,
    pub event_window: u32,
    pub client_principal_evidence: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServerCapabilities {
    pub local_session: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HelloAck {
    pub selected_version: ProtocolVersion,
    pub backend_instance_id: BackendInstanceId,
    pub capabilities: ServerCapabilities,
    pub max_frame_size: u32,
    pub event_window: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandshakeError {
    InvalidLimit,
    NoCompatibleVersion,
    Malformed,
}

pub fn negotiate_hello(
    hello: &Hello,
    backend_instance_id: BackendInstanceId,
    server_max_frame_size: u32,
    server_event_window: u32,
) -> Result<HelloAck, HandshakeError> {
    if hello.supported_versions.len() > MAX_VERSIONS
        || hello.client_principal_evidence.len() > MAX_PRINCIPAL_EVIDENCE
    {
        return Err(HandshakeError::Malformed);
    }
    if hello.max_frame_size == 0
        || hello.event_window == 0
        || server_max_frame_size == 0
        || server_event_window == 0
        || hello.event_window > MAX_EVENT_WINDOW
        || server_event_window > MAX_EVENT_WINDOW
    {
        return Err(HandshakeError::InvalidLimit);
    }

    let selected_version = hello
        .supported_versions
        .iter()
        .copied()
        .find(|version| *version == ProtocolVersion::V1)
        .ok_or(HandshakeError::NoCompatibleVersion)?;

    Ok(HelloAck {
        selected_version,
        backend_instance_id,
        capabilities: ServerCapabilities {
            local_session: true,
        },
        max_frame_size: hello.max_frame_size.min(server_max_frame_size),
        event_window: hello.event_window.min(server_event_window),
    })
}

pub fn encode_hello(hello: &Hello, max_frame_size: u32) -> Result<Vec<u8>, FrameError> {
    if hello.supported_versions.len() > MAX_VERSIONS
        || hello.client_principal_evidence.len() > MAX_PRINCIPAL_EVIDENCE
    {
        return Err(FrameError::Malformed);
    }
    let mut body = Vec::new();
    body.extend_from_slice(&(hello.supported_versions.len() as u16).to_le_bytes());
    for version in &hello.supported_versions {
        body.extend_from_slice(&version.get().to_le_bytes());
    }
    body.extend_from_slice(&hello.max_frame_size.to_le_bytes());
    body.extend_from_slice(&hello.event_window.to_le_bytes());
    body.extend_from_slice(&(hello.client_principal_evidence.len() as u16).to_le_bytes());
    body.extend_from_slice(&hello.client_principal_evidence);
    crate::frame::encode_frame(crate::frame::FrameKind::Hello, &body, max_frame_size)
}

pub fn decode_hello(body: &[u8]) -> Result<Hello, FrameError> {
    let mut reader = Reader::new(body);
    let count = reader.u16()? as usize;
    if count > MAX_VERSIONS {
        return Err(FrameError::Malformed);
    }
    let mut supported_versions = Vec::with_capacity(count);
    for _ in 0..count {
        supported_versions.push(ProtocolVersion::new(reader.u16()?));
    }
    let max_frame_size = reader.u32()?;
    let event_window = reader.u32()?;
    let evidence_len = reader.u16()? as usize;
    if evidence_len > MAX_PRINCIPAL_EVIDENCE {
        return Err(FrameError::Malformed);
    }
    let client_principal_evidence = reader.bytes(evidence_len)?.to_vec();
    reader.finish()?;
    Ok(Hello {
        supported_versions,
        max_frame_size,
        event_window,
        client_principal_evidence,
    })
}

pub fn encode_ack(ack: &HelloAck, max_frame_size: u32) -> Result<Vec<u8>, FrameError> {
    let mut body = Vec::with_capacity(24);
    body.extend_from_slice(&ack.selected_version.get().to_le_bytes());
    body.extend_from_slice(&ack.backend_instance_id.to_bytes());
    body.push(u8::from(ack.capabilities.local_session));
    body.extend_from_slice(&ack.max_frame_size.to_le_bytes());
    body.extend_from_slice(&ack.event_window.to_le_bytes());
    crate::frame::encode_frame(crate::frame::FrameKind::HelloAck, &body, max_frame_size)
}

pub fn decode_ack(body: &[u8]) -> Result<HelloAck, FrameError> {
    let mut reader = Reader::new(body);
    let selected_version = ProtocolVersion::new(reader.u16()?);
    let mut id_bytes = [0; 16];
    id_bytes.copy_from_slice(reader.bytes(16)?);
    let local_session = match reader.u8()? {
        0 => false,
        1 => true,
        _ => return Err(FrameError::Malformed),
    };
    let max_frame_size = reader.u32()?;
    let event_window = reader.u32()?;
    reader.finish()?;
    Ok(HelloAck {
        selected_version,
        backend_instance_id: BackendInstanceId::from_bytes(id_bytes),
        capabilities: ServerCapabilities { local_session },
        max_frame_size,
        event_window,
    })
}

pub fn encode_handshake_error(
    error: HandshakeError,
    max_frame_size: u32,
) -> Result<Vec<u8>, FrameError> {
    let code: u16 = match error {
        HandshakeError::InvalidLimit => 1,
        HandshakeError::NoCompatibleVersion => 2,
        HandshakeError::Malformed => 3,
    };
    crate::frame::encode_frame(
        crate::frame::FrameKind::HandshakeError,
        &code.to_le_bytes(),
        max_frame_size,
    )
}

pub fn decode_handshake_error(body: &[u8]) -> Result<HandshakeError, FrameError> {
    let mut reader = Reader::new(body);
    let code = reader.u16()?;
    reader.finish()?;
    match code {
        1 => Ok(HandshakeError::InvalidLimit),
        2 => Ok(HandshakeError::NoCompatibleVersion),
        3 => Ok(HandshakeError::Malformed),
        _ => Err(FrameError::Malformed),
    }
}

struct Reader<'a> {
    buf: &'a [u8],
    index: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, index: 0 }
    }

    fn u8(&mut self) -> Result<u8, FrameError> {
        Ok(self.bytes(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, FrameError> {
        let bytes = self.bytes(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32, FrameError> {
        let bytes = self.bytes(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn bytes(&mut self, len: usize) -> Result<&'a [u8], FrameError> {
        let end = self.index.checked_add(len).ok_or(FrameError::Malformed)?;
        if end > self.buf.len() {
            return Err(FrameError::Malformed);
        }
        let slice = &self.buf[self.index..end];
        self.index = end;
        Ok(slice)
    }

    fn finish(self) -> Result<(), FrameError> {
        if self.index == self.buf.len() {
            Ok(())
        } else {
            Err(FrameError::Malformed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_v1_is_explicit_and_stable() {
        assert_eq!(ProtocolVersion::V1.get(), 1);
    }

    #[test]
    fn hello_selects_v1_and_narrows_limits() {
        let backend = BackendInstanceId::new();
        let hello = Hello {
            supported_versions: vec![ProtocolVersion::new(99), ProtocolVersion::V1],
            max_frame_size: 1024,
            event_window: 64,
            client_principal_evidence: Vec::new(),
        };

        let ack = negotiate_hello(&hello, backend, 512, 128).unwrap();

        assert_eq!(ack.selected_version, ProtocolVersion::V1);
        assert_eq!(ack.backend_instance_id, backend);
        assert!(ack.capabilities.local_session);
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
            client_principal_evidence: Vec::new(),
        };
        assert_eq!(
            negotiate_hello(&incompatible, backend, 1024, 64),
            Err(HandshakeError::NoCompatibleVersion)
        );

        let zero_limit = Hello {
            supported_versions: vec![ProtocolVersion::V1],
            max_frame_size: 0,
            event_window: 64,
            client_principal_evidence: Vec::new(),
        };
        assert_eq!(
            negotiate_hello(&zero_limit, backend, 1024, 64),
            Err(HandshakeError::InvalidLimit)
        );

        let huge_window = Hello {
            supported_versions: vec![ProtocolVersion::V1],
            max_frame_size: 1024,
            event_window: MAX_EVENT_WINDOW + 1,
            client_principal_evidence: Vec::new(),
        };
        assert_eq!(
            negotiate_hello(&huge_window, backend, 1024, 64),
            Err(HandshakeError::InvalidLimit)
        );
    }

    #[test]
    fn hello_and_ack_round_trip_inside_the_frame_bound() {
        let hello = Hello {
            supported_versions: vec![ProtocolVersion::V1],
            max_frame_size: 1024,
            event_window: 32,
            client_principal_evidence: b"evidence".to_vec(),
        };
        let frame = encode_hello(&hello, 1024).unwrap();
        let decoded = crate::decode_frame(&frame, 1024).unwrap();
        assert_eq!(decode_hello(&decoded.body).unwrap(), hello);

        let ack = negotiate_hello(&hello, BackendInstanceId::new(), 1024, 32).unwrap();
        let encoded = encode_ack(&ack, 1024).unwrap();
        let decoded = crate::decode_frame(&encoded, 1024).unwrap();
        assert_eq!(decode_ack(&decoded.body).unwrap(), ack);
    }
}
