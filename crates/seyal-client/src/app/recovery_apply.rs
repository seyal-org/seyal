//! Recovery coordinator apply paths for ApplicationRoot.

use std::time::Duration;

use seyal_core::{AttachmentId, ExecutionId, PaneId, TerminalTabId};

use super::*;
use crate::composer::RuntimeComposerEligibility;
use crate::presentation::{PresentationAction, PresentationIdentity};
use crate::recovery::{AttemptOutcome, ContinuityIdentity, LaunchResult, RecoveryStage};
use crate::shell::ShellAction;

struct ReplacementClientFacts {
    attachment: AttachmentId,
    pty_generation: u64,
    alternate_screen: bool,
    controller: bool,
    composer: Option<(RuntimeComposerEligibility, u64)>,
}

impl ApplicationRoot {
    pub(super) fn begin_recovery(&mut self, now: Duration) -> Result<(), AppError> {
        self.supersede_pane_recovery_for_primary();
        self.pending_recovery = self.recovery.begin_episode(now);
        Ok(())
    }

    pub(super) fn complete_recovery(
        &mut self,
        generation: u64,
        outcome: AttemptOutcome,
        now: Duration,
        launch: Option<LaunchResult>,
    ) -> Result<(), AppError> {
        let stale = generation != self.recovery.state().generation;
        let had_outstanding = self.recovery.has_outstanding_attempt();
        let effects = self
            .recovery
            .complete_attempt(generation, outcome, now, launch);
        if stale || !had_outstanding {
            // Stale-generation or duplicate/late current-generation completion:
            // dispose only. Never replace a queued Schedule/PerformAttempt the
            // coordinator still expects the host to drain (ADR-015).
            let current = std::mem::take(&mut self.pending_recovery);
            self.pending_recovery = effects;
            self.pending_recovery.extend(current);
            if stale {
                return Err(AppError::StaleRecoveryGeneration);
            }
            return Ok(());
        }
        if outcome == AttemptOutcome::ExecutionEnded && self.pane_recovery.active().is_none() {
            // Latch the primary Pane, not whoever is focused. A sibling can
            // hold focus while the primary shell exits (SPEC-009 §8.2.2).
            // Keep shell bind and Flow authority so completed Block history
            // still projects. New shell releases that bind explicitly.
            let ended_pane = self
                .pane_recovery
                .primary_pane
                .unwrap_or(self.shell.snapshot().focused_pane);
            self.latch_execution_ended_on_pane(ended_pane);
            self.settle_running_blocks_after_exit(ended_pane);
            self.sync_composer_presentation();
        }
        self.pending_recovery = effects;
        Ok(())
    }

    pub(super) fn fire_scheduled_recovery(
        &mut self,
        generation: u64,
        now: Duration,
    ) -> Result<(), AppError> {
        if generation != self.recovery.state().generation {
            return self.fail(AppError::StaleRecoveryGeneration);
        }
        self.pending_recovery = self.recovery.scheduled_fire(generation, now);
        Ok(())
    }

    pub(super) fn ack_recovery_effect(&mut self) -> Result<(), AppError> {
        if !self.pending_recovery.is_empty() {
            self.pending_recovery.remove(0);
        }
        Ok(())
    }

    /// Host open budget: pending `PerformAttempt` remaining, or the claimed
    /// budget after Ack drained that effect.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn recovery_open_budget_millis(&self) -> u64 {
        match self.pending_recovery.first() {
            Some(RecoveryEffect::PerformAttempt { remaining, .. }) => {
                return remaining.as_millis() as u64;
            }
            Some(RecoveryEffect::Schedule { delay, .. }) => return delay.as_millis() as u64,
            Some(RecoveryEffect::DisposeHandle(handle)) => return *handle,
            Some(RecoveryEffect::LaunchHelper { generation }) => return *generation,
            None => {}
        }
        self.recovery
            .claimed_attempt_remaining()
            .map(|remaining| remaining.as_millis() as u64)
            .unwrap_or(0)
    }

    pub(super) fn cancel_recovery(&mut self) -> Result<(), AppError> {
        self.recovery.cancel();
        // Keep DisposeHandle effects so a cancelled episode still drops any
        // opened-but-not-adopted client; hosts must not rely on drain order.
        self.pending_recovery
            .retain(|effect| matches!(effect, RecoveryEffect::DisposeHandle(_)));
        Ok(())
    }

    pub(super) fn advance_recovery_stage(
        &mut self,
        stage: crate::recovery::RecoveryStage,
    ) -> Result<(), AppError> {
        if self.recovery.advance_presentation_stage(stage) {
            Ok(())
        } else {
            Err(AppError::InvalidPayload)
        }
    }

    pub(super) fn begin_reconstruction_attempt(&mut self) -> Result<(), AppError> {
        self.reconstruction.begin_attempt();
        Ok(())
    }

    pub(super) fn commit_reconstruction(
        &mut self,
        runtime: ContinuityIdentity,
        execution: ContinuityIdentity,
        attachment: ContinuityIdentity,
        controller_authority_committed: bool,
        authoritative_snapshot_committed: bool,
    ) -> Result<(), AppError> {
        if self.reconstruction.commit(
            runtime,
            execution,
            attachment,
            controller_authority_committed,
            authoritative_snapshot_committed,
        ) {
            Ok(())
        } else {
            Err(AppError::InvalidPayload)
        }
    }

    pub(super) fn disconnect_reconstruction(&mut self) -> Result<(), AppError> {
        self.reconstruction.disconnect();
        Ok(())
    }

    pub(super) fn clear_reconstruction_continuity(&mut self) -> Result<(), AppError> {
        self.reconstruction.clear_continuity_pins();
        Ok(())
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn replacement_bootstrap_pending(&self) -> bool {
        self.replacement_bootstrap.is_some()
    }

    /// Shell exit keeps Flow history paint. The host must not decide that from
    /// raw poll codes.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn retains_flow_history(&self) -> bool {
        self.flow_history_hold
            || self.recovery.state().stage == RecoveryStage::ExecutionEnded
            || self.composer.any_execution_ended()
    }

    /// 1 when New shell would be admitted for the focused Pane (shell-exit
    /// latch). Hosts must project this fact rather than re-deriving it from
    /// the app-wide recovery stage (ADR-015 / #1379).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn new_shell_available(&self) -> bool {
        let focused = self.shell.snapshot().focused_pane;
        self.composer.execution_ended(focused)
    }

    /// A terminal display loss (shell exit, disconnect, or IO) keeps Flow
    /// history. The host must not re-derive that from poll codes.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn hold_flow_history(&mut self) {
        self.flow_history_hold = true;
    }

    /// SPEC-004 §18 Create+Bind for the ended Pane. Prepare releases only that
    /// Pane. The bind phase uses the created ExecutionId, never `open_first`.
    pub(super) fn create_replacement_execution(
        &mut self,
        execution: Option<ExecutionId>,
        attachment: Option<AttachmentId>,
        pty_generation: u64,
    ) -> Result<(), AppError> {
        let pane = self.replacement_target_pane()?;
        match execution {
            None => self.prepare_replacement(pane),
            Some(execution) => {
                let attachment = attachment.ok_or(AppError::InvalidPayload)?;
                self.finish_replacement_bind(pane, execution, attachment, pty_generation)
            }
        }
    }

    fn replacement_target_pane(&self) -> Result<PaneId, AppError> {
        // The focused Pane is the one whose composer is hidden. A focused
        // sibling must not be released when the primary has ended.
        let focused = self.shell.snapshot().focused_pane;
        if self.composer.execution_ended(focused) {
            return Ok(focused);
        }
        Err(AppError::InvalidPayload)
    }

    fn prepare_replacement(&mut self, pane: PaneId) -> Result<(), AppError> {
        if !self.composer.execution_ended(pane) {
            return Err(AppError::InvalidPayload);
        }
        let _ = self.shell.release_execution(pane);
        self.provisioning.release_dead_binding(pane);
        self.pane_authorities.remove(&pane);
        if self
            .authority
            .is_some_and(|authority| authority.pane == pane)
        {
            self.authority = None;
            let _ = self.presentation.apply(PresentationAction::ClearIdentity);
            self.sync_composer_presentation();
        }
        self.reconstruction.clear_continuity_pins();
        self.replacement_bootstrap = Some(pane);
        Ok(())
    }

    fn finish_replacement_bind(
        &mut self,
        pane: PaneId,
        execution: ExecutionId,
        attachment: AttachmentId,
        pty_generation: u64,
    ) -> Result<(), AppError> {
        if self.replacement_bootstrap != Some(pane) {
            return Err(AppError::StalePane);
        }
        let facts = self.replacement_client_facts(execution)?;
        let attachment = facts
            .as_ref()
            .map(|facts| facts.attachment)
            .unwrap_or(attachment);
        let pty_generation = facts
            .as_ref()
            .map(|facts| facts.pty_generation)
            .unwrap_or(pty_generation);
        let alternate_screen = facts.as_ref().is_some_and(|facts| facts.alternate_screen);
        let controller = facts.as_ref().map(|facts| facts.controller).unwrap_or(true);
        let composer = facts.as_ref().and_then(|facts| facts.composer);
        if pty_generation == 0 {
            return Err(AppError::ZeroPtyGeneration);
        }
        self.record_replacement_client(pane, execution)?;
        self.shell
            .apply(ShellAction::BindExecution { pane, execution })
            .map_err(|_| AppError::AlreadyBound)?;
        self.provisioning.record_adopted_binding(pane, execution);
        let evidence = BindingEvidence {
            execution,
            attachment,
            controller,
            pty_generation,
            alternate_screen,
        };
        self.install_pane_authority(pane, evidence)?;
        self.clear_execution_ended_for_pane(pane);
        self.flow_history_hold = false;
        // The ended shell's submit and composer-status revision do not describe
        // the new attachment. Clear them before applying the adopted client.
        let _ = self
            .composer
            .apply(crate::composer::ComposerAction::ResetAdmission { pane });
        if let Some((eligibility, revision)) = composer {
            self.apply_known_composer_status(pane, eligibility, revision);
        }
        self.replacement_bootstrap = None;
        if self.shell.snapshot().focused_pane == pane {
            self.recovery.mark_usable();
            self.sync_composer_presentation();
        }
        Ok(())
    }

    /// Install or replace per-pane Controller authority for the selected
    /// terminal tab; activate when that pane is focused. Portable: display
    /// refresh / handle fan-out is macOS-only (#1379 Linux compile + #1382 tabs).
    pub(super) fn install_pane_authority(
        &mut self,
        pane: PaneId,
        evidence: BindingEvidence,
    ) -> Result<(), AppError> {
        let terminal_tab = self
            .shell
            .selected_terminal_tab(pane)
            .map_err(|_| AppError::UnknownPane)?
            .ok_or(AppError::UnboundUnauthorized)?;
        self.install_terminal_authority(pane, terminal_tab, evidence)
    }

    pub(super) fn install_terminal_authority(
        &mut self,
        pane: PaneId,
        terminal_tab: TerminalTabId,
        evidence: BindingEvidence,
    ) -> Result<(), AppError> {
        if evidence.pty_generation == 0 {
            return Err(AppError::ZeroPtyGeneration);
        }
        if self
            .shell
            .terminal_execution(pane, terminal_tab)
            .ok()
            .flatten()
            != Some(evidence.execution)
        {
            return Err(AppError::StaleExecution);
        }
        self.bind_pane_recovery(pane, evidence.execution);
        self.remember_primary_pane(pane);
        let authority = PaneAuthority {
            pane,
            execution: evidence.execution,
            attachment: evidence.attachment,
            controller: evidence.controller,
            pty_generation: evidence.pty_generation,
        };
        self.terminal_authorities.insert(terminal_tab, authority);
        if self.shell.selected_terminal_tab(pane).ok().flatten() != Some(terminal_tab) {
            return Ok(());
        }
        self.pane_authorities.insert(pane, authority);
        if self.shell.snapshot().focused_pane == pane {
            let identity = PresentationIdentity::new(evidence.execution, evidence.pty_generation)
                .ok_or(AppError::ZeroPtyGeneration)?;
            // A prior tab may already hold presentation identity; clear then bind
            // so CreateTab's focused leaf becomes the live SPEC-008 fence.
            let _ = self.presentation.apply(PresentationAction::ClearIdentity);
            self.presentation
                .apply(PresentationAction::BindIdentity(identity))
                .map_err(|_| AppError::AlreadyBound)?;
            self.authority = Some(authority);
            self.derive_presentation(evidence.alternate_screen)?;
            self.sync_composer_presentation();
            #[cfg(target_os = "macos")]
            {
                self.refresh_output_from_pane_client(pane);
                if let Some(raw) = self.pane_client_raws.get(&pane).copied() {
                    crate::ffi::set_focused_display_handle(raw);
                }
            }
        }
        Ok(())
    }

    /// The adopted display client is the PTY generation and composer-status
    /// authority. No active client (in-process tests) keeps the action payload.
    fn replacement_client_facts(
        &self,
        execution: ExecutionId,
    ) -> Result<Option<ReplacementClientFacts>, AppError> {
        #[cfg(target_os = "macos")]
        let facts = {
            let handle = crate::ffi::active_handle();
            if handle == 0 {
                None
            } else {
                crate::ffi::with_client(handle, |client| {
                    if client.execution_id() != execution {
                        return None;
                    }
                    let composer = client.composer_status().map(|status| {
                        let eligibility = match status.eligibility {
                            seyal_protocol::framing::ComposerEligibility::Available => {
                                RuntimeComposerEligibility::Available
                            }
                            seyal_protocol::framing::ComposerEligibility::Busy => {
                                RuntimeComposerEligibility::Busy
                            }
                            seyal_protocol::framing::ComposerEligibility::Unsupported => {
                                RuntimeComposerEligibility::Unsupported
                            }
                        };
                        (eligibility, status.revision)
                    });
                    Some(ReplacementClientFacts {
                        attachment: client.attachment_id(),
                        pty_generation: client.cache().generation,
                        alternate_screen: client.cache().alternate_screen,
                        controller: matches!(
                            client.role(),
                            seyal_protocol::framing::Role::Controller
                        ),
                        composer,
                    })
                })
                .flatten()
            }
        };
        #[cfg(not(target_os = "macos"))]
        let facts = {
            let _ = execution;
            None
        };
        Ok(facts)
    }

    /// Remember the adopted client on the Pane so Split can present it.
    /// Call only after the bind generation is known to be usable.
    fn record_replacement_client(
        &mut self,
        pane: PaneId,
        execution: ExecutionId,
    ) -> Result<(), AppError> {
        #[cfg(target_os = "macos")]
        {
            let handle = crate::ffi::active_handle();
            if handle == 0 {
                return Ok(());
            }
            let Some(runtime_id) = crate::ffi::with_client(handle, |client| {
                (client.execution_id() == execution).then(|| client.runtime_id())
            })
            .flatten() else {
                return Ok(());
            };
            if self
                .client_handle
                .as_ref()
                .is_none_or(|owned| owned.raw() != handle)
            {
                self.client_handle = Some(crate::ffi::ClientRegistryHandle::new(handle));
            }
            self.pane_client_raws.insert(pane, handle);
            self.remember_pane_runtime(pane, runtime_id);
            self.seed_provisioning_request_floor_from_wire();
            if self.shell.snapshot().focused_pane == pane {
                crate::ffi::set_focused_display_handle(handle);
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (pane, execution);
        }
        Ok(())
    }

    fn apply_known_composer_status(
        &mut self,
        pane: PaneId,
        eligibility: RuntimeComposerEligibility,
        revision: u64,
    ) {
        let _ = self
            .composer
            .apply(crate::composer::ComposerAction::ApplyRuntimeEligibility {
                pane,
                eligibility: Some(eligibility),
                revision,
            });
        self.integration_unsupported = eligibility == RuntimeComposerEligibility::Unsupported;
        self.recompute_resting();
        if self.authority.is_some() {
            let _ = self.derive_presentation(self.alternate_screen);
        }
    }
}
