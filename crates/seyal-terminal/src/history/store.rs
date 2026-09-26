//! HistoryStore storage, seal, append, truncate, and resident accounting.

use super::eviction::{EvictedBitmap, EvictedIdRange};
use super::reflow::{reflow_rows_allocated_bytes, ReflowCache};
use super::types::{
    line_entirely_before, range_entirely_before, HistoryAnchor, HistoryBreakAfter, HistoryLine,
    HistoryLineRef, Segment, SegmentLine, SegmentUnit, HISTORY_SEGMENT_PAYLOAD_TARGET,
    HISTORY_TAIL_PAYLOAD_LIMIT, NEXT_SEGMENT_AGE,
};
use super::wrap::{
    wrap_chain_pattern, APERIODIC_EXTEND_CELL_FACTOR, APERIODIC_INLINE_RUN_BOUND, WrapChain,
};
use crate::{grapheme_store::GraphemeStore, Cell, LineId};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::mem::size_of;
use std::sync::atomic::Ordering;

#[derive(Clone, Debug, Default)]
pub(crate) struct HistoryStore {
    pub(super) segments: VecDeque<Segment>,
    pub(super) tail: Vec<HistoryLine>,
    pub(super) tail_payload_bytes: usize,
    pub(super) tail_resident_bytes: usize,
    pub(super) segments_resident_bytes: usize,
    pub(super) resident_bytes: usize,
    pub(super) eviction_generation: u64,
    // Exact disjoint runs of source identities whose canonical payload was
    // evicted. Alternate-screen allocations create gaps and must not be
    // absorbed into an unavailable range. The Vec allocation is included in
    // resident_bytes, so this metadata participates in the same hard cap.
    pub(super) evicted_id_ranges: Vec<EvictedIdRange>,
    /// Inclusive dense prefix of actually evicted identities. Never spans a
    /// never-allocated gap (alternate-screen LineIds).
    pub(super) evicted_from: Option<LineId>,
    pub(super) evicted_through: Option<LineId>,
    /// Compacted non-adjacent eviction identities. Gaps in a window are unset.
    pub(super) evicted_bitmaps: Vec<EvictedBitmap>,
    /// Exact leftover runs that do not fit a bitmap window.
    pub(super) evicted_overflow_ranges: Vec<EvictedIdRange>,
    /// Set when overflow is at cap and another compacted range cannot be
    /// stored exactly. Lookup then fail-closes that span as Unavailable.
    pub(super) evicted_index_saturated: bool,
    pub(super) evicted_saturated_from: Option<LineId>,
    pub(super) evicted_saturated_through: Option<LineId>,
    pub(super) reflow_cache: RefCell<Option<ReflowCache>>,
    /// SoftWrap-chain width runs used to answer resize carry columns.
    /// Derived (§9.1), not resident source; closed hard-broken rows are omitted.
    pub(super) wrap_chains: VecDeque<WrapChain>,
}


impl HistoryStore {
    pub(crate) fn entries(&self) -> impl Iterator<Item = HistoryLineRef<'_>> {
        self.segments
            .iter()
            .flat_map(|segment| {
                segment
                    .lines
                    .iter()
                    .map(move |line| HistoryLineRef::Sealed(segment, line))
            })
            .chain(self.tail.iter().map(HistoryLineRef::Tail))
    }

    pub(crate) fn resident_bytes(&self) -> usize {
        self.resident_bytes
    }

    pub(crate) fn reverse_entries(&self) -> impl Iterator<Item = HistoryLineRef<'_>> {
        self.tail
            .iter()
            .rev()
            .map(HistoryLineRef::Tail)
            .chain(self.segments.iter().rev().flat_map(|segment| {
                segment
                    .lines
                    .iter()
                    .rev()
                    .map(move |line| HistoryLineRef::Sealed(segment, line))
            }))
    }

    pub(crate) fn derived_cache_bytes(&self) -> usize {
        self.wrap_index_allocated_bytes().saturating_add(
            self.reflow_cache
                .borrow()
                .as_ref()
                .map_or(0, |cache| reflow_rows_allocated_bytes(&cache.rows)),
        )
    }

    pub(crate) fn drop_derived_cache(&mut self) {
        self.reflow_cache.get_mut().take();
        self.wrap_chains.clear();
        self.wrap_chains.shrink_to_fit();
    }

    pub(crate) fn eviction_generation(&self) -> u64 {
        self.eviction_generation
    }

    pub(crate) fn replace_payload(&mut self, lines: Vec<HistoryLine>) {
        self.reflow_cache.get_mut().take();
        self.wrap_chains.clear();
        self.segments.clear();
        self.tail.clear();
        self.tail_payload_bytes = 0;
        self.tail_resident_bytes = 0;
        self.segments_resident_bytes = 0;
        self.update_resident_bytes();
        for line in lines {
            self.append_line(line);
        }
    }

    /// Active surface plus two screenfuls of slack (SPEC-010 §7).
    pub(crate) fn eager_resize_row_budget(rows: u16) -> usize {
        usize::from(rows).saturating_mul(3).max(1)
    }

    /// Clone only the retained-history suffix that can affect the new
    /// viewport. Older sealed source stays in place and is not copied.
    ///
    /// When the cut lands inside an aperiodic SoftWrap chain, prefer extending
    /// the suffix back to the preceding HardBreak (within an extend budget) so
    /// `start_col` is 0 and resize does not walk retained wrap runs. If the
    /// chain is too large to extend, fall back to cols-dependent spine occupancy.
    pub(crate) fn eager_resize_suffix(
        &self,
        cols: u16,
        visual_rows: usize,
    ) -> (Vec<HistoryLine>, Option<HistoryAnchor>, usize) {
        let budget_cells = usize::from(cols.max(1)).saturating_mul(visual_rows.max(1));
        let extend_budget = budget_cells.saturating_mul(APERIODIC_EXTEND_CELL_FACTOR);
        let mut collected: Vec<HistoryLine> = Vec::new();
        let mut cells = 0usize;
        let mut omitted_joins = false;
        let mut entries = self.reverse_entries();
        while let Some(entry) = entries.next() {
            if cells >= budget_cells {
                if entry.break_after() != HistoryBreakAfter::SoftWrap {
                    break;
                }
                let from_probe = collected.last().map(|line: &HistoryLine| HistoryAnchor {
                    line_id: line.line_id,
                    unit_offset: line.start_offset,
                });
                let aperiodic_long = from_probe
                    .and_then(|from| self.wrap_chain_containing(from))
                    .is_some_and(|chain| {
                        wrap_chain_pattern(chain).is_none()
                            && chain.runs.len() > APERIODIC_INLINE_RUN_BOUND
                    });
                if !aperiodic_long {
                    omitted_joins = true;
                    break;
                }
                // Extend through older SoftWrap members until HardBreak or budget.
                let mut cur = entry;
                let mut reached_hard_boundary = false;
                loop {
                    let add = cur
                        .to_owned_line()
                        .units
                        .iter()
                        .map(|unit| usize::from(unit.width.max(1)))
                        .sum::<usize>();
                    if cells.saturating_add(add.max(1)) > extend_budget && !collected.is_empty() {
                        omitted_joins = true;
                        break;
                    }
                    collected.push(cur.to_owned_line());
                    cells = cells.saturating_add(add.max(1));
                    match entries.next() {
                        None => {
                            reached_hard_boundary = true;
                            break;
                        }
                        Some(next) if next.break_after() == HistoryBreakAfter::SoftWrap => {
                            cur = next;
                        }
                        Some(_) => {
                            // Older HardBreak ends the previous paragraph; soft
                            // chain starts at the lines already collected.
                            reached_hard_boundary = true;
                            break;
                        }
                    }
                }
                if reached_hard_boundary {
                    omitted_joins = false;
                }
                break;
            }
            collected.push(entry.to_owned_line());
            cells = cells.saturating_add(
                collected
                    .last()
                    .map(|line| {
                        line.units
                            .iter()
                            .map(|unit| usize::from(unit.width.max(1)))
                            .sum::<usize>()
                            .max(1)
                    })
                    .unwrap_or(1),
            );
        }
        collected.reverse();
        let from = collected.first().map(|line| HistoryAnchor {
            line_id: line.line_id,
            unit_offset: line.start_offset,
        });
        let start_col = if omitted_joins {
            self.wrap_column_before(from, cols)
        } else {
            0
        };
        (collected, from, start_col)
    }


    pub(crate) fn truncate_from(&mut self, from: HistoryAnchor) {
        self.reflow_cache.get_mut().take();
        while let Some(last) = self.tail.last() {
            if line_entirely_before(last, from) {
                break;
            }
            if last.line_id == from.line_id && last.start_offset < from.unit_offset {
                let keep = from
                    .unit_offset
                    .saturating_sub(last.start_offset)
                    .try_into()
                    .unwrap_or(usize::MAX)
                    .min(last.units.len());
                if keep == 0 {
                    self.tail.pop();
                } else if let Some(last) = self.tail.last_mut() {
                    last.units.truncate(keep);
                    last.break_after = HistoryBreakAfter::SoftWrap;
                }
                self.recount_tail();
                self.update_resident_bytes();
                self.trim_wrap_suffix_from(from);
                return;
            }
            self.tail.pop();
        }
        self.recount_tail();
        while let Some(segment) = self.segments.back() {
            let Some(last_line) = segment.lines.last() else {
                self.segments.pop_back();
                continue;
            };
            if range_entirely_before(
                last_line.line_id,
                last_line.start_offset,
                last_line.unit_len,
                from,
            ) {
                break;
            }
            let segment = self.segments.pop_back().expect("back existed");
            self.segments_resident_bytes = self
                .segments_resident_bytes
                .saturating_sub(segment.resident_bytes);
            let mut keep = Vec::new();
            let mut reached_split = false;
            for line in segment.lines.iter() {
                let owned = HistoryLineRef::Sealed(&segment, line).to_owned_line();
                if line_entirely_before(&owned, from) {
                    keep.push(owned);
                    continue;
                }
                if owned.line_id == from.line_id && owned.start_offset < from.unit_offset {
                    let count = from
                        .unit_offset
                        .saturating_sub(owned.start_offset)
                        .try_into()
                        .unwrap_or(usize::MAX)
                        .min(owned.units.len());
                    if count > 0 {
                        keep.push(HistoryLine {
                            line_id: owned.line_id,
                            units: owned.units[..count].to_vec(),
                            break_after: HistoryBreakAfter::SoftWrap,
                            start_offset: owned.start_offset,
                        });
                    }
                }
                reached_split = true;
                break;
            }
            for line in keep {
                self.push_fragment_inner(line, false);
            }
            if reached_split {
                break;
            }
        }
        self.recount_tail();
        self.update_resident_bytes();
        self.trim_wrap_suffix_from(from);
    }

    pub(super) fn recount_tail(&mut self) {
        self.tail_payload_bytes = self
            .tail
            .iter()
            .map(HistoryLine::canonical_payload_len)
            .sum();
        self.tail_resident_bytes = self
            .tail
            .iter()
            .map(HistoryLine::allocated_bytes)
            .sum::<usize>()
            .saturating_add(
                self.tail
                    .capacity()
                    .saturating_mul(size_of::<HistoryLine>()),
            );
    }

    pub(crate) fn append_row(
        &mut self,
        line_id: LineId,
        break_after: HistoryBreakAfter,
        cells: &[Cell],
        store: &GraphemeStore,
    ) {
        self.append_line(HistoryLine::from_cells(line_id, break_after, cells, store));
    }

    pub(crate) fn append_line(&mut self, line: HistoryLine) {
        self.reflow_cache.get_mut().take();
        let bytes = line.canonical_payload_len();
        // A source line can be arbitrarily long. Fragmenting at unit boundaries
        // keeps the tail bounded while preserving its LineId and break lineage.
        if bytes > HISTORY_SEGMENT_PAYLOAD_TARGET {
            let mut fragment = Vec::new();
            let mut fragment_bytes = 0usize;
            let mut source_offset = line.start_offset;
            let line_id = line.line_id;
            let final_break = line.break_after;
            for unit in line.units {
                let unit_bytes = unit.canonical_payload_len();
                if !fragment.is_empty()
                    && fragment_bytes + unit_bytes > HISTORY_SEGMENT_PAYLOAD_TARGET
                {
                    let fragment_start = source_offset - fragment.len() as u32;
                    self.push_fragment(HistoryLine {
                        line_id,
                        units: std::mem::take(&mut fragment),
                        break_after: HistoryBreakAfter::SoftWrap,
                        start_offset: fragment_start,
                    });
                    fragment_bytes = 0;
                }
                fragment_bytes += unit_bytes;
                fragment.push(unit);
                source_offset = source_offset.saturating_add(1);
            }
            if !fragment.is_empty() {
                let fragment_start = source_offset - fragment.len() as u32;
                self.push_fragment(HistoryLine {
                    line_id,
                    units: fragment,
                    break_after: final_break,
                    start_offset: fragment_start,
                });
            }
        } else {
            self.push_fragment(line);
        }
        self.update_resident_bytes();
        self.evict_to_cap();
    }

    pub(super) fn push_fragment(&mut self, line: HistoryLine) {
        self.push_fragment_inner(line, true);
    }

    pub(super) fn push_fragment_inner(&mut self, line: HistoryLine, record_wrap: bool) {
        let bytes = line.canonical_payload_len();
        if !self.tail.is_empty()
            && (self.tail_payload_bytes + bytes > HISTORY_SEGMENT_PAYLOAD_TARGET
                || self.tail_resident_bytes >= HISTORY_TAIL_PAYLOAD_LIMIT)
        {
            self.seal_tail();
        }
        self.tail_payload_bytes += bytes;
        let line_resident_bytes = line.allocated_bytes();
        let old_capacity = self.tail.capacity();
        if record_wrap {
            self.extend_wrap_line(&line);
        }
        self.tail.push(line);
        self.tail_resident_bytes = self
            .tail_resident_bytes
            .saturating_add(line_resident_bytes)
            .saturating_add(
                self.tail
                    .capacity()
                    .saturating_sub(old_capacity)
                    .saturating_mul(size_of::<HistoryLine>()),
            );
        self.update_resident_bytes();
        if self.tail_payload_bytes >= HISTORY_SEGMENT_PAYLOAD_TARGET
            || self.tail_resident_bytes >= HISTORY_TAIL_PAYLOAD_LIMIT
        {
            self.seal_tail();
        }
    }

    pub(super) fn seal_tail(&mut self) {
        if self.tail.is_empty() {
            return;
        }
        let tail = std::mem::take(&mut self.tail);
        let mut lines = Vec::with_capacity(tail.len());
        let unit_count = tail.iter().map(|line| line.units.len()).sum();
        let payload_len = tail
            .iter()
            .flat_map(|line| &line.units)
            .map(|unit| unit.utf8.len())
            .sum();
        let mut units = Vec::with_capacity(unit_count);
        let mut payload = Vec::with_capacity(payload_len);
        for line in tail {
            let unit_start = u32::try_from(units.len()).unwrap_or(u32::MAX);
            for unit in line.units {
                let payload_start = u32::try_from(payload.len()).unwrap_or(u32::MAX);
                let payload_len = u32::try_from(unit.utf8.len()).unwrap_or(u32::MAX);
                payload.extend_from_slice(&unit.utf8);
                units.push(SegmentUnit {
                    payload_start,
                    payload_len,
                    width: unit.width,
                    style: unit.style,
                });
            }
            lines.push(SegmentLine {
                line_id: line.line_id,
                unit_start,
                unit_len: u32::try_from(units.len())
                    .unwrap_or(u32::MAX)
                    .saturating_sub(unit_start),
                start_offset: line.start_offset,
                break_after: line.break_after,
            });
        }
        let lines = lines.into_boxed_slice();
        let units = units.into_boxed_slice();
        let payload = payload.into_boxed_slice();
        // Segment values live in the VecDeque allocation accounted below.
        // This is the exact heap storage owned through the three boxed slices.
        let resident_bytes = lines
            .len()
            .saturating_mul(size_of::<SegmentLine>())
            .saturating_add(units.len().saturating_mul(size_of::<SegmentUnit>()))
            .saturating_add(payload.len());
        let segment = Segment {
            lines,
            units,
            payload,
            age: NEXT_SEGMENT_AGE.fetch_add(1, Ordering::Relaxed),
            resident_bytes,
        };
        self.tail_payload_bytes = 0;
        self.tail_resident_bytes = 0;
        let old_capacity = self.segments.capacity();
        self.segments.push_back(segment);
        self.segments_resident_bytes = self
            .segments_resident_bytes
            .saturating_add(
                self.segments
                    .back()
                    .map_or(0, |segment| segment.resident_bytes),
            )
            .saturating_add(
                self.segments
                    .capacity()
                    .saturating_sub(old_capacity)
                    .saturating_mul(size_of::<Segment>()),
            );
        self.update_resident_bytes();
    }

    pub(super) fn update_resident_bytes(&mut self) {
        self.resident_bytes = size_of::<Self>()
            .saturating_add(self.tail_resident_bytes)
            .saturating_add(self.segments_resident_bytes)
            .saturating_add(
                self.evicted_id_ranges
                    .capacity()
                    .saturating_mul(size_of::<EvictedIdRange>()),
            )
            .saturating_add(
                self.evicted_overflow_ranges
                    .capacity()
                    .saturating_mul(size_of::<EvictedIdRange>()),
            )
            .saturating_add(
                self.evicted_bitmaps
                    .iter()
                    .map(EvictedBitmap::allocated_bytes)
                    .sum::<usize>(),
            );
    }


}
