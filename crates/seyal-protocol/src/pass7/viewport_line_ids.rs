//! SPEC-004 §8.1 Runtime→client viewport LineIds (type 35, capability bit 9).

use crate::{display::MAX_DISPLAY_ROWS, framing::FramingError};

/// Runtime→client primary viewport LineIds for one display generation
/// (`MessageType::ViewportLineIds` = 35), gated on `CAP_VIEWPORT_LINE_IDS`.
///
/// Wire: `generation(u64 LE)` + `row_count(u16 LE)` + `reserved(u16=0)` +
/// `row_count` little-endian `u64` LineIds. Length must equal the primary
/// viewport row count and every id must be non-zero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewportLineIds {
    pub generation: u64,
    pub line_ids: Vec<u64>,
}

impl ViewportLineIds {
    pub const HEADER_LEN: usize = 12;

    pub fn encode(&self) -> Vec<u8> {
        debug_assert!(self.validate().is_ok());
        let mut out = Vec::with_capacity(Self::HEADER_LEN + self.line_ids.len() * 8);
        out.extend_from_slice(&self.generation.to_le_bytes());
        out.extend_from_slice(&(self.line_ids.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        for id in &self.line_ids {
            out.extend_from_slice(&id.to_le_bytes());
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        if bytes.len() < Self::HEADER_LEN {
            return Err(FramingError::TruncatedPayload);
        }
        let generation = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
        let row_count = u16::from_le_bytes(bytes[8..10].try_into().unwrap());
        let reserved = u16::from_le_bytes(bytes[10..12].try_into().unwrap());
        if reserved != 0 {
            return Err(FramingError::MalformedPayload);
        }
        if row_count == 0 || row_count > MAX_DISPLAY_ROWS {
            return Err(FramingError::MalformedPayload);
        }
        let expected = Self::HEADER_LEN
            .checked_add(
                usize::from(row_count)
                    .checked_mul(8)
                    .ok_or(FramingError::LengthOverflow)?,
            )
            .ok_or(FramingError::LengthOverflow)?;
        if bytes.len() != expected {
            return Err(FramingError::ExactLengthMismatch);
        }
        let mut line_ids = Vec::with_capacity(usize::from(row_count));
        let mut offset = Self::HEADER_LEN;
        for _ in 0..row_count {
            let id = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
            if id == 0 {
                return Err(FramingError::MalformedPayload);
            }
            line_ids.push(id);
            offset += 8;
        }
        let message = Self {
            generation,
            line_ids,
        };
        message.validate()?;
        Ok(message)
    }

    fn validate(&self) -> Result<(), FramingError> {
        if self.generation == 0 {
            return Err(FramingError::MalformedPayload);
        }
        if self.line_ids.is_empty() || self.line_ids.len() > usize::from(MAX_DISPLAY_ROWS) {
            return Err(FramingError::MalformedPayload);
        }
        if self.line_ids.contains(&0) {
            return Err(FramingError::MalformedPayload);
        }
        // Viewport rows may be reordered by insert-line / reverse-index / CSI T
        // without renumbering LineIds, so ids need not be monotonic. They must
        // remain unique within one viewport so start_line mapping is unambiguous.
        let mut seen = std::collections::BTreeSet::new();
        for id in &self.line_ids {
            if !seen.insert(*id) {
                return Err(FramingError::MalformedPayload);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_line_ids_round_trip() {
        let message = ViewportLineIds {
            generation: 42,
            line_ids: vec![1, 2, 3, 10],
        };
        assert_eq!(ViewportLineIds::decode(&message.encode()).unwrap(), message);
    }

    #[test]
    fn viewport_line_ids_reject_zero_ids() {
        let mut encoded = ViewportLineIds {
            generation: 1,
            line_ids: vec![7, 8, 9],
        }
        .encode();
        encoded[12 + 8..12 + 16].copy_from_slice(&0u64.to_le_bytes());
        assert_eq!(
            ViewportLineIds::decode(&encoded),
            Err(FramingError::MalformedPayload)
        );
    }

    #[test]
    fn viewport_line_ids_reject_row_count_mismatch() {
        let mut encoded = ViewportLineIds {
            generation: 1,
            line_ids: vec![1, 2],
        }
        .encode();
        encoded[8..10].copy_from_slice(&3u16.to_le_bytes());
        assert_eq!(
            ViewportLineIds::decode(&encoded),
            Err(FramingError::ExactLengthMismatch)
        );
    }

    #[test]
    fn viewport_line_ids_reject_row_count_zero_or_above_max_rows() {
        let mut encoded = ViewportLineIds {
            generation: 1,
            line_ids: vec![1],
        }
        .encode();
        encoded[8..10].copy_from_slice(&0u16.to_le_bytes());
        assert_eq!(
            ViewportLineIds::decode(&encoded),
            Err(FramingError::MalformedPayload)
        );
        let oversized = MAX_DISPLAY_ROWS + 1;
        let mut encoded = Vec::new();
        encoded.extend_from_slice(&1u64.to_le_bytes());
        encoded.extend_from_slice(&oversized.to_le_bytes());
        encoded.extend_from_slice(&0u16.to_le_bytes());
        for id in 1..=u64::from(oversized) {
            encoded.extend_from_slice(&id.to_le_bytes());
        }
        assert_eq!(
            ViewportLineIds::decode(&encoded),
            Err(FramingError::MalformedPayload)
        );
    }

    #[test]
    fn viewport_line_ids_reject_nonzero_reserved() {
        let mut encoded = ViewportLineIds {
            generation: 1,
            line_ids: vec![1],
        }
        .encode();
        encoded[10..12].copy_from_slice(&1u16.to_le_bytes());
        assert_eq!(
            ViewportLineIds::decode(&encoded),
            Err(FramingError::MalformedPayload)
        );
    }

    #[test]
    fn viewport_line_ids_reject_generation_zero() {
        let mut encoded = ViewportLineIds {
            generation: 1,
            line_ids: vec![1, 2],
        }
        .encode();
        encoded[0..8].copy_from_slice(&0u64.to_le_bytes());
        assert_eq!(
            ViewportLineIds::decode(&encoded),
            Err(FramingError::MalformedPayload)
        );
    }

    #[test]
    fn viewport_line_ids_accept_reordered_unique_rows() {
        let message = ViewportLineIds {
            generation: 7,
            line_ids: vec![1, 4, 2],
        };
        assert_eq!(ViewportLineIds::decode(&message.encode()).unwrap(), message);
    }

    #[test]
    fn viewport_line_ids_reject_duplicate_ids() {
        let mut encoded = ViewportLineIds {
            generation: 1,
            line_ids: vec![1, 2, 3],
        }
        .encode();
        encoded[12 + 16..12 + 24].copy_from_slice(&1u64.to_le_bytes());
        assert_eq!(
            ViewportLineIds::decode(&encoded),
            Err(FramingError::MalformedPayload)
        );
    }
}
