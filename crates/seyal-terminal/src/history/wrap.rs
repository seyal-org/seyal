//! SoftWrap occupancy spines, patterns, and wrap-index maintenance.

use super::store::HistoryStore;
use super::types::{
    HistoryAnchor, HistoryBreakAfter, HistoryLine, HistoryLineRef, HistoryUnit,
    HISTORY_PER_EXECUTION_DERIVED_INDEX_CAP,
};
use crate::LineId;
use std::cell::RefCell;
use std::mem::size_of;

#[derive(Clone, Copy, Debug)]
pub(super) struct WrapFragment {
    pub(super) line_id: LineId,
    pub(super) start_offset: u32,
    pub(super) unit_count: u32,
    pub(super) prefix_units: u32,
}

/// Cols-dependent occupancy after each RLE run (from column 0). Lets aperiodic
/// `wrap_column_before` answer mid-chain cuts in O(log runs + cols) after one
/// O(runs·cols) build for that width (SPEC-010 §7).
#[derive(Clone, Debug)]
pub(super) struct WrapOccupancySpine {
    pub(super) cols: u16,
    pub(super) run_len: usize,
    pub(super) after_run: Vec<u8>,
    pub(super) units_after: Vec<u32>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct WrapChain {
    pub(super) fragments: Vec<WrapFragment>,
    pub(super) runs: Vec<(u8, u32)>,
    /// Repeating RLE prefix length used for occupancy. `2..=WRAP_PATTERN_MAX`
    /// is a compact template; occupancy then ignores the stored run tail.
    pub(super) pattern_len: u32,
    /// Units already dropped from the front of a compacted repeating template.
    pub(super) pattern_phase: u32,
    pub(super) open: bool,
    /// Built lazily for aperiodic chains; invalidated when `runs` change.
    pub(super) spine: RefCell<Option<WrapOccupancySpine>>,
}

impl WrapChain {
    pub(super) fn invalidate_spine(&self) {
        *self.spine.borrow_mut() = None;
    }

    pub(super) fn units_before(&self, from: HistoryAnchor) -> u32 {
        let index = match self.fragments.binary_search_by(|fragment| {
            (fragment.line_id, fragment.start_offset).cmp(&(from.line_id, from.unit_offset))
        }) {
            Ok(index) => return self.fragments[index].prefix_units,
            Err(index) => index,
        };
        if index == 0 {
            return 0;
        }
        let previous = &self.fragments[index - 1];
        if previous.line_id == from.line_id && from.unit_offset > previous.start_offset {
            previous.prefix_units.saturating_add(
                from.unit_offset
                    .saturating_sub(previous.start_offset)
                    .min(previous.unit_count),
            )
        } else if previous.line_id < from.line_id {
            previous.prefix_units.saturating_add(previous.unit_count)
        } else {
            0
        }
    }
}

pub(super) const WRAP_PATTERN_MAX: usize = 8;
/// Aperiodic SoftWrap chains above this run count prefer HardBreak extension
/// (or spine occupancy) instead of a linear mid-chain run walk on resize.
pub(super) const APERIODIC_INLINE_RUN_BOUND: usize = 64;
/// Max cells cloned when extending an aperiodic SoftWrap cut back to HardBreak.
pub(super) const APERIODIC_EXTEND_CELL_FACTOR: usize = 8;

pub(super) fn wrap_pattern_period_units(chain: &WrapChain) -> u32 {
    let Some(pattern) = wrap_chain_pattern(chain) else {
        return 0;
    };
    wrap_runs_unit_sum(pattern)
}

pub(super) fn wrap_chain_pattern(chain: &WrapChain) -> Option<&[(u8, u32)]> {
    let k = chain.pattern_len as usize;
    if chain.pattern_len >= 2 && chain.runs.len() >= k {
        Some(&chain.runs[..k])
    } else {
        None
    }
}

pub(super) fn wrap_chain_is_compacted(
    chain: &WrapChain,
    run_units: u32,
    virtual_units: u32,
) -> bool {
    let k = chain.pattern_len as usize;
    chain.pattern_len >= 2 && k > 0 && chain.runs.len() <= k && run_units < virtual_units
}

pub(super) fn wrap_runs_unit_sum(runs: &[(u8, u32)]) -> u32 {
    runs.iter()
        .map(|(_, count)| *count)
        .fold(0u32, u32::saturating_add)
}

pub(super) fn compact_repeating_wrap_runs(chain: &mut WrapChain) {
    let k = chain.pattern_len as usize;
    if chain.pattern_len >= 2 && chain.runs.len() > k {
        chain.invalidate_spine();
        chain.runs.truncate(k);
        chain.runs.shrink_to_fit();
    }
}

pub(super) fn wrap_pattern_width_at(pattern: &[(u8, u32)], mut index: u32) -> Option<u8> {
    let period = wrap_runs_unit_sum(pattern);
    if period == 0 {
        return None;
    }
    index %= period;
    for &(width, count) in pattern {
        if index < count {
            return Some(width);
        }
        index = index.saturating_sub(count);
    }
    None
}

pub(super) fn wrap_pattern_slice(pattern: &[(u8, u32)], start: u32, count: u32) -> Vec<(u8, u32)> {
    let period = wrap_runs_unit_sum(pattern);
    if period == 0 || count == 0 {
        return Vec::new();
    }
    let mut offset = start % period;
    let mut remaining = count;
    let mut runs: Vec<(u8, u32)> = Vec::new();
    while remaining > 0 {
        let before = remaining;
        let mut idx = 0u32;
        for &(width, run_count) in pattern {
            if remaining == 0 {
                break;
            }
            let run_end = idx.saturating_add(run_count);
            if offset >= run_end {
                idx = run_end;
                continue;
            }
            let skip = offset.saturating_sub(idx);
            let take = run_count.saturating_sub(skip).min(remaining);
            if take > 0 {
                if let Some((last_width, last_count)) = runs.last_mut()
                    && *last_width == width
                {
                    *last_count = last_count.saturating_add(take);
                } else {
                    runs.push((width, take));
                }
                remaining = remaining.saturating_sub(take);
                offset = 0;
            }
            idx = run_end;
        }
        if remaining == before {
            break;
        }
    }
    runs
}

pub(super) fn append_compacted_wrap_runs(
    chain: &mut WrapChain,
    units: &[HistoryUnit],
    prefix_units: u32,
) {
    for (index, unit) in units.iter().enumerate() {
        let width = unit.width.max(1);
        let unit_index = chain
            .pattern_phase
            .saturating_add(prefix_units)
            .saturating_add(index as u32);
        match wrap_pattern_width_at(&chain.runs, unit_index) {
            Some(expected) if expected == width => {}
            _ => {
                chain.invalidate_spine();
                chain.runs = wrap_pattern_slice(
                    &chain.runs,
                    chain.pattern_phase,
                    prefix_units.saturating_add(index as u32),
                );
                chain.pattern_phase = 0;
                chain.pattern_len = 0;
                for unit in &units[index..] {
                    let width = unit.width.max(1);
                    if let Some((last_width, count)) = chain.runs.last_mut()
                        && *last_width == width
                    {
                        *count = count.saturating_add(1);
                    } else {
                        chain.runs.push((width, 1));
                    }
                }
                return;
            }
        }
    }
}

pub(super) fn wrap_occupancy_repeating_from(
    pattern: &[(u8, u32)],
    phase: u32,
    cols: usize,
    units: u32,
) -> usize {
    let period = wrap_runs_unit_sum(pattern);
    if period == 0 {
        return 0;
    }
    let phase = phase % period;
    if phase == 0 {
        return wrap_occupancy_repeating(pattern, cols, units);
    }
    let rotated = wrap_pattern_slice(pattern, phase, period);
    wrap_occupancy_repeating(&rotated, cols, units)
}

pub(super) fn wrap_occupancy_indexed(chain: &WrapChain, cols: usize, units: u32) -> usize {
    if let Some(pattern) = wrap_chain_pattern(chain) {
        wrap_occupancy_repeating_from(pattern, chain.pattern_phase, cols, units)
    } else {
        wrap_occupancy_aperiodic(chain, cols, units)
    }
}

pub(super) fn wrap_occupancy_aperiodic(chain: &WrapChain, cols: usize, units: u32) -> usize {
    if cols == 0 || units == 0 || chain.runs.is_empty() {
        return 0;
    }
    if chain.runs.len() > APERIODIC_INLINE_RUN_BOUND {
        let cols_u16 = u16::try_from(cols).unwrap_or(u16::MAX);
        ensure_aperiodic_spine(chain, cols_u16);
        if let Some(spine) = chain.spine.borrow().as_ref() {
            return spine_occupancy(spine, &chain.runs, cols, units);
        }
    }
    wrap_occupancy_for_runs(&chain.runs, cols, units)
}

pub(super) fn ensure_aperiodic_spine(chain: &WrapChain, cols: u16) {
    {
        let spine = chain.spine.borrow();
        if let Some(existing) = spine.as_ref()
            && existing.cols == cols
            && existing.run_len == chain.runs.len()
        {
            return;
        }
    }
    let width = usize::from(cols);
    let mut after_run = Vec::with_capacity(chain.runs.len());
    let mut units_after = Vec::with_capacity(chain.runs.len());
    let mut used = 0usize;
    let mut total_units = 0u32;
    let mut seen = vec![None; width.saturating_add(1)];
    for &(unit_width, count) in &chain.runs {
        used = wrap_occupancy_run(used, width, unit_width, count, &mut seen);
        total_units = total_units.saturating_add(count);
        after_run.push(u8::try_from(used.min(width)).unwrap_or(u8::MAX));
        units_after.push(total_units);
    }
    *chain.spine.borrow_mut() = Some(WrapOccupancySpine {
        cols,
        run_len: chain.runs.len(),
        after_run,
        units_after,
    });
}

pub(super) fn spine_occupancy(
    spine: &WrapOccupancySpine,
    runs: &[(u8, u32)],
    cols: usize,
    unit_limit: u32,
) -> usize {
    if unit_limit == 0 || cols == 0 {
        return 0;
    }
    let fully = spine
        .units_after
        .partition_point(|&units| units <= unit_limit);
    let (mut used, consumed) = if fully == 0 {
        (0usize, 0u32)
    } else {
        (
            usize::from(spine.after_run[fully - 1]),
            spine.units_after[fully - 1],
        )
    };
    if consumed >= unit_limit {
        return used.min(cols);
    }
    if fully < runs.len() {
        let (width, count) = runs[fully];
        let take = count.min(unit_limit.saturating_sub(consumed));
        let mut seen = vec![None; cols.saturating_add(1)];
        used = wrap_occupancy_run(used, cols, width, take, &mut seen);
    }
    used.min(cols)
}

/// Incremental period-k detection for k in 2..=WRAP_PATTERN_MAX.
/// Must not walk retained runs (SPEC-010 §7).
pub(super) fn note_appended_wrap_run(chain: &mut WrapChain, pushed_new: bool) {
    chain.invalidate_spine();
    let n = chain.runs.len();
    if n <= 1 {
        chain.pattern_len = n as u32;
        return;
    }
    if chain.pattern_len >= 2 {
        let k = chain.pattern_len as usize;
        if n <= k {
            return;
        }
        if wrap_run_extends_pattern(chain, k, pushed_new) {
            return;
        }
        if n <= WRAP_PATTERN_MAX {
            chain.pattern_len = n as u32;
            return;
        }
        chain.pattern_len = 0;
        return;
    }
    if n <= WRAP_PATTERN_MAX {
        chain.pattern_len = n as u32;
    }
}

pub(super) fn wrap_run_extends_pattern(chain: &WrapChain, k: usize, pushed_new: bool) -> bool {
    let n = chain.runs.len();
    if k == 0 || n <= k {
        return true;
    }
    if pushed_new {
        let prev = n - 2;
        if chain.runs[prev] != chain.runs[prev % k] {
            return false;
        }
    }
    let last = n - 1;
    let template = chain.runs[last % k];
    let run = chain.runs[last];
    run.0 == template.0 && run.1 <= template.1
}

pub(super) fn note_pattern_after_suffix_trim(chain: &mut WrapChain) {
    let n = chain.runs.len();
    if n <= 1 {
        chain.pattern_len = n as u32;
        return;
    }
    if chain.pattern_len >= 2 {
        let k = chain.pattern_len as usize;
        if n <= k {
            chain.pattern_len = n as u32;
            return;
        }
        let last = n - 1;
        let template = chain.runs[last % k];
        let run = chain.runs[last];
        if run.0 != template.0 || run.1 > template.1 {
            chain.pattern_len = if n <= WRAP_PATTERN_MAX { n as u32 } else { 0 };
        }
        return;
    }
    if n <= WRAP_PATTERN_MAX {
        chain.pattern_len = n as u32;
    }
}

pub(super) fn note_pattern_after_prefix_trim(
    chain: &mut WrapChain,
    drop_units: u32,
    period_units: u32,
) {
    let n = chain.runs.len();
    if n <= 1 {
        chain.pattern_len = n as u32;
        return;
    }
    if n <= WRAP_PATTERN_MAX && n <= chain.pattern_len as usize {
        chain.pattern_len = n as u32;
        return;
    }
    if chain.pattern_len >= 2 && period_units > 0 && drop_units.is_multiple_of(period_units) {
        return;
    }
    chain.pattern_len = 0;
}

pub(super) fn drop_suffix_wrap_runs(runs: &mut Vec<(u8, u32)>, drop_units: u32) {
    let mut remaining = drop_units;
    while remaining > 0 {
        let Some(last) = runs.last_mut() else {
            break;
        };
        if last.1 <= remaining {
            remaining = remaining.saturating_sub(last.1);
            runs.pop();
        } else {
            last.1 = last.1.saturating_sub(remaining);
            break;
        }
    }
}

pub(super) fn drop_prefix_wrap_runs(runs: &mut Vec<(u8, u32)>, drop_units: u32) {
    let mut remaining = drop_units;
    let mut drain = 0usize;
    while remaining > 0 && drain < runs.len() {
        let count = runs[drain].1;
        if count <= remaining {
            remaining = remaining.saturating_sub(count);
            drain += 1;
        } else {
            runs[drain].1 = count.saturating_sub(remaining);
            remaining = 0;
        }
    }
    if drain > 0 {
        runs.drain(..drain);
    }
}

pub(super) fn wrap_advance(used: usize, cols: usize, unit_width: u8) -> usize {
    let unit_width = usize::from(unit_width.max(1));
    let mut used = used;
    if used > 0 && used + unit_width > cols {
        used = 0;
    }
    if unit_width > cols {
        return 0;
    }
    used = used.saturating_add(unit_width);
    if used >= cols {
        0
    } else {
        used
    }
}

pub(super) fn wrap_occupancy_run(
    mut used: usize,
    cols: usize,
    unit_width: u8,
    count: u32,
    seen: &mut [Option<usize>],
) -> usize {
    if cols == 0 {
        return 0;
    }
    let mut remaining = count as usize;
    if remaining == 0 {
        return used.min(cols);
    }
    seen.fill(None);
    while remaining > 0 {
        if used <= cols
            && let Some(remaining_then) = seen[used]
        {
            let cycle = remaining_then.saturating_sub(remaining);
            if cycle > 0 {
                remaining %= cycle;
                if remaining == 0 {
                    return used.min(cols);
                }
                seen.fill(None);
                continue;
            }
        } else if used <= cols {
            seen[used] = Some(remaining);
        }
        used = wrap_advance(used, cols, unit_width);
        remaining -= 1;
    }
    used.min(cols)
}

pub(super) fn line_wholly_after(entry: HistoryLineRef<'_>, from: HistoryAnchor) -> bool {
    entry.line_id() > from.line_id
        || (entry.line_id() == from.line_id && entry.start_offset() >= from.unit_offset)
}

pub(super) fn line_wholly_before(entry: HistoryLineRef<'_>, from: HistoryAnchor) -> bool {
    let end = entry.start_offset().saturating_add(entry.unit_len());
    entry.line_id() < from.line_id || (entry.line_id() == from.line_id && end <= from.unit_offset)
}

/// Occupancy of the SoftWrap chain containing `from` when the derived wrap
/// index is absent. Walks only that chain, not all retained history.
pub(super) fn wrap_occupancy_from_canonical(
    store: &HistoryStore,
    from: HistoryAnchor,
    cols: usize,
) -> usize {
    let mut fragments_rev: Vec<Vec<(u8, u32)>> = Vec::new();
    let mut saw_soft = false;
    for entry in store.reverse_entries() {
        if line_wholly_after(entry, from) {
            continue;
        }
        if !fragments_rev.is_empty()
            && entry.break_after() == HistoryBreakAfter::HardBreak
            && line_wholly_before(entry, from)
        {
            break;
        }
        if entry.break_after() == HistoryBreakAfter::SoftWrap {
            saw_soft = true;
        }
        let mut fragment_runs: Vec<(u8, u32)> = Vec::new();
        let mut offset = entry.start_offset();
        for unit in entry.units() {
            if entry.line_id() == from.line_id && offset >= from.unit_offset {
                break;
            }
            let width = unit.width().max(1);
            if let Some((last_width, count)) = fragment_runs.last_mut()
                && *last_width == width
            {
                *count = count.saturating_add(1);
            } else {
                fragment_runs.push((width, 1));
            }
            offset = offset.saturating_add(1);
        }
        fragments_rev.push(fragment_runs);
    }
    if !saw_soft {
        return 0;
    }
    let mut runs: Vec<(u8, u32)> = Vec::new();
    for fragment in fragments_rev.into_iter().rev() {
        for (width, count) in fragment {
            if let Some((last_width, last_count)) = runs.last_mut()
                && *last_width == width
            {
                *last_count = last_count.saturating_add(count);
            } else {
                runs.push((width, count));
            }
        }
    }
    let units = wrap_runs_unit_sum(&runs);
    // Same closed-form / ephemeral-spine path as the indexed miss-free case so
    // dropping the derived wrap index cannot reintroduce a linear run walk.
    wrap_occupancy_for_runs(&runs, cols, units)
}

pub(super) fn wrap_runs_match_period(runs: &[(u8, u32)], k: usize) -> bool {
    if k < 2 || runs.len() < k {
        return false;
    }
    for (index, run) in runs.iter().enumerate().skip(k) {
        let template = runs[index % k];
        if run.0 != template.0 {
            return false;
        }
        if index + 1 == runs.len() {
            if run.1 > template.1 {
                return false;
            }
        } else if run.1 != template.1 {
            return false;
        }
    }
    true
}

pub(super) fn wrap_repeating_period(runs: &[(u8, u32)]) -> Option<usize> {
    let n = runs.len();
    if n < 2 {
        return None;
    }
    let max_k = n.min(WRAP_PATTERN_MAX);
    (2..=max_k).find(|&k| n > k && wrap_runs_match_period(runs, k))
}

pub(super) fn wrap_occupancy_for_runs(runs: &[(u8, u32)], cols: usize, unit_limit: u32) -> usize {
    if let Some(k) = wrap_repeating_period(runs) {
        wrap_occupancy_repeating(&runs[..k], cols, unit_limit)
    } else if runs.len() > APERIODIC_INLINE_RUN_BOUND {
        wrap_occupancy_with_ephemeral_spine(runs, cols, unit_limit)
    } else {
        wrap_occupancy_runs(runs, cols, unit_limit)
    }
}

pub(super) fn wrap_occupancy_with_ephemeral_spine(
    runs: &[(u8, u32)],
    cols: usize,
    unit_limit: u32,
) -> usize {
    if cols == 0 || unit_limit == 0 || runs.is_empty() {
        return 0;
    }
    let cols_u16 = u16::try_from(cols).unwrap_or(u16::MAX);
    let mut after_run = Vec::with_capacity(runs.len());
    let mut units_after = Vec::with_capacity(runs.len());
    let mut used = 0usize;
    let mut total_units = 0u32;
    let mut seen = vec![None; cols.saturating_add(1)];
    for &(unit_width, count) in runs {
        used = wrap_occupancy_run(used, cols, unit_width, count, &mut seen);
        total_units = total_units.saturating_add(count);
        after_run.push(u8::try_from(used.min(cols)).unwrap_or(u8::MAX));
        units_after.push(total_units);
    }
    let spine = WrapOccupancySpine {
        cols: cols_u16,
        run_len: runs.len(),
        after_run,
        units_after,
    };
    spine_occupancy(&spine, runs, cols, unit_limit)
}

pub(super) fn wrap_occupancy_runs(runs: &[(u8, u32)], cols: usize, unit_limit: u32) -> usize {
    let mut used = 0usize;
    let mut left = unit_limit;
    let mut seen = vec![None; cols.saturating_add(1)];
    for &(width, count) in runs {
        if left == 0 {
            break;
        }
        let take = count.min(left);
        used = wrap_occupancy_run(used, cols, width, take, &mut seen);
        left = left.saturating_sub(take);
    }
    used.min(cols)
}

pub(super) fn wrap_occupancy_repeating(
    pattern: &[(u8, u32)],
    cols: usize,
    unit_limit: u32,
) -> usize {
    let pattern_units = pattern
        .iter()
        .map(|(_, count)| *count)
        .fold(0u32, u32::saturating_add);
    if cols == 0 || pattern_units == 0 || unit_limit == 0 {
        return 0;
    }
    let mut used = 0usize;
    let mut remaining = unit_limit;
    let mut seen: Vec<Option<u32>> = vec![None; cols.saturating_add(1)];
    let mut scratch: Vec<Option<usize>> = vec![None; cols.saturating_add(1)];
    while remaining > 0 {
        if used <= cols
            && let Some(remaining_then) = seen[used]
        {
            let cycle = remaining_then.saturating_sub(remaining);
            if cycle > 0 {
                remaining %= cycle;
                seen.fill(None);
                continue;
            }
        } else if used <= cols {
            seen[used] = Some(remaining);
        }
        let take = remaining.min(pattern_units);
        used = wrap_occupancy_pattern_once(used, pattern, cols, take, &mut scratch);
        remaining = remaining.saturating_sub(take);
    }
    used.min(cols)
}

pub(super) fn wrap_occupancy_pattern_once(
    mut used: usize,
    pattern: &[(u8, u32)],
    cols: usize,
    unit_limit: u32,
    seen: &mut [Option<usize>],
) -> usize {
    let mut left = unit_limit;
    for &(width, count) in pattern {
        if left == 0 {
            break;
        }
        let take = count.min(left);
        used = wrap_occupancy_run(used, cols, width, take, seen);
        left = left.saturating_sub(take);
    }
    used.min(cols)
}

#[cfg(test)]
pub(super) fn wrap_occupancy(unit_widths: &[u8], cols: usize) -> usize {
    if cols == 0 {
        return 0;
    }
    wrap_occupancy_runs(
        &unit_widths
            .iter()
            .map(|&width| (width.max(1), 1u32))
            .collect::<Vec<_>>(),
        cols,
        u32::try_from(unit_widths.len()).unwrap_or(u32::MAX),
    )
}

impl HistoryStore {
    /// Display column at `from` for a new width. Repeating mixed-width
    /// patterns use a closed-form occupancy path; suffix trims do not scan
    /// retained fragments. If the derived wrap index was dropped, occupancy is
    /// rebuilt from the current SoftWrap chain only (SPEC-010 §9.1).
    pub(crate) fn wrap_column_before(&self, from: Option<HistoryAnchor>, cols: u16) -> usize {
        let Some(from) = from else {
            return 0;
        };
        let width = usize::from(cols);
        if width == 0 {
            return 0;
        }
        if let Some(chain) = self.wrap_chain_containing(from) {
            return wrap_occupancy_indexed(chain, width, chain.units_before(from));
        }
        wrap_occupancy_from_canonical(self, from, width)
    }

    pub(super) fn wrap_chain_containing(&self, from: HistoryAnchor) -> Option<&WrapChain> {
        let index = self.wrap_chains.partition_point(|chain| {
            chain.fragments.first().is_some_and(|first| {
                first.line_id < from.line_id
                    || (first.line_id == from.line_id && first.start_offset < from.unit_offset)
            })
        });
        index
            .checked_sub(1)
            .and_then(|index| self.wrap_chains.get(index))
    }

    pub(super) fn extend_wrap_line(&mut self, line: &HistoryLine) {
        let continue_chain = self.wrap_chains.back().is_some_and(|chain| chain.open);
        if !continue_chain && line.break_after == HistoryBreakAfter::HardBreak {
            return;
        }
        if line.units.is_empty() {
            if continue_chain
                && line.break_after == HistoryBreakAfter::HardBreak
                && let Some(chain) = self.wrap_chains.back_mut()
            {
                chain.open = false;
            }
            return;
        }
        if !continue_chain {
            self.wrap_chains.push_back(WrapChain::default());
        }
        let chain = self
            .wrap_chains
            .back_mut()
            .expect("wrap chain exists after open-or-push");
        let unit_count = u32::try_from(line.units.len()).unwrap_or(u32::MAX);
        let prefix_units = chain.fragments.last().map_or(0, |fragment| {
            fragment.prefix_units.saturating_add(fragment.unit_count)
        });
        chain.fragments.push(WrapFragment {
            line_id: line.line_id,
            start_offset: line.start_offset,
            unit_count,
            prefix_units,
        });
        let run_units = wrap_runs_unit_sum(&chain.runs);
        let compacted = wrap_chain_is_compacted(chain, run_units, prefix_units);
        if compacted {
            append_compacted_wrap_runs(chain, &line.units, prefix_units);
        } else {
            for unit in &line.units {
                let width = unit.width.max(1);
                let pushed_new = if let Some((last_width, count)) = chain.runs.last_mut()
                    && *last_width == width
                {
                    *count = count.saturating_add(1);
                    false
                } else {
                    chain.runs.push((width, 1));
                    true
                };
                note_appended_wrap_run(chain, pushed_new);
            }
            compact_repeating_wrap_runs(chain);
        }
        chain.open = line.break_after == HistoryBreakAfter::SoftWrap;
        self.enforce_wrap_index_cap();
    }

    pub(super) fn trim_wrap_suffix_from(&mut self, from: HistoryAnchor) {
        while let Some(chain) = self.wrap_chains.back() {
            match chain.fragments.first() {
                None => {
                    self.wrap_chains.pop_back();
                }
                Some(first)
                    if first.line_id > from.line_id
                        || (first.line_id == from.line_id
                            && first.start_offset >= from.unit_offset) =>
                {
                    self.wrap_chains.pop_back();
                }
                _ => break,
            }
        }
        let Some(chain) = self.wrap_chains.back_mut() else {
            return;
        };
        let keep_units = chain.units_before(from);
        let total = chain.fragments.last().map_or(0, |fragment| {
            fragment.prefix_units.saturating_add(fragment.unit_count)
        });
        if keep_units >= total {
            return;
        }
        let split = chain.fragments.partition_point(|fragment| {
            fragment.line_id < from.line_id
                || (fragment.line_id == from.line_id && fragment.start_offset < from.unit_offset)
        });
        chain.fragments.truncate(split);
        if let Some(last) = chain.fragments.last_mut()
            && last.line_id == from.line_id
            && last.start_offset < from.unit_offset
        {
            last.unit_count = from
                .unit_offset
                .saturating_sub(last.start_offset)
                .min(last.unit_count);
        }
        let run_units = wrap_runs_unit_sum(&chain.runs);
        let compacted = wrap_chain_is_compacted(chain, run_units, total);
        if compacted {
            if keep_units == 0 {
                chain.runs.clear();
                chain.pattern_len = 0;
                chain.pattern_phase = 0;
                chain.invalidate_spine();
            }
        } else {
            drop_suffix_wrap_runs(&mut chain.runs, total.saturating_sub(keep_units));
            note_pattern_after_suffix_trim(chain);
            chain.invalidate_spine();
        }
        chain.open = true;
    }

    pub(super) fn trim_wrap_before_retained(&mut self) {
        let Some(first) = self.entries().next() else {
            self.wrap_chains.clear();
            return;
        };
        let keep = HistoryAnchor {
            line_id: first.line_id(),
            unit_offset: first.start_offset(),
        };
        while let Some(chain) = self.wrap_chains.front() {
            match chain.fragments.last() {
                None => {
                    self.wrap_chains.pop_front();
                }
                Some(last)
                    if last.line_id < keep.line_id
                        || (last.line_id == keep.line_id
                            && last.start_offset.saturating_add(last.unit_count)
                                <= keep.unit_offset) =>
                {
                    self.wrap_chains.pop_front();
                }
                _ => break,
            }
        }
        let Some(chain) = self.wrap_chains.front_mut() else {
            return;
        };
        let drop_units = chain.units_before(keep);
        let split = chain.fragments.partition_point(|fragment| {
            fragment.line_id < keep.line_id
                || (fragment.line_id == keep.line_id && fragment.start_offset < keep.unit_offset)
        });
        if split > 0 {
            chain.fragments.drain(0..split);
        }
        let mut prefix = 0u32;
        for fragment in &mut chain.fragments {
            fragment.prefix_units = prefix;
            prefix = prefix.saturating_add(fragment.unit_count);
        }
        let run_units = wrap_runs_unit_sum(&chain.runs);
        let compacted =
            wrap_chain_is_compacted(chain, run_units, drop_units.saturating_add(prefix));
        if compacted {
            if prefix == 0 {
                chain.runs.clear();
                chain.pattern_len = 0;
                chain.pattern_phase = 0;
            } else {
                chain.pattern_phase = chain.pattern_phase.saturating_add(drop_units);
            }
            chain.invalidate_spine();
        } else {
            let period_units = wrap_pattern_period_units(chain);
            drop_prefix_wrap_runs(&mut chain.runs, drop_units);
            note_pattern_after_prefix_trim(chain, drop_units, period_units);
            chain.invalidate_spine();
        }
    }

    pub(super) fn enforce_wrap_index_cap(&mut self) {
        for chain in &mut self.wrap_chains {
            compact_repeating_wrap_runs(chain);
        }
        while self.wrap_index_allocated_bytes() > HISTORY_PER_EXECUTION_DERIVED_INDEX_CAP
            && self.wrap_chains.len() > 1
        {
            let drop_closed = self.wrap_chains.front().is_some_and(|chain| !chain.open);
            if !drop_closed {
                break;
            }
            self.wrap_chains.pop_front();
        }
        if self.wrap_index_allocated_bytes() > HISTORY_PER_EXECUTION_DERIVED_INDEX_CAP {
            self.wrap_chains.clear();
            self.wrap_chains.shrink_to_fit();
        }
    }

    pub(super) fn wrap_index_allocated_bytes(&self) -> usize {
        const RUN: usize = size_of::<(u8, u32)>();
        self.wrap_chains
            .capacity()
            .saturating_mul(size_of::<WrapChain>())
            .saturating_add(
                self.wrap_chains
                    .iter()
                    .map(|chain| {
                        let spine_bytes = chain.spine.borrow().as_ref().map_or(0, |spine| {
                            spine
                                .after_run
                                .capacity()
                                .saturating_add(spine.units_after.capacity().saturating_mul(4))
                        });
                        chain
                            .fragments
                            .capacity()
                            .saturating_mul(size_of::<WrapFragment>())
                            .saturating_add(chain.runs.capacity().saturating_mul(RUN))
                            .saturating_add(spine_bytes)
                    })
                    .sum::<usize>(),
            )
    }
}
