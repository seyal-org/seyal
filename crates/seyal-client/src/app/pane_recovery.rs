//! Serial, binding-fenced queue for per-Pane attachment recovery.

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use seyal_core::{AttachmentId, ExecutionId, PaneId};

use super::{AppError, ApplicationRoot, BindingEvidence};

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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PaneRecoveryExpectation {
    pub runtime_id: u128,
    pub attachment_id: AttachmentId,
}

impl PaneRecoveryQueue {
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
            self.pane_recovery_expectations.remove(&pane);
            self.pane_recovery_retry_generations.remove(&pane);
            self.pane_runtime_ids.remove(&pane);
            if active_was_replaced {
                self.cancel_pane_recovery_episode();
            }
        }
        current
    }

    pub(crate) fn pane_recovery_target(&self) -> Option<PaneRecoveryTarget> {
        (self.pane_recovery_episode_generation == Some(self.recovery.state().generation))
            .then(|| self.pane_recovery.active())
            .flatten()
    }

    pub(crate) fn pane_recovery_target_for_pane(
        &self,
        pane: PaneId,
    ) -> Option<(PaneRecoveryTarget, u64, bool)> {
        let target = self.pane_recovery.current_target(pane)?;
        self.pane_recovery_expectations.get(&pane)?;

        if self.pane_recovery.active() == Some(target) {
            return (self.pane_recovery_episode_generation
                == Some(self.recovery.state().generation))
            .then_some((target, self.recovery.state().generation, false));
        }
        if self.pane_recovery.is_pending(target) {
            return None;
        }

        if let Some(generation) = self.pane_recovery_retry_generations.get(&pane) {
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
        self.pane_recovery_expectations.insert(pane, expectation);
        self.pane_runtime_ids.insert(pane, expectation.runtime_id);
        self.pane_recovery.record_loss(target);
    }

    pub(crate) fn begin_next_pane_recovery(&mut self, now: Duration) -> Option<PaneRecoveryTarget> {
        if let Some(active) = self.pane_recovery.active() {
            if self.pane_recovery_episode_generation == Some(self.recovery.state().generation) {
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
            self.pane_recovery_episode_generation = None;
        }
        if self.primary_recovery_in_progress() {
            return None;
        }
        let target = self.pane_recovery.start_next()?;
        self.pending_recovery = self.recovery.begin_episode(now);
        self.pane_recovery_episode_generation = Some(self.recovery.state().generation);
        Some(target)
    }

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
        let explicitly_aborted = self.pane_recovery_retry_generations.get(&target.pane_id())
            == Some(&recovery_generation);
        if !naturally_retryable && !explicitly_aborted {
            return false;
        }
        if !self.pane_recovery.is_current(target)
            || !self
                .pane_recovery_expectations
                .contains_key(&target.pane_id())
        {
            return false;
        }
        let enqueued = self.pane_recovery.record_loss(target);
        if enqueued {
            self.pane_recovery_retry_generations
                .remove(&target.pane_id());
        }
        enqueued
    }

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
            .pane_recovery_expectations
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
        if self.primary_pane == Some(pane) && self.client_handle.is_none() {
            self.client_handle = Some(crate::ffi::ClientRegistryHandle::new(handle));
        } else {
            self.extra_pane_clients
                .insert(pane, crate::ffi::ClientRegistryHandle::new(handle));
        }
        self.pane_runtime_ids.insert(pane, candidate.0);
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
        self.pane_recovery_episode_generation = None;
        self.pane_recovery_expectations.remove(&pane);
        Ok(())
    }

    pub(crate) fn finish_pane_recovery_if_terminal(&mut self) {
        use crate::recovery::RecoveryStage;
        if matches!(
            self.recovery.state().stage,
            RecoveryStage::Exhausted | RecoveryStage::Blocked | RecoveryStage::ExecutionEnded
        ) && let Some(target) = self.pane_recovery.active()
            && self.pane_recovery_episode_generation == Some(self.recovery.state().generation)
        {
            self.pane_recovery.complete(target);
            self.pane_recovery_episode_generation = None;
        }
    }

    /// Retires a rejected attempt only if the callback still names the exact
    /// active binding and primary-coordinator generation. Retain the recorded
    /// loss expectation so the host can offer an explicit retry.
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
        self.pane_recovery_retry_generations
            .insert(target.pane_id(), recovery_generation);
        self.pane_recovery.complete(target);
        self.pane_recovery_episode_generation = None;
        self.cancel_pane_recovery_episode();
        true
    }

    pub(crate) fn invalidate_pane_recovery(&mut self, pane: PaneId) {
        let active_removed = self
            .pane_recovery
            .active()
            .is_some_and(|active| active.pane_id() == pane);
        self.pane_recovery.remove_pane(pane);
        self.pane_recovery_expectations.remove(&pane);
        self.pane_recovery_retry_generations.remove(&pane);
        self.pane_runtime_ids.remove(&pane);
        if active_removed {
            self.cancel_pane_recovery_episode();
        }
    }

    fn cancel_pane_recovery_episode(&mut self) {
        self.pane_recovery_episode_generation = None;
        self.recovery.cancel();
        self.pending_recovery
            .retain(|effect| matches!(effect, crate::recovery::RecoveryEffect::DisposeHandle(_)));
    }
}

#[cfg(test)]
mod tests {
    use super::{ApplicationRoot, PaneRecoveryExpectation, PaneRecoveryQueue, PaneRecoveryTarget};
    use seyal_core::{AttachmentId, ExecutionId, PaneId};

    fn target(pane: u8, execution: u8, generation: u64) -> PaneRecoveryTarget {
        PaneRecoveryTarget::new(
            PaneId::from_bytes([pane; 16]),
            ExecutionId::from_bytes([execution; 16]),
            generation,
        )
    }

    #[test]
    fn loss_targets_the_exact_current_pane_execution_and_binding_generation() {
        let mut queue = PaneRecoveryQueue::default();
        let lost = target(1, 11, 4);
        queue.set_binding(lost);
        queue.set_binding(target(2, 22, 7));

        assert!(queue.record_loss(lost));
        assert_eq!(queue.start_next(), Some(lost));
        assert_eq!(queue.active(), Some(lost));
    }

    #[test]
    fn duplicate_loss_for_the_same_binding_is_deduplicated() {
        let mut queue = PaneRecoveryQueue::default();
        let lost = target(1, 11, 4);
        queue.set_binding(lost);

        assert!(queue.record_loss(lost));
        assert!(!queue.record_loss(lost));
        assert_eq!(queue.pending_len(), 1);
        assert_eq!(queue.start_next(), Some(lost));
        assert!(!queue.record_loss(lost));
    }

    #[test]
    fn multiple_lost_panes_are_started_serially_in_loss_order() {
        let mut queue = PaneRecoveryQueue::default();
        let first = target(1, 11, 4);
        let second = target(2, 22, 7);
        queue.set_binding(first);
        queue.set_binding(second);
        queue.record_loss(first);
        queue.record_loss(second);

        assert_eq!(queue.start_next(), Some(first));
        assert_eq!(queue.start_next(), None, "only one recovery may be active");
        assert!(queue.complete(first));
        assert_eq!(queue.start_next(), Some(second));
    }

    #[test]
    fn completion_is_fenced_to_the_active_target_and_current_binding() {
        let mut queue = PaneRecoveryQueue::default();
        let active = target(1, 11, 4);
        let other = target(2, 22, 7);
        queue.set_binding(active);
        queue.set_binding(other);
        queue.record_loss(active);
        queue.record_loss(other);
        queue.start_next();

        assert!(!queue.complete(other));
        assert_eq!(queue.active(), Some(active));
        assert!(queue.complete(active));
    }

    #[test]
    fn removal_invalidates_pending_and_active_recovery_targets() {
        let mut queue = PaneRecoveryQueue::default();
        let active = target(1, 11, 4);
        let pending = target(2, 22, 7);
        queue.set_binding(active);
        queue.set_binding(pending);
        queue.record_loss(active);
        queue.record_loss(pending);
        queue.start_next();

        queue.remove_pane(active.pane_id());
        assert!(!queue.complete(active));
        assert_eq!(queue.pending_len(), 1);
        assert_eq!(queue.start_next(), Some(pending));

        queue.remove_pane(pending.pane_id());
        assert!(!queue.complete(pending));
        assert_eq!(queue.start_next(), None);
    }

    #[test]
    fn rebinding_invalidates_stale_work_and_rejects_its_late_completion() {
        let mut queue = PaneRecoveryQueue::default();
        let old = target(1, 11, 4);
        let new = target(1, 33, 5);
        let sibling = target(2, 22, 7);
        queue.set_binding(old);
        queue.set_binding(sibling);
        queue.record_loss(old);
        queue.record_loss(sibling);
        queue.start_next();

        queue.set_binding(new);
        assert!(!queue.complete(old));
        assert_eq!(queue.active(), None);
        assert_eq!(queue.pending_len(), 1, "the sibling remains queued");
        assert!(queue.record_loss(new));
        assert_eq!(queue.start_next(), Some(sibling));
        assert!(queue.complete(sibling));
        assert_eq!(queue.start_next(), Some(new));
    }

    #[test]
    fn binding_generation_changes_only_when_the_execution_binding_changes() {
        let pane = PaneId::from_bytes([1; 16]);
        let first_execution = ExecutionId::from_bytes([11; 16]);
        let second_execution = ExecutionId::from_bytes([12; 16]);
        let mut queue = PaneRecoveryQueue::default();

        let first = queue.bind(pane, first_execution);
        assert_eq!(queue.bind(pane, first_execution), first);
        queue.remove_pane(pane);
        let rebound = queue.bind(pane, second_execution);
        assert_eq!(rebound.binding_generation(), first.binding_generation() + 1);
        assert_eq!(rebound.execution_id(), second_execution);
    }

    #[test]
    fn primary_recovery_blocks_the_serial_pane_queue_without_replacing_its_episode() {
        use std::time::Duration;

        let mut root = ApplicationRoot::new();
        let pane = root.snapshot().shell.focused_pane;
        let execution = ExecutionId::from_bytes([0x77; 16]);
        let target = root.bind_pane_recovery(pane, execution);
        root.pane_recovery_expectations.insert(
            pane,
            PaneRecoveryExpectation {
                runtime_id: 3,
                attachment_id: AttachmentId::from_bytes([0x18; 16]),
            },
        );
        assert!(root.pane_recovery.record_loss(target));
        root.begin_recovery(Duration::ZERO).unwrap();
        let primary_generation = root.recovery.state().generation;

        assert_eq!(
            root.begin_next_pane_recovery(Duration::from_millis(1)),
            None
        );
        assert_eq!(root.recovery.state().generation, primary_generation);
        assert_eq!(root.pane_recovery.pending_len(), 1);
    }

    #[test]
    fn primary_recovery_supersedes_an_active_pane_episode_and_pane_restarts_afterward() {
        use std::time::Duration;

        let mut root = ApplicationRoot::new();
        let pane = root.snapshot().shell.focused_pane;
        let execution = ExecutionId::from_bytes([0x79; 16]);
        let target = root.bind_pane_recovery(pane, execution);
        root.pane_recovery_expectations.insert(
            pane,
            PaneRecoveryExpectation {
                runtime_id: 5,
                attachment_id: AttachmentId::from_bytes([0x20; 16]),
            },
        );
        root.pane_recovery.record_loss(target);
        assert_eq!(root.begin_next_pane_recovery(Duration::ZERO), Some(target));
        let pane_generation = root.recovery.state().generation;
        assert_eq!(root.pane_recovery_target(), Some(target));

        root.begin_recovery(Duration::from_millis(1)).unwrap();
        let primary_generation = root.recovery.state().generation;
        assert_ne!(primary_generation, pane_generation);
        assert_eq!(
            root.begin_next_pane_recovery(Duration::from_millis(2)),
            None
        );
        assert_eq!(root.pane_recovery.active(), Some(target));
        assert_eq!(root.pane_recovery_target(), None);

        // Once the primary episode settles, the unchanged Pane binding is
        // re-queued and gets its own new coordinator generation.
        root.recovery.cancel();
        assert_eq!(
            root.begin_next_pane_recovery(Duration::from_millis(3)),
            Some(target)
        );
        assert_ne!(root.recovery.state().generation, primary_generation);
        assert_eq!(root.pane_recovery_target(), Some(target));
    }

    #[test]
    fn retryable_attempts_keep_target_and_exhaustion_releases_only_its_slot() {
        use crate::recovery::{AttemptOutcome, RecoveryEffect};
        use std::time::Duration;

        let mut root = ApplicationRoot::new();
        let pane = root.snapshot().shell.focused_pane;
        let execution = ExecutionId::from_bytes([0x78; 16]);
        let target = root.bind_pane_recovery(pane, execution);
        root.pane_recovery_expectations.insert(
            pane,
            PaneRecoveryExpectation {
                runtime_id: 4,
                attachment_id: AttachmentId::from_bytes([0x19; 16]),
            },
        );
        root.pane_recovery.record_loss(target);
        assert_eq!(root.begin_next_pane_recovery(Duration::ZERO), Some(target));
        let generation = root.recovery.state().generation;
        let mut now = Duration::ZERO;

        loop {
            assert_eq!(root.pane_recovery.active(), Some(target));
            root.complete_recovery(generation, AttemptOutcome::Retryable, now, None)
                .unwrap();
            if root.recovery.state().stage == crate::recovery::RecoveryStage::Exhausted {
                break;
            }
            let delay = root
                .pending_recovery
                .iter()
                .find_map(|effect| match effect {
                    RecoveryEffect::Schedule { delay, .. } => Some(*delay),
                    _ => None,
                });
            now += delay.expect("retry schedule");
            root.fire_scheduled_recovery(generation, now).unwrap();
        }

        assert_eq!(root.pane_recovery.active(), None);
        assert!(root.pane_recovery_expectations.contains_key(&pane));
        let exhausted_generation = root.recovery.state().generation;
        assert!(root.retry_pane_recovery(target, exhausted_generation));
        assert!(!root.retry_pane_recovery(target, exhausted_generation.wrapping_sub(1)));
        assert_eq!(root.begin_next_pane_recovery(now), Some(target));
    }

    #[test]
    fn rejected_commit_releases_only_the_exact_active_episode_for_retry() {
        use std::time::Duration;

        let mut root = ApplicationRoot::new();
        let pane = root.snapshot().shell.focused_pane;
        let sibling = PaneId::from_bytes([0x33; 16]);
        let target = root.bind_pane_recovery(pane, ExecutionId::from_bytes([0x79; 16]));
        let sibling_target = root.bind_pane_recovery(sibling, ExecutionId::from_bytes([0x7a; 16]));
        root.pane_recovery_expectations.insert(
            pane,
            PaneRecoveryExpectation {
                runtime_id: 5,
                attachment_id: AttachmentId::from_bytes([0x20; 16]),
            },
        );
        root.pane_recovery_expectations.insert(
            sibling,
            PaneRecoveryExpectation {
                runtime_id: 6,
                attachment_id: AttachmentId::from_bytes([0x21; 16]),
            },
        );
        root.pane_recovery.record_loss(target);
        root.pane_recovery.record_loss(sibling_target);
        assert_eq!(root.begin_next_pane_recovery(Duration::ZERO), Some(target));
        let generation = root.recovery.state().generation;

        assert!(!root.abort_pane_recovery_commit(target, generation.wrapping_sub(1)));
        assert_eq!(root.pane_recovery.active(), Some(target));
        assert!(root.abort_pane_recovery_commit(target, generation));
        assert_eq!(root.pane_recovery.active(), None);
        assert_eq!(root.pane_recovery.pending_len(), 1);
        assert!(root.pane_recovery_expectations.contains_key(&pane));
        assert!(root.pane_recovery_expectations.contains_key(&sibling));
        assert_ne!(root.recovery.state().generation, generation);
        assert!(root.retry_pane_recovery(target, generation));
        assert_eq!(root.pane_recovery.pending_len(), 2);
    }

    #[test]
    fn per_pane_target_is_active_then_retryable_and_rebind_invalidates_it() {
        use crate::recovery::{AttemptOutcome, RecoveryEffect, RecoveryStage};
        use std::time::Duration;

        let mut root = ApplicationRoot::new();
        let pane = root.snapshot().shell.focused_pane;
        let execution = ExecutionId::from_bytes([0x7b; 16]);
        let target = root.bind_pane_recovery(pane, execution);
        root.pane_recovery_expectations.insert(
            pane,
            PaneRecoveryExpectation {
                runtime_id: 7,
                attachment_id: AttachmentId::from_bytes([0x22; 16]),
            },
        );
        assert_eq!(root.pane_recovery_target_for_pane(pane), None);
        root.pane_recovery.record_loss(target);
        assert_eq!(root.pane_recovery_target_for_pane(pane), None);

        let mut now = Duration::ZERO;
        assert_eq!(root.begin_next_pane_recovery(now), Some(target));
        let generation = root.recovery.state().generation;
        assert_eq!(
            root.pane_recovery_target_for_pane(pane),
            Some((target, generation, false))
        );

        loop {
            root.complete_recovery(generation, AttemptOutcome::Retryable, now, None)
                .unwrap();
            if root.recovery.state().stage == RecoveryStage::Exhausted {
                break;
            }
            let delay = root
                .pending_recovery
                .iter()
                .find_map(|effect| match effect {
                    RecoveryEffect::Schedule { delay, .. } => Some(*delay),
                    _ => None,
                })
                .expect("retry schedule");
            now += delay;
            root.fire_scheduled_recovery(generation, now).unwrap();
        }

        root.finish_pane_recovery_if_terminal();
        assert_eq!(
            root.pane_recovery_target_for_pane(pane),
            Some((target, generation, true))
        );
        root.bind_pane_recovery(pane, ExecutionId::from_bytes([0x7c; 16]));
        assert_eq!(root.pane_recovery_target_for_pane(pane), None);
    }
}
