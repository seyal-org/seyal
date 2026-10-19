//! Serial, binding-fenced queue for per-Pane attachment recovery.

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use crate::shell::ShellSnapshot;
use seyal_core::{AttachmentId, ExecutionId, PaneId};

use super::ApplicationRoot;
#[cfg(target_os = "macos")]
use super::{AppError, BindingEvidence};

/// Identifies one Pane binding that may be recovered after attachment loss.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PaneRecoveryTarget {
    pane_id: PaneId,
    execution_id: ExecutionId,
    binding_generation: u64,
}

impl PaneRecoveryTarget {
    pub(crate) const fn new(
        pane_id: PaneId,
        execution_id: ExecutionId,
        binding_generation: u64,
    ) -> Self {
        Self {
            pane_id,
            execution_id,
            binding_generation,
        }
    }

    pub(crate) const fn pane_id(self) -> PaneId {
        self.pane_id
    }

    pub(crate) const fn execution_id(self) -> ExecutionId {
        self.execution_id
    }

    #[cfg(any(target_os = "macos", test))]
    pub(crate) const fn binding_generation(self) -> u64 {
        self.binding_generation
    }
}

/// Owns pending and in-flight Pane recovery work for one client session.
///
/// The queue serializes attempts and accepts work only for each Pane's current
/// exact execution binding. A rebind or removal invalidates both queued work
/// and an in-flight target, so a late completion cannot affect a newer binding.
#[derive(Debug, Default)]
pub(super) struct PaneRecoveryQueue {
    bindings: HashMap<PaneId, PaneRecoveryTarget>,
    latest_generation: HashMap<PaneId, u64>,
    pending: VecDeque<PaneRecoveryTarget>,
    active: Option<PaneRecoveryTarget>,
    pub(super) episode_generation: Option<u64>,
    pub(super) expectations: HashMap<PaneId, PaneRecoveryExpectation>,
    pub(super) retry_generations: HashMap<PaneId, u64>,
    pub(super) runtime_ids: HashMap<PaneId, u128>,
    pub(super) primary_pane: Option<PaneId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PaneRecoveryExpectation {
    pub runtime_id: u128,
    pub attachment_id: AttachmentId,
}

impl PaneRecoveryQueue {
    pub(super) fn from_shell_snapshot(snapshot: &ShellSnapshot) -> Self {
        Self::from_bindings(
            snapshot
                .panes
                .iter()
                .filter_map(|pane| pane.execution.map(|execution| (pane.id, execution))),
        )
    }

    fn from_bindings(bindings: impl IntoIterator<Item = (PaneId, ExecutionId)>) -> Self {
        let mut queue = Self::default();
        for (pane_id, execution_id) in bindings {
            queue.bind(pane_id, execution_id);
        }
        queue
    }

    /// Binds a Pane to an execution and returns its current exact target.
    /// Repeating the same binding preserves its generation; changing the
    /// execution advances the Pane-local generation.
    pub(super) fn bind(
        &mut self,
        pane_id: PaneId,
        execution_id: ExecutionId,
    ) -> PaneRecoveryTarget {
        if let Some(current) = self.bindings.get(&pane_id)
            && current.execution_id == execution_id
        {
            return *current;
        }

        let generation = self
            .latest_generation
            .get(&pane_id)
            .copied()
            .unwrap_or(0)
            .wrapping_add(1)
            .max(1);
        let target = PaneRecoveryTarget::new(pane_id, execution_id, generation);
        self.set_binding(target);
        target
    }

    /// Records the current binding. Replacing a Pane binding invalidates all
    /// outstanding work for its previous execution/generation.
    pub(super) fn set_binding(&mut self, target: PaneRecoveryTarget) -> bool {
        if self.bindings.get(&target.pane_id) == Some(&target) {
            return false;
        }

        self.invalidate_pane(target.pane_id);
        self.latest_generation
            .entry(target.pane_id)
            .and_modify(|generation| *generation = (*generation).max(target.binding_generation))
            .or_insert(target.binding_generation);
        self.bindings.insert(target.pane_id, target);
        true
    }

    pub(super) fn current_target(&self, pane_id: PaneId) -> Option<PaneRecoveryTarget> {
        self.bindings.get(&pane_id).copied()
    }

    #[cfg(any(target_os = "macos", test))]
    pub(super) fn is_current(&self, target: PaneRecoveryTarget) -> bool {
        self.bindings.get(&target.pane_id) == Some(&target)
    }

    /// Removes a Pane binding and invalidates its queued or active recovery.
    pub(super) fn remove_pane(&mut self, pane_id: PaneId) -> bool {
        let was_bound = self.bindings.remove(&pane_id).is_some();
        let had_work = self.invalidate_pane(pane_id);
        was_bound || had_work
    }

    /// Enqueues one loss if it belongs to the exact current binding and is not
    /// already pending or active. Returns whether new work was added.
    #[cfg(any(target_os = "macos", test))]
    pub(super) fn record_loss(&mut self, target: PaneRecoveryTarget) -> bool {
        if self.bindings.get(&target.pane_id) != Some(&target)
            || self.active == Some(target)
            || self.pending.contains(&target)
        {
            return false;
        }
        self.pending.push_back(target);
        true
    }

    /// Claims the next pending target only when no attempt is currently active.
    #[cfg(any(target_os = "macos", test))]
    pub(super) fn start_next(&mut self) -> Option<PaneRecoveryTarget> {
        if self.active.is_some() {
            return None;
        }

        while let Some(target) = self.pending.pop_front() {
            if self.bindings.get(&target.pane_id) == Some(&target) {
                self.active = Some(target);
                return Some(target);
            }
        }
        None
    }

    /// Completes only the currently active, still-current exact target.
    /// Stale or duplicate completions leave the active attempt untouched.
    pub(super) fn complete(&mut self, target: PaneRecoveryTarget) -> bool {
        if self.active != Some(target) || self.bindings.get(&target.pane_id) != Some(&target) {
            return false;
        }
        self.active = None;
        true
    }

    pub(super) const fn active(&self) -> Option<PaneRecoveryTarget> {
        self.active
    }

    #[cfg(test)]
    pub(super) fn pending_len(&self) -> usize {
        self.pending.len()
    }

    #[cfg(any(target_os = "macos", test))]
    pub(super) fn is_pending(&self, target: PaneRecoveryTarget) -> bool {
        self.pending.contains(&target)
    }

    fn invalidate_pane(&mut self, pane_id: PaneId) -> bool {
        let old_len = self.pending.len();
        self.pending.retain(|target| target.pane_id != pane_id);
        let active_removed = self.active.is_some_and(|target| {
            if target.pane_id == pane_id {
                self.active = None;
                true
            } else {
                false
            }
        });
        active_removed || self.pending.len() != old_len
    }
}

impl ApplicationRoot {
    #[cfg(any(target_os = "macos", test))]
    pub(super) fn remember_pane_runtime(&mut self, pane: PaneId, runtime_id: u128) {
        self.pane_recovery.runtime_ids.insert(pane, runtime_id);
        self.remember_primary_pane(pane);
    }

    pub(super) fn remember_primary_pane(&mut self, pane: PaneId) {
        if self.pane_recovery.primary_pane.is_none() {
            self.pane_recovery.primary_pane = Some(pane);
        }
    }

    pub(super) fn bind_pane_recovery(
        &mut self,
        pane: PaneId,
        execution: ExecutionId,
    ) -> PaneRecoveryTarget {
        let previous = self.pane_recovery.current_target(pane);
        let active_was_replaced = self
            .pane_recovery
            .active()
            .is_some_and(|active| active.pane_id() == pane && active.execution_id() != execution);
        let current = self.pane_recovery.bind(pane, execution);
        if previous != Some(current) {
            self.pane_recovery.expectations.remove(&pane);
            self.pane_recovery.retry_generations.remove(&pane);
            self.pane_recovery.runtime_ids.remove(&pane);
            if active_was_replaced {
                self.cancel_pane_recovery_episode();
            }
        }
        current
    }

    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn pane_recovery_target(&self) -> Option<PaneRecoveryTarget> {
        (self.pane_recovery.episode_generation == Some(self.recovery.state().generation))
            .then(|| self.pane_recovery.active())
            .flatten()
    }

    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn pane_recovery_target_for_pane(
        &self,
        pane: PaneId,
    ) -> Option<(PaneRecoveryTarget, u64, bool)> {
        let target = self.pane_recovery.current_target(pane)?;
        self.pane_recovery.expectations.get(&pane)?;

        if self.pane_recovery.active() == Some(target) {
            return (self.pane_recovery.episode_generation
                == Some(self.recovery.state().generation))
            .then_some((target, self.recovery.state().generation, false));
        }
        if self.pane_recovery.is_pending(target) {
            return None;
        }

        if let Some(generation) = self.pane_recovery.retry_generations.get(&pane) {
            return (*generation != 0).then_some((target, *generation, true));
        }

        if matches!(
            self.recovery.state().stage,
            crate::recovery::RecoveryStage::Exhausted
                | crate::recovery::RecoveryStage::Blocked
                | crate::recovery::RecoveryStage::ExecutionEnded
        ) {
            return Some((target, self.recovery.state().generation, true));
        }
        None
    }

    #[cfg(target_os = "macos")]
    pub(super) fn note_pane_attachment_loss(&mut self, pane: PaneId, handle: u64) {
        if self
            .extra_pane_clients
            .get(&pane)
            .is_none_or(|owned| owned.raw() != handle)
        {
            // The create-admitting primary connection keeps its existing
            // whole-session recovery path.
            return;
        }
        let Some(target) = self.pane_recovery.current_target(pane) else {
            return;
        };
        let captured = crate::ffi::with_client(handle, |client| PaneRecoveryExpectation {
            runtime_id: client.runtime_id(),
            attachment_id: client.attachment_id(),
        });
        let Some(expectation) = captured else {
            return;
        };
        let shell_execution = self.shell.pane_execution(pane).ok().flatten();
        let authority = self.pane_authorities.get(&pane);
        if shell_execution != Some(target.execution_id())
            || authority.is_none_or(|authority| {
                authority.execution != target.execution_id()
                    || authority.attachment != expectation.attachment_id
            })
        {
            return;
        }
        self.pane_recovery.expectations.insert(pane, expectation);
        self.pane_recovery
            .runtime_ids
            .insert(pane, expectation.runtime_id);
        self.pane_recovery.record_loss(target);
    }

    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn begin_next_pane_recovery(&mut self, now: Duration) -> Option<PaneRecoveryTarget> {
        if let Some(active) = self.pane_recovery.active() {
            if self.pane_recovery.episode_generation == Some(self.recovery.state().generation) {
                return Some(active);
            }
            // The shared coordinator generation changed while this Pane's
            // async open was outstanding. If primary recovery owns it, let
            // the primary effect proceed and keep this target queued for a
            // fresh bounded Pane episode after primary recovery settles.
            if self.primary_recovery_in_progress() {
                return None;
            }
            self.pane_recovery.complete(active);
            self.pane_recovery.record_loss(active);
            self.pane_recovery.episode_generation = None;
        }
        if self.primary_recovery_in_progress() {
            return None;
        }
        let target = self.pane_recovery.start_next()?;
        self.pending_recovery = self.recovery.begin_episode(now);
        self.pane_recovery.episode_generation = Some(self.recovery.state().generation);
        Some(target)
    }

    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn retry_pane_recovery(
        &mut self,
        target: PaneRecoveryTarget,
        recovery_generation: u64,
    ) -> bool {
        use crate::recovery::RecoveryStage;

        let state = self.recovery.state();
        let naturally_retryable = state.generation == recovery_generation
            && matches!(
                state.stage,
                RecoveryStage::Exhausted | RecoveryStage::Blocked | RecoveryStage::ExecutionEnded
            );
        let explicitly_aborted = self.pane_recovery.retry_generations.get(&target.pane_id())
            == Some(&recovery_generation);
        if !naturally_retryable && !explicitly_aborted {
            return false;
        }
        if !self.pane_recovery.is_current(target)
            || !self
                .pane_recovery
                .expectations
                .contains_key(&target.pane_id())
        {
            return false;
        }
        let enqueued = self.pane_recovery.record_loss(target);
        if enqueued {
            self.pane_recovery
                .retry_generations
                .remove(&target.pane_id());
        }
        enqueued
    }

    #[cfg(any(target_os = "macos", test))]
    fn primary_recovery_in_progress(&self) -> bool {
        use crate::recovery::RecoveryStage;
        match self.recovery.state().stage {
            RecoveryStage::Discovering
            | RecoveryStage::StartingRuntime
            | RecoveryStage::WaitingForController
            | RecoveryStage::Reconstructing
            | RecoveryStage::RestoringInteraction => true,
            RecoveryStage::Usable => !self.reconstruction.can_mutate(),
            RecoveryStage::Disconnected
            | RecoveryStage::Exhausted
            | RecoveryStage::Blocked
            | RecoveryStage::ExecutionEnded => false,
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn commit_pane_recovery(
        &mut self,
        target: PaneRecoveryTarget,
        recovery_generation: u64,
        handle: u64,
    ) -> Result<(), AppError> {
        use seyal_runtime::local_ipc::framing::Role;

        if self.pane_recovery.active() != Some(target)
            || self.recovery.state().generation != recovery_generation
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
                super::session::project_cache_text(cache),
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
            self.recovery.state().stage,
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
        let evidence = BindingEvidence {
            execution: target.execution_id(),
            attachment: candidate.2,
            controller: true,
            pty_generation: candidate.6.max(1),
            alternate_screen: candidate.5,
        };
        self.pane_client_raws.insert(pane, handle);
        if self.pane_recovery.primary_pane == Some(pane) && self.client_handle.is_none() {
            self.client_handle = Some(crate::ffi::ClientRegistryHandle::new(handle));
        } else {
            self.extra_pane_clients
                .insert(pane, crate::ffi::ClientRegistryHandle::new(handle));
        }
        self.pane_recovery.runtime_ids.insert(pane, candidate.0);
        self.pane_authorities.insert(
            pane,
            super::PaneAuthority {
                pane,
                execution: evidence.execution,
                attachment: evidence.attachment,
                controller: evidence.controller,
                pty_generation: evidence.pty_generation,
            },
        );
        if self.shell.snapshot().focused_pane == pane {
            let identity = crate::presentation::PresentationIdentity::new(
                evidence.execution,
                evidence.pty_generation,
            )
            .ok_or(AppError::ZeroPtyGeneration)?;
            self.presentation
                .apply(crate::presentation::PresentationAction::BindIdentity(
                    identity,
                ))
                .map_err(|_| AppError::AlreadyBound)?;
            self.authority = self.pane_authorities.get(&pane).copied();
            self.derive_presentation(evidence.alternate_screen)?;
            self.sync_composer_presentation();
            self.output_utf8 = candidate.7;
            crate::ffi::set_focused_display_handle(handle);
        }
        if !self
            .recovery
            .advance_presentation_stage(crate::recovery::RecoveryStage::Usable)
        {
            return Err(AppError::InvalidPayload);
        }
        self.pane_recovery.complete(target);
        self.pane_recovery.episode_generation = None;
        self.pane_recovery.expectations.remove(&pane);
        Ok(())
    }

    pub(crate) fn finish_pane_recovery_if_terminal(&mut self) {
        use crate::recovery::RecoveryStage;
        if matches!(
            self.recovery.state().stage,
            RecoveryStage::Exhausted | RecoveryStage::Blocked | RecoveryStage::ExecutionEnded
        ) && let Some(target) = self.pane_recovery.active()
            && self.pane_recovery.episode_generation == Some(self.recovery.state().generation)
        {
            self.pane_recovery.complete(target);
            self.pane_recovery.episode_generation = None;
        }
    }

    /// Retires a rejected attempt only if the callback still names the exact
    /// active binding and primary-coordinator generation. Retain the recorded
    /// loss expectation so the host can offer an explicit retry.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn abort_pane_recovery_commit(
        &mut self,
        target: PaneRecoveryTarget,
        recovery_generation: u64,
    ) -> bool {
        if self.pane_recovery.active() != Some(target)
            || !self.pane_recovery.is_current(target)
            || self.recovery.state().generation != recovery_generation
        {
            return false;
        }
        self.pane_recovery
            .retry_generations
            .insert(target.pane_id(), recovery_generation);
        self.pane_recovery.complete(target);
        self.cancel_pane_recovery_episode();
        true
    }

    pub(crate) fn invalidate_pane_recovery(&mut self, pane: PaneId) {
        let active_removed = self
            .pane_recovery
            .active()
            .is_some_and(|active| active.pane_id() == pane);
        self.pane_recovery.remove_pane(pane);
        self.pane_recovery.expectations.remove(&pane);
        self.pane_recovery.retry_generations.remove(&pane);
        self.pane_recovery.runtime_ids.remove(&pane);
        if active_removed {
            self.cancel_pane_recovery_episode();
        }
    }

    fn cancel_pane_recovery_episode(&mut self) {
        let owns_coordinator =
            self.pane_recovery.episode_generation == Some(self.recovery.state().generation);
        self.pane_recovery.episode_generation = None;
        if !owns_coordinator {
            return;
        }
        self.recovery.cancel();
        self.pending_recovery
            .retain(|effect| matches!(effect, crate::recovery::RecoveryEffect::DisposeHandle(_)));
    }
}

#[cfg(test)]
#[path = "pane_recovery_tests.rs"]
mod tests;
