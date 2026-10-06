//! Wire codecs for ADR-012 §12 / SY-006 presence and capability negotiation.
//!
//! Carries enforcement class + presence-source tiers on the protocol boundary
//! so Hello/capability handshake surfaces never invent an implicit “full
//! control” class. Agent Backend remains the sole AgentRun transition writer.

use seyal_agent_core::{
    reject_duplicate_capability_ids, CapabilityId, CapabilitySupport, EnforcementClass,
    NegotiatedCapability, PresenceCapabilityProjection, PresenceObservation, PresenceSourceTier,
};

use crate::FrameError;

/// Maximum negotiated capabilities in one advertisement (bounded metadata).
pub const MAX_NEGOTIATED_CAPABILITIES: usize = 64;

/// Encode one [`NegotiatedCapability`] as `id_u16_le || support_u8` (3 bytes).
pub fn encode_negotiated_capability(cap: NegotiatedCapability) -> [u8; 3] {
    let id = cap.id.as_u16().to_le_bytes();
    [id[0], id[1], cap.support.as_u8()]
}

/// Decode one negotiated capability from 3 bytes.
pub fn decode_negotiated_capability(bytes: [u8; 3]) -> Result<NegotiatedCapability, FrameError> {
    let id = CapabilityId::from_u16(u16::from_le_bytes([bytes[0], bytes[1]]))
        .ok_or(FrameError::Malformed)?;
    let support = CapabilitySupport::from_u8(bytes[2]).ok_or(FrameError::Malformed)?;
    Ok(NegotiatedCapability { id, support })
}

/// Encode a capability advertisement: `count_u16_le` + capabilities.
pub fn encode_capability_advertisement(
    capabilities: &[NegotiatedCapability],
) -> Result<Vec<u8>, FrameError> {
    if capabilities.len() > MAX_NEGOTIATED_CAPABILITIES {
        return Err(FrameError::Malformed);
    }
    reject_duplicate_capability_ids(capabilities).map_err(|_| FrameError::Malformed)?;
    let mut out = Vec::with_capacity(2 + capabilities.len() * 3);
    out.extend_from_slice(&(capabilities.len() as u16).to_le_bytes());
    for cap in capabilities {
        out.extend_from_slice(&encode_negotiated_capability(*cap));
    }
    Ok(out)
}

/// Decode a capability advertisement produced by [`encode_capability_advertisement`].
///
/// Duplicate [`CapabilityId`]s are rejected (no first-wins / order-dependent merge).
pub fn decode_capability_advertisement(
    body: &[u8],
) -> Result<Vec<NegotiatedCapability>, FrameError> {
    if body.len() < 2 {
        return Err(FrameError::Malformed);
    }
    let count = u16::from_le_bytes([body[0], body[1]]) as usize;
    if count > MAX_NEGOTIATED_CAPABILITIES {
        return Err(FrameError::Malformed);
    }
    let expected = 2 + count * 3;
    if body.len() != expected {
        return Err(FrameError::Malformed);
    }
    let mut caps = Vec::with_capacity(count);
    for i in 0..count {
        let start = 2 + i * 3;
        let chunk = [body[start], body[start + 1], body[start + 2]];
        caps.push(decode_negotiated_capability(chunk)?);
    }
    reject_duplicate_capability_ids(&caps).map_err(|_| FrameError::Malformed)?;
    Ok(caps)
}

/// Encode presence observation: `source_u8 || enforcement_u8 || typed_boundary_u8`.
pub fn encode_presence_observation(obs: PresenceObservation) -> [u8; 3] {
    [
        obs.source.as_u8(),
        obs.enforcement.as_u8(),
        u8::from(obs.typed_backend_boundary),
    ]
}

/// Decode presence observation; rejects illegal source/class combinations.
pub fn decode_presence_observation(bytes: [u8; 3]) -> Result<PresenceObservation, FrameError> {
    let source = PresenceSourceTier::from_u8(bytes[0]).ok_or(FrameError::Malformed)?;
    let enforcement = EnforcementClass::from_u8(bytes[1]).ok_or(FrameError::Malformed)?;
    let typed_backend_boundary = match bytes[2] {
        0 => false,
        1 => true,
        _ => return Err(FrameError::Malformed),
    };
    PresenceObservation::new(source, enforcement, typed_backend_boundary)
        .map_err(|_| FrameError::Malformed)
}

/// Encode a full presence/capability projection for handshake/API surfaces.
///
/// Layout: `cap_advertisement || has_presence_u8 || [presence_3]?`
pub fn encode_presence_capability_projection(
    projection: &PresenceCapabilityProjection,
) -> Result<Vec<u8>, FrameError> {
    let mut out = encode_capability_advertisement(&projection.capabilities)?;
    match projection.presence {
        Some(obs) => {
            out.push(1);
            out.extend_from_slice(&encode_presence_observation(obs));
        }
        None => out.push(0),
    }
    Ok(out)
}

/// Decode a presence/capability projection (structural only).
///
/// Callers must install via [`PresenceCapabilityProjection::install_capabilities`]
/// under an explicit [`seyal_agent_core::CapabilityInstallTrust`] binding before
/// treating `BackendEnforced` as authoritative.
pub fn decode_presence_capability_projection(
    body: &[u8],
) -> Result<PresenceCapabilityProjection, FrameError> {
    if body.len() < 3 {
        return Err(FrameError::Malformed);
    }
    let count = u16::from_le_bytes([body[0], body[1]]) as usize;
    if count > MAX_NEGOTIATED_CAPABILITIES {
        return Err(FrameError::Malformed);
    }
    let caps_end = 2 + count * 3;
    if body.len() < caps_end + 1 {
        return Err(FrameError::Malformed);
    }
    let capabilities = decode_capability_advertisement(&body[..caps_end])?;
    let mut projection = PresenceCapabilityProjection::new(capabilities);
    match body[caps_end] {
        0 => {
            if body.len() != caps_end + 1 {
                return Err(FrameError::Malformed);
            }
        }
        1 => {
            if body.len() != caps_end + 1 + 3 {
                return Err(FrameError::Malformed);
            }
            let obs = decode_presence_observation([
                body[caps_end + 1],
                body[caps_end + 2],
                body[caps_end + 3],
            ])?;
            projection
                .record_presence(obs)
                .map_err(|_| FrameError::Malformed)?;
        }
        _ => return Err(FrameError::Malformed),
    }
    Ok(projection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use seyal_agent_core::{CapabilityId, EnforcementClass, PresenceSourceTier};

    #[test]
    fn capability_advertisement_round_trip_carries_enforcement_classes() {
        let caps = vec![
            NegotiatedCapability::supported(CapabilityId::Pause, EnforcementClass::Observed),
            NegotiatedCapability::supported(
                CapabilityId::Approve,
                EnforcementClass::UpstreamRequestable,
            ),
            NegotiatedCapability::supported(
                CapabilityId::ModelSelect,
                EnforcementClass::BackendEnforced,
            ),
            NegotiatedCapability::unsupported(CapabilityId::Deny),
            NegotiatedCapability::unknown(CapabilityId::Account),
        ];
        let encoded = encode_capability_advertisement(&caps).unwrap();
        let decoded = decode_capability_advertisement(&encoded).unwrap();
        assert_eq!(decoded, caps);
        assert_eq!(decoded[0].enforcement(), Some(EnforcementClass::Observed));
        assert_eq!(
            decoded[2].enforcement(),
            Some(EnforcementClass::BackendEnforced)
        );
    }

    #[test]
    fn presence_projection_round_trip() {
        let mut projection =
            PresenceCapabilityProjection::new(vec![NegotiatedCapability::supported(
                CapabilityId::LifecycleObservation,
                EnforcementClass::Observed,
            )]);
        projection
            .record_presence(
                PresenceObservation::new(
                    PresenceSourceTier::StructuredAdapter,
                    EnforcementClass::UpstreamRequestable,
                    false,
                )
                .unwrap(),
            )
            .unwrap();
        let encoded = encode_presence_capability_projection(&projection).unwrap();
        let decoded = decode_presence_capability_projection(&encoded).unwrap();
        assert_eq!(decoded, projection);
    }

    #[test]
    fn decode_rejects_heuristic_backend_enforced() {
        let bytes = [
            PresenceSourceTier::LowConfidenceHeuristic.as_u8(),
            EnforcementClass::BackendEnforced.as_u8(),
            1,
        ];
        assert_eq!(
            decode_presence_observation(bytes),
            Err(FrameError::Malformed)
        );
    }

    #[test]
    fn presence_decode_rejects_duplicate_capability_ids() {
        let pause = encode_negotiated_capability(NegotiatedCapability::supported(
            CapabilityId::Pause,
            EnforcementClass::Observed,
        ));
        let mut body = Vec::new();
        body.extend_from_slice(&2u16.to_le_bytes());
        body.extend_from_slice(&pause);
        body.extend_from_slice(&pause);
        assert_eq!(
            decode_capability_advertisement(&body),
            Err(FrameError::Malformed)
        );
    }
}
