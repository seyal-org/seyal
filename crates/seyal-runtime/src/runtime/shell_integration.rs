//! Trusted zsh shell integration inside Runtime (ADR-009, 2026-09-16
//! amendment): composer admission, the submission byte contract, and the
//! application of trusted markers and Runtime events to the per-execution
//! integration state. Hooks are installed at spawn by
//! `ShellIntegrationPolicy`. The consented session stager is the one later
//! PTY write, and it lives in `session_bootstrap`.

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(target_os = "macos")]
pub(super) enum ShellIntegrationMode {
    ZshHook,
    Unsupported,
}

/// Test-only mirror of spawn-time mode selection from
/// `compose_child_command` / `ShellIntegrationPolicy::supports`.
#[cfg(all(test, target_os = "macos"))]
fn shell_integration_mode(command: &seyal_exec::CommandSpec) -> ShellIntegrationMode {
    if crate::ShellIntegrationPolicy::supports(command) {
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
    /// Target generation did not match the generation the client displayed.
    /// Nothing was written; the draft stays.
    StaleTarget,
    /// Line written to the running program. No Block is created.
    Relayed,
}

#[cfg(target_os = "macos")]
const BRACKET_PASTE_START: &[u8] = b"\x1b[200~";
#[cfg(target_os = "macos")]
const BRACKET_PASTE_END: &[u8] = b"\x1b[201~";

/// ADR-009 mechanism 4: exactly the bytes a person would produce at that
/// prompt. Single-line text is raw; text containing a line break is sent
/// only as a bracketed paste, and only when the shell enabled DECSET 2004.
#[cfg(target_os = "macos")]
pub(super) fn encode_submission(command: &str, bracketed_paste: bool) -> Option<Vec<u8>> {
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
        self.submit_composer_command_fenced(id, command, None)
    }

    /// `target_generation` is `Some` only when the client negotiated
    /// `CAP_SESSION_BOOTSTRAP` and sent the fenced composer schema.
    #[cfg(target_os = "macos")]
    pub(crate) fn submit_composer_command_fenced(
        &mut self,
        id: ExecutionId,
        command: String,
        target_generation: Option<u64>,
    ) -> Result<ComposerAdmission, RuntimeError> {
        if command.is_empty() || command.len() > MAX_COMMAND_BYTES {
            return Err(RuntimeError::CapacityExceeded);
        }
        self.note_input_target(id);
        let fence = {
            let entry = self
                .entries
                .get(&id)
                .ok_or(RuntimeError::UnknownExecution)?;
            if entry.session.holds_direct_input() || !entry.terminal_io_active() {
                FencedAdmission::Busy
            } else if let Some(generation) = target_generation {
                if generation != entry.target_generation {
                    FencedAdmission::Stale
                } else {
                    let derived =
                        super::session_overlay::derive_composer(composer_facts(entry, true));
                    match derived {
                        super::session_overlay::DerivedComposer::Relay { .. } => {
                            FencedAdmission::Relay
                        }
                        super::session_overlay::DerivedComposer::Unavailable(_) => {
                            FencedAdmission::Busy
                        }
                        super::session_overlay::DerivedComposer::Submit => {
                            FencedAdmission::Continue
                        }
                    }
                }
            } else {
                FencedAdmission::Continue
            }
        };
        match fence {
            FencedAdmission::Busy => return Ok(ComposerAdmission::Busy),
            FencedAdmission::Stale => return Ok(ComposerAdmission::StaleTarget),
            FencedAdmission::Relay => {
                let mut bytes = Vec::with_capacity(command.len() + 1);
                bytes.extend_from_slice(command.as_bytes());
                bytes.push(b'\r');
                self.input_ingress(id)?
                    .try_submit_kind(bytes, InputKind::Direct)?;
                return Ok(ComposerAdmission::Relayed);
            }
            FencedAdmission::Continue => {}
        }
        let session_submit = self.entries.get(&id).is_some_and(|entry| {
            matches!(
                entry.session,
                super::session_overlay::SessionOverlay::Trusted { .. }
            )
        });
        if session_submit {
            return self.submit_session_command(id, command);
        }
        let entry = self
            .entries
            .get(&id)
            .ok_or(RuntimeError::UnknownExecution)?;
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
        self.note_input_target(id);
        self.publish_composer_status_if_changed(id);
        Ok(ComposerAdmission::Accepted(block_id))
    }

    /// Publish the composer eligibility to attached clients when it flipped
    /// since the last publish (ADR-009 invariant 7; #978). Called only at
    /// integration-state transition points, never per byte: one enum compare
    /// per transition, and one bounded frame per attachment per flip.
    #[cfg(target_os = "macos")]
    pub(in crate::runtime) fn publish_composer_status_if_changed(&mut self, id: ExecutionId) {
        self.note_input_target(id);
        let publish_status = {
            let Some(entry) = self.entries.get_mut(&id) else {
                return;
            };
            let eligibility = composer_eligibility(entry);
            let derived = super::session_overlay::derive_composer(composer_facts(entry, true));
            let stamp = (
                derived.mode() as u8,
                derived.reason() as u8,
                entry.target_generation,
            );
            let eligibility_changed = entry.published_composer_eligibility != Some(eligibility);
            let target_changed = entry.published_target_stamp != Some(stamp);
            if !eligibility_changed && !target_changed {
                return;
            }
            entry.published_target_stamp = Some(stamp);
            entry.published_target_generation = entry.target_generation;
            entry.composer_target_revision = entry.composer_target_revision.saturating_add(1);
            if eligibility_changed {
                entry.published_composer_eligibility = Some(eligibility);
                entry.composer_status_revision = entry.composer_status_revision.saturating_add(1);
            }
            eligibility_changed
        };
        if publish_status {
            self.publish_composer_status(id);
        } else {
            self.publish_composer_target(id);
        }
    }

    /// Consume bounded canonical parser events after their bytes were applied
    /// to TerminalState. Local markers must carry the execution nonce. Session
    /// markers must carry S. Anything else is counted and ignored. This path
    /// never reads a prompt, row text, or terminal cell payload.
    #[cfg(target_os = "macos")]
    pub(super) fn observe_shell_integration_events(
        &mut self,
        id: ExecutionId,
    ) -> Result<(), RuntimeError> {
        let mut changed = false;
        loop {
            let event = {
                let entry = self
                    .entries
                    .get_mut(&id)
                    .ok_or(RuntimeError::UnknownExecution)?;
                entry.execution.take_shell_integration_event()
            };
            let Some(event) = event else {
                break;
            };
            changed |= self.dispatch_shell_integration_event(id, event)?;
        }
        let session_alternate = {
            let entry = self
                .entries
                .get_mut(&id)
                .ok_or(RuntimeError::UnknownExecution)?;
            // Canonical alternate-screen entry while the prompt gate is open
            // means a program owns the terminal (mechanism 5).
            if matches!(
                entry.integration,
                IntegrationState::AtPrompt | IntegrationState::Pending
            ) && entry.execution.terminal().modes().alternate_screen
            {
                changed |=
                    apply_local_integration_event(entry, IntegrationEvent::AlternateScreenEntered);
            }
            let session_alternate = matches!(
                entry.session,
                super::session_overlay::SessionOverlay::Trusted { .. }
            ) && matches!(
                entry.session_runtime.integration,
                IntegrationState::AtPrompt | IntegrationState::Pending
            ) && entry.execution.terminal().modes().alternate_screen;
            if changed {
                entry.block_revision = entry.block_revision.saturating_add(1);
            }
            session_alternate
        };
        if session_alternate && self.note_session_alternate_screen(id)? {
            changed = true;
            if let Some(entry) = self.entries.get_mut(&id) {
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
            let _ = apply_local_integration_event(entry, IntegrationEvent::DirectInputAdmitted);
        }
        self.publish_composer_status_if_changed(id);
    }

    /// Primary child exit or PTY EOF: complete any Running Block through
    /// lifecycle truth (invariant 15) and stop trusting markers.
    #[cfg(target_os = "macos")]
    pub(super) fn note_execution_ended(&mut self, id: ExecutionId) {
        let _ = self.close_session(id, super::session_bootstrap::SessionClose::End);
        let changed = match self.entries.get_mut(&id) {
            Some(entry) => {
                let changed =
                    apply_local_integration_event(entry, IntegrationEvent::ExecutionEnded);
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
    pub(in crate::runtime) fn consider_session_candidate(&mut self, _id: ExecutionId) {}

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

    #[cfg(target_os = "macos")]
    pub(in crate::runtime) fn note_input_target(&mut self, id: ExecutionId) {
        let Some(entry) = self.entries.get_mut(&id) else {
            return;
        };
        let integration = if matches!(
            entry.session,
            super::session_overlay::SessionOverlay::Trusted { .. }
        ) {
            entry.session_runtime.integration
        } else {
            entry.integration
        };
        let target = super::session_overlay::input_target(shell_fact(integration));
        if entry.input_target != target {
            entry.input_target = target;
            entry.target_generation = entry.target_generation.saturating_add(1).max(1);
        }
        let held = entry.session.holds_direct_input();
        entry
            .direct_input_held
            .store(held, std::sync::atomic::Ordering::Release);
    }

    /// Cold-path §4.1 Candidate when a host Block is Running on the primary
    /// screen. F9 argv is not sampled; proactive Offer is not published.
    #[cfg(target_os = "macos")]
    pub(in crate::runtime) fn consider_session_candidate(&mut self, id: ExecutionId) {
        let changed = {
            let Some(entry) = self.entries.get_mut(&id) else {
                return;
            };
            let IntegrationState::Running { block: Some(block) } = entry.integration else {
                return;
            };
            // Automatic Candidate only from None. Declined/Failed stay until
            // an explicit offer or a different host Block resets them.
            if !matches!(entry.session, super::session_overlay::SessionOverlay::None) {
                return;
            }
            let host_block = block.raw();
            if entry.execution.terminal().modes().alternate_screen
                || !entry.pending_input.is_empty()
                || entry.pending_composer.is_some()
            {
                return;
            }
            match entry
                .session
                .on(super::session_overlay::SessionEvent::MarkCandidate { host_block })
            {
                Ok(next) => {
                    entry.session = next;
                    entry.session_revision = entry.session_revision.saturating_add(1);
                    true
                }
                Err(_) => false,
            }
        };
        if changed {
            self.note_input_target(id);
            self.publish_session_frames(id);
        }
    }

    /// Move a host Block to `Offered` when the overlay gate allows it and the
    /// local integration is `Running` for that Block. Explicit consent is the
    /// production entry (type 43 Request). Proactive F9 argv hints are not
    /// implemented, so the terminal hot path does not call this.
    #[cfg(target_os = "macos")]
    pub(crate) fn offer_session(
        &mut self,
        id: ExecutionId,
        host_block: u64,
    ) -> Result<(), RuntimeError> {
        let entry = self
            .entries
            .get_mut(&id)
            .ok_or(RuntimeError::UnknownExecution)?;
        let running = matches!(
            entry.integration,
            IntegrationState::Running { block: Some(block) } if block.raw() == host_block
        );
        if !running || entry.execution.terminal().modes().alternate_screen {
            return Err(RuntimeError::ShellIntegration(
                "session offer requires a running host block on the primary screen",
            ));
        }
        // Already Offered for this host: republish; do not bump generation.
        if matches!(
            entry.session,
            super::session_overlay::SessionOverlay::Offered {
                host_block: offered,
                ..
            } if offered == host_block
        ) {
            self.publish_session_frames(id);
            self.publish_composer_status_if_changed(id);
            return Ok(());
        }
        let candidate = match entry.session {
            super::session_overlay::SessionOverlay::Candidate {
                host_block: current,
            } if current == host_block => entry.session,
            _ => entry
                .session
                .on(super::session_overlay::SessionEvent::MarkCandidate { host_block })
                .map_err(|_| {
                    RuntimeError::ShellIntegration("session overlay refuses a new candidate")
                })?,
        };
        let generation = entry.next_offer_generation;
        entry.next_offer_generation = generation.saturating_add(1).max(1);
        entry.session = candidate
            .on(super::session_overlay::SessionEvent::Offer {
                offer_generation: generation,
            })
            .map_err(|_| RuntimeError::ShellIntegration("session overlay refuses the offer"))?;
        entry.session_revision = entry.session_revision.saturating_add(1);
        self.note_input_target(id);
        self.publish_session_frames(id);
        self.publish_composer_status_if_changed(id);
        Ok(())
    }

    /// Controller consent. Decline stays on the relay path. Accept arms and
    /// admits the echo-off stager.
    #[cfg(target_os = "macos")]
    pub(crate) fn apply_session_consent(
        &mut self,
        id: ExecutionId,
        host_block: u64,
        offer_generation: u64,
        presentation_epoch: u64,
        accept: bool,
    ) -> Result<(), RuntimeError> {
        let entry = self
            .entries
            .get_mut(&id)
            .ok_or(RuntimeError::UnknownExecution)?;
        if entry.session.host_block() != Some(host_block)
            || entry.session.offer_generation() != Some(offer_generation)
            || presentation_epoch != entry.offer_presentation_epoch
        {
            return Err(RuntimeError::ShellIntegration(
                "session consent fence rejected",
            ));
        }
        let next = if accept {
            entry
                .session
                .on(super::session_overlay::SessionEvent::Accept)
        } else {
            entry
                .session
                .on(super::session_overlay::SessionEvent::Decline)
        }
        .map_err(|_| RuntimeError::ShellIntegration("session consent rejected"))?;
        entry.session = next;
        entry.session_revision = entry.session_revision.saturating_add(1);
        if accept {
            self.begin_armed_bootstrap(id)?;
        } else {
            self.note_input_target(id);
            self.publish_session_frames(id);
            self.publish_composer_status_if_changed(id);
        }
        self.consider_session_candidate(id);
        Ok(())
    }
}

#[cfg(target_os = "macos")]
enum FencedAdmission {
    Busy,
    Stale,
    Relay,
    Continue,
}

#[cfg(target_os = "macos")]
pub(in crate::runtime) fn composer_facts(
    entry: &Entry,
    controller: bool,
) -> super::session_overlay::ComposerFacts {
    super::session_overlay::ComposerFacts {
        integration: shell_fact(entry.integration),
        primary_screen: !entry.execution.terminal().modes().alternate_screen,
        // Pane Raw/Flow is client presentation. Runtime derives the Flow
        // target; the client hides the composer when the Pane is Raw.
        flow: true,
        controller,
        overlay: entry.session,
        session_integration: shell_fact(entry.session_runtime.integration),
    }
}

#[cfg(target_os = "macos")]
pub(in crate::runtime) fn shell_fact(state: IntegrationState) -> super::session_overlay::ShellFact {
    use super::session_overlay::ShellFact;
    match state {
        IntegrationState::Unproven => ShellFact::Unproven,
        IntegrationState::AtPrompt => ShellFact::AtPrompt,
        IntegrationState::Pending => ShellFact::Pending,
        IntegrationState::Running { block } => ShellFact::Running {
            block: block.map(|id| id.raw()),
        },
        IntegrationState::Terminated => ShellFact::Terminated,
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

/// Complete one Block from a trusted `D`, a lost `D`, or execution end.
/// The parser-stamped line wins; otherwise the current cursor is the fallback.
#[cfg(target_os = "macos")]
pub(super) fn complete_timeline_block(
    entry: &mut Entry,
    block_id: crate::command_block_timeline::CommandBlockId,
    exit: BlockExit,
    line: Option<seyal_exec::LineId>,
) -> bool {
    let mut end_line = line.map(|line| line.0).unwrap_or_else(|| {
        let cursor = entry.execution.terminal().cursor();
        entry
            .execution
            .terminal()
            .line_id(cursor.row)
            .map(|line| line.0)
            .unwrap_or(1)
    });
    // ED2 (`clear`) seals start into history and remints the viewport before D.
    let mut span_start = None;
    if let Some(start_line) = entry
        .block_timeline
        .records()
        .find(|record| record.id == block_id)
        .map(|record| record.start_line)
    {
        let (clamped_start, clamped_end) = entry
            .execution
            .terminal()
            .clamp_block_span_across_ed2_remint(
                seyal_exec::LineId(start_line),
                seyal_exec::LineId(end_line),
            );
        if clamped_start.0 != start_line {
            span_start = Some(clamped_start.0);
        }
        end_line = clamped_end.0;
    }
    let exit_status = match exit {
        BlockExit::Code(code) => Some(code),
        BlockExit::Unknown => None,
    };
    entry
        .block_timeline
        .complete_span(block_id, span_start, end_line, exit_status)
        .is_ok()
}

/// Apply one trusted local-scope event and carry out its Block effects.
/// Returns whether published Block metadata changed.
#[cfg(target_os = "macos")]
pub(super) fn apply_local_integration_event(entry: &mut Entry, event: IntegrationEvent) -> bool {
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
                changed |= complete_timeline_block(entry, block_id, exit, line);
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
        use seyal_exec::CommandSpec;
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
