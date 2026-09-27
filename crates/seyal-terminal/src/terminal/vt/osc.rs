//! OSC and host-presentation / shell-integration dispatch for TerminalCore.

use super::super::state::{ShellIntegrationEvent, ShellIntegrationToken};
use super::TerminalCore;
use crate::presentation::{parse_osc_presentation, HostPresentationEvent};
use crate::LineId;

impl TerminalCore {
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

    fn enqueue_presentation(&mut self, event: HostPresentationEvent) {
        if self.presentation_events.len() == self.presentation_events.capacity() {
            self.record_deferred();
            return;
        }
        self.presentation_events.push_back(event);
    }

    pub(super) fn dispatch_osc(&mut self, bytes: &[u8], truncated: bool) {
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
}
