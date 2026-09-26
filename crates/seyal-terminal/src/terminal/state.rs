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
