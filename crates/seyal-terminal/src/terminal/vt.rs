//! VT performer: TerminalCore, Actions dispatch, print/execute/CSI/OSC.

use super::state::{Diagnostics, ShellIntegrationEvent, ShellIntegrationToken};
use crate::{
    active_grapheme::{
        append_payload, build_lead_cell, edge_decision_late_widen, edge_decision_new_unit,
        try_append_scalar, ActiveGrapheme, EdgeDecision,
    },
    damage::{DamageTracker, Mutation},
    grapheme_store::GraphemeStore,
    line::LineIdAllocator,
    parser::Actions,
    presentation::{parse_osc_presentation, HostPresentationEvent, MAX_HOST_PRESENTATION_EVENTS},
    protocol_reply::{
        encode_decrqm_private, encode_dsr_cpr, encode_kitty_flags, encode_primary_da,
        ProtocolReply, MAX_PROTOCOL_REPLIES,
    },
    screen::Screen,
    selection::{CopyMode, SearchSession, SelectionSession},
    width::{grapheme_terminal_width, AmbiguousWidthPolicy},
    LineId, ModeState, MouseReporting, TerminalError,
};
use std::collections::VecDeque;

pub(super) const KEYBOARD_STACK_CAPACITY: usize = 16;
pub(super) const KEYBOARD_FLAGS_MASK: u8 = 0b11;

pub(super) struct TerminalCore {
    pub(super) primary: Screen,
    pub(super) alternate: Option<Screen>,
    pub(super) line_ids: LineIdAllocator,
    pub(super) modes: ModeState,
    pub(super) damage: DamageTracker,
    pub(super) diagnostics: Diagnostics,
    pub(super) fault: Option<TerminalError>,
    pub(super) shell_events: VecDeque<ShellIntegrationEvent>,
    pub(super) presentation_events: VecDeque<HostPresentationEvent>,
    pub(super) protocol_replies: VecDeque<ProtocolReply>,
    pub(super) primary_keyboard_flags: u8,
    pub(super) alternate_keyboard_flags: u8,
    pub(super) primary_keyboard_stack: [u8; KEYBOARD_STACK_CAPACITY],
    pub(super) primary_keyboard_stack_len: usize,
    pub(super) alternate_keyboard_stack: [u8; KEYBOARD_STACK_CAPACITY],
    pub(super) alternate_keyboard_stack_len: usize,
    pub(super) grapheme_store: GraphemeStore,
    pub(super) active_grapheme: Option<ActiveGrapheme>,
    pub(super) ambiguous_width: AmbiguousWidthPolicy,
    pub(super) selection: SelectionSession,
    pub(super) search: SearchSession,
    pub(super) copy_mode: CopyMode,
    pub(super) copy_buffer: Option<String>,
}

impl TerminalCore {
    pub(super) fn new(cols: u16, rows: u16) -> Result<Self, TerminalError> {
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

    pub(super) fn invalidate_active_grapheme(&mut self) {
        self.active_grapheme = None;
    }

    pub(super) fn current(&self) -> &Screen {
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
    /// trusted `D` marker is recognized. A cursor at column 0 means the
    /// output ended with a newline (or zsh's `PROMPT_SP` already moved to a
    /// fresh row, filling it with spaces before `precmd` emits `D`), so the
    /// cursor's row is where the next prompt will be drawn, not output: use
    /// the row before it. A cursor past column 0 is still on the last output
    /// row (no trailing newline). The row's content is not a usable signal:
    /// `PROMPT_SP` writes spaces into it. A command with no output backs up
    /// before its start line; Runtime clamps that (#1015).
    fn completion_line(&self) -> LineId {
        let cursor = self.current().cursor(self.modes.cursor_visible);
        let screen = self.current();
        let row = if cursor.col == 0 && cursor.row > 0 {
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

    pub(super) fn apply(&mut self, mutation: Mutation) {
        self.damage.mark(mutation);
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

pub(super) fn param_one(params: &[u16], index: usize) -> u16 {
    match params.get(index).copied().unwrap_or(0) {
        0 => 1,
        value => value,
    }
}

pub(super) fn param_zero(params: &[u16], index: usize) -> u16 {
    params.get(index).copied().unwrap_or(0)
}
