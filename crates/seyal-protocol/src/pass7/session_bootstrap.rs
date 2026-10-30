//! ADR-026 / SPEC-004 types 42–45 and the capability-bound BlockTimeline scope.
//!
//! Field widths below are the frozen implementation schema for #1388. Peers
//! select the BlockTimeline schema from negotiated `CAP_SESSION_BOOTSTRAP`,
//! never from payload length. Type 23 `ComposerStatus` is unchanged.

use crate::framing::FramingError;
use crate::{AttachmentId, ExecutionId};

use super::{attachment_id_from, exact_len};

pub const CAP_SESSION_BOOTSTRAP: u32 = 1 << 13;

/// Timeline header `schema` value when bit 13 was mutually negotiated.
pub const BLOCK_TIMELINE_SCHEMA_SESSION: u16 = 1;

fn execution_id_from(bytes: &[u8]) -> ExecutionId {
    let mut raw = [0u8; 16];
    raw.copy_from_slice(&bytes[..16]);
    ExecutionId::from_bytes(raw)
}

fn write_execution_id(out: &mut Vec<u8>, id: ExecutionId) {
    out.extend_from_slice(&id.to_bytes());
}

/// Block scope carried on a session-capable `BlockTimeline` record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum BlockScope {
    #[default]
    Local = 0,
    Session = 1,
}

impl BlockScope {
    fn from_u8(value: u8) -> Result<Self, FramingError> {
        match value {
            0 => Ok(Self::Local),
            1 => Ok(Self::Session),
            _ => Err(FramingError::MalformedPayload),
        }
    }
}

/// Presentation trust. `SessionTrusted` never substitutes for `LocalTrusted`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum BlockTrust {
    #[default]
    LocalTrusted = 0,
    SessionTrusted = 1,
    Unverified = 2,
}

impl BlockTrust {
    fn from_u8(value: u8) -> Result<Self, FramingError> {
        match value {
            0 => Ok(Self::LocalTrusted),
            1 => Ok(Self::SessionTrusted),
            2 => Ok(Self::Unverified),
            _ => Err(FramingError::MalformedPayload),
        }
    }
}

/// Composer input target class (ADR-026 §3.2). Carried only on type 45.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum InputTargetClass {
    None = 0,
    TrustedShell = 1,
    ForegroundProgram = 2,
}

impl InputTargetClass {
    fn from_u8(value: u8) -> Result<Self, FramingError> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::TrustedShell),
            2 => Ok(Self::ForegroundProgram),
            _ => Err(FramingError::MalformedPayload),
        }
    }
}

/// Derived composer mode. `Relay` is plaintext to the foreground program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ComposerTargetMode {
    Unavailable = 0,
    Submit = 1,
    Relay = 2,
}

impl ComposerTargetMode {
    fn from_u8(value: u8) -> Result<Self, FramingError> {
        match value {
            0 => Ok(Self::Unavailable),
            1 => Ok(Self::Submit),
            2 => Ok(Self::Relay),
            _ => Err(FramingError::MalformedPayload),
        }
    }
}

/// Why the composer is unavailable. `None` when mode is Submit or Relay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ComposerTargetReason {
    None = 0,
    NotAtPrompt = 1,
    AlternateScreen = 2,
    NotController = 3,
    NotFlow = 4,
    BootstrapHold = 5,
    NoKnownBlock = 6,
    Terminated = 7,
    Unsupported = 8,
}

impl ComposerTargetReason {
    fn from_u8(value: u8) -> Result<Self, FramingError> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::NotAtPrompt),
            2 => Ok(Self::AlternateScreen),
            3 => Ok(Self::NotController),
            4 => Ok(Self::NotFlow),
            5 => Ok(Self::BootstrapHold),
            6 => Ok(Self::NoKnownBlock),
            7 => Ok(Self::Terminated),
            8 => Ok(Self::Unsupported),
            _ => Err(FramingError::MalformedPayload),
        }
    }
}

/// F4 session overlay projected to a negotiated client. Type 44.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SessionOverlayWire {
    None = 0,
    Candidate = 1,
    Offered = 2,
    Declined = 3,
    Armed = 4,
    Delivering = 5,
    Trusted = 6,
    Failed = 7,
    Ended = 8,
}

impl SessionOverlayWire {
    fn from_u8(value: u8) -> Result<Self, FramingError> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Candidate),
            2 => Ok(Self::Offered),
            3 => Ok(Self::Declined),
            4 => Ok(Self::Armed),
            5 => Ok(Self::Delivering),
            6 => Ok(Self::Trusted),
            7 => Ok(Self::Failed),
            8 => Ok(Self::Ended),
            _ => Err(FramingError::MalformedPayload),
        }
    }
}

/// Controller consent decision. Type 43. Always and Never are reserved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SessionConsentDecision {
    Decline = 0,
    Accept = 1,
}

impl SessionConsentDecision {
    fn from_u8(value: u8) -> Result<Self, FramingError> {
        match value {
            0 => Ok(Self::Decline),
            1 => Ok(Self::Accept),
            _ => Err(FramingError::MalformedPayload),
        }
    }
}

/// Runtime→client type 42. Exact 64 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionOffer {
    pub attachment_id: AttachmentId,
    pub execution_id: ExecutionId,
    pub host_block_id: u64,
    pub offer_generation: u64,
    /// Echoed by `SessionConsent`. Runtime-owned fence, not a client guess.
    pub presentation_epoch: u64,
}

impl SessionOffer {
    const WIRE_LEN: usize = 64;

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        out.extend_from_slice(&self.attachment_id.to_bytes());
        write_execution_id(&mut out, self.execution_id);
        out.extend_from_slice(&self.host_block_id.to_le_bytes());
        out.extend_from_slice(&self.offer_generation.to_le_bytes());
        out.extend_from_slice(&self.presentation_epoch.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes());
        debug_assert_eq!(out.len(), Self::WIRE_LEN);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        if u64::from_le_bytes(bytes[56..64].try_into().unwrap()) != 0
            || bytes[..16] == [0; 16]
            || bytes[16..32] == [0; 16]
        {
            return Err(FramingError::MalformedPayload);
        }
        let host_block_id = u64::from_le_bytes(bytes[32..40].try_into().unwrap());
        let offer_generation = u64::from_le_bytes(bytes[40..48].try_into().unwrap());
        if host_block_id == 0 || offer_generation == 0 {
            return Err(FramingError::MalformedPayload);
        }
        Ok(Self {
            attachment_id: attachment_id_from(&bytes[..16]),
            execution_id: execution_id_from(&bytes[16..32]),
            host_block_id,
            offer_generation,
            presentation_epoch: u64::from_le_bytes(bytes[48..56].try_into().unwrap()),
        })
    }
}

/// Client→Runtime type 43. Exact 64 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionConsent {
    pub attachment_id: AttachmentId,
    pub execution_id: ExecutionId,
    pub host_block_id: u64,
    pub offer_generation: u64,
    pub presentation_epoch: u64,
    pub decision: SessionConsentDecision,
}

impl SessionConsent {
    const WIRE_LEN: usize = 64;

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        out.extend_from_slice(&self.attachment_id.to_bytes());
        write_execution_id(&mut out, self.execution_id);
        out.extend_from_slice(&self.host_block_id.to_le_bytes());
        out.extend_from_slice(&self.offer_generation.to_le_bytes());
        out.extend_from_slice(&self.presentation_epoch.to_le_bytes());
        out.push(self.decision as u8);
        out.extend_from_slice(&[0; 7]);
        debug_assert_eq!(out.len(), Self::WIRE_LEN);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        if bytes[57..64] != [0; 7] || bytes[..16] == [0; 16] || bytes[16..32] == [0; 16] {
            return Err(FramingError::MalformedPayload);
        }
        let host_block_id = u64::from_le_bytes(bytes[32..40].try_into().unwrap());
        let offer_generation = u64::from_le_bytes(bytes[40..48].try_into().unwrap());
        if host_block_id == 0 || offer_generation == 0 {
            return Err(FramingError::MalformedPayload);
        }
        Ok(Self {
            attachment_id: attachment_id_from(&bytes[..16]),
            execution_id: execution_id_from(&bytes[16..32]),
            host_block_id,
            offer_generation,
            presentation_epoch: u64::from_le_bytes(bytes[48..56].try_into().unwrap()),
            decision: SessionConsentDecision::from_u8(bytes[56])?,
        })
    }
}

/// Runtime→client type 44. Exact 56 bytes. `revision` moves forward only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionStatus {
    pub attachment_id: AttachmentId,
    pub execution_id: ExecutionId,
    pub host_block_id: u64,
    pub overlay: SessionOverlayWire,
    pub revision: u64,
}

impl SessionStatus {
    const WIRE_LEN: usize = 56;

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        out.extend_from_slice(&self.attachment_id.to_bytes());
        write_execution_id(&mut out, self.execution_id);
        out.extend_from_slice(&self.host_block_id.to_le_bytes());
        out.push(self.overlay as u8);
        out.extend_from_slice(&[0; 7]);
        out.extend_from_slice(&self.revision.to_le_bytes());
        debug_assert_eq!(out.len(), Self::WIRE_LEN);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        if bytes[41..48] != [0; 7] || bytes[..16] == [0; 16] || bytes[16..32] == [0; 16] {
            return Err(FramingError::MalformedPayload);
        }
        let revision = u64::from_le_bytes(bytes[48..56].try_into().unwrap());
        if revision == 0 {
            return Err(FramingError::MalformedPayload);
        }
        Ok(Self {
            attachment_id: attachment_id_from(&bytes[..16]),
            execution_id: execution_id_from(&bytes[16..32]),
            host_block_id: u64::from_le_bytes(bytes[32..40].try_into().unwrap()),
            overlay: SessionOverlayWire::from_u8(bytes[40])?,
            revision,
        })
    }
}

/// Runtime→client type 45. Exact 48 bytes. Not a longer type-23 body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComposerTarget {
    pub attachment_id: AttachmentId,
    pub target: InputTargetClass,
    pub scope: BlockScope,
    pub mode: ComposerTargetMode,
    pub reason: ComposerTargetReason,
    pub block_id: u64,
    pub target_generation: u64,
    pub revision: u64,
}

impl ComposerTarget {
    const WIRE_LEN: usize = 48;

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::WIRE_LEN);
        out.extend_from_slice(&self.attachment_id.to_bytes());
        out.push(self.target as u8);
        out.push(self.scope as u8);
        out.push(self.mode as u8);
        out.push(self.reason as u8);
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&self.block_id.to_le_bytes());
        out.extend_from_slice(&self.target_generation.to_le_bytes());
        out.extend_from_slice(&self.revision.to_le_bytes());
        debug_assert_eq!(out.len(), Self::WIRE_LEN);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        exact_len(bytes, Self::WIRE_LEN)?;
        if bytes[..16] == [0; 16] {
            return Err(FramingError::MalformedPayload);
        }
        let target_generation = u64::from_le_bytes(bytes[32..40].try_into().unwrap());
        let revision = u64::from_le_bytes(bytes[40..48].try_into().unwrap());
        if target_generation == 0 || revision == 0 {
            return Err(FramingError::MalformedPayload);
        }
        let mode = ComposerTargetMode::from_u8(bytes[18])?;
        let reason = ComposerTargetReason::from_u8(bytes[19])?;
        if mode != ComposerTargetMode::Unavailable && reason != ComposerTargetReason::None {
            return Err(FramingError::MalformedPayload);
        }
        Ok(Self {
            attachment_id: attachment_id_from(&bytes[..16]),
            target: InputTargetClass::from_u8(bytes[16])?,
            scope: BlockScope::from_u8(bytes[17])?,
            mode,
            reason,
            block_id: u64::from_le_bytes(bytes[24..32].try_into().unwrap()),
            target_generation,
            revision,
        })
    }
}

/// Explicit scope/trust suffix on a session-schema record. Presence is the
/// schema tag in the timeline header plus this fixed trailer, not a length guess.
pub(crate) fn encode_scope_trust(scope: BlockScope, trust: BlockTrust) -> [u8; 4] {
    [scope as u8, trust as u8, 1, 0]
}

pub(crate) fn decode_scope_trust(bytes: &[u8]) -> Result<(BlockScope, BlockTrust), FramingError> {
    if bytes.len() != 4 || bytes[2] != 1 || bytes[3] != 0 {
        return Err(FramingError::MalformedPayload);
    }
    Ok((
        BlockScope::from_u8(bytes[0])?,
        BlockTrust::from_u8(bytes[1])?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attachment() -> AttachmentId {
        AttachmentId::from_bytes(9u128.to_le_bytes())
    }

    fn execution() -> ExecutionId {
        ExecutionId::from_bytes(4u128.to_le_bytes())
    }

    #[test]
    fn capability_bit_is_thirteen_and_gated_constant() {
        assert_eq!(CAP_SESSION_BOOTSTRAP, 1 << 13);
    }

    #[test]
    fn session_messages_round_trip_at_exact_length() {
        let offer = SessionOffer {
            attachment_id: attachment(),
            execution_id: execution(),
            host_block_id: 7,
            offer_generation: 2,
            presentation_epoch: 1,
        };
        assert_eq!(offer.encode().len(), 64);
        assert_eq!(SessionOffer::decode(&offer.encode()).unwrap(), offer);

        let consent = SessionConsent {
            attachment_id: attachment(),
            execution_id: execution(),
            host_block_id: 7,
            offer_generation: 2,
            presentation_epoch: 1,
            decision: SessionConsentDecision::Decline,
        };
        assert_eq!(consent.encode().len(), 64);
        assert_eq!(SessionConsent::decode(&consent.encode()).unwrap(), consent);

        let status = SessionStatus {
            attachment_id: attachment(),
            execution_id: execution(),
            host_block_id: 7,
            overlay: SessionOverlayWire::Declined,
            revision: 3,
        };
        assert_eq!(status.encode().len(), 56);
        assert_eq!(SessionStatus::decode(&status.encode()).unwrap(), status);

        let target = ComposerTarget {
            attachment_id: attachment(),
            target: InputTargetClass::ForegroundProgram,
            scope: BlockScope::Local,
            mode: ComposerTargetMode::Relay,
            reason: ComposerTargetReason::None,
            block_id: 7,
            target_generation: 4,
            revision: 5,
        };
        assert_eq!(target.encode().len(), 48);
        assert_eq!(ComposerTarget::decode(&target.encode()).unwrap(), target);
    }

    #[test]
    fn malformed_session_payloads_fail_closed() {
        let offer = SessionOffer {
            attachment_id: attachment(),
            execution_id: execution(),
            host_block_id: 7,
            offer_generation: 2,
            presentation_epoch: 0,
        };
        let mut short = offer.encode();
        short.pop();
        assert!(SessionOffer::decode(&short).is_err());
        let mut zero_gen = offer.encode();
        zero_gen[40..48].copy_from_slice(&0u64.to_le_bytes());
        assert!(SessionOffer::decode(&zero_gen).is_err());

        let mut target = ComposerTarget {
            attachment_id: attachment(),
            target: InputTargetClass::None,
            scope: BlockScope::Local,
            mode: ComposerTargetMode::Relay,
            reason: ComposerTargetReason::BootstrapHold,
            block_id: 0,
            target_generation: 1,
            revision: 1,
        }
        .encode();
        assert!(ComposerTarget::decode(&target).is_err());
        target[19] = 0;
        assert!(ComposerTarget::decode(&target).is_ok());
    }

    #[test]
    fn scope_trust_trailer_requires_explicit_presence() {
        assert_eq!(
            decode_scope_trust(&encode_scope_trust(
                BlockScope::Session,
                BlockTrust::SessionTrusted
            ))
            .unwrap(),
            (BlockScope::Session, BlockTrust::SessionTrusted)
        );
        assert!(decode_scope_trust(&[0, 0, 0, 0]).is_err());
    }
}
