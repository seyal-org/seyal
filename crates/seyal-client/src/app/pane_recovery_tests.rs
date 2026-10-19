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
    root.pane_recovery.expectations.insert(
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
    root.pane_recovery.expectations.insert(
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
fn removing_superseded_pane_does_not_cancel_primary_recovery_or_its_effects() {
    use std::time::Duration;

    let mut root = ApplicationRoot::new();
    let pane = root.snapshot().shell.focused_pane;
    let target = root.bind_pane_recovery(pane, ExecutionId::from_bytes([0x91; 16]));
    root.pane_recovery.record_loss(target);
    assert_eq!(root.begin_next_pane_recovery(Duration::ZERO), Some(target));

    root.begin_recovery(Duration::from_millis(1)).unwrap();
    let primary_state = root.recovery.state();
    let primary_effects = root.pending_recovery.clone();
    assert!(!primary_effects.is_empty());

    root.invalidate_pane_recovery(pane);

    assert!(
        root.recovery.state() == primary_state && root.pending_recovery == primary_effects,
        "Pane removal cancelled primary recovery: state {:?} (expected {:?}), effects {:?} (expected {:?})",
        root.recovery.state(),
        primary_state,
        root.pending_recovery,
        primary_effects
    );
}

#[test]
fn rebinding_superseded_pane_does_not_cancel_primary_recovery_or_its_effects() {
    use std::time::Duration;

    let mut root = ApplicationRoot::new();
    let pane = root.snapshot().shell.focused_pane;
    let target = root.bind_pane_recovery(pane, ExecutionId::from_bytes([0x92; 16]));
    root.pane_recovery.record_loss(target);
    assert_eq!(root.begin_next_pane_recovery(Duration::ZERO), Some(target));

    root.begin_recovery(Duration::from_millis(1)).unwrap();
    let primary_state = root.recovery.state();
    let primary_effects = root.pending_recovery.clone();
    assert!(!primary_effects.is_empty());

    root.bind_pane_recovery(pane, ExecutionId::from_bytes([0x93; 16]));

    assert!(
        root.recovery.state() == primary_state && root.pending_recovery == primary_effects,
        "Pane rebind cancelled primary recovery: state {:?} (expected {:?}), effects {:?} (expected {:?})",
        root.recovery.state(),
        primary_state,
        root.pending_recovery,
        primary_effects
    );
}

#[test]
fn retryable_attempts_keep_target_and_exhaustion_releases_only_its_slot() {
    use crate::recovery::{AttemptOutcome, RecoveryEffect};
    use std::time::Duration;

    let mut root = ApplicationRoot::new();
    let pane = root.snapshot().shell.focused_pane;
    let execution = ExecutionId::from_bytes([0x78; 16]);
    let target = root.bind_pane_recovery(pane, execution);
    root.pane_recovery.expectations.insert(
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
    assert!(root.pane_recovery.expectations.contains_key(&pane));
    let exhausted_generation = root.recovery.state().generation;
    assert!(root.retry_pane_recovery(target, exhausted_generation));
    assert!(!root.retry_pane_recovery(target, exhausted_generation.wrapping_sub(1)));
    assert_eq!(root.begin_next_pane_recovery(now), Some(target));
}

#[test]
fn exhausted_pane_recovery_does_not_drop_a_live_sibling_from_the_queue() {
    use crate::recovery::{AttemptOutcome, RecoveryEffect, RecoveryStage};
    use std::time::Duration;

    let mut root = ApplicationRoot::new();
    let pane = root.snapshot().shell.focused_pane;
    let sibling = PaneId::from_bytes([0x34; 16]);
    let failed = root.bind_pane_recovery(pane, ExecutionId::from_bytes([0x81; 16]));
    let sibling_target = root.bind_pane_recovery(sibling, ExecutionId::from_bytes([0x82; 16]));
    for (pane_id, runtime_id, attachment) in [(pane, 8, 0x28), (sibling, 9, 0x29)] {
        root.pane_recovery.expectations.insert(
            pane_id,
            PaneRecoveryExpectation {
                runtime_id,
                attachment_id: AttachmentId::from_bytes([attachment; 16]),
            },
        );
    }
    assert!(root.pane_recovery.record_loss(failed));
    assert!(root.pane_recovery.record_loss(sibling_target));
    assert_eq!(root.begin_next_pane_recovery(Duration::ZERO), Some(failed));
    let generation = root.recovery.state().generation;
    let mut now = Duration::ZERO;

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
            .expect("bounded retry schedule");
        now += delay;
        root.fire_scheduled_recovery(generation, now).unwrap();
    }

    root.finish_pane_recovery_if_terminal();
    assert_eq!(root.pane_recovery.active(), None);
    assert_eq!(root.pane_recovery.pending_len(), 1);
    assert!(root.pane_recovery.expectations.contains_key(&pane));
    assert!(root.pane_recovery.expectations.contains_key(&sibling));
    assert_eq!(root.begin_next_pane_recovery(now), Some(sibling_target));
    assert_eq!(root.pane_recovery.active(), Some(sibling_target));
}

#[test]
fn rejected_commit_releases_only_the_exact_active_episode_for_retry() {
    use std::time::Duration;

    let mut root = ApplicationRoot::new();
    let pane = root.snapshot().shell.focused_pane;
    let sibling = PaneId::from_bytes([0x33; 16]);
    let target = root.bind_pane_recovery(pane, ExecutionId::from_bytes([0x79; 16]));
    let sibling_target = root.bind_pane_recovery(sibling, ExecutionId::from_bytes([0x7a; 16]));
    root.pane_recovery.expectations.insert(
        pane,
        PaneRecoveryExpectation {
            runtime_id: 5,
            attachment_id: AttachmentId::from_bytes([0x20; 16]),
        },
    );
    root.pane_recovery.expectations.insert(
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
    assert!(root.pane_recovery.expectations.contains_key(&pane));
    assert!(root.pane_recovery.expectations.contains_key(&sibling));
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
    root.pane_recovery.expectations.insert(
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
