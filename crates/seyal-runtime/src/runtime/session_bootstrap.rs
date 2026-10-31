//! ADR-026 session bootstrap (Slice B).
//!
//! One secret-free stager line turns echo off, emits a K-authenticated
//! readiness marker, and `read -t`s exactly [`PAYLOAD_LEN`] bytes. Those bytes
//! are the bash or zsh hook payload, or padding the stager will not eval.
//! Trust starts at the shell's own first prompt marker, which carries S.
//! Session `A`/`C`/`D` markers are trusted only when they carry S.

use std::io::Read;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use seyal_exec::{SessionShellKind, ShellIntegrationEvent, ShellIntegrationToken};

use crate::input::InputKind;
use crate::{ExecutionId, RuntimeError};

use super::entry::{Entry, PendingComposerCommand};
use super::integration_state::{Effect, IntegrationEvent, IntegrationState};
use super::session_overlay::{SessionEvent, SessionOverlay};
use super::session_payload::{padding_bytes, payload_bytes, stager_line, PAYLOAD_LEN};
use super::Runtime;

/// Armed wait for readiness. Longer than the remote `read -t`. Delivering
/// refreshes the deadline after the exact-N write is queued.
const ARMED_DEADLINE: Duration = Duration::from_secs(5);
const DELIVER_DEADLINE: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SessionClose {
    /// Handshake failed. Armed writes nothing further. Delivering finishes
    /// exactly N bytes with padding when the payload was not queued, then
    /// grants no trust.
    Fail,
    /// Trusted-secret session ended. Candidate/Offered clear to `None`.
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MarkerAudience {
    Local,
    Session,
    Untrusted,
}

pub(crate) fn marker_audience(
    local: Option<ShellIntegrationToken>,
    session: Option<ShellIntegrationToken>,
    token: ShellIntegrationToken,
) -> MarkerAudience {
    if local == Some(token) {
        MarkerAudience::Local
    } else if session == Some(token) {
        MarkerAudience::Session
    } else {
        MarkerAudience::Untrusted
    }
}

/// In-memory session secret and the session's own mechanism-5 state.
#[derive(Clone, Debug)]
pub(crate) struct SessionRuntime {
    pub(crate) secret: Option<ShellIntegrationToken>,
    challenge: Option<ShellIntegrationToken>,
    pub(crate) integration: IntegrationState,
    pending: Option<PendingComposerCommand>,
    pub(crate) deadline: Option<Instant>,
    /// Set once exactly [`PAYLOAD_LEN`] bytes were accepted into ingress.
    delivered: Option<usize>,
}

impl Default for SessionRuntime {
    fn default() -> Self {
        Self {
            secret: None,
            challenge: None,
            integration: IntegrationState::Unproven,
            pending: None,
            deadline: None,
            delivered: None,
        }
    }
}

impl SessionRuntime {
    /// Fresh S and K for one bootstrap attempt. Revokes any earlier S.
    pub(super) fn armed(
        secret: ShellIntegrationToken,
        challenge: ShellIntegrationToken,
        deadline: Option<Instant>,
    ) -> Self {
        Self {
            secret: Some(secret),
            challenge: Some(challenge),
            deadline,
            ..Self::default()
        }
    }
}

impl Runtime {
    /// Overlay is already `Armed`. Generate S and K, hold direct input, and
    /// admit the one stager line.
    pub(super) fn begin_armed_bootstrap(&mut self, id: ExecutionId) -> Result<(), RuntimeError> {
        let stager = {
            let entry = self
                .entries
                .get_mut(&id)
                .ok_or(RuntimeError::UnknownExecution)?;
            if !matches!(entry.session, SessionOverlay::Armed { .. }) {
                return Err(RuntimeError::ShellIntegration(
                    "session bootstrap requires Armed",
                ));
            }
            let challenge = fresh_secret(&[entry.shell_nonce])?;
            let secret = fresh_secret(&[entry.shell_nonce, Some(challenge)])?;
            let stager = stager_line(challenge)?;
            let mut secret_hex = String::new();
            secret.write_hex(&mut secret_hex);
            if stager
                .windows(secret_hex.len())
                .any(|window| window == secret_hex.as_bytes())
            {
                return Err(RuntimeError::ShellIntegration(
                    "session stager must not carry S",
                ));
            }
            entry.session_runtime =
                SessionRuntime::armed(secret, challenge, Some(Instant::now() + ARMED_DEADLINE));
            stager
        };
        self.note_input_target(id);
        if self.queue_bootstrap(id, stager).is_err() {
            self.close_session(id, SessionClose::Fail)?;
            return Ok(());
        }
        self.publish_bootstrap(id, false, true);
        Ok(())
    }

    pub(super) fn dispatch_shell_integration_event(
        &mut self,
        id: ExecutionId,
        event: ShellIntegrationEvent,
    ) -> Result<bool, RuntimeError> {
        let (token, integration_event) = match event {
            ShellIntegrationEvent::Readiness { token, shell } => {
                return self.accept_readiness(id, token, shell);
            }
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
        let audience = {
            let entry = self
                .entries
                .get(&id)
                .ok_or(RuntimeError::UnknownExecution)?;
            marker_audience(entry.shell_nonce, entry.session_runtime.secret, token)
        };
        match audience {
            MarkerAudience::Local => {
                let active = self.entries.get(&id).is_some_and(|entry| {
                    matches!(
                        entry.session,
                        SessionOverlay::Candidate { .. }
                            | SessionOverlay::Offered { .. }
                            | SessionOverlay::Armed { .. }
                            | SessionOverlay::Delivering { .. }
                            | SessionOverlay::Trusted { .. }
                    )
                });
                if active {
                    // Trusted sessions end. Candidate/Offered return to None
                    // so a later host Block can become Candidate again.
                    self.close_session(id, SessionClose::End)?;
                }
                let before = self.entries.get(&id).map(|entry| entry.integration);
                let changed = {
                    let entry = self
                        .entries
                        .get_mut(&id)
                        .ok_or(RuntimeError::UnknownExecution)?;
                    super::shell_integration::apply_local_integration_event(
                        entry,
                        integration_event,
                    )
                };
                if let Some(block) =
                    self.entries
                        .get(&id)
                        .and_then(|entry| match (before, entry.integration) {
                            (
                                Some(IntegrationState::Running {
                                    block: Some(previous),
                                }),
                                IntegrationState::Running { block: Some(next) },
                            ) if previous == next => None,
                            (_, IntegrationState::Running { block: Some(next) }) => {
                                Some(next.raw())
                            }
                            _ => None,
                        })
                {
                    self.note_local_host_block_started(id, block);
                }
                self.end_orphaned_spawn(id)?;
                Ok(changed)
            }
            MarkerAudience::Session => self.apply_session_marker(id, integration_event),
            MarkerAudience::Untrusted => {
                let entry = self
                    .entries
                    .get_mut(&id)
                    .ok_or(RuntimeError::UnknownExecution)?;
                entry.untrusted_markers = entry.untrusted_markers.saturating_add(1);
                Ok(false)
            }
        }
    }

    pub(super) fn note_session_alternate_screen(
        &mut self,
        id: ExecutionId,
    ) -> Result<bool, RuntimeError> {
        let entry = self
            .entries
            .get_mut(&id)
            .ok_or(RuntimeError::UnknownExecution)?;
        if !matches!(entry.session, SessionOverlay::Trusted { .. }) {
            return Ok(false);
        }
        Ok(apply_session_effects(
            entry,
            IntegrationEvent::AlternateScreenEntered,
        ))
    }

    pub(super) fn submit_session_command(
        &mut self,
        id: ExecutionId,
        command: String,
    ) -> Result<super::shell_integration::ComposerAdmission, RuntimeError> {
        let entry = self
            .entries
            .get(&id)
            .ok_or(RuntimeError::UnknownExecution)?;
        if !matches!(entry.session, SessionOverlay::Trusted { .. })
            || entry.session_runtime.integration != IntegrationState::AtPrompt
            || entry.execution.terminal().modes().alternate_screen
            || entry.session_runtime.pending.is_some()
            || !entry.pending_input.is_empty()
            || entry.reserved_input.load(Ordering::Acquire) != 0
        {
            return Ok(super::shell_integration::ComposerAdmission::Busy);
        }
        let modes = entry.execution.terminal().modes();
        let Some(bytes) =
            super::shell_integration::encode_submission(&command, modes.bracketed_paste)
        else {
            return Ok(super::shell_integration::ComposerAdmission::Invalid);
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
        entry.session_runtime.pending = Some(PendingComposerCommand { command, block_id });
        entry.session_runtime.integration = entry.session_runtime.integration.submitted();
        self.note_input_target(id);
        self.publish_composer_status_if_changed(id);
        Ok(super::shell_integration::ComposerAdmission::Accepted(
            block_id,
        ))
    }

    /// A new local host Block replaced Declined/Failed/Ended for a different
    /// host. Reset those to `None`, then consider Candidate only from `None`.
    pub(super) fn note_local_host_block_started(&mut self, id: ExecutionId, block: u64) {
        let cleared = {
            let Some(entry) = self.entries.get_mut(&id) else {
                return;
            };
            let other_host = match entry.session {
                SessionOverlay::Declined { host_block }
                | SessionOverlay::Failed { host_block }
                | SessionOverlay::Ended { host_block } => host_block != block,
                _ => false,
            };
            if !other_host {
                false
            } else if let Ok(next) = entry.session.on(SessionEvent::Clear) {
                entry.session = next;
                entry.session_revision = entry.session_revision.saturating_add(1);
                true
            } else {
                false
            }
        };
        if cleared {
            self.note_input_target(id);
            self.publish_session_frames(id);
        }
        self.consider_session_candidate(id);
    }

    pub(super) fn close_session(
        &mut self,
        id: ExecutionId,
        kind: SessionClose,
    ) -> Result<(), RuntimeError> {
        let Some(entry) = self.entries.get(&id) else {
            return Ok(());
        };
        let overlay = entry.session;
        let delivered = entry.session_runtime.delivered;
        enum CloseStep {
            Fail {
                pad: bool,
            },
            End,
            /// Candidate/Offered completing for this host. Not `Ended`.
            Clear,
        }
        let step = match (overlay, kind) {
            (SessionOverlay::Delivering { .. }, _) if delivered != Some(PAYLOAD_LEN) => {
                CloseStep::Fail { pad: true }
            }
            (SessionOverlay::Armed { .. } | SessionOverlay::Delivering { .. }, _)
            | (SessionOverlay::SpawnArmed { .. }, SessionClose::Fail) => {
                CloseStep::Fail { pad: false }
            }
            (SessionOverlay::Trusted { .. }, SessionClose::End) => CloseStep::End,
            (
                SessionOverlay::Candidate { .. }
                | SessionOverlay::Offered { .. }
                | SessionOverlay::SpawnArmed { .. },
                SessionClose::End,
            ) => CloseStep::Clear,
            _ => return Ok(()),
        };
        if matches!(step, CloseStep::Fail { pad: true }) {
            let _ = self.queue_bootstrap(id, padding_bytes());
        }
        let changed = {
            let entry = self
                .entries
                .get_mut(&id)
                .ok_or(RuntimeError::UnknownExecution)?;
            let (changed, next) = match step {
                CloseStep::Clear => (false, entry.session.on(SessionEvent::Clear).ok()),
                CloseStep::End => (
                    apply_session_effects(entry, IntegrationEvent::ExecutionEnded),
                    entry.session.on(SessionEvent::End).ok(),
                ),
                CloseStep::Fail { .. } => (
                    apply_session_effects(entry, IntegrationEvent::ExecutionEnded),
                    entry.session.on(SessionEvent::Fail).ok(),
                ),
            };
            if let Some(next) = next {
                entry.session = next;
            }
            wipe_token(&mut entry.session_runtime.secret);
            wipe_token(&mut entry.session_runtime.challenge);
            entry.session_runtime.pending = None;
            entry.session_runtime.deadline = None;
            entry.session_runtime.delivered = None;
            entry.session_revision = entry.session_revision.saturating_add(1);
            changed
        };
        self.note_input_target(id);
        self.publish_bootstrap(id, changed, true);
        Ok(())
    }

    pub(super) fn expire_session_bootstraps(&mut self, now: Instant) -> Result<(), RuntimeError> {
        let due = self
            .entries
            .iter()
            .filter_map(|(&id, entry)| {
                entry
                    .session_runtime
                    .deadline
                    .filter(|deadline| *deadline <= now)
                    .map(|_| id)
            })
            .collect::<Vec<_>>();
        for id in due {
            self.close_session(id, SessionClose::Fail)?;
        }
        Ok(())
    }

    /// Controller left during Armed or Delivering (ADR-026 §10 T15).
    pub(super) fn abort_bootstrap_on_detach(
        &mut self,
        id: ExecutionId,
    ) -> Result<(), RuntimeError> {
        let inflight = self.entries.get(&id).is_some_and(|entry| {
            matches!(
                entry.session,
                SessionOverlay::Armed { .. } | SessionOverlay::Delivering { .. }
            )
        });
        if inflight {
            self.close_session(id, SessionClose::Fail)?;
        }
        Ok(())
    }

    fn accept_readiness(
        &mut self,
        id: ExecutionId,
        token: ShellIntegrationToken,
        shell: SessionShellKind,
    ) -> Result<bool, RuntimeError> {
        let ready = {
            let entry = self
                .entries
                .get_mut(&id)
                .ok_or(RuntimeError::UnknownExecution)?;
            let spawn = matches!(entry.session, SessionOverlay::SpawnArmed { .. });
            if !(spawn || matches!(entry.session, SessionOverlay::Armed { .. }))
                || entry.session_runtime.challenge != Some(token)
                || entry.session_runtime.secret.is_none()
            {
                entry.untrusted_markers = entry.untrusted_markers.saturating_add(1);
                return Ok(false);
            }
            // K is single-use. A later readiness in this batch cannot match.
            wipe_token(&mut entry.session_runtime.challenge);
            let secret = entry.session_runtime.secret.expect("checked above");
            // The spawn rcfile is bash. A zsh login shell reports itself and
            // takes no payload; Failed keeps the consented typed path.
            let next = entry.session.on(SessionEvent::BeginDelivering).ok();
            (
                next.filter(|_| !spawn || shell == SessionShellKind::Bash),
                secret,
            )
        };
        let (Some(delivering), secret) = ready else {
            self.close_session(id, SessionClose::Fail)?;
            return Ok(false);
        };
        let payload = payload_bytes(shell, secret);
        debug_assert_eq!(payload.len(), PAYLOAD_LEN);
        {
            let entry = self
                .entries
                .get_mut(&id)
                .ok_or(RuntimeError::UnknownExecution)?;
            entry.session = delivering;
            entry.session_runtime.deadline = Some(Instant::now() + DELIVER_DEADLINE);
            entry.session_revision = entry.session_revision.saturating_add(1);
        }
        self.note_input_target(id);
        if self.queue_bootstrap(id, payload).is_err() {
            // Delivering without a queued buffer: padding if the PTY still
            // accepts a full N, otherwise fail with no invented bytes.
            self.close_session(id, SessionClose::Fail)?;
            return Ok(false);
        }
        let entry = self
            .entries
            .get_mut(&id)
            .ok_or(RuntimeError::UnknownExecution)?;
        entry.session_runtime.delivered = Some(PAYLOAD_LEN);
        self.publish_bootstrap(id, false, true);
        Ok(false)
    }

    fn apply_session_marker(
        &mut self,
        id: ExecutionId,
        event: IntegrationEvent,
    ) -> Result<bool, RuntimeError> {
        let entry = self
            .entries
            .get_mut(&id)
            .ok_or(RuntimeError::UnknownExecution)?;
        let mut changed = false;
        match entry.session {
            SessionOverlay::Delivering { .. }
                if matches!(event, IntegrationEvent::PromptStarted) =>
            {
                match entry.session.on(SessionEvent::Trust) {
                    Ok(trusted) => {
                        entry.session = trusted;
                        entry.session_runtime.deadline = None;
                        entry.session_revision = entry.session_revision.saturating_add(1);
                        changed = apply_session_effects(entry, event);
                    }
                    Err(_) => {
                        entry.untrusted_markers = entry.untrusted_markers.saturating_add(1);
                    }
                }
            }
            SessionOverlay::Trusted { .. } => {
                changed = apply_session_effects(entry, event);
            }
            _ => {
                entry.untrusted_markers = entry.untrusted_markers.saturating_add(1);
            }
        }
        let session_changed = matches!(event, IntegrationEvent::PromptStarted)
            && self
                .entries
                .get(&id)
                .is_some_and(|entry| matches!(entry.session, SessionOverlay::Trusted { .. }));
        self.note_input_target(id);
        self.publish_bootstrap(id, changed, session_changed);
        // Block publication already happened. The caller must not bump again.
        Ok(false)
    }

    fn queue_bootstrap(&mut self, id: ExecutionId, bytes: Vec<u8>) -> Result<(), RuntimeError> {
        self.input_ingress(id)?
            .try_submit_kind(bytes, InputKind::Bootstrap)
    }

    fn publish_bootstrap(&mut self, id: ExecutionId, blocks_changed: bool, session_changed: bool) {
        if blocks_changed {
            if let Some(entry) = self.entries.get_mut(&id) {
                entry.block_revision = entry.block_revision.saturating_add(1);
            }
            self.publish_block_timeline(id);
        }
        if session_changed {
            self.publish_session_frames(id);
        }
        self.publish_composer_status_if_changed(id);
    }
}

fn apply_session_effects(entry: &mut Entry, event: IntegrationEvent) -> bool {
    let transition = entry.session_runtime.integration.on(event);
    entry.session_runtime.integration = transition.next;
    let mut changed = false;
    for effect in transition.effects.into_iter().flatten() {
        match effect {
            Effect::StartPendingBlock { line } => {
                let Some(pending) = entry.session_runtime.pending.take() else {
                    continue;
                };
                let host = entry.session.host_block().unwrap_or(0);
                if entry
                    .block_timeline
                    .start_session(pending.block_id, pending.command, line.0, host)
                    .is_ok()
                {
                    entry.session_runtime.integration = entry
                        .session_runtime
                        .integration
                        .block_started(pending.block_id);
                    changed = true;
                }
            }
            Effect::DropPending => {
                entry.session_runtime.pending = None;
            }
            Effect::Complete(block_id, exit, line) => {
                changed |=
                    super::shell_integration::complete_timeline_block(entry, block_id, exit, line);
            }
        }
    }
    changed
}

pub(super) fn fresh_secret(
    exclude: &[Option<ShellIntegrationToken>],
) -> Result<ShellIntegrationToken, RuntimeError> {
    let mut file = std::fs::File::open("/dev/urandom").map_err(RuntimeError::Io)?;
    for _ in 0..8 {
        let mut bytes = [0u8; 16];
        file.read_exact(&mut bytes).map_err(RuntimeError::Io)?;
        let token = ShellIntegrationToken::from_bytes(bytes);
        if exclude.iter().flatten().all(|other| *other != token) {
            return Ok(token);
        }
    }
    Err(RuntimeError::ShellIntegration(
        "session secret collided with an active secret",
    ))
}

fn wipe_token(slot: &mut Option<ShellIntegrationToken>) {
    // Best-effort. `black_box` keeps the overwrite from being deleted; the
    // ADR does not require a guaranteed zeroize of every copy.
    if let Some(token) = slot.take() {
        let mut bytes = token.as_bytes();
        bytes.fill(0);
        std::hint::black_box(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::super::session_payload::{alphabet_ok, MAX_CANON, TRAILER};
    use super::*;

    fn token(byte: u8) -> ShellIntegrationToken {
        ShellIntegrationToken::from_bytes([byte; 16])
    }

    #[test]
    fn forged_marker_matches_neither_scope() {
        let local = token(1);
        let session = token(2);
        assert_eq!(
            marker_audience(Some(local), Some(session), token(9)),
            MarkerAudience::Untrusted
        );
        assert_eq!(
            marker_audience(Some(local), Some(session), local),
            MarkerAudience::Local
        );
        assert_eq!(
            marker_audience(Some(local), Some(session), session),
            MarkerAudience::Session
        );
        assert_eq!(
            marker_audience(Some(local), None, session),
            MarkerAudience::Untrusted
        );
    }

    #[test]
    fn stager_turns_echo_off_before_readiness_and_fits_max_canon() {
        let challenge = token(0xab);
        let secret = token(0xcd);
        let stager = stager_line(challenge).expect("stager");
        assert!(stager.len() <= MAX_CANON);
        assert_eq!(*stager.last().unwrap(), b'\r');
        let line = std::str::from_utf8(&stager[..stager.len() - 1]).unwrap();
        let echo = line.find("stty -echo").unwrap();
        let marker = line.find("133;R").unwrap();
        assert!(echo < marker);
        assert!(line.contains("read -t 2 -k 512"));
        assert!(line.contains("read -r -t 2 -N 512"));
        assert!(!line.starts_with('~'));
        let mut secret_hex = String::new();
        secret.write_hex(&mut secret_hex);
        assert!(!line.contains(&secret_hex));
        let mut challenge_hex = String::new();
        challenge.write_hex(&mut challenge_hex);
        assert!(line.contains(&challenge_hex));
    }

    #[test]
    fn payload_is_exact_n_printable_and_padding_is_not_evald() {
        let secret = token(0x11);
        for shell in [SessionShellKind::Zsh, SessionShellKind::Bash] {
            let payload = payload_bytes(shell, secret);
            assert_eq!(payload.len(), PAYLOAD_LEN);
            assert!(alphabet_ok(&payload));
            assert!(payload
                .windows(TRAILER.len())
                .any(|window| window == TRAILER.as_bytes()));
            assert!(payload
                .windows(b"stty echo".len())
                .any(|window| window == b"stty echo"));
            let text = String::from_utf8(payload).unwrap();
            assert!(text.contains("133;A") && text.contains("133;C") && text.contains("133;D"));
            let mut secret_hex = String::new();
            secret.write_hex(&mut secret_hex);
            assert!(text.contains(&secret_hex));
        }
        let padding = padding_bytes();
        assert_eq!(padding.len(), PAYLOAD_LEN);
        assert!(alphabet_ok(&padding));
        assert!(!padding
            .windows(TRAILER.len())
            .any(|window| window == TRAILER.as_bytes()));
    }

    #[test]
    fn session_trust_does_not_equal_local_trust() {
        use seyal_protocol::framing::BlockTrust;
        assert_ne!(BlockTrust::SessionTrusted, BlockTrust::LocalTrusted);
    }
}
