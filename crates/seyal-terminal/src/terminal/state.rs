//! TerminalState type, construction, and viewport/accessors.

use super::vt::TerminalCore;
use crate::{
    parser::Parser, presentation::HostPresentationEvent, protocol_reply::ProtocolReply,
    AmbiguousWidthPolicy, Cell, CursorState, Damage, LineId, ModeState, TerminalError,
};

/// Soft ceiling aligned with Candidate-D display maxima. Larger geometries are
/// rejected cheaply so embedders cannot force multi-gigabyte grid allocations.
pub const MAX_TERMINAL_COLUMNS: u16 = 512;
pub const MAX_TERMINAL_ROWS: u16 = 256;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Diagnostics {
    pub deferred_sequences: u64,
    pub unknown_sequences: u64,
    pub malformed_sequences: u64,
    pub grapheme_payload_overflow_count: u64,
    pub grapheme_store_capacity_fallback_count: u64,
}

/// Remote shell named by an ADR-026 readiness marker (`z` or `b`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionShellKind {
    Zsh,
    Bash,
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
    /// OSC `133;R;<K>;<z|b>`. Fourth marker shape (ADR-026 §6.4). It does not
    /// grant trust; Runtime accepts it only while the session overlay is Armed
    /// and `<K>` is the current challenge.
    Readiness {
        token: ShellIntegrationToken,
        shell: SessionShellKind,
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
    pub(super) parser: Parser,
    pub(super) core: TerminalCore,
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
        // Full consumption for callers that do not interleave Runtime observe.
        self.core.attribution_observe_yield = false;
        let mut offset = 0;
        while offset < bytes.len() {
            self.core.attribution_observe_yield = false;
            let consumed = self
                .parser
                .feed_until_attribution_yield(&bytes[offset..], &mut self.core);
            offset = offset.saturating_add(consumed);
            if consumed == 0 {
                break;
            }
            // Clear yield so the next slice continues without a stuck request.
            self.core.attribution_observe_yield = false;
        }
        self.core.damage.commit();
        self.core.fault.map_or(Ok(()), Err)
    }

    /// Feed until a trusted shell marker requests Runtime observe, or until the
    /// buffer ends (#1484 Finding F / D3). Returns bytes consumed.
    pub fn feed_yielding_for_attribution(&mut self, bytes: &[u8]) -> Result<usize, TerminalError> {
        if let Some(error) = self.core.fault {
            return Err(error);
        }
        self.core.attribution_observe_yield = false;
        let consumed = self
            .parser
            .feed_until_attribution_yield(bytes, &mut self.core);
        self.core.damage.commit();
        self.core.fault.map_or(Ok(consumed), Err)
    }

    pub fn clear_attribution_observe_yield(&mut self) {
        self.core.attribution_observe_yield = false;
    }

    pub fn attribution_observe_yield_requested(&self) -> bool {
        self.core.attribution_observe_yield
    }

    pub fn finish_input(&mut self) -> Result<(), TerminalError> {
        if let Some(error) = self.core.fault {
            return Err(error);
        }
        self.parser.finish(&mut self.core);
        self.core.damage.commit();
        self.core.fault.map_or(Ok(()), Err)
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
    pub(super) fn primary_row_break_after(&self, row: u16) -> Option<crate::HistoryBreakAfter> {
        self.core.primary.row_break_after(row)
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

    /// After Runtime accepts a trusted CommandStart, direct subsequent primary
    /// VT writes into a Block-owned output region (ADR-009 native Blocks).
    /// Returns `(header_region, output_region)`.
    pub fn begin_native_block_output(
        &mut self,
        block_id: u64,
    ) -> Result<(u64, u64), TerminalError> {
        use crate::primary_blocks::types::BlockId;
        self.core
            .primary
            .engine
            .begin_block_output(BlockId(block_id))
            .map(|block| (block.header.0, block.output.0))
    }

    /// After CommandFinished / ambiguity, end the innermost Block output region
    /// (restoring a nested outer region when present).
    pub fn end_native_block_output(&mut self) {
        self.core.primary.engine.end_block_output();
    }

    /// Drop a provisional CommandStart arm that never became a Block.
    pub fn cancel_pending_native_output(&mut self) {
        self.core.primary.engine.cancel_pending_output();
    }

    /// Current VT write ownership region (native Block output or unstructured).
    pub fn native_active_region(&self) -> u64 {
        self.core.primary.engine.active_region().0
    }

    /// Drain ordered attribution events for one Runtime observe turn.
    pub fn drain_attribution_journal(&mut self) -> Vec<crate::PrimaryEvent> {
        self.core.primary.engine.journal.drain()
    }

    pub fn attribution_journal_overflowed(&self) -> bool {
        self.core.primary.engine.journal.overflowed()
    }

    /// True when journal overflow must end Block VT ownership (F06 fault-inject
    /// only). Production ring overflow keeps the active Block region (#1484).
    pub fn attribution_journal_fail_closed_overflow(&self) -> bool {
        self.core.primary.engine.journal.fail_closed_on_overflow()
    }

    /// Direct BlockId → RegionId content handles for Flow (no history-range assembly).
    pub fn primary_block_content_refs(&self) -> Vec<crate::BlockContentRef> {
        self.core.primary.engine.block_content_refs()
    }

    /// Live primary-window cells for one RegionId (bounded to current VT window).
    pub fn primary_live_region_cells(&self, region_id: u64) -> Vec<(u16, u16, crate::Cell)> {
        self.core
            .primary
            .engine
            .live_region_cells(crate::RegionId(region_id))
    }

    /// Sealed LineIds owned by one RegionId (HistoryStore holds cell payload).
    pub fn primary_sealed_region_line_ids(&self, region_id: u64) -> Vec<crate::LineId> {
        self.core
            .primary
            .engine
            .sealed_rows(crate::RegionId(region_id))
            .iter()
            .map(|row| row.line_id)
            .collect()
    }

    /// Sealed ownership row count for one RegionId (u32-saturated).
    pub fn primary_sealed_row_count(&self, region_id: u64) -> u32 {
        self.core
            .primary
            .engine
            .sealed_row_count(crate::RegionId(region_id))
    }

    /// Number of sealed ownership rows recorded for one RegionId.
    pub fn primary_sealed_rows_len(&self, region_id: u64) -> usize {
        self.core
            .primary
            .engine
            .sealed_rows(crate::RegionId(region_id))
            .len()
    }

    /// Visit sealed rows owned by one RegionId in ownership order as
    /// `(line_id, owned (col, cell))`; `visit` returns `false` to stop.
    /// Payload comes from HistoryStore (sole sealed owner); each row is a
    /// binary-searched point lookup, never a linear history walk. Compacted
    /// history (Lead units only) maps owned columns by Lead enumeration order.
    pub fn primary_visit_sealed_region_rows(
        &self,
        region_id: u64,
        visit: impl FnMut(crate::LineId, &[(u16, crate::Cell)]) -> bool,
    ) {
        self.primary_visit_sealed_region_rows_from(region_id, 0, visit);
    }

    /// Like [`Self::primary_visit_sealed_region_rows`], starting at sealed-row
    /// index `start_index` so callers can O(1)-jump whole skipped pages (#1484).
    pub fn primary_visit_sealed_region_rows_from(
        &self,
        region_id: u64,
        start_index: usize,
        mut visit: impl FnMut(crate::LineId, &[(u16, crate::Cell)]) -> bool,
    ) {
        use crate::CellRole;
        let history = self.core.primary.history();
        let window_cols = usize::from(self.core.primary.engine.store.window_cols().max(1));
        let mut owned: Vec<(u16, crate::Cell)> = Vec::new();
        let sealed = self
            .core
            .primary
            .engine
            .sealed_rows(crate::RegionId(region_id));
        for row in sealed.iter().skip(start_index) {
            if history.range_intersects_evicted(row.line_id, row.line_id) {
                continue;
            }
            let Some(entry) = history
                .entries_from(row.line_id)
                .next()
                .filter(|entry| entry.line_id() == row.line_id)
            else {
                continue;
            };
            let cells = entry.presentation_cells();
            owned.clear();
            if cells.len() >= window_cols {
                // Keep Empty cells — filtering them dropped whole sealed rows
                // whose owned columns were blank-padded spaces (#1484 MP0).
                owned.extend(
                    row.cols
                        .iter()
                        .filter_map(|&col| cells.get(usize::from(col)).map(|cell| (col, *cell))),
                );
            } else {
                let leads = cells.iter().filter(|cell| cell.role == CellRole::Lead);
                owned.extend(row.cols.iter().zip(leads).map(|(&col, cell)| (col, *cell)));
            }
            if !owned.is_empty() && !visit(row.line_id, &owned) {
                return;
            }
        }
    }

    /// Sealed cells for one RegionId as `(line_id, col, cell)`.
    ///
    /// Payload comes from HistoryStore (sole sealed owner). Column filter uses
    /// the RegionGrid ownership index recorded at seal time; when the stored
    /// history presentation is compacted (Lead units only), owned columns are
    /// matched by enumeration order among Lead cells.
    pub fn primary_sealed_region_cells(
        &self,
        region_id: u64,
    ) -> Vec<(crate::LineId, u16, crate::Cell)> {
        let mut out = Vec::new();
        self.primary_visit_sealed_region_rows(region_id, |line_id, owned| {
            out.extend(owned.iter().map(|&(col, cell)| (line_id, col, cell)));
            true
        });
        out
    }

    /// Indexed Flow viewport: Blocks intersecting `[y, y + window_rows)`.
    pub fn primary_visible_block_spans(
        &self,
        y: u64,
        window_rows: u32,
    ) -> Vec<crate::VisibleBlockSpan> {
        self.core.primary.engine.visible_block_spans(y, window_rows)
    }

    /// Total presentation-row height of the indexed Block list.
    pub fn primary_viewport_total_height(&self) -> u64 {
        self.core.primary.engine.viewport_total_height()
    }

    pub fn primary_viewport_block_height(&self, block_id: u64) -> u32 {
        self.core
            .primary
            .engine
            .viewport_height_of(crate::BlockId(block_id))
    }

    /// Contiguous live-window output spans for each Block (row_count 0 = off-window).
    pub fn primary_on_window_region_spans(&self) -> Vec<crate::OnWindowRegionSpan> {
        self.core.primary.engine.on_window_region_spans()
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
