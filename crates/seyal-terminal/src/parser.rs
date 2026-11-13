const MAX_PARAMS: usize = 32;
const MAX_UTF8: usize = 4;
const MAX_OSC_BYTES: usize = 4096;

pub(crate) trait Actions {
    fn print(&mut self, character: char);
    fn execute(&mut self, byte: u8);
    fn csi(&mut self, params: &[u16], private: Option<u8>, ignored: bool, final_byte: u8);
    fn esc(&mut self, final_byte: u8, had_intermediate: bool);
    fn osc(&mut self, bytes: &[u8], truncated: bool);
    fn deferred_string(&mut self);
    fn malformed(&mut self);
    /// Finding F / D3: Runtime should observe attribution before more bytes.
    fn should_yield_for_attribution(&self) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Ground,
    Escape,
    EscapeIntermediate,
    Csi,
    Osc,
    OscEscape,
    IgnoredString,
    IgnoredStringEscape,
}

pub(crate) struct Parser {
    state: State,
    params: [u16; MAX_PARAMS],
    param_len: usize,
    param_acc: u32,
    has_param_acc: bool,
    private: Option<u8>,
    ignored: bool,
    utf8: [u8; MAX_UTF8],
    utf8_len: u8,
    utf8_needed: u8,
    putback: Option<u8>,
    osc: [u8; MAX_OSC_BYTES],
    osc_len: usize,
    osc_truncated: bool,
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser {
    pub(crate) fn new() -> Self {
        Self {
            state: State::Ground,
            params: [0; MAX_PARAMS],
            param_len: 0,
            param_acc: 0,
            has_param_acc: false,
            private: None,
            ignored: false,
            utf8: [0; MAX_UTF8],
            utf8_len: 0,
            utf8_needed: 0,
            putback: None,
            osc: [0; MAX_OSC_BYTES],
            osc_len: 0,
            osc_truncated: false,
        }
    }

    /// Feed until Ground after an attribution yield request, or until `bytes` ends.
    /// Returns the number of input bytes consumed (putback is not counted twice).
    pub(crate) fn feed_until_attribution_yield(
        &mut self,
        bytes: &[u8],
        actions: &mut impl Actions,
    ) -> usize {
        let mut index = 0;
        while index < bytes.len() || self.putback.is_some() {
            let from_putback = self.putback.is_some();
            let byte = match self.putback.take() {
                Some(byte) => byte,
                None => {
                    let byte = bytes[index];
                    index += 1;
                    byte
                }
            };
            self.step(byte, actions);
            if actions.should_yield_for_attribution()
                && self.state == State::Ground
                && self.putback.is_none()
                && self.utf8_needed == 0
            {
                break;
            }
            let _ = from_putback;
        }
        index
    }

    pub(crate) fn finish(&mut self, actions: &mut impl Actions) {
        if self.utf8_needed != 0 {
            self.abort_utf8(actions);
        }
        if self.state != State::Ground {
            actions.malformed();
        }
        self.state = State::Ground;
        self.putback = None;
        self.reset_sequence();
    }

    fn step(&mut self, byte: u8, actions: &mut impl Actions) {
        match self.state {
            State::Ground => self.ground(byte, actions),
            State::Escape => self.escape(byte, actions),
            State::EscapeIntermediate => self.escape_intermediate(byte, actions),
            State::Csi => self.csi_byte(byte, actions),
            State::Osc => self.osc(byte, actions),
            State::OscEscape => self.osc_escape(byte, actions),
            State::IgnoredString => self.ignored_string(byte, actions),
            State::IgnoredStringEscape => self.ignored_string_escape(byte, actions),
        }
    }

    fn ground(&mut self, byte: u8, actions: &mut impl Actions) {
        if self.utf8_needed != 0 {
            if (0x80..=0xbf).contains(&byte) {
                self.utf8[usize::from(self.utf8_len)] = byte;
                self.utf8_len += 1;
                if self.utf8_len == self.utf8_needed {
                    self.finish_utf8(actions);
                }
                return;
            }
            self.abort_utf8(actions);
            self.putback = Some(byte);
            return;
        }

        match byte {
            0x00..=0x17 | 0x19 | 0x1c..=0x1f => actions.execute(byte),
            0x18 | 0x1a => actions.execute(byte),
            0x1b => {
                self.reset_sequence();
                self.state = State::Escape;
            }
            0x20..=0x7e => actions.print(char::from(byte)),
            0x7f => {}
            0xc2..=0xdf => self.start_utf8(byte, 2),
            0xe0..=0xef => self.start_utf8(byte, 3),
            0xf0..=0xf4 => self.start_utf8(byte, 4),
            _ => self.replacement(actions),
        }
    }

    fn escape(&mut self, byte: u8, actions: &mut impl Actions) {
        match byte {
            0x00..=0x17 | 0x19 | 0x1c..=0x1f => actions.execute(byte),
            0x18 | 0x1a => {
                self.state = State::Ground;
                actions.execute(byte);
            }
            0x1b => self.reset_sequence(),
            b'[' => {
                self.reset_sequence();
                self.state = State::Csi;
            }
            b']' => {
                self.reset_sequence();
                self.reset_osc();
                self.state = State::Osc;
            }
            b'P' | b'X' | b'^' | b'_' => {
                self.reset_sequence();
                self.state = State::IgnoredString;
            }
            0x20..=0x2f => self.state = State::EscapeIntermediate,
            0x30..=0x7e => {
                actions.esc(byte, false);
                self.state = State::Ground;
                self.reset_sequence();
            }
            _ => {
                actions.malformed();
                self.state = State::Ground;
                self.reset_sequence();
            }
        }
    }

    fn escape_intermediate(&mut self, byte: u8, actions: &mut impl Actions) {
        match byte {
            0x00..=0x17 | 0x19 | 0x1c..=0x1f => actions.execute(byte),
            0x18 | 0x1a => {
                self.state = State::Ground;
                actions.execute(byte);
            }
            0x1b => {
                self.reset_sequence();
                self.state = State::Escape;
            }
            0x20..=0x2f => {}
            0x30..=0x7e => {
                actions.esc(byte, true);
                self.state = State::Ground;
                self.reset_sequence();
            }
            _ => {
                actions.malformed();
                self.state = State::Ground;
                self.reset_sequence();
            }
        }
    }

    fn csi_byte(&mut self, byte: u8, actions: &mut impl Actions) {
        match byte {
            0x00..=0x17 | 0x19 | 0x1c..=0x1f => actions.execute(byte),
            0x18 | 0x1a => {
                self.state = State::Ground;
                actions.execute(byte);
                self.reset_sequence();
            }
            0x1b => {
                self.reset_sequence();
                self.state = State::Escape;
            }
            b'0'..=b'9' => self.push_digit(byte - b'0'),
            b';' => self.finish_param(),
            b':' => self.ignored = true,
            0x3c..=0x3f if self.param_len == 0 && !self.has_param_acc && self.private.is_none() => {
                self.private = Some(byte);
            }
            0x20..=0x2f | 0x3c..=0x3f => self.ignored = true,
            0x40..=0x7e => {
                if self.has_param_acc || self.param_len != 0 {
                    self.finish_param();
                }
                actions.csi(
                    &self.params[..self.param_len],
                    self.private,
                    self.ignored,
                    byte,
                );
                self.state = State::Ground;
                self.reset_sequence();
            }
            0x7f => {}
            _ => self.ignored = true,
        }
    }

    fn osc(&mut self, byte: u8, actions: &mut impl Actions) {
        match byte {
            0x07 => {
                actions.osc(&self.osc[..self.osc_len], self.osc_truncated);
                self.state = State::Ground;
                self.reset_osc();
            }
            0x18 | 0x1a => {
                self.state = State::Ground;
                actions.execute(byte);
            }
            0x1b => self.state = State::OscEscape,
            _ => self.push_osc(byte),
        }
    }

    fn osc_escape(&mut self, byte: u8, actions: &mut impl Actions) {
        if byte == b'\\' {
            actions.osc(&self.osc[..self.osc_len], self.osc_truncated);
            self.state = State::Ground;
            self.reset_osc();
        } else {
            self.state = State::Escape;
            self.reset_sequence();
            self.putback = Some(byte);
        }
    }

    fn ignored_string(&mut self, byte: u8, actions: &mut impl Actions) {
        match byte {
            0x18 | 0x1a => {
                self.state = State::Ground;
                actions.execute(byte);
            }
            0x1b => self.state = State::IgnoredStringEscape,
            _ => {}
        }
    }

    fn ignored_string_escape(&mut self, byte: u8, actions: &mut impl Actions) {
        if byte == b'\\' {
            actions.deferred_string();
            self.state = State::Ground;
        } else if byte == 0x1b {
            self.state = State::IgnoredStringEscape;
        } else {
            self.state = State::IgnoredString;
        }
    }

    fn start_utf8(&mut self, lead: u8, needed: u8) {
        self.utf8 = [0; MAX_UTF8];
        self.utf8[0] = lead;
        self.utf8_len = 1;
        self.utf8_needed = needed;
    }

    fn reset_osc(&mut self) {
        self.osc_len = 0;
        self.osc_truncated = false;
    }

    fn push_osc(&mut self, byte: u8) {
        if self.osc_len < MAX_OSC_BYTES {
            self.osc[self.osc_len] = byte;
            self.osc_len += 1;
        } else {
            self.osc_truncated = true;
        }
    }

    fn finish_utf8(&mut self, actions: &mut impl Actions) {
        let len = usize::from(self.utf8_needed);
        let decoded = std::str::from_utf8(&self.utf8[..len])
            .ok()
            .and_then(|text| text.chars().next());
        self.utf8_len = 0;
        self.utf8_needed = 0;
        match decoded {
            Some(character) => actions.print(character),
            None => self.replacement(actions),
        }
    }

    fn abort_utf8(&mut self, actions: &mut impl Actions) {
        self.utf8_len = 0;
        self.utf8_needed = 0;
        self.replacement(actions);
    }

    fn replacement(&self, actions: &mut impl Actions) {
        actions.malformed();
        actions.print('\u{fffd}');
    }

    fn push_digit(&mut self, digit: u8) {
        self.has_param_acc = true;
        self.param_acc = self
            .param_acc
            .saturating_mul(10)
            .saturating_add(u32::from(digit));
        if self.param_acc > u32::from(u16::MAX) {
            self.ignored = true;
        }
    }

    fn finish_param(&mut self) {
        if self.param_len == MAX_PARAMS {
            self.ignored = true;
            self.param_acc = 0;
            self.has_param_acc = false;
            return;
        }
        self.params[self.param_len] = self.param_acc.min(u32::from(u16::MAX)) as u16;
        self.param_len += 1;
        self.param_acc = 0;
        self.has_param_acc = false;
    }

    fn reset_sequence(&mut self) {
        self.params = [0; MAX_PARAMS];
        self.param_len = 0;
        self.param_acc = 0;
        self.has_param_acc = false;
        self.private = None;
        self.ignored = false;
    }
}
