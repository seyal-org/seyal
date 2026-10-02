//! SPEC-017 §10 RunOutputSegment read-protocol fields and §12 fingerprint refs.
//!
//! Shared by store (durable encode) and protocol (client decode) without
//! introducing a store↔protocol edge. Raw secret bytes never appear here.

/// Discriminant for a RunEvent payload that references `output_segment` rows
/// with fingerprint/retention metadata (SPEC-017 §10).
pub const OUTPUT_REF_KIND: u8 = 9;
/// Pre-AB-1.5 segment ref without fingerprint/retention (§10 incomplete).
pub const OUTPUT_REF_KIND_LEGACY: u8 = 8;

/// Fixed size of [`encode_output_ref`].
pub const OUTPUT_REF_LEN: usize = 83;

/// Default high-volume stdout retention class id (RetainedStream).
pub const RETENTION_POLICY_RETAINED_STREAM: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamKind {
    Stdout = 0,
    Stderr = 1,
}

impl StreamKind {
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Stdout),
            1 => Some(Self::Stderr),
            _ => None,
        }
    }

    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// SPEC-017 §12 sensitive fingerprint identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FingerprintRef {
    /// Digest of non-sensitive public content. Never use for secret-bearing bytes.
    PublicContentDigest([u8; 32]),
    /// Keyed digest; key scope/version is explicit.
    LocalKeyedDigest { key_version: u32, digest: [u8; 32] },
    /// Opaque identity that does not embed payload bytes.
    OpaqueContentIdentity([u8; 16]),
}

impl FingerprintRef {
    const KIND_PUBLIC: u8 = 1;
    const KIND_LOCAL_KEYED: u8 = 2;
    const KIND_OPAQUE: u8 = 3;

    /// Deterministic public digest for non-sensitive stream bytes.
    /// Not a cryptographic claim; algorithm version is folded into the first byte.
    pub fn public_content_digest(bytes: &[u8]) -> Self {
        let mut out = [0_u8; 32];
        out[0] = 1; // algorithm version
        let len = (bytes.len() as u64).to_le_bytes();
        for (i, b) in len.iter().enumerate() {
            out[1 + i] ^= *b;
        }
        for (i, b) in bytes.iter().enumerate() {
            let j = 1 + (i % 31);
            out[j] = out[j].wrapping_add(b.rotate_left((i % 5) as u32));
            out[1 + ((j + 3) % 31)] ^= out[j];
        }
        Self::PublicContentDigest(out)
    }

    /// Opaque identity derived from ordinals only — never from payload bytes.
    pub fn opaque_from_ordinals(first_ordinal: u64, last_ordinal: u64) -> Self {
        let mut id = [0_u8; 16];
        id[..8].copy_from_slice(&first_ordinal.to_le_bytes());
        id[8..].copy_from_slice(&last_ordinal.to_le_bytes());
        Self::OpaqueContentIdentity(id)
    }

    fn encode(self, out: &mut [u8; 37]) {
        match self {
            Self::PublicContentDigest(digest) => {
                out[0] = Self::KIND_PUBLIC;
                out[1..5].copy_from_slice(&0_u32.to_le_bytes());
                out[5..37].copy_from_slice(&digest);
            }
            Self::LocalKeyedDigest {
                key_version,
                digest,
            } => {
                out[0] = Self::KIND_LOCAL_KEYED;
                out[1..5].copy_from_slice(&key_version.to_le_bytes());
                out[5..37].copy_from_slice(&digest);
            }
            Self::OpaqueContentIdentity(id) => {
                out[0] = Self::KIND_OPAQUE;
                out[1..5].copy_from_slice(&0_u32.to_le_bytes());
                out[5..21].copy_from_slice(&id);
                out[21..37].fill(0);
            }
        }
    }

    fn decode(bytes: &[u8; 37]) -> Option<Self> {
        let key_version = u32::from_le_bytes(bytes[1..5].try_into().ok()?);
        let mut digest = [0_u8; 32];
        digest.copy_from_slice(&bytes[5..37]);
        match bytes[0] {
            Self::KIND_PUBLIC => {
                if key_version != 0 {
                    return None;
                }
                Some(Self::PublicContentDigest(digest))
            }
            Self::KIND_LOCAL_KEYED => Some(Self::LocalKeyedDigest {
                key_version,
                digest,
            }),
            Self::KIND_OPAQUE => {
                if key_version != 0 || digest[16..].iter().any(|b| *b != 0) {
                    return None;
                }
                let mut id = [0_u8; 16];
                id.copy_from_slice(&digest[..16]);
                Some(Self::OpaqueContentIdentity(id))
            }
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetentionPolicyRef {
    pub policy_id: u32,
    pub policy_generation: u32,
}

impl RetentionPolicyRef {
    pub const fn retained_stream() -> Self {
        Self {
            policy_id: RETENTION_POLICY_RETAINED_STREAM,
            policy_generation: 1,
        }
    }
}

/// SPEC-017 §10 segment reference carried by a RunEvent (payload_ref + metadata).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputRef {
    pub first_segment_index: u32,
    pub segment_count: u32,
    /// Byte offset within the first referenced segment where this event's bytes begin.
    pub byte_offset: u32,
    pub byte_length: u64,
    pub first_ordinal: u64,
    pub last_ordinal: u64,
    pub stream_kind: StreamKind,
    pub fingerprint_ref: FingerprintRef,
    pub retention_policy_ref: RetentionPolicyRef,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputRefError {
    /// Payload is not an output reference at all.
    NotOutputRef,
    /// Legacy kind=8 ref without fingerprint/retention (§10 incomplete).
    MissingFingerprint,
    /// Truncated, wrong length, or internally inconsistent encoding.
    Corrupt,
}

/// Encode a SPEC-017 §10 output reference. Raw output bytes never appear here.
pub fn encode_output_ref(output: &OutputRef) -> Vec<u8> {
    let mut payload = Vec::with_capacity(OUTPUT_REF_LEN);
    payload.push(OUTPUT_REF_KIND);
    payload.extend_from_slice(&output.first_segment_index.to_le_bytes());
    payload.extend_from_slice(&output.segment_count.to_le_bytes());
    payload.extend_from_slice(&output.byte_offset.to_le_bytes());
    payload.extend_from_slice(&output.byte_length.to_le_bytes());
    payload.extend_from_slice(&output.first_ordinal.to_le_bytes());
    payload.extend_from_slice(&output.last_ordinal.to_le_bytes());
    payload.push(output.stream_kind.as_u8());
    let mut fingerprint = [0_u8; 37];
    output.fingerprint_ref.encode(&mut fingerprint);
    payload.extend_from_slice(&fingerprint);
    payload.extend_from_slice(&output.retention_policy_ref.policy_id.to_le_bytes());
    payload.extend_from_slice(&output.retention_policy_ref.policy_generation.to_le_bytes());
    debug_assert_eq!(payload.len(), OUTPUT_REF_LEN);
    payload
}

/// Decode a segment reference payload. Missing/corrupt fingerprint is explicit.
pub fn decode_output_ref(payload: &[u8]) -> Result<OutputRef, OutputRefError> {
    if payload.is_empty() {
        return Err(OutputRefError::NotOutputRef);
    }
    match payload[0] {
        OUTPUT_REF_KIND_LEGACY => return Err(OutputRefError::MissingFingerprint),
        OUTPUT_REF_KIND => {}
        _ => return Err(OutputRefError::NotOutputRef),
    }
    if payload.len() != OUTPUT_REF_LEN {
        return Err(OutputRefError::Corrupt);
    }
    let first_segment_index = u32::from_le_bytes(payload[1..5].try_into().unwrap());
    let segment_count = u32::from_le_bytes(payload[5..9].try_into().unwrap());
    let byte_offset = u32::from_le_bytes(payload[9..13].try_into().unwrap());
    let byte_length = u64::from_le_bytes(payload[13..21].try_into().unwrap());
    let first_ordinal = u64::from_le_bytes(payload[21..29].try_into().unwrap());
    let last_ordinal = u64::from_le_bytes(payload[29..37].try_into().unwrap());
    let stream_kind = StreamKind::from_u8(payload[37]).ok_or(OutputRefError::Corrupt)?;
    let mut fingerprint_bytes = [0_u8; 37];
    fingerprint_bytes.copy_from_slice(&payload[38..75]);
    let fingerprint_ref =
        FingerprintRef::decode(&fingerprint_bytes).ok_or(OutputRefError::Corrupt)?;
    let retention_policy_ref = RetentionPolicyRef {
        policy_id: u32::from_le_bytes(payload[75..79].try_into().unwrap()),
        policy_generation: u32::from_le_bytes(payload[79..83].try_into().unwrap()),
    };
    if segment_count == 0 && byte_length != 0 {
        return Err(OutputRefError::Corrupt);
    }
    Ok(OutputRef {
        first_segment_index,
        segment_count,
        byte_offset,
        byte_length,
        first_ordinal,
        last_ordinal,
        stream_kind,
        fingerprint_ref,
        retention_policy_ref,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_ref_round_trips_and_rejects_legacy_missing_fingerprint() {
        let output = OutputRef {
            first_segment_index: 2,
            segment_count: 3,
            byte_offset: 10,
            byte_length: 100,
            first_ordinal: 4,
            last_ordinal: 7,
            stream_kind: StreamKind::Stdout,
            fingerprint_ref: FingerprintRef::public_content_digest(b"hello"),
            retention_policy_ref: RetentionPolicyRef::retained_stream(),
        };
        let encoded = encode_output_ref(&output);
        assert_eq!(encoded.len(), OUTPUT_REF_LEN);
        assert_eq!(decode_output_ref(&encoded), Ok(output));

        let mut legacy = vec![OUTPUT_REF_KIND_LEGACY];
        legacy.extend_from_slice(&[0_u8; 32]);
        assert_eq!(
            decode_output_ref(&legacy),
            Err(OutputRefError::MissingFingerprint)
        );
        assert_eq!(
            decode_output_ref(&[OUTPUT_REF_KIND, 1, 2, 3]),
            Err(OutputRefError::Corrupt)
        );
        assert_eq!(decode_output_ref(&[1]), Err(OutputRefError::NotOutputRef));
    }

    #[test]
    fn public_digest_never_embeds_raw_secret_bytes() {
        let secret = b"super-secret-token-value-xyz";
        let FingerprintRef::PublicContentDigest(digest) =
            FingerprintRef::public_content_digest(secret)
        else {
            panic!("expected public digest");
        };
        assert!(!digest.windows(secret.len()).any(|w| w == secret));
        let opaque = FingerprintRef::opaque_from_ordinals(1, 2);
        let encoded = encode_output_ref(&OutputRef {
            first_segment_index: 0,
            segment_count: 1,
            byte_offset: 0,
            byte_length: secret.len() as u64,
            first_ordinal: 1,
            last_ordinal: 2,
            stream_kind: StreamKind::Stdout,
            fingerprint_ref: opaque,
            retention_policy_ref: RetentionPolicyRef::retained_stream(),
        });
        assert!(!encoded.windows(secret.len()).any(|w| w == secret));
    }
}
