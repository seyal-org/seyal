use crate::{
    active_grapheme::{
        append_payload, build_lead_cell, edge_decision_late_widen, edge_decision_new_unit,
        try_append_scalar, ActiveGrapheme, EdgeDecision,
    },
    damage::{DamageTracker, Mutation},
    grapheme_store::GraphemeStore,
    line::LineIdAllocator,
    parser::{Actions, Parser},
    presentation::{parse_osc_presentation, HostPresentationEvent, MAX_HOST_PRESENTATION_EVENTS},
    protocol_reply::{
        encode_decrqm_private, encode_dsr_cpr, encode_kitty_flags, encode_primary_da,
        ProtocolReply, MAX_PROTOCOL_REPLIES,
    },
    screen::{PreparedScreen, Screen},
    selection::{
        encode_paste, format_history_copy, order_visual, skip_continuation, CopyMode,
        CopyModeMotion, PasteError, SearchSession, SelectionKind, SelectionSession, VisualPos,
        MAX_SEARCH_MATCHES,
    },
    width::{grapheme_terminal_width, AmbiguousWidthPolicy},
    Cell, CellRole, CursorState, Damage, HistoryAnchor, HistoryAnchorResolution, HistoryBreakAfter,
    HistoryMatch, HistoryRangeError, HistoryUnitView, HistoryWireCell, LineId, ModeState,
    MouseReporting, ReflowRow, TerminalError,
};
use std::collections::VecDeque;

/// Soft ceiling aligned with Candidate-D display maxima. Larger geometries are
/// rejected cheaply so embedders cannot force multi-gigabyte grid allocations.
pub const MAX_TERMINAL_COLUMNS: u16 = 512;
pub const MAX_TERMINAL_ROWS: u16 = 256;
const KEYBOARD_STACK_CAPACITY: usize = 16;
const KEYBOARD_FLAGS_MASK: u8 = 0b11;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Diagnostics {
    pub deferred_sequences: u64,
    pub unknown_sequences: u64,
    pub malformed_sequences: u64,
    pub grapheme_payload_overflow_count: u64,
    pub grapheme_store_capacity_fallback_count: u64,
}

/// Bounded shell-integration metadata emitted by the canonical VT parser.
/// Terminal cells and arbitrary OSC payloads are never exposed through this
/// interface. Correlation of tokens to workspace/Block lifecycle is owned by
/// Runtime/application integration, not by this crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellIntegrationEvent {
    /// OSC `133;A;<nonce>`: the shell is about to draw a prompt.
    PromptStarted { token: ShellIntegrationToken },
    /// `line` is the cursor's logical line at the moment this marker was
    /// recognized, before any later bytes in the same feed are applied. A
    /// caller sampling "the current cursor" instead, after draining a whole
    /// batch of queued events, would observe the position after every later
    /// marker/output in that same batch, not the position at this marker.
    CommandStarted {
        token: ShellIntegrationToken,
        line: LineId,
    },
    /// `line` is the last line of the command's own output: when output
    /// ends with a trailing newline, the cursor's row at recognition time is
    /// still empty (about to be overwritten by the next prompt), so `line`
    /// is the row before it; otherwise the cursor's row is used as-is.
    CommandFinished {
        token: ShellIntegrationToken,
        exit_status: i32,
        line: LineId,
    },
}

/// Runtime-issued nonce carried by the shell integration marker. A marker is
/// only meaningful when an external integrator correlates it with pending
/// work; arbitrary OSC 133 traffic remains bounded and is otherwise ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShellIntegrationToken([u8; 16]);

impl ShellIntegrationToken {
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(self) -> [u8; 16] {
        self.0
    }

    pub(crate) fn from_hex(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 32 {
            return None;
        }
        let mut token = [0u8; 16];
        let (pairs, remainder) = bytes.as_chunks::<2>();
        if !remainder.is_empty() {
            return None;
        }
        for (index, pair) in pairs.iter().enumerate() {
            token[index] = (hex(pair[0])? << 4) | hex(pair[1])?;
        }
        Some(Self(token))
    }

    pub fn write_hex(self, out: &mut String) {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for byte in self.0 {
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0xf) as usize] as char);
        }
    }
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub struct TerminalState {
    parser: Parser,
    core: TerminalCore,
}

impl TerminalState {
    pub fn new(cols: u16, rows: u16) -> Result<Self, TerminalError> {
        Ok(Self {
            parser: Parser::new(),
            core: TerminalCore::new(cols, rows)?,
        })
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Result<(), TerminalError> {
        if let Some(error) = self.core.fault {
            return Err(error);
        }
        self.parser.feed(bytes, &mut self.core);
        self.core.damage.commit();
        self.core.fault.map_or(Ok(()), Err)
    }

    pub fn finish_input(&mut self) -> Result<(), TerminalError> {
        if let Some(error) = self.core.fault {
            return Err(error);
        }
        self.parser.finish(&mut self.core);
        self.core.damage.commit();
        self.core.fault.map_or(Ok(()), Err)
    }

    /// Convenience prepare+commit for VT-only consumers. Prefer
    /// [`prepare_resize`] / [`commit_resize`] when coordinating with a PTY.
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), TerminalError> {
        let prepared = self.prepare_resize(cols, rows)?;
        self.commit_resize(prepared);
        Ok(())
    }

    /// Fallible canonical resize preparation. Does not mutate live geometry or
    /// damage. Must be completed with [`commit_resize`] or dropped.
    pub fn prepare_resize(
        &mut self,
        cols: u16,
        rows: u16,
    ) -> Result<PreparedResize, TerminalError> {
        self.core.prepare_resize(cols, rows)
    }

    /// Infallible commit of a prepared resize. Damage/projection become
    /// observable only after this returns.
    pub fn commit_resize(&mut self, prepared: PreparedResize) {
        self.core.commit_resize(prepared);
    }

    pub fn cols(&self) -> u16 {
        self.core.current().cols()
    }

    pub fn rows(&self) -> u16 {
        self.core.current().rows()
    }

    pub fn cursor(&self) -> CursorState {
        self.core.current().cursor(self.core.modes.cursor_visible)
    }

    pub fn modes(&self) -> ModeState {
        self.core.modes
    }

    pub fn diagnostics(&self) -> Diagnostics {
        let mut diagnostics = self.core.diagnostics;
        diagnostics.grapheme_payload_overflow_count =
            self.core.grapheme_store.grapheme_payload_overflow_count;
        diagnostics.grapheme_store_capacity_fallback_count = self
            .core
            .grapheme_store
            .grapheme_store_capacity_fallback_count;
        diagnostics
    }

    pub fn ambiguous_width_policy(&self) -> AmbiguousWidthPolicy {
        self.core.ambiguous_width
    }

    pub fn set_ambiguous_width_policy(&mut self, policy: AmbiguousWidthPolicy) {
        if self.core.ambiguous_width != policy {
            self.core.ambiguous_width = policy;
            self.core.invalidate_active_grapheme();
        }
    }

    pub fn grapheme_store_live_bytes(&self) -> usize {
        self.core.grapheme_store.live_bytes()
    }

    pub fn pending_wrap(&self) -> bool {
        self.core.current().pending_wrap()
    }

    pub fn cell(&self, col: u16, row: u16) -> Option<Cell> {
        self.core.current().cell(col, row)
    }

    /// UTF-8 payload for a physical cell used by Candidate-D grapheme projection.
    ///
    /// Empty/Continuation return an empty slice. Lead returns store UTF-8 when
    /// present, otherwise the inline scalar encoding (overflow yields U+FFFD).
    pub fn lead_utf8(&self, col: u16, row: u16) -> Option<std::borrow::Cow<'_, [u8]>> {
        let cell = self.cell(col, row)?;
        match cell.role {
            crate::CellRole::Empty | crate::CellRole::Continuation => {
                Some(std::borrow::Cow::Borrowed(b""))
            }
            crate::CellRole::Lead => {
                if cell.overflow {
                    return Some(std::borrow::Cow::Borrowed("\u{FFFD}".as_bytes()));
                }
                if let Some(bytes) = self.core.grapheme_store.get(cell.store_id) {
                    Some(std::borrow::Cow::Borrowed(bytes))
                } else {
                    let mut buf = [0u8; 4];
                    let encoded = cell.character.encode_utf8(&mut buf);
                    Some(std::borrow::Cow::Owned(encoded.as_bytes().to_vec()))
                }
            }
        }
    }

    pub fn line_id(&self, row: u16) -> Option<LineId> {
        self.core.current().line_id(row)
    }

    #[cfg(test)]
    pub fn primary_source_break_count(&self) -> usize {
        self.core.primary.source_break_len()
    }

    #[cfg(test)]
    fn primary_row_break_after(&self, row: u16) -> Option<crate::HistoryBreakAfter> {
        self.core.primary.row_break_after(row)
    }

    /// Returns a bounded primary-screen history range. The returned rows are
    /// an explicit read-only projection of **primary** retained history plus
    /// primary visible rows. Alternate-screen cells are never included; the
    /// portable API still returns primary history while alternate screen is
    /// active so embedders (not VT) own Blocks/TUI presentation policy.
    ///
    /// Work is bounded by retained history plus visible rows, never by the
    /// numeric distance between `start` and `end` (LineIds may be sparse).
    pub fn primary_history_range(
        &self,
        start: LineId,
        end: LineId,
        max_lines: usize,
    ) -> Result<Vec<(LineId, Vec<Cell>)>, HistoryRangeError> {
        if max_lines == 0 || end < start {
            return Ok(Vec::new());
        }
        if self
            .core
            .primary
            .history()
            .range_intersects_evicted(start, end)
        {
            return Err(HistoryRangeError::Stale);
        }
        let mut lines: Vec<(LineId, Vec<Cell>)> = Vec::new();
        for entry in self.core.primary.history_entries() {
            let id = entry.line_id();
            if id < start {
                continue;
            }
            if id > end {
                break;
            }
            let cells = entry.presentation_cells();
            if let Some((last_id, last_cells)) = lines.last_mut()
                && *last_id == id
            {
                last_cells.extend(cells);
            } else {
                lines.push((id, cells));
                if lines.len() >= max_lines {
                    return Ok(lines);
                }
            }
        }
        for row in 0..self.core.primary.rows() {
            let Some(id) = self.core.primary.line_id(row) else {
                continue;
            };
            if id < start || id > end {
                continue;
            }
            if lines.iter().any(|(existing, _)| *existing == id) {
                continue;
            }
            let Some(cells) = self.core.primary.cell_row(row) else {
                continue;
            };
            lines.push((id, cells.to_vec()));
            if lines.len() >= max_lines {
                break;
            }
        }
        Ok(lines)
    }

    /// History rows for the Pass-7 snapshot wire, including full grapheme
    /// UTF-8. Continuation placeholders carry empty text.
    pub fn primary_history_wire_range(
        &self,
        start: LineId,
        end: LineId,
        max_lines: usize,
        skip_leads: u32,
    ) -> Result<Vec<(LineId, Vec<HistoryWireCell>)>, HistoryRangeError> {
        if max_lines == 0 || end < start {
            return Ok(Vec::new());
        }
        if self
            .core
            .primary
            .history()
            .range_intersects_evicted(start, end)
        {
            return Err(HistoryRangeError::Stale);
        }
        let mut skip = skip_leads;
        let mut lines: Vec<(LineId, Vec<HistoryWireCell>)> = Vec::new();
        for entry in self.core.primary.history_entries() {
            let id = entry.line_id();
            if id < start {
                continue;
            }
            if id > end {
                break;
            }
            let cells = skip_wire_leads(entry.wire_cells(), &mut skip);
            if cells.is_empty() {
                continue;
            }
            if let Some((last_id, last_cells)) = lines.last_mut()
                && *last_id == id
            {
                last_cells.extend(cells);
            } else {
                lines.push((id, cells));
                if skip == 0 && lines.len() >= max_lines {
                    return Ok(lines);
                }
            }
        }
        for row in 0..self.core.primary.rows() {
            let Some(id) = self.core.primary.line_id(row) else {
                continue;
            };
            if id < start || id > end {
                continue;
            }
            if lines.iter().any(|(existing, _)| *existing == id) {
                continue;
            }
            let Some(cells) = self.core.primary.cell_row(row) else {
                continue;
            };
            let content_end = cells
                .iter()
                .rposition(|cell| cell.role != CellRole::Empty)
                .map_or(0, |index| index + 1);
            let mut wire = Vec::new();
            for cell in &cells[..content_end] {
                match cell.role {
                    CellRole::Continuation => {
                        wire.push(HistoryWireCell {
                            text: String::new(),
                            width: 0,
                            style: cell.style,
                            continuation: true,
                        });
                    }
                    CellRole::Empty => {
                        wire.push(HistoryWireCell {
                            text: " ".into(),
                            width: 1,
                            style: cell.style,
                            continuation: false,
                        });
                    }
                    CellRole::Lead => {
                        let text = if cell.overflow {
                            "\u{FFFD}".to_owned()
                        } else {
                            self.core.grapheme_store.get(cell.store_id).map_or_else(
                                || cell.character.to_string(),
                                |bytes| String::from_utf8_lossy(bytes).into_owned(),
                            )
                        };
                        wire.push(HistoryWireCell {
                            text,
                            width: cell.width.max(1),
                            style: cell.style,
                            continuation: false,
                        });
                    }
                }
            }
            lines.push((id, skip_wire_leads(wire, &mut skip)));
            if let Some((_, cells)) = lines.last()
                && cells.is_empty()
            {
                lines.pop();
            }
            if skip == 0 && lines.len() >= max_lines {
                break;
            }
        }
        Ok(lines)
    }

    /// Returns complete canonical source units for a bounded primary-history
    /// range. This projection preserves multi-scalar grapheme payloads and
    /// source anchors; callers that only need legacy scalar cells can use
    /// [`Self::primary_history_range`].
    pub fn primary_history_units_range(
        &self,
        start: LineId,
        end: LineId,
        max_units: usize,
    ) -> Vec<HistoryUnitView> {
        let mut units = self
            .core
            .primary
            .history()
            .source_units(start, end, max_units)
            .into_iter()
            .collect::<Vec<_>>();
        if units.len() >= max_units {
            return units;
        }
        let mut visible_offsets = std::collections::HashMap::<LineId, u32>::new();
        for row in 0..self.core.primary.rows() {
            let Some(line_id) = self.core.primary.line_id(row) else {
                continue;
            };
            if line_id < start || line_id > end {
                continue;
            }
            let Some(cells) = self.core.primary.cell_row(row) else {
                continue;
            };
            let content_end = cells
                .iter()
                .rposition(|cell| cell.role != CellRole::Empty)
                .map_or(0, |index| index + 1);
            let unit_offset = visible_offsets.entry(line_id).or_default();
            for (col, cell) in cells[..content_end].iter().enumerate() {
                let CellRole::Lead = cell.role else {
                    continue;
                };
                let text = if cell.overflow {
                    "\u{FFFD}".to_owned()
                } else {
                    self.core.grapheme_store.get(cell.store_id).map_or_else(
                        || cell.character.to_string(),
                        |bytes| String::from_utf8_lossy(bytes).into_owned(),
                    )
                };
                let anchor =
                    self.core
                        .primary
                        .cell_anchor(col as u16, row)
                        .unwrap_or(HistoryAnchor {
                            line_id,
                            unit_offset: *unit_offset,
                        });
                if !units.iter().any(|unit| unit.anchor == anchor) {
                    units.push(HistoryUnitView {
                        anchor,
                        text,
                        width: cell.width.max(1),
                        style: cell.style,
                        // Live viewport rows are not sealed HistoryStore
                        // records; treat them as ephemeral wrap fragments.
                        break_after: HistoryBreakAfter::SoftWrap,
                    });
                    if units.len() >= max_units {
                        return units;
                    }
                }
                *unit_offset = (*unit_offset).saturating_add(1);
            }
        }
        units
    }

    /// Derives width-specific rows from canonical retained history. The
    /// projection is bounded by `max_rows` and does not rewrite source text.
    /// Each lead cell carries the complete grapheme UTF-8 payload.
    pub fn primary_history_reflow(&self, cols: u16, max_rows: usize) -> Vec<ReflowRow> {
        self.core.primary.history().reflow(cols, max_rows)
    }

    pub fn primary_history_reflow_uncached(&self, cols: u16, max_rows: usize) -> Vec<ReflowRow> {
        self.core.primary.history().reflow_uncached(cols, max_rows)
    }

    pub fn primary_history_resident_bytes(&self) -> usize {
        self.core.primary.history().resident_bytes()
    }

    pub fn primary_history_derived_cache_bytes(&self) -> usize {
        self.core.primary.history().derived_cache_bytes()
    }

    pub fn drop_primary_history_derived_cache(&mut self) {
        self.core.primary.history_mut().drop_derived_cache();
    }

    pub fn primary_history_eviction_generation(&self) -> u64 {
        self.core.primary.history().eviction_generation()
    }

    pub fn primary_history_oldest_segment_age(&self) -> Option<u64> {
        self.core.primary.history().oldest_segment_age()
    }

    pub fn evict_oldest_primary_history_segment(&mut self) -> usize {
        self.core.primary.history_mut().evict_oldest_segment()
    }

    /// Resolves a retained source anchor without conflating an evicted source
    /// with an invalid line or unit offset.
    pub fn primary_history_unit(&self, anchor: HistoryAnchor) -> HistoryAnchorResolution {
        self.core.primary.history().resolve_anchor(anchor)
    }

    pub fn primary_history_search(&self, needle: &str, max_matches: usize) -> Vec<HistoryMatch> {
        self.core.primary.history().search(needle, max_matches)
    }

    pub fn primary_history_selection(
        &self,
        start: HistoryAnchor,
        end: HistoryAnchor,
    ) -> Result<Vec<HistoryUnitView>, HistoryRangeError> {
        if start > end {
            return Err(HistoryRangeError::Unrepresentable);
        }
        // Eviction of either endpoint is explicit (SPEC-010 §11). Active-only
        // anchors resolve as Invalid/not-in-store and may still be copied from
        // the live grid below.
        if matches!(
            self.primary_history_unit(start),
            HistoryAnchorResolution::Unavailable
        ) || matches!(
            self.primary_history_unit(end),
            HistoryAnchorResolution::Unavailable
        ) {
            return Err(HistoryRangeError::Stale);
        }

        let mut selected = match self.core.primary.history().selection(start, end) {
            Ok(units) => units,
            Err(HistoryRangeError::Stale) => Vec::new(),
            Err(other) => return Err(other),
        };

        for row in 0..self.core.primary.rows() {
            let Some(cells) = self.core.primary.cell_row(row) else {
                continue;
            };
            let content_end = cells
                .iter()
                .rposition(|cell| cell.role != CellRole::Empty)
                .map_or(0, |index| index + 1);
            let break_after = self
                .core
                .primary
                .row_break_after(row)
                .unwrap_or(HistoryBreakAfter::HardBreak);
            for (col, cell) in cells[..content_end].iter().enumerate() {
                let CellRole::Lead = cell.role else {
                    continue;
                };
                let Some(anchor) = self.core.primary.cell_anchor(col as u16, row) else {
                    continue;
                };
                if anchor < start || anchor > end {
                    continue;
                }
                if selected.iter().any(|unit| unit.anchor == anchor) {
                    continue;
                }
                if selected.len() >= crate::history::HISTORY_SELECTION_UNIT_CAP {
                    return Err(HistoryRangeError::Unrepresentable);
                }
                let text = if cell.overflow {
                    "\u{FFFD}".to_owned()
                } else {
                    self.core.grapheme_store.get(cell.store_id).map_or_else(
                        || cell.character.to_string(),
                        |bytes| String::from_utf8_lossy(bytes).into_owned(),
                    )
                };
                selected.push(HistoryUnitView {
                    anchor,
                    text,
                    width: cell.width.max(1),
                    style: cell.style,
                    break_after,
                });
            }
        }
        selected.sort_by_key(|unit| unit.anchor);
        if selected.is_empty() {
            return Err(HistoryRangeError::Stale);
        }
        Ok(selected)
    }

    pub fn copy_history_range(
        &self,
        start: HistoryAnchor,
        end: HistoryAnchor,
    ) -> Result<String, HistoryRangeError> {
        let units = self.primary_history_selection(start, end)?;
        format_history_copy(&units)
    }

    pub fn encode_host_paste(&self, bytes: &[u8]) -> Result<Vec<u8>, PasteError> {
        encode_paste(bytes, self.core.modes.bracketed_paste)
    }

    pub fn selection_session(&self) -> SelectionSession {
        self.core.selection
    }

    pub fn search_session(&self) -> &SearchSession {
        &self.core.search
    }

    pub fn copy_mode(&self) -> CopyMode {
        self.core.copy_mode
    }

    pub fn clear_selection(&mut self) {
        self.core.selection.clear();
        self.core.copy_buffer = None;
        self.bump_selection_damage();
    }

    pub fn set_linear_selection(&mut self, start: VisualPos, end: VisualPos) {
        if let Some((start, end)) = Self::clamp_visual_pair(self.cols(), self.rows(), start, end) {
            let start_anchor = self.anchor_at(start.col, start.row);
            let end_anchor = self.anchor_at(end.col, end.row);
            self.core
                .selection
                .set_linear(start, end, start_anchor, end_anchor);
            self.bump_selection_damage();
        }
    }

    pub fn set_rectangular_selection(&mut self, start: VisualPos, end: VisualPos) {
        if let Some((start, end)) = Self::clamp_visual_pair(self.cols(), self.rows(), start, end) {
            let start_anchor = self.anchor_at(start.col, start.row);
            let end_anchor = self.anchor_at(end.col, end.row);
            self.core
                .selection
                .set_rectangular(start, end, start_anchor, end_anchor);
            self.bump_selection_damage();
        }
    }

    pub fn copy_selection_text(&self) -> Result<String, HistoryRangeError> {
        // Linear selections with captured source anchors copy from canonical
        // history so scroll/reflow cannot silently retarget the endpoints.
        if self.core.selection.kind == SelectionKind::Linear
            && !self.core.modes.alternate_screen
            && let Some((start, end)) = self.core.selection.ordered_anchors()
        {
            return self.copy_history_range(start, end);
        }
        let Some((start, end)) = self.core.selection.ordered_corners() else {
            return Err(HistoryRangeError::Stale);
        };
        match self.core.selection.kind {
            SelectionKind::Linear => self.copy_visual_linear(start, end),
            SelectionKind::Rectangular => self.copy_visual_rectangular(start, end),
        }
    }

    /// True when the cell is inside the current host selection.
    ///
    /// Linear selections with source anchors use those anchors (SPEC-010 §11)
    /// so scrolled-off content does not keep highlighting new occupants.
    pub fn selection_covers_cell(&self, col: u16, row: u16) -> bool {
        let selection = self.core.selection;
        if selection.kind == SelectionKind::Linear
            && !self.core.modes.alternate_screen
            && let Some((lo, hi)) = selection.ordered_anchors()
        {
            return self
                .anchor_at(col, row)
                .is_some_and(|anchor| anchor >= lo && anchor <= hi);
        }
        selection.contains_cell(col, row, self.cols())
    }

    pub fn search_and_select(&mut self, needle: &str, forward: bool) -> Option<HistoryMatch> {
        if needle.is_empty() {
            self.core.search.clear();
            return None;
        }
        if self.core.search.needle != needle {
            self.core.search.needle = needle.to_owned();
            self.core.search.index = None;
        }
        let matches = self.primary_history_search(needle, MAX_SEARCH_MATCHES);
        let found = self.core.search.step(&matches, forward).copied()?;
        let start_visual = self.visual_pos_for_anchor(found.start);
        let end_visual = self.visual_pos_for_anchor(found.end);
        // Prefer resolved visual corners when both are on-screen; otherwise
        // still retain the canonical search anchors for copy.
        match (start_visual, end_visual) {
            (Some(start), Some(end)) => {
                self.core
                    .selection
                    .set_linear(start, end, Some(found.start), Some(found.end));
            }
            _ => {
                self.core.selection.kind = SelectionKind::Linear;
                self.core.selection.start = start_visual;
                self.core.selection.end = end_visual;
                self.core.selection.start_anchor = Some(found.start);
                self.core.selection.end_anchor = Some(found.end);
            }
        }
        self.bump_selection_damage();
        Some(found)
    }

    pub fn enter_copy_mode(&mut self) {
        let cursor = self.cursor();
        self.core.copy_mode.enter(VisualPos {
            col: cursor.col,
            row: cursor.row,
        });
        self.bump_selection_damage();
    }

    pub fn exit_copy_mode(&mut self) {
        self.core.copy_mode.exit();
        self.bump_selection_damage();
    }

    pub fn copy_mode_motion(&mut self, motion: CopyModeMotion) {
        if !self.core.copy_mode.active {
            return;
        }
        let moving_right = matches!(motion, CopyModeMotion::Right | CopyModeMotion::LineEnd);
        self.core
            .copy_mode
            .apply_motion(motion, self.cols(), self.rows());
        if let Some(cell) = self.cell(
            self.core.copy_mode.cursor.col,
            self.core.copy_mode.cursor.row,
        ) {
            self.core.copy_mode.cursor.col = skip_continuation(
                cell.role,
                moving_right,
                self.core.copy_mode.cursor.col,
                self.cols(),
            );
        }
        if let Some((start, end, kind)) = self.core.copy_mode.selection() {
            self.apply_copy_mode_selection(start, end, kind);
        }
        self.bump_selection_damage();
    }

    pub fn copy_mode_toggle_anchor(&mut self) {
        self.core.copy_mode.toggle_anchor();
        if let Some((start, end, kind)) = self.core.copy_mode.selection() {
            self.apply_copy_mode_selection(start, end, kind);
        }
        self.bump_selection_damage();
    }

    pub fn copy_mode_toggle_kind(&mut self) {
        self.core.copy_mode.toggle_kind();
        if let Some((start, end, kind)) = self.core.copy_mode.selection() {
            self.apply_copy_mode_selection(start, end, kind);
        }
        self.bump_selection_damage();
    }

    fn apply_copy_mode_selection(&mut self, start: VisualPos, end: VisualPos, kind: SelectionKind) {
        let start_anchor = self.anchor_at(start.col, start.row);
        let end_anchor = self.anchor_at(end.col, end.row);
        match kind {
            SelectionKind::Linear => {
                self.core
                    .selection
                    .set_linear(start, end, start_anchor, end_anchor);
            }
            SelectionKind::Rectangular => {
                self.core
                    .selection
                    .set_rectangular(start, end, start_anchor, end_anchor);
            }
        }
    }

    pub fn yank_selection(&mut self) -> Result<String, HistoryRangeError> {
        let text = self.copy_selection_text()?;
        self.core.copy_buffer = Some(text.clone());
        self.core.copy_mode.exit();
        self.bump_selection_damage();
        Ok(text)
    }

    fn bump_selection_damage(&mut self) {
        let rows = self.rows();
        self.core.damage.mark(Mutation::full(rows));
        self.core.damage.commit();
    }

    pub fn take_copy_buffer(&mut self) -> Option<String> {
        self.core.copy_buffer.take()
    }

    fn clamp_visual_pair(
        cols: u16,
        rows: u16,
        start: VisualPos,
        end: VisualPos,
    ) -> Option<(VisualPos, VisualPos)> {
        if cols == 0 || rows == 0 {
            return None;
        }
        let clamp = |pos: VisualPos| VisualPos {
            col: pos.col.min(cols.saturating_sub(1)),
            row: pos.row.min(rows.saturating_sub(1)),
        };
        Some((clamp(start), clamp(end)))
    }

    fn cell_copy_text(&self, col: u16, row: u16) -> Option<String> {
        let cell = self.cell(col, row)?;
        match cell.role {
            CellRole::Empty => Some(" ".to_owned()),
            CellRole::Continuation => Some(String::new()),
            CellRole::Lead => {
                let utf8 = self.lead_utf8(col, row)?;
                Some(String::from_utf8_lossy(&utf8).into_owned())
            }
        }
    }

    fn copy_visual_linear(
        &self,
        start: VisualPos,
        end: VisualPos,
    ) -> Result<String, HistoryRangeError> {
        let (start, end) = order_visual(start, end);
        if let (Some(start_anchor), Some(end_anchor)) = (
            self.anchor_at(start.col, start.row),
            self.anchor_at(end.col, end.row),
        ) && !self.core.modes.alternate_screen
        {
            let (lo, hi) = if start_anchor <= end_anchor {
                (start_anchor, end_anchor)
            } else {
                (end_anchor, start_anchor)
            };
            if let Ok(text) = self.copy_history_range(lo, hi) {
                return Ok(text);
            }
        }
        self.copy_visual_cells(start, end, false)
    }

    fn copy_visual_rectangular(
        &self,
        start: VisualPos,
        end: VisualPos,
    ) -> Result<String, HistoryRangeError> {
        self.copy_visual_cells(start, end, true)
    }

    fn copy_visual_cells(
        &self,
        start: VisualPos,
        end: VisualPos,
        rectangular: bool,
    ) -> Result<String, HistoryRangeError> {
        let min_row = start.row.min(end.row);
        let max_row = start.row.max(end.row);
        let min_col = start.col.min(end.col);
        let max_col = start.col.max(end.col);
        let mut out = String::new();
        for row in min_row..=max_row {
            let (row_start, row_end) = if rectangular {
                (min_col, max_col)
            } else if row == min_row && row == max_row {
                (start.col.min(end.col), start.col.max(end.col))
            } else if row == min_row {
                if (start.row, start.col) <= (end.row, end.col) {
                    (start.col, self.cols().saturating_sub(1))
                } else {
                    (end.col, self.cols().saturating_sub(1))
                }
            } else if row == max_row {
                if (start.row, start.col) <= (end.row, end.col) {
                    (0, end.col)
                } else {
                    (0, start.col)
                }
            } else {
                (0, self.cols().saturating_sub(1))
            };
            for col in row_start..=row_end {
                if let Some(text) = self.cell_copy_text(col, row) {
                    if out.len().saturating_add(text.len()) > crate::MAX_COPY_BYTES {
                        return Err(HistoryRangeError::Unrepresentable);
                    }
                    out.push_str(&text);
                }
            }
            let emit_break = if rectangular {
                row < max_row
            } else {
                row < max_row
                    && self.core.current().row_break_after(row)
                        == Some(HistoryBreakAfter::HardBreak)
            };
            if emit_break {
                if out.len().saturating_add(1) > crate::MAX_COPY_BYTES {
                    return Err(HistoryRangeError::Unrepresentable);
                }
                out.push('\n');
            }
        }
        Ok(out)
    }

    fn anchor_at(&self, col: u16, row: u16) -> Option<HistoryAnchor> {
        self.core.current().cell_anchor(col, row)
    }

    fn visual_pos_for_anchor(&self, anchor: HistoryAnchor) -> Option<VisualPos> {
        for row in 0..self.rows() {
            for col in 0..self.cols() {
                if self.anchor_at(col, row) == Some(anchor) {
                    return Some(VisualPos { col, row });
                }
            }
        }
        None
    }

    pub fn row_text(&self, row: u16) -> Option<String> {
        if row >= self.rows() {
            return None;
        }
        Some(
            (0..self.cols())
                .filter_map(|col| self.cell(col, row))
                .map(|cell| cell.character)
                .collect(),
        )
    }

    pub fn damage_generation(&self) -> u64 {
        self.core.damage.generation()
    }

    pub fn take_damage(&mut self) -> Option<Damage> {
        self.core.damage.take()
    }

    pub fn take_shell_integration_event(&mut self) -> Option<ShellIntegrationEvent> {
        self.core.shell_events.pop_front()
    }

    /// Transfers one bounded, untrusted host-presentation event (OSC title/CWD/hyperlink).
    /// Embedders must treat payloads as display-only input, never as host authority.
    pub fn take_host_presentation_event(&mut self) -> Option<HostPresentationEvent> {
        self.core.presentation_events.pop_front()
    }

    /// Transfers one bounded terminal-generated protocol reply. Transport
    /// layers write these opaque bytes to the child PTY without interpreting
    /// query semantics.
    pub fn take_protocol_reply(&mut self) -> Option<ProtocolReply> {
        self.core.protocol_replies.pop_front()
    }
}

/// Opaque prepared resize held until [`TerminalState::commit_resize`].
pub struct PreparedResize {
    rows: u16,
    primary: PreparedScreen,
    alternate: Option<PreparedScreen>,
}

struct TerminalCore {
    primary: Screen,
    alternate: Option<Screen>,
    line_ids: LineIdAllocator,
    modes: ModeState,
    damage: DamageTracker,
    diagnostics: Diagnostics,
    fault: Option<TerminalError>,
    shell_events: VecDeque<ShellIntegrationEvent>,
    presentation_events: VecDeque<HostPresentationEvent>,
    protocol_replies: VecDeque<ProtocolReply>,
    primary_keyboard_flags: u8,
    alternate_keyboard_flags: u8,
    primary_keyboard_stack: [u8; KEYBOARD_STACK_CAPACITY],
    primary_keyboard_stack_len: usize,
    alternate_keyboard_stack: [u8; KEYBOARD_STACK_CAPACITY],
    alternate_keyboard_stack_len: usize,
    grapheme_store: GraphemeStore,
    active_grapheme: Option<ActiveGrapheme>,
    ambiguous_width: AmbiguousWidthPolicy,
    selection: SelectionSession,
    search: SearchSession,
    copy_mode: CopyMode,
    copy_buffer: Option<String>,
}

impl TerminalCore {
    fn new(cols: u16, rows: u16) -> Result<Self, TerminalError> {
        let mut line_ids = LineIdAllocator::new();
        let primary = Screen::new(cols, rows, &mut line_ids, true)?;
        let mut damage = DamageTracker::default();
        damage.mark(Mutation::full(rows));
        damage.commit();
        Ok(Self {
            primary,
            alternate: None,
            line_ids,
            modes: ModeState::default(),
            damage,
            diagnostics: Diagnostics::default(),
            fault: None,
            shell_events: VecDeque::with_capacity(16),
            presentation_events: VecDeque::with_capacity(MAX_HOST_PRESENTATION_EVENTS),
            protocol_replies: VecDeque::with_capacity(MAX_PROTOCOL_REPLIES),
            primary_keyboard_flags: 0,
            alternate_keyboard_flags: 0,
            primary_keyboard_stack: [0; KEYBOARD_STACK_CAPACITY],
            primary_keyboard_stack_len: 0,
            alternate_keyboard_stack: [0; KEYBOARD_STACK_CAPACITY],
            alternate_keyboard_stack_len: 0,
            grapheme_store: GraphemeStore::default(),
            active_grapheme: None,
            ambiguous_width: AmbiguousWidthPolicy::default(),
            selection: SelectionSession::default(),
            search: SearchSession::default(),
            copy_mode: CopyMode::default(),
            copy_buffer: None,
        })
    }

    fn invalidate_active_grapheme(&mut self) {
        self.active_grapheme = None;
    }

    fn current(&self) -> &Screen {
        if self.modes.alternate_screen {
            self.alternate.as_ref().unwrap_or(&self.primary)
        } else {
            &self.primary
        }
    }

    /// The cursor's logical line right now. Used to stamp a shell-integration
    /// marker's line at the moment it is recognized (see
    /// `ShellIntegrationEvent`), never as a later re-sample: a caller that
    /// re-samples after draining a whole batch of queued events would
    /// observe the position after every later marker/output in that batch.
    fn current_line(&self) -> LineId {
        let cursor = self.current().cursor(self.modes.cursor_visible);
        self.current().line_id(cursor.row).unwrap_or(LineId(1))
    }

    /// The line where a command's real output ends, at the moment the
    /// trusted `D` marker is recognized. When output ends with a trailing
    /// newline (the common case), the cursor sits on a fresh row that has
    /// no content yet -- that row is not the command's output, it is simply
    /// where the shell's own next prompt will shortly be drawn, onto the
    /// same row a caller would otherwise capture as this Block's last line.
    /// The previous row is the true last output line in that case; a row
    /// that already has content (no trailing newline) is used as-is.
    fn completion_line(&self) -> LineId {
        let cursor = self.current().cursor(self.modes.cursor_visible);
        let screen = self.current();
        let row_is_empty = screen
            .cell_row(cursor.row)
            .is_none_or(|cells| cells.iter().all(|cell| cell.role == CellRole::Empty));
        let row = if row_is_empty && cursor.row > 0 {
            cursor.row - 1
        } else {
            cursor.row
        };
        screen.line_id(row).unwrap_or(LineId(1))
    }

    fn current_mut(&mut self) -> &mut Screen {
        if self.modes.alternate_screen
            && let Some(screen) = &mut self.alternate
        {
            return screen;
        }
        &mut self.primary
    }

    fn apply(&mut self, mutation: Mutation) {
        self.damage.mark(mutation);
    }

    fn prepare_resize(&mut self, cols: u16, rows: u16) -> Result<PreparedResize, TerminalError> {
        if let Some(error) = self.fault {
            return Err(error);
        }
        if cols == 0 || rows == 0 {
            return Err(TerminalError::InvalidSize);
        }
        if cols > MAX_TERMINAL_COLUMNS || rows > MAX_TERMINAL_ROWS {
            return Err(TerminalError::InvalidSize);
        }

        #[cfg(feature = "test-fault-injection")]
        if crate::test_fault::take(crate::test_fault::FaultPoint::ResizePrepare) {
            return Err(TerminalError::LineIdentityExhausted);
        }

        let mut required_ids = usize::from(rows.saturating_sub(self.primary.rows()));
        if let Some(screen) = &self.alternate {
            required_ids += usize::from(rows.saturating_sub(screen.rows()));
        }
        if !self.line_ids.can_allocate(required_ids) {
            return Err(TerminalError::LineIdentityExhausted);
        }

        let primary =
            self.primary
                .prepare_resize(cols, rows, &mut self.line_ids, &self.grapheme_store)?;
        let alternate = if let Some(screen) = &self.alternate {
            Some(screen.prepare_resize(cols, rows, &mut self.line_ids, &self.grapheme_store)?)
        } else {
            None
        };
        Ok(PreparedResize {
            rows,
            primary,
            alternate,
        })
    }

    fn commit_resize(&mut self, prepared: PreparedResize) {
        self.invalidate_active_grapheme();
        let primary = self
            .primary
            .commit_prepared(prepared.primary, &mut self.grapheme_store);
        let alternate = if let Some(prepared_alt) = prepared.alternate {
            if let Some(screen) = &mut self.alternate {
                screen.commit_prepared(prepared_alt, &mut self.grapheme_store)
            } else {
                Mutation::none()
            }
        } else {
            Mutation::none()
        };
        self.apply(
            primary
                .merge(alternate)
                .merge(Mutation::full(prepared.rows)),
        );
        self.damage.commit();
    }

    fn enqueue_protocol_reply(&mut self, reply: ProtocolReply) {
        if self.protocol_replies.len() == self.protocol_replies.capacity() {
            self.record_deferred();
            return;
        }
        self.protocol_replies.push_back(reply);
    }

    fn current_keyboard_state_mut(
        &mut self,
    ) -> (&mut u8, &mut [u8; KEYBOARD_STACK_CAPACITY], &mut usize) {
        if self.modes.alternate_screen {
            (
                &mut self.alternate_keyboard_flags,
                &mut self.alternate_keyboard_stack,
                &mut self.alternate_keyboard_stack_len,
            )
        } else {
            (
                &mut self.primary_keyboard_flags,
                &mut self.primary_keyboard_stack,
                &mut self.primary_keyboard_stack_len,
            )
        }
    }

    fn sync_keyboard_flags(&mut self) {
        let flags = self.modes.keyboard_flags & KEYBOARD_FLAGS_MASK;
        if self.modes.alternate_screen {
            self.alternate_keyboard_flags = flags;
        } else {
            self.primary_keyboard_flags = flags;
        }
    }

    fn set_keyboard_flags(&mut self, flags: u8, mode: u16) -> bool {
        let flags = flags & KEYBOARD_FLAGS_MASK;
        let next = match mode {
            1 => flags,
            2 => self.modes.keyboard_flags | flags,
            3 => self.modes.keyboard_flags & !flags,
            _ => return false,
        } & KEYBOARD_FLAGS_MASK;
        {
            let (_current, stack, len) = self.current_keyboard_state_mut();
            if *len == 0 {
                stack[0] = next;
                *len = 1;
            } else {
                stack[*len - 1] = next;
            }
        }
        self.modes.keyboard_flags = next;
        self.sync_keyboard_flags();
        true
    }

    fn push_keyboard_flags(&mut self, flags: u8) {
        let flags = flags & KEYBOARD_FLAGS_MASK;
        {
            let (_current, stack, len) = self.current_keyboard_state_mut();
            if *len == KEYBOARD_STACK_CAPACITY {
                stack.copy_within(1.., 0);
                *len -= 1;
            }
            stack[*len] = flags;
            *len += 1;
        }
        self.modes.keyboard_flags = flags;
        self.sync_keyboard_flags();
    }

    fn pop_keyboard_flags(&mut self, count: u16) {
        let current = {
            let (_current, stack, len) = self.current_keyboard_state_mut();
            let remove = usize::from(count).min(*len);
            *len -= remove;
            if *len == 0 {
                0
            } else {
                stack[*len - 1]
            }
        };
        self.modes.keyboard_flags = current;
        self.sync_keyboard_flags();
    }

    fn reply_kitty_flags(&mut self) {
        if let Some(reply) = encode_kitty_flags(self.modes.keyboard_flags) {
            self.enqueue_protocol_reply(reply);
        } else {
            self.record_deferred();
        }
    }

    fn reply_dsr_cpr(&mut self) {
        let cursor = self.current().cursor(self.modes.cursor_visible);
        if let Some(reply) = encode_dsr_cpr(cursor.row, cursor.col) {
            self.enqueue_protocol_reply(reply);
        } else {
            self.record_deferred();
        }
    }

    fn reply_decrqm(&mut self, params: &[u16]) {
        for mode in params {
            match *mode {
                7 => {
                    let status = if self.modes.wraparound { 1 } else { 2 };
                    if let Some(reply) = encode_decrqm_private(7, status) {
                        self.enqueue_protocol_reply(reply);
                    } else {
                        self.record_deferred();
                    }
                }
                25 => {
                    let status = if self.modes.cursor_visible { 1 } else { 2 };
                    if let Some(reply) = encode_decrqm_private(25, status) {
                        self.enqueue_protocol_reply(reply);
                    } else {
                        self.record_deferred();
                    }
                }
                1 => {
                    let status = if self.modes.application_cursor { 1 } else { 2 };
                    if let Some(reply) = encode_decrqm_private(1, status) {
                        self.enqueue_protocol_reply(reply);
                    } else {
                        self.record_deferred();
                    }
                }
                66 => {
                    let status = if self.modes.application_keypad { 1 } else { 2 };
                    if let Some(reply) = encode_decrqm_private(66, status) {
                        self.enqueue_protocol_reply(reply);
                    } else {
                        self.record_deferred();
                    }
                }
                1049 => {
                    let status = if self.modes.alternate_screen { 1 } else { 2 };
                    if let Some(reply) = encode_decrqm_private(1049, status) {
                        self.enqueue_protocol_reply(reply);
                    } else {
                        self.record_deferred();
                    }
                }
                2004 => {
                    let status = if self.modes.bracketed_paste { 1 } else { 2 };
                    if let Some(reply) = encode_decrqm_private(2004, status) {
                        self.enqueue_protocol_reply(reply);
                    } else {
                        self.record_deferred();
                    }
                }
                2027 => {
                    let status = if self.modes.unicode_core { 1 } else { 2 };
                    if let Some(reply) = encode_decrqm_private(2027, status) {
                        self.enqueue_protocol_reply(reply);
                    } else {
                        self.record_deferred();
                    }
                }
                1000 => self.reply_mouse_reporting(1000, MouseReporting::Button),
                1002 => self.reply_mouse_reporting(1002, MouseReporting::ButtonDrag),
                1003 => self.reply_mouse_reporting(1003, MouseReporting::Any),
                1006 => {
                    let status = if self.modes.mouse_sgr { 1 } else { 2 };
                    if let Some(reply) = encode_decrqm_private(1006, status) {
                        self.enqueue_protocol_reply(reply);
                    } else {
                        self.record_deferred();
                    }
                }
                _ => self.record_deferred(),
            }
        }
    }

    fn set_mouse_reporting(&mut self, level: MouseReporting, enabled: bool) {
        if enabled {
            self.modes.mouse_reporting = level;
        } else if self.modes.mouse_reporting == level {
            self.modes.mouse_reporting = MouseReporting::Off;
        }
    }

    fn reply_mouse_reporting(&mut self, mode: u16, level: MouseReporting) {
        let status = if self.modes.mouse_reporting == level {
            1
        } else {
            2
        };
        if let Some(reply) = encode_decrqm_private(mode, status) {
            self.enqueue_protocol_reply(reply);
        } else {
            self.record_deferred();
        }
    }

    fn reply_primary_da(&mut self) {
        if let Some(reply) = encode_primary_da() {
            self.enqueue_protocol_reply(reply);
        } else {
            self.record_deferred();
        }
    }

    fn enqueue_presentation(&mut self, event: HostPresentationEvent) {
        if self.presentation_events.len() == self.presentation_events.capacity() {
            self.record_deferred();
            return;
        }
        self.presentation_events.push_back(event);
    }

    fn editing_mutation<F>(&mut self, op: F) -> Mutation
    where
        F: FnOnce(
            &mut Screen,
            &mut LineIdAllocator,
            &mut GraphemeStore,
        ) -> Result<Mutation, TerminalError>,
    {
        let result = if self.modes.alternate_screen {
            if let Some(screen) = &mut self.alternate {
                op(screen, &mut self.line_ids, &mut self.grapheme_store)
            } else {
                op(
                    &mut self.primary,
                    &mut self.line_ids,
                    &mut self.grapheme_store,
                )
            }
        } else {
            op(
                &mut self.primary,
                &mut self.line_ids,
                &mut self.grapheme_store,
            )
        };
        match result {
            Ok(mutation) => mutation,
            Err(error) => {
                self.record_fault(error);
                Mutation::none()
            }
        }
    }

    fn set_cursor_visible(&mut self, visible: bool) {
        if self.modes.cursor_visible == visible {
            return;
        }
        self.modes.cursor_visible = visible;
        let row = self.current().cursor(visible).row;
        self.apply(Mutation::row(row));
    }

    fn set_alternate_screen(&mut self, enabled: bool) -> Result<(), TerminalError> {
        if enabled == self.modes.alternate_screen {
            return Ok(());
        }
        self.invalidate_active_grapheme();
        self.selection.clear();
        self.copy_mode.exit();
        self.copy_buffer = None;

        self.sync_keyboard_flags();
        if enabled {
            let cols = self.primary.cols();
            let rows = self.primary.rows();
            let pen = self.primary.pen();
            let mut screen = Screen::new(cols, rows, &mut self.line_ids, false)?;
            screen.inherit_pen_for_clean_buffer(pen, &mut self.grapheme_store);
            self.alternate = Some(screen);
            self.modes.alternate_screen = true;
            self.modes.keyboard_flags = self.alternate_keyboard_flags;
            self.apply(Mutation::full(rows));
        } else {
            if let Some(mut screen) = self.alternate.take() {
                screen.release_all_payloads(&mut self.grapheme_store);
            }
            self.modes.alternate_screen = false;
            self.modes.keyboard_flags = self.primary_keyboard_flags;
            self.apply(Mutation::full(self.primary.rows()));
        }
        Ok(())
    }

    fn print_current(&mut self, character: char) -> Result<Mutation, TerminalError> {
        self.print_scalar(character)
    }

    fn print_scalar(&mut self, character: char) -> Result<Mutation, TerminalError> {
        let unicode_core = self.modes.unicode_core;
        let wraparound = self.modes.wraparound;
        let ambiguous = self.ambiguous_width;
        let style = self.current().pen();

        // Append to active grapheme when eligible.
        if let Some(active) = self.active_grapheme.as_ref() {
            if try_append_scalar(active, character, unicode_core)
                || (!unicode_core
                    && grapheme_terminal_width(&character.to_string(), ambiguous) == 0)
            {
                return self.append_to_active(character, wraparound, ambiguous);
            }
            // Boundary: active unit stays committed; start a new one.
            self.active_grapheme = None;
        }

        // Legacy combining onto previous cell without active anchor.
        if !unicode_core {
            let width = grapheme_terminal_width(&character.to_string(), ambiguous);
            if width == 0 {
                return Ok(Mutation::none());
            }
        }

        let mut text = String::new();
        text.push(character);
        let width = if unicode_core {
            grapheme_terminal_width(&text, ambiguous)
        } else {
            grapheme_terminal_width(&text, ambiguous).max(1)
        };

        if width == 0 {
            // Isolated combining in Unicode-core with no active base: ignore.
            return Ok(Mutation::none());
        }

        let cursor = self.current().cursor(true);
        match edge_decision_new_unit(cursor.col, self.current().cols(), width, wraparound) {
            EdgeDecision::IgnoreUnit => {
                // SPEC-011 §8.4: ignore atomically; leave active unset.
                return Ok(Mutation::none());
            }
            EdgeDecision::RejectExtension | EdgeDecision::Place => {}
        }

        let lead = build_lead_cell(&text, width, style, false, &mut self.grapheme_store, None);
        let (mutation, soft_wrapped, lead_col, lead_row) = {
            let line_ids = &mut self.line_ids;
            let store = &mut self.grapheme_store;
            let screen = if self.modes.alternate_screen {
                self.alternate.as_mut().unwrap_or(&mut self.primary)
            } else {
                &mut self.primary
            };
            screen.place_new_unit(lead, wraparound, line_ids, store)?
        };
        let _ = soft_wrapped;

        self.active_grapheme = Some(ActiveGrapheme {
            col: lead_col,
            row: lead_row,
            utf8: text,
            width,
            style,
            store_id: self
                .current()
                .cell(lead_col, lead_row)
                .map(|c| c.store_id)
                .unwrap_or(crate::grapheme_store::INLINE_STORE_ID),
            overflow: false,
        });
        Ok(mutation)
    }

    fn append_to_active(
        &mut self,
        character: char,
        wraparound: bool,
        ambiguous: AmbiguousWidthPolicy,
    ) -> Result<Mutation, TerminalError> {
        let Some(mut active) = self.active_grapheme.take() else {
            return Ok(Mutation::none());
        };
        let previous_width = active.width;
        let result = append_payload(&mut active, character, &mut self.grapheme_store, ambiguous);

        if result.width_changed && result.width > previous_width {
            match edge_decision_late_widen(
                active.col,
                self.current().cols(),
                result.width,
                wraparound,
            ) {
                EdgeDecision::RejectExtension => {
                    // SPEC-011 §8.4: reject only the width-changing extension.
                    active.utf8.pop();
                    self.active_grapheme = Some(active);
                    return Ok(Mutation::none());
                }
                EdgeDecision::IgnoreUnit => {
                    active.utf8.pop();
                    self.active_grapheme = Some(active);
                    return Ok(Mutation::none());
                }
                EdgeDecision::Place => {
                    if active.col + 1 >= self.current().cols() && wraparound {
                        // Late widen that must soft-wrap: clear old, re-place on next row.
                        let style = active.style;
                        let text = active.utf8.clone();
                        let overflow = active.overflow;
                        let old_store = active.store_id;
                        let clear_mut = {
                            let store = &mut self.grapheme_store;
                            let screen = if self.modes.alternate_screen {
                                self.alternate.as_mut().unwrap_or(&mut self.primary)
                            } else {
                                &mut self.primary
                            };
                            screen.clear_unit_at(active.col, active.row, store)
                        };
                        // Soft-wrap lineage: mark previous row wrap by setting pending and LF.
                        {
                            let screen = if self.modes.alternate_screen {
                                self.alternate.as_mut().unwrap_or(&mut self.primary)
                            } else {
                                &mut self.primary
                            };
                            screen.set_pending_wrap(true);
                        }
                        let lead = build_lead_cell(
                            &text,
                            result.width,
                            style,
                            overflow,
                            &mut self.grapheme_store,
                            Some(old_store),
                        );
                        let (place_mut, _, lead_col, lead_row) = {
                            let line_ids = &mut self.line_ids;
                            let store = &mut self.grapheme_store;
                            let screen = if self.modes.alternate_screen {
                                self.alternate.as_mut().unwrap_or(&mut self.primary)
                            } else {
                                &mut self.primary
                            };
                            screen.place_new_unit(lead, wraparound, line_ids, store)?
                        };
                        active.col = lead_col;
                        active.row = lead_row;
                        active.width = result.width;
                        active.store_id = self
                            .current()
                            .cell(lead_col, lead_row)
                            .map(|c| c.store_id)
                            .unwrap_or(crate::grapheme_store::INLINE_STORE_ID);
                        self.active_grapheme = Some(active);
                        return Ok(clear_mut.merge(place_mut));
                    }
                }
            }
        }

        active.width = result.width;
        let release = if active.store_id != crate::grapheme_store::INLINE_STORE_ID {
            Some(active.store_id)
        } else {
            None
        };
        let lead = build_lead_cell(
            &active.utf8,
            active.width.max(1),
            active.style,
            active.overflow,
            &mut self.grapheme_store,
            release,
        );
        active.store_id = lead.store_id;
        active.overflow = lead.overflow;
        let mutation = {
            let store = &mut self.grapheme_store;
            let screen = if self.modes.alternate_screen {
                self.alternate.as_mut().unwrap_or(&mut self.primary)
            } else {
                &mut self.primary
            };
            screen.replace_active_lead(active.col, active.row, lead, previous_width, store)
        };
        // Adjust cursor after late widen occupying an extra cell.
        if result.width > previous_width && result.width >= 2 {
            let screen = if self.modes.alternate_screen {
                self.alternate.as_mut().unwrap_or(&mut self.primary)
            } else {
                &mut self.primary
            };
            let cursor = screen.cursor(true);
            if !screen.pending_wrap() && cursor.col == active.col + 1 {
                // Was width-1 with cursor after lead; now need cursor after continuation.
                let _ = screen; // cursor advance handled below
            }
        }
        if result.width > previous_width {
            let cols = self.current().cols();
            let screen = if self.modes.alternate_screen {
                self.alternate.as_mut().unwrap_or(&mut self.primary)
            } else {
                &mut self.primary
            };
            let next = active.col.saturating_add(u16::from(result.width));
            if next >= cols {
                // Move cursor to last col with pending wrap.
                let _ = screen.set_col(cols.saturating_sub(1));
                screen.set_pending_wrap(true);
            } else {
                let _ = screen.set_col(next);
            }
        }
        self.active_grapheme = Some(active);
        Ok(mutation)
    }

    fn execute_current(&mut self, byte: u8) -> Result<Mutation, TerminalError> {
        if let 0x08..=0x0d = byte {
            self.invalidate_active_grapheme();
        }
        if self.modes.alternate_screen
            && let Some(screen) = &mut self.alternate
        {
            return screen.execute(byte, &mut self.line_ids, &mut self.grapheme_store);
        }
        self.primary
            .execute(byte, &mut self.line_ids, &mut self.grapheme_store)
    }

    fn record_fault(&mut self, error: TerminalError) {
        if self.fault.is_none() {
            self.fault = Some(error);
        }
    }

    fn record_deferred(&mut self) {
        self.diagnostics.deferred_sequences = self.diagnostics.deferred_sequences.saturating_add(1);
    }

    fn record_unknown(&mut self) {
        self.diagnostics.unknown_sequences = self.diagnostics.unknown_sequences.saturating_add(1);
    }

    fn record_malformed(&mut self) {
        self.diagnostics.malformed_sequences =
            self.diagnostics.malformed_sequences.saturating_add(1);
    }
}

impl Actions for TerminalCore {
    fn print(&mut self, character: char) {
        if self.fault.is_some() {
            return;
        }
        match self.print_current(character) {
            Ok(mutation) => self.apply(mutation),
            Err(error) => self.record_fault(error),
        }
    }

    fn execute(&mut self, byte: u8) {
        if self.fault.is_some() {
            return;
        }
        match self.execute_current(byte) {
            Ok(mutation) => self.apply(mutation),
            Err(error) => self.record_fault(error),
        }
    }

    fn csi(&mut self, params: &[u16], private: Option<u8>, ignored: bool, final_byte: u8) {
        if self.fault.is_some() {
            return;
        }
        if ignored {
            // DECRQM uses intermediate `$` which the ECMA-48 parser marks as
            // ignored; handle the known private-mode query without advertising
            // unsupported modes.
            if private == Some(b'?') && final_byte == b'p' {
                self.reply_decrqm(params);
            } else {
                self.record_deferred();
            }
            return;
        }

        if private == Some(b'?') && final_byte == b'u' {
            if params.is_empty() {
                self.reply_kitty_flags();
            } else {
                self.record_deferred();
            }
            return;
        }
        if matches!(private, Some(b'=') | Some(b'>') | Some(b'<')) && final_byte == b'u' {
            if private == Some(b'=') && params.len() <= 2 {
                let flags = params.first().copied().unwrap_or(0) as u8;
                let mode = params.get(1).copied().unwrap_or(1);
                if self.set_keyboard_flags(flags, mode) {
                    return;
                }
            } else if private == Some(b'>') && params.len() <= 1 {
                self.push_keyboard_flags(params.first().copied().unwrap_or(0) as u8);
                return;
            } else if private == Some(b'<') && params.len() <= 1 {
                self.pop_keyboard_flags(params.first().copied().unwrap_or(1));
                return;
            }
            self.record_deferred();
            return;
        }

        if private.is_some() {
            if private == Some(b'?') && matches!(final_byte, b'h' | b'l') {
                let enabled = final_byte == b'h';
                for mode in params {
                    match *mode {
                        1 => self.modes.application_cursor = enabled,
                        7 => {
                            if self.modes.wraparound != enabled {
                                self.modes.wraparound = enabled;
                                self.invalidate_active_grapheme();
                            }
                        }
                        25 => self.set_cursor_visible(enabled),
                        66 => self.modes.application_keypad = enabled,
                        2027 => {
                            if self.modes.unicode_core != enabled {
                                self.modes.unicode_core = enabled;
                                self.invalidate_active_grapheme();
                            }
                        }
                        1049 => {
                            if let Err(error) = self.set_alternate_screen(enabled) {
                                self.record_fault(error);
                                break;
                            }
                        }
                        2004 => {
                            self.modes.bracketed_paste = enabled;
                        }
                        1000 => self.set_mouse_reporting(MouseReporting::Button, enabled),
                        1002 => self.set_mouse_reporting(MouseReporting::ButtonDrag, enabled),
                        1003 => self.set_mouse_reporting(MouseReporting::Any, enabled),
                        1006 => self.modes.mouse_sgr = enabled,
                        _ => self.record_deferred(),
                    }
                }
            } else {
                self.record_deferred();
            }
            return;
        }

        let mutation = match final_byte {
            b'A' => {
                self.invalidate_active_grapheme();
                self.current_mut().cursor_up(param_one(params, 0))
            }
            b'B' => {
                self.invalidate_active_grapheme();
                self.current_mut().cursor_down(param_one(params, 0))
            }
            b'C' => {
                self.invalidate_active_grapheme();
                self.current_mut().cursor_forward(param_one(params, 0))
            }
            b'D' => {
                self.invalidate_active_grapheme();
                self.current_mut().cursor_back(param_one(params, 0))
            }
            b'H' | b'f' => {
                self.invalidate_active_grapheme();
                self.current_mut().set_cursor(
                    param_one(params, 0).saturating_sub(1),
                    param_one(params, 1).saturating_sub(1),
                )
            }
            b'G' => {
                self.invalidate_active_grapheme();
                self.current_mut()
                    .set_col(param_one(params, 0).saturating_sub(1))
            }
            b'd' => {
                self.invalidate_active_grapheme();
                self.current_mut()
                    .set_row(param_one(params, 0).saturating_sub(1))
            }
            b'J' => {
                self.invalidate_active_grapheme();
                let mode = param_zero(params, 0);
                let store = &mut self.grapheme_store;
                if self.modes.alternate_screen {
                    if let Some(screen) = &mut self.alternate {
                        screen.erase_display(mode, store)
                    } else {
                        self.primary.erase_display(mode, store)
                    }
                } else {
                    self.primary.erase_display(mode, store)
                }
            }
            b'K' => {
                self.invalidate_active_grapheme();
                let mode = param_zero(params, 0);
                let store = &mut self.grapheme_store;
                if self.modes.alternate_screen {
                    if let Some(screen) = &mut self.alternate {
                        screen.erase_line(mode, store)
                    } else {
                        self.primary.erase_line(mode, store)
                    }
                } else {
                    self.primary.erase_line(mode, store)
                }
            }
            b's' => {
                self.current_mut().save_cursor();
                Mutation::none()
            }
            b'u' => {
                self.invalidate_active_grapheme();
                self.current_mut().restore_cursor()
            }
            b'm' => {
                if self.current_mut().apply_sgr(params) {
                    self.record_deferred();
                }
                Mutation::none()
            }
            b'n' => {
                match param_zero(params, 0) {
                    6 => self.reply_dsr_cpr(),
                    _ => self.record_unknown(),
                }
                Mutation::none()
            }
            b'c' => {
                match param_zero(params, 0) {
                    0 => self.reply_primary_da(),
                    _ => self.record_unknown(),
                }
                Mutation::none()
            }
            b'r' => {
                self.invalidate_active_grapheme();
                let top = param_zero(params, 0);
                let bottom = param_zero(params, 1);
                self.current_mut().set_scroll_region(top, bottom)
            }
            b'@' => {
                self.invalidate_active_grapheme();
                let count = param_one(params, 0);
                self.editing_mutation(|screen, _, store| Ok(screen.insert_characters(count, store)))
            }
            b'P' => {
                self.invalidate_active_grapheme();
                let count = param_one(params, 0);
                self.editing_mutation(|screen, _, store| Ok(screen.delete_characters(count, store)))
            }
            b'X' => {
                self.invalidate_active_grapheme();
                let count = param_one(params, 0);
                self.editing_mutation(|screen, _, store| Ok(screen.erase_characters(count, store)))
            }
            b'L' => {
                self.invalidate_active_grapheme();
                let count = param_one(params, 0);
                self.editing_mutation(|screen, line_ids, store| {
                    screen.insert_lines(count, line_ids, store)
                })
            }
            b'M' => {
                self.invalidate_active_grapheme();
                let count = param_one(params, 0);
                self.editing_mutation(|screen, line_ids, store| {
                    screen.delete_lines(count, line_ids, store)
                })
            }
            b'S' => {
                self.invalidate_active_grapheme();
                let count = param_one(params, 0);
                self.editing_mutation(|screen, line_ids, store| {
                    screen.scroll_up(
                        count,
                        line_ids,
                        Some(store),
                        crate::HistoryBreakAfter::HardBreak,
                    )
                })
            }
            b'T' => {
                self.invalidate_active_grapheme();
                let count = param_one(params, 0);
                self.editing_mutation(|screen, line_ids, store| {
                    screen.scroll_down(count, line_ids, Some(store))
                })
            }
            b'h' | b'l' => {
                self.record_deferred();
                Mutation::none()
            }
            _ => {
                self.record_unknown();
                Mutation::none()
            }
        };
        self.apply(mutation);
    }

    fn esc(&mut self, final_byte: u8, had_intermediate: bool) {
        if self.fault.is_some() {
            return;
        }
        if had_intermediate {
            self.record_deferred();
            return;
        }
        let mutation = match final_byte {
            b'=' => {
                self.modes.application_keypad = true;
                Mutation::none()
            }
            b'>' => {
                self.modes.application_keypad = false;
                Mutation::none()
            }
            b'7' => {
                self.current_mut().save_cursor();
                Mutation::none()
            }
            b'8' => self.current_mut().restore_cursor(),
            b'D' => {
                self.invalidate_active_grapheme();
                self.editing_mutation(|screen, line_ids, store| screen.index_down(line_ids, store))
            }
            b'M' => {
                self.invalidate_active_grapheme();
                self.editing_mutation(|screen, line_ids, store| {
                    screen.reverse_index(line_ids, store)
                })
            }
            b'E' => {
                self.invalidate_active_grapheme();
                self.editing_mutation(|screen, line_ids, store| screen.next_line(line_ids, store))
            }
            _ => {
                self.record_unknown();
                Mutation::none()
            }
        };
        self.apply(mutation);
    }

    fn osc(&mut self, bytes: &[u8], truncated: bool) {
        if self.fault.is_some() {
            return;
        }
        if truncated {
            self.record_deferred();
            return;
        }
        if let Some(event) = parse_osc_presentation(bytes) {
            self.enqueue_presentation(event);
            return;
        }
        if self.modes.alternate_screen {
            self.record_deferred();
            return;
        }
        let event =
            match bytes.strip_prefix(b"133;") {
                Some(payload) => {
                    let mut fields = payload.split(|byte| *byte == b';');
                    match (fields.next(), fields.next(), fields.next()) {
                        (Some(b"A"), Some(token), None) => ShellIntegrationToken::from_hex(token)
                            .map(|token| ShellIntegrationEvent::PromptStarted { token }),
                        (Some(b"C"), Some(token), None) => ShellIntegrationToken::from_hex(token)
                            .map(|token| ShellIntegrationEvent::CommandStarted {
                                token,
                                line: self.current_line(),
                            }),
                        (Some(b"D"), Some(token), Some(status)) => {
                            ShellIntegrationToken::from_hex(token).and_then(|token| {
                                std::str::from_utf8(status)
                                    .ok()
                                    .and_then(|status| status.parse::<i32>().ok())
                                    .map(|exit_status| ShellIntegrationEvent::CommandFinished {
                                        token,
                                        exit_status,
                                        line: self.completion_line(),
                                    })
                            })
                        }
                        _ => None,
                    }
                }
                _ => None,
            };
        let Some(event) = event else {
            self.record_deferred();
            return;
        };
        if self.shell_events.len() == self.shell_events.capacity() {
            self.record_deferred();
            return;
        }
        self.shell_events.push_back(event);
    }

    fn deferred_string(&mut self) {
        if self.fault.is_none() {
            self.record_deferred();
        }
    }

    fn malformed(&mut self) {
        if self.fault.is_none() {
            self.record_malformed();
        }
    }
}

fn param_one(params: &[u16], index: usize) -> u16 {
    match params.get(index).copied().unwrap_or(0) {
        0 => 1,
        value => value,
    }
}

fn param_zero(params: &[u16], index: usize) -> u16 {
    params.get(index).copied().unwrap_or(0)
}

fn skip_wire_leads(cells: Vec<HistoryWireCell>, skip: &mut u32) -> Vec<HistoryWireCell> {
    if *skip == 0 {
        return cells;
    }
    let mut index = 0usize;
    while index < cells.len() && *skip > 0 {
        if cells[index].continuation {
            index += 1;
            continue;
        }
        *skip = skip.saturating_sub(1);
        index += 1;
        while index < cells.len() && cells[index].continuation {
            index += 1;
        }
    }
    cells[index..].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_identity_exhaustion_is_explicit_and_does_not_duplicate_scroll_id() {
        let mut terminal = TerminalState::new(2, 1).expect("valid terminal");
        terminal.core.line_ids = LineIdAllocator::with_next(Some(u64::MAX));

        terminal
            .feed(b"A\r\n")
            .expect("last available line id may be allocated once");
        let last = terminal.line_id(0).expect("visible line has id");
        assert_eq!(last, LineId(u64::MAX));

        assert_eq!(
            terminal.feed(b"\r\n"),
            Err(TerminalError::LineIdentityExhausted)
        );
        assert_eq!(terminal.line_id(0), Some(last));
        assert_eq!(
            terminal.feed(b"ignored after fault"),
            Err(TerminalError::LineIdentityExhausted)
        );
        assert_eq!(terminal.line_id(0), Some(last));
    }

    #[test]
    fn resize_preflights_line_identity_for_primary_and_alternate_atomically() {
        let mut terminal = TerminalState::new(2, 1).expect("valid terminal");
        terminal.core.line_ids = LineIdAllocator::with_next(Some(u64::MAX));
        terminal
            .feed(b"\x1b[?1049h")
            .expect("alternate consumes final available id");
        assert!(terminal.modes().alternate_screen);

        assert_eq!(
            terminal.resize(2, 2),
            Err(TerminalError::LineIdentityExhausted)
        );
        assert_eq!((terminal.cols(), terminal.rows()), (2, 1));
        terminal
            .feed(b"\x1b[?1049l")
            .expect("leaving alternate needs no new id");
        assert_eq!((terminal.cols(), terminal.rows()), (2, 1));
    }

    #[test]
    fn exposes_bounded_trusted_shell_events_without_exposing_osc_payload() {
        let mut terminal = TerminalState::new(80, 24).unwrap();
        terminal
            .feed(b"\x1b]133;C;00112233445566778899aabbccddeeff\x07\x1b]133;D;00112233445566778899aabbccddeeff;17\x1b\\")
            .unwrap();
        let token = ShellIntegrationToken::from_bytes([
            0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee,
            0xff,
        ]);

        assert_eq!(
            terminal.take_shell_integration_event(),
            Some(ShellIntegrationEvent::CommandStarted {
                token,
                line: LineId(1),
            })
        );
        assert_eq!(
            terminal.take_shell_integration_event(),
            Some(ShellIntegrationEvent::CommandFinished {
                token,
                exit_status: 17,
                line: LineId(1),
            })
        );
        assert_eq!(terminal.take_shell_integration_event(), None);
    }

    #[test]
    fn command_finished_line_is_the_last_output_row_not_the_next_empty_one() {
        // Regression: output ending with a trailing newline leaves the
        // cursor on a fresh, still-empty row when `D` fires. That row is
        // about to be overwritten by the shell's own next prompt, not part
        // of the command's output, so `line` must back up to the row that
        // actually holds the output.
        let mut terminal = TerminalState::new(80, 24).unwrap();
        let token_bytes = [
            0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee,
            0xff,
        ];
        let token = ShellIntegrationToken::from_bytes(token_bytes);
        terminal
            .feed(b"\x1b]133;C;00112233445566778899aabbccddeeff\x07")
            .unwrap();
        assert_eq!(
            terminal.take_shell_integration_event(),
            Some(ShellIntegrationEvent::CommandStarted {
                token,
                line: LineId(1)
            })
        );
        // Real output on row 1, ending with a newline: cursor moves to a
        // fresh, empty row 2 before the `D` marker is even parsed.
        terminal.feed(b"output-line\r\n").unwrap();
        terminal
            .feed(b"\x1b]133;D;00112233445566778899aabbccddeeff;0\x07")
            .unwrap();
        assert_eq!(
            terminal.take_shell_integration_event(),
            Some(ShellIntegrationEvent::CommandFinished {
                token,
                exit_status: 0,
                line: LineId(1),
            }),
            "line must be the row holding the real output, not the empty row after it"
        );
    }

    #[test]
    fn command_finished_line_stays_on_the_output_row_without_a_trailing_newline() {
        let mut terminal = TerminalState::new(80, 24).unwrap();
        let token = ShellIntegrationToken::from_bytes([
            0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee,
            0xff,
        ]);
        terminal
            .feed(b"\x1b]133;C;00112233445566778899aabbccddeeff\x07")
            .unwrap();
        assert_eq!(
            terminal.take_shell_integration_event(),
            Some(ShellIntegrationEvent::CommandStarted {
                token,
                line: LineId(1)
            })
        );
        // No trailing newline: cursor stays on the same row as the output.
        terminal.feed(b"no-newline-output").unwrap();
        terminal
            .feed(b"\x1b]133;D;00112233445566778899aabbccddeeff;0\x07")
            .unwrap();
        assert_eq!(
            terminal.take_shell_integration_event(),
            Some(ShellIntegrationEvent::CommandFinished {
                token,
                exit_status: 0,
                line: LineId(1),
            })
        );
    }

    #[test]
    fn unbound_or_malformed_markers_are_not_lifecycle_events() {
        let mut terminal = TerminalState::new(80, 24).unwrap();
        terminal
            .feed(b"\x1b]133;C\x07\x1b]133;C;short\x07\x1b]133;D;00112233445566778899aabbccddeeff;bad\x07")
            .unwrap();
        assert_eq!(terminal.take_shell_integration_event(), None);
    }

    #[test]
    fn prompt_start_marker_is_a_trusted_shell_event_with_exact_shape() {
        let mut terminal = TerminalState::new(80, 24).unwrap();
        let token = ShellIntegrationToken::from_bytes([0xab; 16]);
        terminal
            .feed(b"\x1b]133;A;abababababababababababababababab\x07")
            .unwrap();
        assert_eq!(
            terminal.take_shell_integration_event(),
            Some(ShellIntegrationEvent::PromptStarted { token })
        );
        // Shapes outside A;<32hex> / C;<32hex> / D;<32hex>;<i32> stay deferred.
        terminal
            .feed(b"\x1b]133;A\x07\x1b]133;A;abababababababababababababababab;extra\x07\x1b]133;C;abababababababababababababababab;pwd\x07\x1b]133;B;abababababababababababababababab\x07")
            .unwrap();
        assert_eq!(terminal.take_shell_integration_event(), None);
    }

    #[test]
    fn primary_history_range_returns_scrolled_rows_by_line_id() {
        let mut terminal = TerminalState::new(4, 2).unwrap();
        terminal.feed(b"one\r\ntwo\r\nthree").unwrap();
        let first = terminal.line_id(0).unwrap();
        let last = terminal.line_id(1).unwrap();
        let rows = terminal.primary_history_range(LineId(1), last, 8).unwrap();
        assert_eq!(rows.first().map(|(id, _)| *id), Some(LineId(1)));
        assert!(rows.iter().any(|(_, cells)| {
            cells
                .iter()
                .map(|cell| cell.character)
                .collect::<String>()
                .starts_with("one")
        }));
        assert!(terminal
            .primary_history_range(first, last, 0)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn primary_history_range_remains_available_during_alternate_screen() {
        let mut terminal = TerminalState::new(4, 2).unwrap();
        terminal.feed(b"one\r\ntwo\r\nthree").unwrap();
        let last = terminal.line_id(1).unwrap();
        let before = terminal.primary_history_range(LineId(1), last, 8).unwrap();
        assert!(!before.is_empty());
        terminal.feed(b"\x1b[?1049h").unwrap();
        assert!(terminal.modes().alternate_screen);
        let during = terminal.primary_history_range(LineId(1), last, 8).unwrap();
        assert_eq!(
            during.len(),
            before.len(),
            "primary history must remain readable while alternate screen is active"
        );
        assert_eq!(
            during
                .iter()
                .map(|(id, cells)| (*id, cells.iter().map(|c| c.character).collect::<String>()))
                .collect::<Vec<_>>(),
            before
                .iter()
                .map(|(id, cells)| (*id, cells.iter().map(|c| c.character).collect::<String>()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn retained_history_records_softwrap_and_hardbreak_lineage() {
        let mut terminal = TerminalState::new(2, 1).unwrap();
        terminal.feed(b"ab").unwrap();
        terminal.feed(b"c\n").unwrap();
        let history: Vec<_> = terminal
            .core
            .primary
            .history_entries()
            .map(|entry| {
                (
                    entry.line_id(),
                    entry.break_after(),
                    entry
                        .presentation_cells()
                        .iter()
                        .map(|cell| cell.character)
                        .collect::<String>(),
                )
            })
            .collect();

        assert!(
            history.iter().any(|(_, break_after, text)| {
                matches!(break_after, crate::HistoryBreakAfter::SoftWrap) && text == "ab"
            }),
            "soft-wrapped overflow row should be retained with SoftWrap lineage"
        );
        assert!(
            history.iter().any(|(_, break_after, text)| {
                matches!(break_after, crate::HistoryBreakAfter::HardBreak) && text == "c"
            }),
            "explicit line feed should be retained with HardBreak lineage and no viewport padding"
        );
    }

    #[test]
    fn history_projection_omits_viewport_padding_but_keeps_explicit_spaces() {
        let mut terminal = TerminalState::new(4, 1).unwrap();
        terminal.feed(b"a b\r\n").unwrap();

        let units = terminal.primary_history_units_range(LineId(1), LineId(1), 8);
        assert_eq!(
            units
                .iter()
                .map(|unit| unit.text.as_str())
                .collect::<String>(),
            "a b"
        );
        assert_eq!(
            terminal
                .primary_history_range(LineId(1), LineId(1), 8)
                .unwrap()[0]
                .1
                .len(),
            3
        );
    }

    #[test]
    fn primary_resize_reflows_active_soft_wrapped_source() {
        let mut terminal = TerminalState::new(4, 2).unwrap();
        terminal.feed(b"abcdef").unwrap();
        terminal.resize(8, 2).unwrap();
        assert_eq!(terminal.row_text(0).as_deref(), Some("abcdef  "));
        assert_eq!(
            terminal.primary_row_break_after(0),
            Some(crate::HistoryBreakAfter::HardBreak),
            "resize must carry the source boundary onto the materialized row"
        );
        terminal.resize(3, 2).unwrap();
        let text = format!(
            "{}{}",
            terminal.row_text(0).unwrap(),
            terminal.row_text(1).unwrap()
        );
        assert!(
            text.starts_with("abc"),
            "rows={:?} reflow text={text:?}",
            (terminal.row_text(0), terminal.row_text(1))
        );
        assert!(text.contains("def"), "reflow text={text:?}");
    }

    #[test]
    fn alternate_screen_never_adds_primary_history() {
        let mut terminal = TerminalState::new(4, 1).unwrap();
        terminal.feed(b"primary\r\n").unwrap();
        let before = terminal.primary_history_resident_bytes();
        terminal
            .feed(b"\x1b[?1049halternate\r\nalternate\r\n\x1b[?1049l")
            .unwrap();
        assert_eq!(terminal.primary_history_resident_bytes(), before);
        assert!(terminal.primary_history_eviction_generation() == 0);
    }

    #[test]
    fn source_breaks_stay_bounded_to_active_lines_after_long_output() {
        let mut terminal = TerminalState::new(8, 2).unwrap();
        for i in 0..200 {
            terminal.feed(format!("line-{i}\r\n").as_bytes()).unwrap();
        }
        assert!(
            terminal.primary_source_break_count() <= 4,
            "source_breaks leaked retained lineage metadata: {}",
            terminal.primary_source_break_count()
        );
    }

    #[test]
    fn retained_unit_preserves_canonical_multiscalar_payload_and_anchor() {
        let mut terminal = TerminalState::new(2, 1).unwrap();
        terminal.feed("界\u{301}\r\n".as_bytes()).unwrap();
        let line_id = terminal
            .primary_history_units_range(LineId(1), LineId(u64::MAX), 1)
            .into_iter()
            .next()
            .expect("wide source row is retained")
            .anchor
            .line_id;
        let unit = terminal.primary_history_unit(HistoryAnchor {
            line_id,
            unit_offset: 0,
        });
        assert!(matches!(
            unit,
            HistoryAnchorResolution::Resolved { ref text, width: 2, .. }
                if text == "界\u{301}"
        ));

        let projected = terminal.primary_history_units_range(line_id, line_id, 8);
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].anchor.unit_offset, 0);
        assert_eq!(projected[0].text, "界\u{301}");
        assert_eq!(projected[0].width, 2);
    }

    #[test]
    fn history_reflow_is_invariant_under_input_chunking() {
        let input = "alpha界\u{301}xyz\r\nsecond-line\r\nthird";
        let mut one_shot = TerminalState::new(6, 1).unwrap();
        one_shot.feed(input.as_bytes()).unwrap();
        let mut bytewise = TerminalState::new(6, 1).unwrap();
        for byte in input.as_bytes() {
            bytewise.feed(std::slice::from_ref(byte)).unwrap();
        }
        assert_eq!(
            one_shot.primary_history_reflow(5, 64),
            bytewise.primary_history_reflow(5, 64)
        );
    }

    #[test]
    fn prepare_resize_leaves_geometry_and_damage_unchanged_until_commit() {
        let mut terminal = TerminalState::new(4, 2).unwrap();
        let _ = terminal.take_damage();
        let generation = terminal.damage_generation();
        let prepared = terminal.prepare_resize(8, 4).expect("prepare");
        assert_eq!((terminal.cols(), terminal.rows()), (4, 2));
        assert_eq!(terminal.damage_generation(), generation);
        assert!(terminal.take_damage().is_none());

        terminal.commit_resize(prepared);
        assert_eq!((terminal.cols(), terminal.rows()), (8, 4));
        let damage = terminal.take_damage().expect("damage after commit");
        assert!(damage.full);
        assert!(terminal.damage_generation() > generation);
    }

    #[test]
    fn dsr_cpr_emits_ordered_replies_with_chunk_equivalence() {
        let mut one_shot = TerminalState::new(80, 24).unwrap();
        one_shot.feed(b"\x1b[10;20H\x1b[6n").unwrap();
        let expected = one_shot.take_protocol_reply().expect("cpr reply");
        assert_eq!(expected.as_bytes(), b"\x1b[10;20R");
        assert!(one_shot.take_protocol_reply().is_none());

        let mut chunked = TerminalState::new(80, 24).unwrap();
        for byte in b"\x1b[10;20H\x1b[6n" {
            chunked.feed(std::slice::from_ref(byte)).unwrap();
        }
        assert_eq!(
            chunked.take_protocol_reply().map(|r| r.as_bytes().to_vec()),
            Some(expected.as_bytes().to_vec())
        );
    }

    #[test]
    fn multiple_queries_preserve_reply_order_and_bound() {
        let mut terminal = TerminalState::new(80, 24).unwrap();
        terminal.feed(b"\x1b[1;1H\x1b[6n\x1b[2;3H\x1b[6n").unwrap();
        assert_eq!(
            terminal.take_protocol_reply().unwrap().as_bytes(),
            b"\x1b[1;1R"
        );
        assert_eq!(
            terminal.take_protocol_reply().unwrap().as_bytes(),
            b"\x1b[2;3R"
        );

        let mut flood = TerminalState::new(80, 24).unwrap();
        let before = flood.diagnostics().deferred_sequences;
        for _ in 0..(MAX_PROTOCOL_REPLIES + 4) {
            flood.feed(b"\x1b[6n").unwrap();
        }
        let mut drained = 0usize;
        while flood.take_protocol_reply().is_some() {
            drained += 1;
        }
        assert_eq!(drained, MAX_PROTOCOL_REPLIES);
        assert!(flood.diagnostics().deferred_sequences > before);
    }

    #[test]
    fn decrqm_mode_25_and_unknown_queries_are_safe() {
        let mut terminal = TerminalState::new(80, 24).unwrap();
        terminal.feed(b"\x1b[?25l\x1b[?25$p").unwrap();
        assert_eq!(
            terminal.take_protocol_reply().unwrap().as_bytes(),
            b"\x1b[?25;2$y"
        );

        // Mode 2027 defaults to set and is queryable (SPEC-011 §3).
        terminal.feed(b"\x1b[?2027$p").unwrap();
        assert_eq!(
            terminal.take_protocol_reply().unwrap().as_bytes(),
            b"\x1b[?2027;1$y"
        );

        let deferred_before = terminal.diagnostics().deferred_sequences;
        terminal.feed(b"\x1b[0n\x1b[?999$p").unwrap();
        assert!(terminal.take_protocol_reply().is_none());
        assert!(terminal.diagnostics().deferred_sequences > deferred_before);
    }

    #[test]
    fn replies_enqueued_before_feed_fault_remain_takeable() {
        let mut terminal = TerminalState::new(2, 1).expect("valid terminal");
        terminal.core.line_ids = LineIdAllocator::with_next(Some(u64::MAX));
        terminal
            .feed(b"A\r\n")
            .expect("consume final available line id");
        assert_eq!(
            terminal.feed(b"\x1b[6n\r\n"),
            Err(TerminalError::LineIdentityExhausted)
        );
        assert_eq!(
            terminal.take_protocol_reply().unwrap().as_bytes(),
            b"\x1b[1;1R"
        );
        assert!(terminal.take_protocol_reply().is_none());
    }

    #[test]
    fn huge_sparse_history_span_is_bounded_by_retained_storage() {
        use std::time::{Duration, Instant};

        let mut terminal = TerminalState::new(4, 2).unwrap();
        terminal.feed(b"one\r\ntwo\r\nthree\r\nfour").unwrap();
        // Alternate screen burns LineIds, creating gaps in primary identity space.
        terminal.feed(b"\x1b[?1049h\x1b[?1049l").unwrap();
        terminal.feed(b"five\r\nsix").unwrap();

        let started = Instant::now();
        let rows = terminal
            .primary_history_range(LineId(1), LineId(u64::MAX), 512)
            .unwrap();
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "history lookup must not scale with numeric LineId distance"
        );
        assert!(rows.len() <= 512);
        assert!(!rows.is_empty());

        let absent = terminal
            .primary_history_range(LineId(u64::MAX - 10), LineId(u64::MAX), 8)
            .unwrap();
        assert!(absent.is_empty());
    }

    #[test]
    fn oversized_geometry_is_rejected_without_mutating_state() {
        assert!(matches!(
            TerminalState::new(MAX_TERMINAL_COLUMNS, MAX_TERMINAL_ROWS + 1),
            Err(TerminalError::InvalidSize)
        ));
        assert!(matches!(
            TerminalState::new(MAX_TERMINAL_COLUMNS + 1, MAX_TERMINAL_ROWS),
            Err(TerminalError::InvalidSize)
        ));
        let mut terminal = TerminalState::new(80, 24).unwrap();
        let generation = terminal.damage_generation();
        assert!(matches!(
            terminal.prepare_resize(u16::MAX, u16::MAX),
            Err(TerminalError::InvalidSize)
        ));
        assert_eq!((terminal.cols(), terminal.rows()), (80, 24));
        assert_eq!(terminal.damage_generation(), generation);
        terminal
            .prepare_resize(MAX_TERMINAL_COLUMNS, MAX_TERMINAL_ROWS)
            .expect("max accepted geometry prepares");
    }
}
