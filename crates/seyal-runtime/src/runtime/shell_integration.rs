//! Trusted zsh shell integration inside Runtime (ADR-009, 2026-09-16
//! amendment): composer admission, the submission byte contract, and the
//! application of trusted markers and Runtime events to the per-execution
//! integration state. Hooks are installed at spawn by
//! `ShellIntegrationPolicy`; nothing here writes instrumentation to the PTY.

#[cfg(target_os = "macos")]
use seyal_exec::{CommandSpec, ShellIntegrationEvent};

#[cfg(target_os = "macos")]
use crate::command_block_timeline::{CommandBlockId, MAX_COMMAND_BYTES};
#[cfg(target_os = "macos")]
use crate::input::InputKind;
use crate::{ExecutionId, RuntimeError};

#[cfg(target_os = "macos")]
use super::entry::{Entry, PendingComposerCommand};
#[cfg(target_os = "macos")]
use super::integration_state::{BlockExit, Effect, IntegrationEvent, IntegrationState};
use super::Runtime;
#[cfg(target_os = "macos")]
use crate::local_ipc::framing::ComposerEligibility;
#[cfg(target_os = "macos")]
use crate::ShellIntegrationPolicy;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(target_os = "macos")]
pub(super) enum ShellIntegrationMode {
    ZshHook,
    Unsupported,
}

#[cfg(target_os = "macos")]
pub(super) fn shell_integration_mode(command: &CommandSpec) -> ShellIntegrationMode {
    if ShellIntegrationPolicy::supports(command) {
        ShellIntegrationMode::ZshHook
    } else {
        ShellIntegrationMode::Unsupported
    }
}

/// Result of one composer admission attempt. `Busy`, `Unsupported` and
/// `Invalid` are correlated application results, not transport failures, so
/// the Pane keeps its draft and remains connected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(target_os = "macos")]
pub(crate) enum ComposerAdmission {
    Accepted(CommandBlockId),
    /// The shell has not announced a clean prompt, input is still unwritten,
    /// a submission is in flight, or a Block is running.
    Busy,
    /// The shell never proves trusted integration; bytes went the raw way.
    Unsupported,
    /// The text cannot be sent as one command line (multi-line without
    /// bracketed paste); nothing was written.
    Invalid,
}

#[cfg(target_os = "macos")]
const BRACKET_PASTE_START: &[u8] = b"\x1b[200~";
#[cfg(target_os = "macos")]
const BRACKET_PASTE_END: &[u8] = b"\x1b[201~";

/// ADR-009 mechanism 4: exactly the bytes a person would produce at that
/// prompt. Single-line text is raw; text containing a line break is sent
/// only as a bracketed paste, and only when the shell enabled DECSET 2004.
#[cfg(target_os = "macos")]
fn encode_submission(command: &str, bracketed_paste: bool) -> Option<Vec<u8>> {
    let multi_line = command.bytes().any(|byte| byte == b'\n' || byte == b'\r');
    if !multi_line {
        let mut bytes = Vec::with_capacity(command.len() + 1);
        bytes.extend_from_slice(command.as_bytes());
        bytes.push(b'\r');
        return Some(bytes);
    }
    if !bracketed_paste {
        return None;
    }
    let mut bytes =
        Vec::with_capacity(BRACKET_PASTE_START.len() + command.len() + BRACKET_PASTE_END.len() + 1);
    bytes.extend_from_slice(BRACKET_PASTE_START);
    bytes.extend_from_slice(command.as_bytes());
    bytes.extend_from_slice(BRACKET_PASTE_END);
    bytes.push(b'\r');
    Some(bytes)
}

impl Runtime {
    /// Admit one complete Pane-composer command. This deliberately uses a
    /// distinct Runtime operation from raw terminal input: only a trusted
    /// OSC-133 start event can turn this pending metadata into a Running Block.
    #[cfg(target_os = "macos")]
    pub(crate) fn submit_composer_command(
        &mut self,
        id: ExecutionId,
        command: String,
    ) -> Result<ComposerAdmission, RuntimeError> {
        if command.is_empty() || command.len() > MAX_COMMAND_BYTES {
            return Err(RuntimeError::CapacityExceeded);
        }
        let entry = self
            .entries
            .get(&id)
            .ok_or(RuntimeError::UnknownExecution)?;
        if !entry.terminal_io_active() {
            return Ok(ComposerAdmission::Busy);
        }
        if entry.shell_integration_mode == ShellIntegrationMode::Unsupported {
            // Unsupported shells remain fully usable through the ordinary raw
            // PTY path, but never receive synthetic Block metadata.
            let mut bytes = Vec::with_capacity(command.len() + 1);
            bytes.extend_from_slice(command.as_bytes());
            bytes.push(b'\r');
            self.input_ingress(id)?
                .try_submit_kind(bytes, InputKind::Direct)?;
            return Ok(ComposerAdmission::Unsupported);
        }
        let modes = entry.execution.terminal().modes();
        // Admission-time prompt gating: the shell announced a prompt, nothing
        // was admitted since, and no accepted bytes are still unwritten.
        let eligible = entry.integration.composer_eligible()
            && !modes.alternate_screen
            && entry.pending_composer.is_none()
            && entry.pending_input.is_empty()
            && entry
                .reserved_input
                .load(std::sync::atomic::Ordering::Acquire)
                == 0;
        if !eligible {
            return Ok(ComposerAdmission::Busy);
        }
        let Some(bytes) = encode_submission(&command, modes.bracketed_paste) else {
            return Ok(ComposerAdmission::Invalid);
        };
        self.input_ingress(id)?
            .try_submit_kind(bytes, InputKind::Composer)?;
        let entry = self
            .entries
            .get_mut(&id)
            .ok_or(RuntimeError::UnknownExecution)?;
        let block_id = entry
            .block_timeline
            .allocate_id()
            .map_err(|_| RuntimeError::CapacityExceeded)?;
        // Pending only: Running Block metadata is published after trusted C.
        // The Block's start line comes from the parser-stamped line the C
        // marker carries (see ShellIntegrationEvent::CommandStarted), never
        // sampled here: the cursor right now is still on the prompt row.
        entry.pending_composer = Some(PendingComposerCommand { command, block_id });
        entry.integration = entry.integration.submitted();
        self.publish_composer_status_if_changed(id);
        Ok(ComposerAdmission::Accepted(block_id))
    }

    /// Publish the composer eligibility to attached clients when it flipped
    /// since the last publish (ADR-009 invariant 7; #978). Called only at
    /// integration-state transition points, never per byte: one enum compare
    /// per transition, and one bounded frame per attachment per flip.
    #[cfg(target_os = "macos")]
    pub(in crate::runtime) fn publish_composer_status_if_changed(&mut self, id: ExecutionId) {
        let Some(entry) = self.entries.get_mut(&id) else {
            return;
        };
        let eligibility = composer_eligibility(entry);
        if entry.published_composer_eligibility == Some(eligibility) {
            return;
        }
        entry.published_composer_eligibility = Some(eligibility);
        entry.composer_status_revision = entry.composer_status_revision.saturating_add(1);
        self.publish_composer_status(id);
    }

    /// Consume bounded canonical parser events after their bytes were applied
    /// to TerminalState. Only markers carrying this execution's nonce are
    /// trusted; the rest are counted and ignored. This path never reads a
    /// prompt, row text, or terminal cell payload.
    #[cfg(target_os = "macos")]
    pub(super) fn observe_shell_integration_events(
        &mut self,
        id: ExecutionId,
    ) -> Result<(), RuntimeError> {
        let mut changed = false;
        {
            let entry = self
                .entries
                .get_mut(&id)
                .ok_or(RuntimeError::UnknownExecution)?;
            while let Some(event) = entry.execution.take_shell_integration_event() {
                let (token, event) = match event {
                    ShellIntegrationEvent::PromptStarted { token } => {
                        (token, IntegrationEvent::PromptStarted)
                    }
                    ShellIntegrationEvent::CommandStarted { token, line } => {
                        (token, IntegrationEvent::CommandStarted { line })
                    }
                    ShellIntegrationEvent::CommandFinished {
                        token,
                        exit_status,
                        line,
                    } => (
                        token,
                        IntegrationEvent::CommandFinished { exit_status, line },
                    ),
                };
                if entry.shell_nonce != Some(token) {
                    entry.untrusted_markers = entry.untrusted_markers.saturating_add(1);
                    continue;
                }
                changed |= apply_integration_event(entry, event);
            }
            // Canonical alternate-screen entry while the prompt gate is open
            // means a program owns the terminal (mechanism 5).
            if matches!(
                entry.integration,
                IntegrationState::AtPrompt | IntegrationState::Pending
            ) && entry.execution.terminal().modes().alternate_screen
            {
                changed |= apply_integration_event(entry, IntegrationEvent::AlternateScreenEntered);
            }
            if changed {
                entry.block_revision = entry.block_revision.saturating_add(1);
            }
        }
        if changed {
            self.publish_block_timeline(id);
        }
        self.publish_composer_status_if_changed(id);
        Ok(())
    }

    /// Runtime admitted bytes that did not come from the composer. Called at
    /// admission, before the bytes reach the PTY, so Flow can never inherit a
    /// stale prompt from direct input (mechanism 5).
    #[cfg(target_os = "macos")]
    pub(super) fn note_direct_input_admitted(&mut self, id: ExecutionId) {
        if let Some(entry) = self.entries.get_mut(&id) {
            // No Block effects are possible from this event.
            let _ = apply_integration_event(entry, IntegrationEvent::DirectInputAdmitted);
        }
        self.publish_composer_status_if_changed(id);
    }

    /// Primary child exit or PTY EOF: complete any Running Block through
    /// lifecycle truth (invariant 15) and stop trusting markers.
    #[cfg(target_os = "macos")]
    pub(super) fn note_execution_ended(&mut self, id: ExecutionId) {
        let changed = match self.entries.get_mut(&id) {
            Some(entry) => {
                let changed = apply_integration_event(entry, IntegrationEvent::ExecutionEnded);
                if changed {
                    entry.block_revision = entry.block_revision.saturating_add(1);
                }
                changed
            }
            None => false,
        };
        if changed {
            self.publish_block_timeline(id);
        }
        self.publish_composer_status_if_changed(id);
    }

    #[cfg(not(target_os = "macos"))]
    pub(super) fn note_direct_input_admitted(&mut self, _id: ExecutionId) {}

    #[cfg(not(target_os = "macos"))]
    pub(super) fn note_execution_ended(&mut self, _id: ExecutionId) {}

    /// Non-macOS runtimes do not expose the local composer/block route, but
    /// still drain parser events so a raw execution cannot retain a bounded
    /// queue of shell-integration notifications indefinitely.
    #[cfg(not(target_os = "macos"))]
    pub(super) fn observe_shell_integration_events(
        &mut self,
        id: ExecutionId,
    ) -> Result<(), RuntimeError> {
        let entry = self
            .entries
            .get_mut(&id)
            .ok_or(RuntimeError::UnknownExecution)?;
        while entry.execution.take_shell_integration_event().is_some() {}
        Ok(())
    }
}

/// The eligibility Runtime would apply to a composer submission right now
/// (mechanism 5), reduced to the facts that only change at transition points.
/// Transient unwritten-input bookkeeping is deliberately left out: it drains
/// without a transition, so publishing it could strand clients on `Busy`;
/// admission itself still checks it and answers a correlated `Busy`.
#[cfg(target_os = "macos")]
pub(in crate::runtime) fn composer_eligibility(entry: &Entry) -> ComposerEligibility {
    if entry.shell_integration_mode == ShellIntegrationMode::Unsupported {
        return ComposerEligibility::Unsupported;
    }
    if entry.terminal_io_active()
        && entry.integration.composer_eligible()
        && !entry.execution.terminal().modes().alternate_screen
    {
        ComposerEligibility::Available
    } else {
        ComposerEligibility::Busy
    }
}

/// Apply one trusted event to the entry's integration state and carry out
/// its Block effects. Returns whether published Block metadata changed.
#[cfg(target_os = "macos")]
fn apply_integration_event(entry: &mut Entry, event: IntegrationEvent) -> bool {
    let transition = entry.integration.on(event);
    entry.integration = transition.next;
    let mut changed = false;
    for effect in transition.effects.into_iter().flatten() {
        match effect {
            Effect::StartPendingBlock { line } => {
                let Some(pending) = entry.pending_composer.take() else {
                    continue;
                };
                if entry
                    .block_timeline
                    .start(pending.block_id, pending.command, line.0)
                    .is_ok()
                {
                    entry.integration = entry.integration.block_started(pending.block_id);
                    changed = true;
                }
                // On failure the shell is still usable; no stale Running state
                // is published and the state stays Running(none).
            }
            Effect::DropPending => {
                entry.pending_composer = None;
            }
            Effect::Complete(block_id, exit, line) => {
                // A trusted D marker's parser-stamped line is authoritative.
                // Without one (lost D, or execution ended with no D observed)
                // the current cursor is the best available fallback.
                let end_line = line.map(|line| line.0).unwrap_or_else(|| {
                    let cursor = entry.execution.terminal().cursor();
                    entry
                        .execution
                        .terminal()
                        .line_id(cursor.row)
                        .map(|line| line.0)
                        .unwrap_or(1)
                });
                let exit_status = match exit {
                    BlockExit::Code(code) => Some(code),
                    BlockExit::Unknown => None,
                };
                if entry
                    .block_timeline
                    .complete(block_id, end_line, exit_status)
                    .is_ok()
                {
                    changed = true;
                }
            }
        }
    }
    changed
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn single_line_submission_is_raw_bytes_plus_return() {
        assert_eq!(encode_submission("pwd", false), Some(b"pwd\r".to_vec()));
        assert_eq!(encode_submission("pwd", true), Some(b"pwd\r".to_vec()));
        assert_eq!(
            encode_submission("echo 'a; b'", false),
            Some(b"echo 'a; b'\r".to_vec())
        );
    }

    #[test]
    fn multi_line_submission_requires_bracketed_paste() {
        assert_eq!(encode_submission("echo a\necho b", false), None);
        assert_eq!(encode_submission("echo a\recho b", false), None);
        assert_eq!(
            encode_submission("echo a\necho b", true),
            Some(b"\x1b[200~echo a\necho b\x1b[201~\r".to_vec())
        );
    }

    #[test]
    fn only_zsh_is_block_capable_and_other_shells_remain_raw() {
        assert_eq!(
            shell_integration_mode(&CommandSpec::new("/bin/zsh")),
            ShellIntegrationMode::ZshHook
        );
        assert_eq!(
            shell_integration_mode(&CommandSpec::new("/opt/homebrew/bin/zsh")),
            ShellIntegrationMode::ZshHook
        );
        assert_eq!(
            shell_integration_mode(&CommandSpec::new("/bin/sh")),
            ShellIntegrationMode::Unsupported
        );
    }

    #[test]
    fn busy_composer_admission_is_a_correlated_result_not_a_transport_error() {
        assert_eq!(ComposerAdmission::Busy, ComposerAdmission::Busy);
        assert_ne!(ComposerAdmission::Busy, ComposerAdmission::Unsupported);
        assert_ne!(ComposerAdmission::Invalid, ComposerAdmission::Busy);
    }
}
