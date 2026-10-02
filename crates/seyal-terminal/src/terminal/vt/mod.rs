//! VT performer: one `TerminalCore` authority with Actions dispatch.
//!
//! Sibling modules add `impl TerminalCore` blocks by responsibility (print,
//! execute, CSI/ESC, OSC). There is still exactly one VT/state engine.

mod csi;
mod execute;
mod osc;
mod print;

use super::state::{Diagnostics, ShellIntegrationEvent};
use crate::{
    active_grapheme::ActiveGrapheme,
    damage::{DamageTracker, Mutation},
    grapheme_store::GraphemeStore,
    line::LineIdAllocator,
    parser::Actions,
    presentation::{HostPresentationEvent, MAX_HOST_PRESENTATION_EVENTS},
    protocol_reply::{ProtocolReply, MAX_PROTOCOL_REPLIES},
    screen::Screen,
    selection::{CopyMode, SearchSession, SelectionSession},
    width::AmbiguousWidthPolicy,
    ModeState, TerminalError,
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
        self.dispatch_csi(params, private, ignored, final_byte);
    }

    fn esc(&mut self, final_byte: u8, had_intermediate: bool) {
        self.dispatch_esc(final_byte, had_intermediate);
    }

    fn osc(&mut self, bytes: &[u8], truncated: bool) {
        self.dispatch_osc(bytes, truncated);
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
