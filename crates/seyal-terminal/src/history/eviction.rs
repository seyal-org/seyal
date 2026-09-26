//! Evicted identity ranges and sparse eviction bitmaps.

use super::store::HistoryStore;
use super::types::{
    HISTORY_PER_EXECUTION_BYTE_CAP, Segment,
};
use crate::LineId;
use std::mem::size_of;

pub const MAX_EVICTED_ID_RANGES: usize = 1_024;
pub(super) const MAX_EVICTED_BITMAP_BITS: u64 = 65_536;
pub(super) const MAX_EVICTED_BITMAPS: usize = 8;
pub(super) const MAX_EVICTED_OVERFLOW_RANGES: usize = 1_024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct EvictedIdRange {
    pub(super) first: LineId,
    pub(super) last: LineId,
}

/// Exact sparse eviction bits for compacted ranges. Unset bits in the window
/// stay never-allocated (`Invalid`); set bits stay `Unavailable`.
#[derive(Clone, Debug, Default)]
pub(super) struct EvictedBitmap {
    pub(super) origin: u64,
    pub(super) len: u64,
    pub(super) words: Vec<u64>,
}

impl EvictedBitmap {
    pub(super) fn allocated_bytes(&self) -> usize {
        size_of::<Self>().saturating_add(self.words.capacity().saturating_mul(size_of::<u64>()))
    }

    pub(super) fn contains(&self, id: u64) -> bool {
        let Some(bit) = id.checked_sub(self.origin) else {
            return false;
        };
        if bit >= self.len {
            return false;
        }
        let word = (bit / 64) as usize;
        let shift = (bit % 64) as u32;
        self.words
            .get(word)
            .is_some_and(|value| (*value >> shift) & 1 == 1)
    }

    pub(super) fn intersects(&self, start: u64, end: u64) -> bool {
        if self.len == 0 {
            return false;
        }
        let window_last = self.origin.saturating_add(self.len.saturating_sub(1));
        if end < self.origin || start > window_last {
            return false;
        }
        let from_bit = start.max(self.origin) - self.origin;
        let to_bit = end.min(window_last) - self.origin;
        let mut bit = from_bit;
        while bit <= to_bit {
            let word_idx = (bit / 64) as usize;
            let offset = bit % 64;
            let Some(word) = self.words.get(word_idx).copied() else {
                break;
            };
            let bits_left = 64 - offset;
            let span = (to_bit - bit + 1).min(bits_left);
            let mask = if span == 64 {
                u64::MAX
            } else {
                (1u64 << span) - 1
            };
            if (word >> offset) & mask != 0 {
                return true;
            }
            bit = bit.saturating_add(span);
            if span == 0 {
                break;
            }
        }
        false
    }

    pub(super) fn try_insert_range(&mut self, first: u64, last: u64) -> bool {
        if last < first {
            return false;
        }
        if self.len == 0 {
            let span = last.saturating_sub(first).saturating_add(1);
            if span == 0 || span > MAX_EVICTED_BITMAP_BITS {
                return false;
            }
            self.origin = first;
            self.resize_to(span);
            self.set_range(first, last);
            return true;
        }
        if first < self.origin || last < self.origin {
            return false;
        }
        let current_last = self.origin.saturating_add(self.len.saturating_sub(1));
        let new_last = current_last.max(last);
        let span = new_last.saturating_sub(self.origin).saturating_add(1);
        if span > MAX_EVICTED_BITMAP_BITS {
            return false;
        }
        self.resize_to(span);
        self.set_range(first, last);
        true
    }

    pub(super) fn resize_to(&mut self, span: u64) {
        self.len = span;
        let words = usize::try_from(span.div_ceil(64)).unwrap_or(usize::MAX);
        if self.words.len() < words {
            self.words.resize(words, 0);
        }
    }

    pub(super) fn set_range(&mut self, first: u64, last: u64) {
        let mut id = first;
        loop {
            let bit = id - self.origin;
            let word = (bit / 64) as usize;
            let shift = (bit % 64) as u32;
            if let Some(slot) = self.words.get_mut(word) {
                *slot |= 1u64 << shift;
            }
            if id == last {
                break;
            }
            let Some(next) = id.checked_add(1) else {
                break;
            };
            id = next;
        }
    }
}


impl HistoryStore {
    pub(super) fn evict_to_cap(&mut self) {
        self.update_resident_bytes();
        let mut evicted = false;
        while self.resident_bytes > HISTORY_PER_EXECUTION_BYTE_CAP {
            if self.segments.is_empty() {
                if self.tail.is_empty() {
                    break;
                }
                self.seal_tail();
                if self.segments.is_empty() {
                    break;
                }
            }
            let Some(segment) = self.segments.pop_front() else {
                break;
            };
            self.record_evicted_segment(&segment);
            self.segments_resident_bytes = self
                .segments_resident_bytes
                .saturating_sub(segment.resident_bytes);
            self.update_resident_bytes();
            self.eviction_generation = self.eviction_generation.wrapping_add(1);
            evicted = true;
        }
        if evicted {
            self.trim_wrap_before_retained();
        }
    }

    pub(crate) fn oldest_segment_age(&self) -> Option<u64> {
        self.segments.front().map(|segment| segment.age)
    }

    pub(crate) fn evict_oldest_segment(&mut self) -> usize {
        self.reflow_cache.get_mut().take();
        let before = self.resident_bytes;
        let Some(segment) = self.segments.pop_front() else {
            return 0;
        };
        self.record_evicted_segment(&segment);
        self.segments_resident_bytes = self
            .segments_resident_bytes
            .saturating_sub(segment.resident_bytes);
        self.update_resident_bytes();
        self.eviction_generation = self.eviction_generation.wrapping_add(1);
        self.trim_wrap_before_retained();
        before.saturating_sub(self.resident_bytes)
    }

    pub(super) fn record_evicted_segment(&mut self, segment: &Segment) {
        for line_id in segment.lines.iter().map(|line| line.line_id) {
            let Some(last) = self.evicted_id_ranges.last_mut() else {
                self.evicted_id_ranges.push(EvictedIdRange {
                    first: line_id,
                    last: line_id,
                });
                continue;
            };
            if line_id <= last.last {
                continue;
            }
            if last.last.0.checked_add(1) == Some(line_id.0) {
                last.last = line_id;
            } else {
                self.evicted_id_ranges.push(EvictedIdRange {
                    first: line_id,
                    last: line_id,
                });
            }
        }
        self.compact_evicted_id_ranges();
    }

    #[cfg(test)]
    pub(super) fn record_evicted_range_for_test(&mut self, first: u64, last: u64) {
        self.evicted_id_ranges.push(EvictedIdRange {
            first: LineId(first),
            last: LineId(last),
        });
        self.compact_evicted_id_ranges();
    }

    pub(super) fn compact_evicted_id_ranges(&mut self) {
        while self.evicted_id_ranges.len() > MAX_EVICTED_ID_RANGES {
            let oldest = self.evicted_id_ranges.remove(0);
            if self.admit_compacted_evicted_range(oldest) {
                continue;
            }
            if self.evicted_overflow_ranges.len() < MAX_EVICTED_OVERFLOW_RANGES {
                self.evicted_overflow_ranges.push(oldest);
            } else {
                self.fail_closed_evicted_range(oldest);
            }
        }
    }

    pub(super) fn fail_closed_evicted_range(&mut self, range: EvictedIdRange) {
        self.evicted_index_saturated = true;
        self.evicted_saturated_from = Some(match self.evicted_saturated_from {
            Some(from) => from.min(range.first),
            None => range.first,
        });
        self.evicted_saturated_through = Some(match self.evicted_saturated_through {
            Some(through) => through.max(range.last),
            None => range.last,
        });
    }

    pub(super) fn saturated_contains(&self, line_id: LineId) -> bool {
        match (self.evicted_saturated_from, self.evicted_saturated_through) {
            (Some(from), Some(through)) if self.evicted_index_saturated => {
                line_id >= from && line_id <= through
            }
            _ => false,
        }
    }

    pub(super) fn saturated_intersects(&self, start: LineId, end: LineId) -> bool {
        match (self.evicted_saturated_from, self.evicted_saturated_through) {
            (Some(from), Some(through)) if self.evicted_index_saturated => {
                start <= through && end >= from
            }
            _ => false,
        }
    }

    pub(super) fn admit_compacted_evicted_range(&mut self, range: EvictedIdRange) -> bool {
        match (self.evicted_from, self.evicted_through) {
            (None, None) => {
                self.evicted_from = Some(range.first);
                self.evicted_through = Some(range.last);
                return true;
            }
            (Some(_from), Some(through)) if through.0.checked_add(1) == Some(range.first.0) => {
                self.evicted_through = Some(range.last);
                return true;
            }
            _ => {}
        }
        for bitmap in &mut self.evicted_bitmaps {
            if bitmap.try_insert_range(range.first.0, range.last.0) {
                return true;
            }
        }
        if self.evicted_bitmaps.len() >= MAX_EVICTED_BITMAPS {
            return false;
        }
        let mut bitmap = EvictedBitmap::default();
        if bitmap.try_insert_range(range.first.0, range.last.0) {
            self.evicted_bitmaps.push(bitmap);
            return true;
        }
        let mut start = range.first.0;
        while start <= range.last.0 && self.evicted_bitmaps.len() < MAX_EVICTED_BITMAPS {
            let chunk_last = start
                .saturating_add(MAX_EVICTED_BITMAP_BITS.saturating_sub(1))
                .min(range.last.0);
            let mut chunk = EvictedBitmap::default();
            if !chunk.try_insert_range(start, chunk_last) {
                break;
            }
            self.evicted_bitmaps.push(chunk);
            if chunk_last == range.last.0 {
                return true;
            }
            let Some(next) = chunk_last.checked_add(1) else {
                return true;
            };
            start = next;
        }
        start > range.last.0
    }

    pub(super) fn dense_evicted_prefix_contains(&self, line_id: LineId) -> bool {
        match (self.evicted_from, self.evicted_through) {
            (Some(from), Some(through)) => line_id >= from && line_id <= through,
            _ => false,
        }
    }

    pub(super) fn dense_evicted_prefix_intersects(&self, start: LineId, end: LineId) -> bool {
        match (self.evicted_from, self.evicted_through) {
            (Some(from), Some(through)) => start <= through && end >= from,
            _ => false,
        }
    }

    pub(super) fn overflow_contains(line_id: LineId, ranges: &[EvictedIdRange]) -> bool {
        let index = ranges.partition_point(|range| range.last < line_id);
        ranges
            .get(index)
            .is_some_and(|range| line_id >= range.first)
    }

    pub(super) fn overflow_intersects(start: LineId, end: LineId, ranges: &[EvictedIdRange]) -> bool {
        let index = ranges.partition_point(|range| range.last < start);
        ranges.get(index).is_some_and(|range| range.first <= end)
    }

    pub(crate) fn range_intersects_evicted(&self, start: LineId, end: LineId) -> bool {
        if self.saturated_intersects(start, end) {
            return true;
        }
        if self.dense_evicted_prefix_intersects(start, end) {
            return true;
        }
        if self
            .evicted_bitmaps
            .iter()
            .any(|bitmap| bitmap.intersects(start.0, end.0))
        {
            return true;
        }
        if Self::overflow_intersects(start, end, &self.evicted_overflow_ranges) {
            return true;
        }
        Self::overflow_intersects(start, end, &self.evicted_id_ranges)
    }

    pub(super) fn line_was_evicted(&self, line_id: LineId) -> bool {
        if self.saturated_contains(line_id) {
            return true;
        }
        if self.dense_evicted_prefix_contains(line_id) {
            return true;
        }
        if self
            .evicted_bitmaps
            .iter()
            .any(|bitmap| bitmap.contains(line_id.0))
        {
            return true;
        }
        if Self::overflow_contains(line_id, &self.evicted_overflow_ranges) {
            return true;
        }
        Self::overflow_contains(line_id, &self.evicted_id_ranges)
    }

}
