//! Native Block RegionId map (#1484 Phase D).
//!
//! Gated on [`CAP_NATIVE_BLOCK_REGIONS`]. Carries BlockId → header/output
//! RegionIds plus optional on-window row spans so Flow can bind and paint
//! completed on-window Blocks from the primary frame without history-range IPC.

use crate::framing::FramingError;

/// Peer accepts Runtime→client native Block RegionId maps (type 48).
pub const CAP_NATIVE_BLOCK_REGIONS: u32 = 1 << 14;

pub const MAX_NATIVE_REGION_RECORDS: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeBlockRegions {
    pub block_id: u64,
    pub header_region: u64,
    pub output_region: u64,
    /// First live VT window row owned by `output_region` (0 when off-window).
    pub first_row: u16,
    /// Contiguous on-window row count for `output_region` (0 = off-window /
    /// sealed; client must not invent primary-frame clips).
    pub row_count: u16,
    /// Sealed ownership rows for `output_region` (HistoryStore-backed). Zero
    /// when the Block has no sealed content yet. Wire width is u32 (#1484:
    /// u16 saturated at 65_535 and broke 100k/1M Flow height / AX books).
    pub sealed_rows: u32,
}

/// Bounded replacement map for one attached execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockNativeRegions {
    pub revision: u64,
    /// Indexed Flow transcript height in presentation rows (0 when unknown).
    pub total_height: u64,
    pub records: Vec<NativeBlockRegions>,
}

impl BlockNativeRegions {
    const HEADER_LEN: usize = 24;
    const RECORD_LEN: usize = 32;

    pub fn try_encode(&self) -> Result<Vec<u8>, FramingError> {
        if self.records.len() > MAX_NATIVE_REGION_RECORDS {
            return Err(FramingError::OversizedPayload);
        }
        let mut out = Vec::with_capacity(Self::HEADER_LEN + self.records.len() * Self::RECORD_LEN);
        out.extend_from_slice(&self.revision.to_le_bytes());
        out.extend_from_slice(&self.total_height.to_le_bytes());
        out.extend_from_slice(&(self.records.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        for record in &self.records {
            if record.block_id == 0 || record.header_region == 0 || record.output_region == 0 {
                return Err(FramingError::MalformedPayload);
            }
            if record.row_count == 0 && record.first_row != 0 {
                return Err(FramingError::MalformedPayload);
            }
            out.extend_from_slice(&record.block_id.to_le_bytes());
            out.extend_from_slice(&record.header_region.to_le_bytes());
            out.extend_from_slice(&record.output_region.to_le_bytes());
            out.extend_from_slice(&record.first_row.to_le_bytes());
            out.extend_from_slice(&record.row_count.to_le_bytes());
            out.extend_from_slice(&record.sealed_rows.to_le_bytes());
            if out.len() > crate::framing::MAX_FRAME_PAYLOAD as usize {
                return Err(FramingError::OversizedPayload);
            }
        }
        Ok(out)
    }

    pub fn encode(&self) -> Vec<u8> {
        self.try_encode()
            .expect("BlockNativeRegions must satisfy bounded wire limits")
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        if bytes.len() > crate::framing::MAX_FRAME_PAYLOAD as usize {
            return Err(FramingError::OversizedPayload);
        }
        if bytes.len() < Self::HEADER_LEN {
            return Err(FramingError::TruncatedPayload);
        }
        let revision = u64::from_le_bytes(bytes[..8].try_into().unwrap());
        let total_height = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
        let count = u16::from_le_bytes(bytes[16..18].try_into().unwrap()) as usize;
        if u16::from_le_bytes(bytes[18..20].try_into().unwrap()) != 0
            || u32::from_le_bytes(bytes[20..24].try_into().unwrap()) != 0
        {
            return Err(FramingError::MalformedPayload);
        }
        if count > MAX_NATIVE_REGION_RECORDS {
            return Err(FramingError::OversizedPayload);
        }
        let expected = Self::HEADER_LEN
            .checked_add(
                count
                    .checked_mul(Self::RECORD_LEN)
                    .ok_or(FramingError::MalformedPayload)?,
            )
            .ok_or(FramingError::MalformedPayload)?;
        if bytes.len() != expected {
            return Err(FramingError::MalformedPayload);
        }
        let mut records = Vec::with_capacity(count);
        let mut offset = Self::HEADER_LEN;
        for _ in 0..count {
            let block_id = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
            let header_region =
                u64::from_le_bytes(bytes[offset + 8..offset + 16].try_into().unwrap());
            let output_region =
                u64::from_le_bytes(bytes[offset + 16..offset + 24].try_into().unwrap());
            let first_row = u16::from_le_bytes(bytes[offset + 24..offset + 26].try_into().unwrap());
            let row_count = u16::from_le_bytes(bytes[offset + 26..offset + 28].try_into().unwrap());
            let sealed_rows =
                u32::from_le_bytes(bytes[offset + 28..offset + 32].try_into().unwrap());
            if block_id == 0 || header_region == 0 || output_region == 0 {
                return Err(FramingError::MalformedPayload);
            }
            if row_count == 0 && first_row != 0 {
                return Err(FramingError::MalformedPayload);
            }
            records.push(NativeBlockRegions {
                block_id,
                header_region,
                output_region,
                first_row,
                row_count,
                sealed_rows,
            });
            offset += Self::RECORD_LEN;
        }
        Ok(Self {
            revision,
            total_height,
            records,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_native_region_map() {
        let map = BlockNativeRegions {
            revision: 9,
            total_height: 42,
            records: vec![
                NativeBlockRegions {
                    block_id: 1,
                    header_region: 2,
                    output_region: 3,
                    first_row: 4,
                    row_count: 5,
                    sealed_rows: 0,
                },
                NativeBlockRegions {
                    block_id: 4,
                    header_region: 5,
                    output_region: 6,
                    first_row: 0,
                    row_count: 0,
                    sealed_rows: 9,
                },
            ],
        };
        assert_eq!(BlockNativeRegions::decode(&map.encode()), Ok(map));
    }

    #[test]
    fn rejects_zero_ids() {
        let map = BlockNativeRegions {
            revision: 1,
            total_height: 0,
            records: vec![NativeBlockRegions {
                block_id: 0,
                header_region: 1,
                output_region: 2,
                first_row: 0,
                row_count: 0,
                sealed_rows: 0,
            }],
        };
        assert!(map.try_encode().is_err());
    }

    #[test]
    fn rejects_first_row_without_count() {
        let map = BlockNativeRegions {
            revision: 1,
            total_height: 0,
            records: vec![NativeBlockRegions {
                block_id: 1,
                header_region: 2,
                output_region: 3,
                first_row: 7,
                row_count: 0,
                sealed_rows: 0,
            }],
        };
        assert!(map.try_encode().is_err());
    }
}
