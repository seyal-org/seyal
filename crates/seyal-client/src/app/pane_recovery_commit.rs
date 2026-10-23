//! Atomic admission of a replacement Controller for a recovered Pane.

use super::*;

impl ApplicationRoot {
    #[cfg(target_os = "macos")]
    pub(crate) fn commit_pane_recovery(
        &mut self,
        target: PaneRecoveryTarget,
        recovery_generation: u64,
        handle: u64,
    ) -> Result<(), AppError> {
        use seyal_runtime::local_ipc::framing::Role;

        if self.pane_recovery.active() != Some(target)
            || self.pane_recovery.coordinator.state().generation != recovery_generation
            || !self.pane_recovery.is_current(target)
            || self.shell.pane_execution(target.pane_id()).ok().flatten()
                != Some(target.execution_id())
        {
            return Err(AppError::StalePane);
        }
        let expectation = self
            .pane_recovery
            .expectations
            .get(&target.pane_id())
            .copied()
            .ok_or(AppError::NoLiveClient)?;
        let Some(candidate) = crate::ffi::with_client(handle, |client| {
            let cache = client.cache();
            let snapshot_complete = cache.generation != 0
                && cache.rows != 0
                && cache.columns != 0
                && cache.cells.len() == usize::from(cache.rows) * usize::from(cache.columns);
            (
                client.runtime_id(),
                client.execution_id(),
                client.attachment_id(),
                matches!(client.role(), Role::Controller),
                snapshot_complete,
                cache.alternate_screen,
                cache.generation,
                crate::app::session::project_cache_text(cache),
            )
        }) else {
            return Err(AppError::NoLiveClient);
        };
        if candidate.0 != expectation.runtime_id {
            return Err(AppError::StaleAttachment);
        }
        if candidate.1 != target.execution_id() {
            return Err(AppError::StaleExecution);
        }
        if candidate.2 == expectation.attachment_id {
            return Err(AppError::StaleAttachment);
        }
        if !candidate.3 {
            return Err(AppError::NotController);
        }
        if !candidate.4 {
            return Err(AppError::InvalidPayload);
        }
        if !matches!(
            self.pane_recovery.coordinator.state().stage,
            crate::recovery::RecoveryStage::Reconstructing
                | crate::recovery::RecoveryStage::RestoringInteraction
        ) {
            return Err(AppError::InvalidPayload);
        }

        let pane = target.pane_id();
        if self
            .pane_client_raws
            .iter()
            .any(|(mapped_pane, raw)| *raw == handle && *mapped_pane != pane)
        {
            return Err(AppError::AlreadyBound);
        }
        let evidence = crate::app::BindingEvidence {
            execution: target.execution_id(),
            attachment: candidate.2,
            controller: true,
            pty_generation: candidate.6.max(1),
            alternate_screen: candidate.5,
        };

        // Stage every fallible part of the focused presentation commit and
        // Pane coordinator transition before publishing the replacement
        // handle, runtime id, or authority. The FFI caller disconnects a
        // rejected candidate, so a failure here must leave no root mapping to
        // that candidate behind.
        let focused = self.shell.snapshot().focused_pane == pane;
        let (next_presentation, next_presentation_mode) = if focused {
            let identity = crate::presentation::PresentationIdentity::new(
                evidence.execution,
                evidence.pty_generation,
            )
            .ok_or(AppError::ZeroPtyGeneration)?;
            let mut presentation = self.presentation.clone();
            presentation
                .apply(crate::presentation::PresentationAction::BindIdentity(
                    identity,
                ))
                .map_err(|_| AppError::AlreadyBound)?;
            let desired_mode = if evidence.alternate_screen {
                crate::presentation::PresentationMode::Tui
            } else {
                self.resting
            };
            let current = presentation.snapshot();
            if current.mode != desired_mode {
                presentation
                    .apply(crate::presentation::PresentationAction::Transition {
                        mode: desired_mode,
                        identity,
                        explicit: self.explicit_raw
                            && desired_mode == crate::presentation::PresentationMode::Raw,
                        epoch: current.epoch,
                    })
                    .map_err(|_| AppError::StalePresentationEpoch)?;
            }
            (Some(presentation), Some(desired_mode))
        } else {
            (None, None)
        };
        let mut next_coordinator = self.pane_recovery.coordinator.clone();
        if !next_coordinator.advance_presentation_stage(RecoveryStage::Usable) {
            return Err(AppError::InvalidPayload);
        }

        let authority = crate::app::PaneAuthority {
            pane,
            execution: evidence.execution,
            attachment: evidence.attachment,
            controller: evidence.controller,
            pty_generation: evidence.pty_generation,
        };
        self.pane_client_raws.insert(pane, handle);
        if self.pane_recovery.primary_pane == Some(pane) && self.client_handle.is_none() {
            self.client_handle = Some(crate::ffi::ClientRegistryHandle::new(handle));
        } else {
            self.extra_pane_clients
                .insert(pane, crate::ffi::ClientRegistryHandle::new(handle));
        }
        self.pane_recovery.runtime_ids.insert(pane, candidate.0);
        self.pane_authorities.insert(pane, authority);
        if let (Some(presentation), Some(desired_mode)) =
            (next_presentation, next_presentation_mode)
        {
            let mode_changed = self.presentation.snapshot().mode != desired_mode;
            self.presentation = presentation;
            self.alternate_screen = evidence.alternate_screen;
            self.authority = Some(authority);
            self.sync_focused_presentation_mode();
            if mode_changed {
                self.clear_chord_prefix();
            }
            self.sync_composer_presentation();
            self.output_utf8 = candidate.7;
            crate::ffi::set_focused_display_handle(handle);
        }
        self.pane_recovery.coordinator = next_coordinator;
        self.pane_recovery.complete(target);
        self.pane_recovery.episode_generation = None;
        self.pane_recovery.expectations.remove(&pane);
        Ok(())
    }
}
