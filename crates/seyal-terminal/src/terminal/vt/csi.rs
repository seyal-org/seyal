//! CSI / ESC dispatch, keyboard protocol replies, and private-mode helpers.

use super::{TerminalCore, KEYBOARD_FLAGS_MASK, KEYBOARD_STACK_CAPACITY};
use crate::{
    damage::Mutation,
    protocol_reply::{
        encode_decrqm_private, encode_dsr_cpr, encode_kitty_flags, encode_primary_da, ProtocolReply,
    },
    MouseReporting,
};

impl TerminalCore {
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

    pub(super) fn sync_keyboard_flags(&mut self) {
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

    pub(super) fn dispatch_csi(
        &mut self,
        params: &[u16],
        private: Option<u8>,
        ignored: bool,
        final_byte: u8,
    ) {
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

    pub(super) fn dispatch_esc(&mut self, final_byte: u8, had_intermediate: bool) {
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
