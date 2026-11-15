//! One bounded content request/response for all visible Block RegionGrid clips.

use crate::{framing::FramingError, AttachmentId};

use super::{attachment_id_from, HistoryRangeSnapshot, HistoryRangeStatus};

pub const MAX_BLOCK_VIEWPORT_CLIPS: usize = 128;
pub const MAX_BLOCK_VIEWPORT_CLIP_ROWS: u32 = 1_024;
pub const MAX_BLOCK_VIEWPORT_TOTAL_ROWS: u32 = 4_096;
pub const CAP_BLOCK_VIEWPORT_CONTENT: u32 = 1 << 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockViewportContentClip {
    pub region_id: u64,
    pub first_row: u32,
    pub row_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockViewportContentRequest {
    pub attachment_id: AttachmentId,
    pub request_id: u64,
    pub plan_generation: u64,
    pub clips: Vec<BlockViewportContentClip>,
}

impl BlockViewportContentRequest {
    const HEADER_LEN: usize = 36;
    const CLIP_LEN: usize = 16;

    pub fn try_encode(&self) -> Result<Vec<u8>, FramingError> {
        if self.request_id == 0
            || self.plan_generation == 0
            || self.clips.is_empty()
            || self.clips.len() > MAX_BLOCK_VIEWPORT_CLIPS
        {
            return Err(FramingError::MalformedPayload);
        }
        let mut total_rows = 0u32;
        let mut out = Vec::with_capacity(Self::HEADER_LEN + self.clips.len() * Self::CLIP_LEN);
        out.extend_from_slice(&self.attachment_id.to_bytes());
        out.extend_from_slice(&self.request_id.to_le_bytes());
        out.extend_from_slice(&self.plan_generation.to_le_bytes());
        out.extend_from_slice(&(self.clips.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        for clip in &self.clips {
            if clip.region_id == 0
                || clip.row_count == 0
                || clip.row_count > MAX_BLOCK_VIEWPORT_CLIP_ROWS
                || clip.first_row.checked_add(clip.row_count).is_none()
            {
                return Err(FramingError::MalformedPayload);
            }
            total_rows = total_rows.saturating_add(clip.row_count);
            if total_rows > MAX_BLOCK_VIEWPORT_TOTAL_ROWS {
                return Err(FramingError::OversizedPayload);
            }
            out.extend_from_slice(&clip.region_id.to_le_bytes());
            out.extend_from_slice(&clip.first_row.to_le_bytes());
            out.extend_from_slice(&clip.row_count.to_le_bytes());
        }
        Ok(out)
    }

    pub fn encode(&self) -> Vec<u8> {
        self.try_encode()
            .expect("bounded BlockViewport content request")
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        if bytes.len() < Self::HEADER_LEN {
            return Err(FramingError::TruncatedPayload);
        }
        let request_id = u64::from_le_bytes(bytes[16..24].try_into().unwrap());
        let plan_generation = u64::from_le_bytes(bytes[24..32].try_into().unwrap());
        let count = u16::from_le_bytes(bytes[32..34].try_into().unwrap()) as usize;
        if request_id == 0 || plan_generation == 0 || bytes[34..36] != [0; 2] {
            return Err(FramingError::MalformedPayload);
        }
        if count == 0 || count > MAX_BLOCK_VIEWPORT_CLIPS {
            return Err(FramingError::OversizedPayload);
        }
        let expected = Self::HEADER_LEN
            .checked_add(
                count
                    .checked_mul(Self::CLIP_LEN)
                    .ok_or(FramingError::MalformedPayload)?,
            )
            .ok_or(FramingError::MalformedPayload)?;
        if bytes.len() != expected {
            return Err(FramingError::ExactLengthMismatch);
        }
        let mut clips = Vec::with_capacity(count);
        let mut total_rows = 0u32;
        let mut offset = Self::HEADER_LEN;
        for _ in 0..count {
            let clip = BlockViewportContentClip {
                region_id: u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap()),
                first_row: u32::from_le_bytes(bytes[offset + 8..offset + 12].try_into().unwrap()),
                row_count: u32::from_le_bytes(bytes[offset + 12..offset + 16].try_into().unwrap()),
            };
            if clip.region_id == 0
                || clip.row_count == 0
                || clip.row_count > MAX_BLOCK_VIEWPORT_CLIP_ROWS
                || clip.first_row.checked_add(clip.row_count).is_none()
            {
                return Err(FramingError::MalformedPayload);
            }
            total_rows = total_rows.saturating_add(clip.row_count);
            if total_rows > MAX_BLOCK_VIEWPORT_TOTAL_ROWS {
                return Err(FramingError::OversizedPayload);
            }
            clips.push(clip);
            offset += Self::CLIP_LEN;
        }
        Ok(Self {
            attachment_id: attachment_id_from(&bytes[..16]),
            request_id,
            plan_generation,
            clips,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockViewportRegionContent {
    pub region_id: u64,
    pub first_row: u32,
    pub snapshot: HistoryRangeSnapshot,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockViewportContentSnapshot {
    pub request_id: u64,
    pub plan_generation: u64,
    pub revision: u64,
    pub status: HistoryRangeStatus,
    pub regions: Vec<BlockViewportRegionContent>,
}

impl BlockViewportContentSnapshot {
    const HEADER_LEN: usize = 32;
    const REGION_HEADER_LEN: usize = 16;

    pub fn try_encode(&self) -> Result<Vec<u8>, FramingError> {
        if self.request_id == 0
            || self.plan_generation == 0
            || self.regions.len() > MAX_BLOCK_VIEWPORT_CLIPS
        {
            return Err(FramingError::MalformedPayload);
        }
        let mut out = Vec::with_capacity(Self::HEADER_LEN);
        out.extend_from_slice(&self.request_id.to_le_bytes());
        out.extend_from_slice(&self.plan_generation.to_le_bytes());
        out.extend_from_slice(&self.revision.to_le_bytes());
        out.push(self.status as u8);
        out.push(0);
        out.extend_from_slice(&(self.regions.len() as u16).to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        for region in &self.regions {
            if region.region_id == 0
                || region.snapshot.request_id != self.request_id
                || region.snapshot.block_id != region.region_id
            {
                return Err(FramingError::MalformedPayload);
            }
            let payload = region.snapshot.try_encode()?;
            out.extend_from_slice(&region.region_id.to_le_bytes());
            out.extend_from_slice(&region.first_row.to_le_bytes());
            out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            out.extend_from_slice(&payload);
            if out.len() > crate::framing::MAX_FRAME_PAYLOAD as usize {
                return Err(FramingError::OversizedPayload);
            }
        }
        Ok(out)
    }

    pub fn encode(&self) -> Vec<u8> {
        self.try_encode()
            .expect("bounded BlockViewport content snapshot")
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        if bytes.len() < Self::HEADER_LEN {
            return Err(FramingError::TruncatedPayload);
        }
        if bytes.len() > crate::framing::MAX_FRAME_PAYLOAD as usize {
            return Err(FramingError::OversizedPayload);
        }
        let request_id = u64::from_le_bytes(bytes[..8].try_into().unwrap());
        let plan_generation = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
        let revision = u64::from_le_bytes(bytes[16..24].try_into().unwrap());
        let status = match bytes[24] {
            0 => HistoryRangeStatus::Complete,
            1 => HistoryRangeStatus::Truncated,
            2 => HistoryRangeStatus::Stale,
            3 => HistoryRangeStatus::Unsupported,
            _ => return Err(FramingError::MalformedPayload),
        };
        let count = u16::from_le_bytes(bytes[26..28].try_into().unwrap()) as usize;
        if request_id == 0 || plan_generation == 0 || bytes[25] != 0 || bytes[28..32] != [0; 4] {
            return Err(FramingError::MalformedPayload);
        }
        if count > MAX_BLOCK_VIEWPORT_CLIPS {
            return Err(FramingError::OversizedPayload);
        }
        let mut regions = Vec::with_capacity(count);
        let mut offset = Self::HEADER_LEN;
        for _ in 0..count {
            let header_end = offset
                .checked_add(Self::REGION_HEADER_LEN)
                .ok_or(FramingError::MalformedPayload)?;
            let header = bytes
                .get(offset..header_end)
                .ok_or(FramingError::TruncatedPayload)?;
            let region_id = u64::from_le_bytes(header[..8].try_into().unwrap());
            let first_row = u32::from_le_bytes(header[8..12].try_into().unwrap());
            let len = u32::from_le_bytes(header[12..16].try_into().unwrap()) as usize;
            let end = header_end
                .checked_add(len)
                .ok_or(FramingError::MalformedPayload)?;
            let snapshot = HistoryRangeSnapshot::decode(
                bytes
                    .get(header_end..end)
                    .ok_or(FramingError::TruncatedPayload)?,
            )?;
            if region_id == 0 || snapshot.request_id != request_id || snapshot.block_id != region_id
            {
                return Err(FramingError::MalformedPayload);
            }
            regions.push(BlockViewportRegionContent {
                region_id,
                first_row,
                snapshot,
            });
            offset = end;
        }
        if offset != bytes.len() {
            return Err(FramingError::ExactLengthMismatch);
        }
        Ok(Self {
            request_id,
            plan_generation,
            revision,
            status,
            regions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pass7::{HistoryCell, HistoryRow};

    fn attachment() -> AttachmentId {
        AttachmentId::from_bytes(11u128.to_le_bytes())
    }

    #[test]
    fn viewport_content_request_round_trips_bounded_region_clips() {
        let request = BlockViewportContentRequest {
            attachment_id: attachment(),
            request_id: 9,
            plan_generation: 4,
            clips: vec![
                BlockViewportContentClip {
                    region_id: 12,
                    first_row: 0,
                    row_count: 3,
                },
                BlockViewportContentClip {
                    region_id: 14,
                    first_row: 8,
                    row_count: 5,
                },
            ],
        };
        assert_eq!(
            BlockViewportContentRequest::decode(&request.encode()),
            Ok(request)
        );
    }

    #[test]
    fn viewport_content_request_rejects_zero_ids_and_unbounded_rows() {
        let mut request = BlockViewportContentRequest {
            attachment_id: attachment(),
            request_id: 1,
            plan_generation: 1,
            clips: vec![BlockViewportContentClip {
                region_id: 1,
                first_row: 0,
                row_count: MAX_BLOCK_VIEWPORT_CLIP_ROWS + 1,
            }],
        };
        assert!(request.try_encode().is_err());
        request.clips[0].row_count = 1;
        request.clips[0].region_id = 0;
        assert!(request.try_encode().is_err());
    }

    #[test]
    fn viewport_content_snapshot_round_trips_one_coherent_response() {
        let cell = HistoryCell::from_text("x", 0, 0, 0, &mut Vec::new()).unwrap();
        let snapshot = BlockViewportContentSnapshot {
            request_id: 7,
            plan_generation: 3,
            revision: 18,
            status: HistoryRangeStatus::Complete,
            regions: vec![BlockViewportRegionContent {
                region_id: 5,
                first_row: 2,
                snapshot: HistoryRangeSnapshot {
                    request_id: 7,
                    block_id: 5,
                    revision: 18,
                    status: HistoryRangeStatus::Complete,
                    rows: vec![HistoryRow {
                        line_id: 2,
                        cells: vec![cell],
                    }],
                    sidecar: Vec::new(),
                },
            }],
        };
        assert_eq!(
            BlockViewportContentSnapshot::decode(&snapshot.encode()),
            Ok(snapshot)
        );
    }
}
