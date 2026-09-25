pub const ABSOLUTE_MAX_FRAME_SIZE: u32 = 64 * 1024;
const HEADER_LEN: usize = 10;
const MAGIC: &[u8; 4] = b"AGB1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameKind {
    Hello,
    HelloAck,
    HandshakeError,
}

impl FrameKind {
    fn code(self) -> u16 {
        match self {
            Self::Hello => 1,
            Self::HelloAck => 2,
            Self::HandshakeError => 3,
        }
    }

    fn from_code(code: u16) -> Result<Self, FrameError> {
        match code {
            1 => Ok(Self::Hello),
            2 => Ok(Self::HelloAck),
            3 => Ok(Self::HandshakeError),
            _ => Err(FrameError::Malformed),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub kind: FrameKind,
    pub body: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameError {
    Malformed,
    Oversized,
    Incomplete,
}

pub fn accepted_body_len(header: &[u8], max_frame_size: u32) -> Result<usize, FrameError> {
    if header.len() < HEADER_LEN || max_frame_size < HEADER_LEN as u32 {
        return Err(FrameError::Malformed);
    }
    if &header[..4] != MAGIC {
        return Err(FrameError::Malformed);
    }
    let _ = FrameKind::from_code(u16::from_le_bytes([header[4], header[5]]))?;
    let body_len = u32::from_le_bytes([header[6], header[7], header[8], header[9]]);
    let max_body = max_frame_size - HEADER_LEN as u32;
    if body_len > max_body {
        return Err(FrameError::Oversized);
    }
    Ok(body_len as usize)
}

pub fn encode_frame(
    kind: FrameKind,
    body: &[u8],
    max_frame_size: u32,
) -> Result<Vec<u8>, FrameError> {
    let max_body = max_frame_size
        .checked_sub(HEADER_LEN as u32)
        .ok_or(FrameError::Malformed)?;
    if body.len() > max_body as usize {
        return Err(FrameError::Oversized);
    }
    let mut frame = Vec::with_capacity(HEADER_LEN + body.len());
    frame.extend_from_slice(MAGIC);
    frame.extend_from_slice(&kind.code().to_le_bytes());
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(body);
    Ok(frame)
}

pub fn decode_frame(bytes: &[u8], max_frame_size: u32) -> Result<Frame, FrameError> {
    if bytes.len() < HEADER_LEN {
        return Err(FrameError::Incomplete);
    }
    let body_len = accepted_body_len(&bytes[..HEADER_LEN], max_frame_size)?;
    let total = HEADER_LEN + body_len;
    if bytes.len() < total {
        return Err(FrameError::Incomplete);
    }
    if bytes.len() != total {
        return Err(FrameError::Malformed);
    }
    let kind = FrameKind::from_code(u16::from_le_bytes([bytes[4], bytes[5]]))?;
    Ok(Frame {
        kind,
        body: bytes[HEADER_LEN..].to_vec(),
    })
}

/// Feed untrusted bytes in arbitrary slices. The decoder never allocates a body
/// larger than `max_frame_size` and rejects a declared oversized length before
/// reading it.
pub fn push_untrusted(chunks: &[&[u8]], max_frame_size: u32) -> Result<Option<Frame>, FrameError> {
    let mut buf = Vec::new();
    for chunk in chunks {
        let remaining = (max_frame_size as usize).saturating_sub(buf.len());
        if chunk.len() > remaining && buf.len() < HEADER_LEN {
            buf.extend_from_slice(&chunk[..remaining.min(chunk.len())]);
            if buf.len() >= HEADER_LEN {
                let _ = accepted_body_len(&buf[..HEADER_LEN], max_frame_size)?;
            }
            return Err(FrameError::Oversized);
        }
        if buf.len() >= HEADER_LEN {
            let body_len = accepted_body_len(&buf[..HEADER_LEN], max_frame_size)?;
            if buf.len() + chunk.len() > HEADER_LEN + body_len {
                return Err(FrameError::Malformed);
            }
        } else if buf.len() + chunk.len() > max_frame_size as usize {
            let take = (max_frame_size as usize).saturating_sub(buf.len());
            buf.extend_from_slice(&chunk[..take]);
            if buf.len() >= HEADER_LEN {
                let _ = accepted_body_len(&buf[..HEADER_LEN], max_frame_size)?;
            }
            return Err(FrameError::Oversized);
        }
        buf.extend_from_slice(chunk);
        if buf.len() >= HEADER_LEN {
            let _ = accepted_body_len(&buf[..HEADER_LEN], max_frame_size)?;
        }
    }
    if buf.is_empty() {
        return Ok(None);
    }
    decode_frame(&buf, max_frame_size).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_declared_length_is_rejected_before_allocation() {
        let mut header = [0; HEADER_LEN];
        header[..4].copy_from_slice(MAGIC);
        header[4..6].copy_from_slice(&1_u16.to_le_bytes());
        header[6..10].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            accepted_body_len(&header, ABSOLUTE_MAX_FRAME_SIZE),
            Err(FrameError::Oversized)
        );
    }

    #[test]
    fn property_untrusted_bytes_stay_bounded() {
        let mut state = 0xA6E0_u64;
        for _ in 0..400 {
            let len = (next(&mut state) as usize % 80) + 1;
            let mut bytes = vec![0; len];
            for byte in &mut bytes {
                *byte = next(&mut state);
            }
            let split = (next(&mut state) as usize) % bytes.len();
            let result = push_untrusted(&[&bytes[..split], &bytes[split..]], 128);
            if let Ok(Some(frame)) = &result {
                assert!(frame.body.len() <= 128 - HEADER_LEN);
            }
        }
    }

    fn next(state: &mut u64) -> u8 {
        *state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        (*state >> 33) as u8
    }
}
