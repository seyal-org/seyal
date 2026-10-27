use super::{ApplicationRoot, PaneRecoveryExpectation, PaneRecoveryQueue, PaneRecoveryTarget};
use crate::app::{AppAction, BindingEvidence};
use seyal_core::{AttachmentId, ExecutionId, PaneId};

fn make_primary_usable(root: &mut ApplicationRoot) {
    use crate::recovery::{AttemptOutcome, RecoveryStage};
    use std::time::Duration;

    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: BindingEvidence {
            execution: ExecutionId::from_bytes([0x41; 16]),
            attachment: AttachmentId::from_bytes([0x42; 16]),
            controller: true,
            pty_generation: 1,
            alternate_screen: false,
        },
    })
    .unwrap();
    root.apply(AppAction::BeginRecovery {
        now: Duration::ZERO,
    })
    .unwrap();
    let generation = root.snapshot().recovery_generation;
    root.apply(AppAction::CompleteRecovery {
        generation,
        outcome: AttemptOutcome::Opened {
            handle: 0x43,
            adopted: true,
        },
        now: Duration::from_millis(1),
        launch: None,
    })
    .unwrap();
    root.apply(AppAction::AdvanceRecoveryStage {
        stage: RecoveryStage::RestoringInteraction,
    })
    .unwrap();
    root.apply(AppAction::AdvanceRecoveryStage {
        stage: RecoveryStage::Usable,
    })
    .unwrap();
    root.apply(AppAction::BeginReconstructionAttempt).unwrap();
    root.apply(AppAction::CommitReconstruction {
        runtime: crate::recovery::ContinuityIdentity { low: 1, high: 2 },
        execution: crate::recovery::ContinuityIdentity { low: 3, high: 4 },
        attachment: crate::recovery::ContinuityIdentity { low: 5, high: 6 },
        controller_authority_committed: true,
        authoritative_snapshot_committed: true,
    })
    .unwrap();
    root.apply(AppAction::ApplyRuntimeComposerStatus {
        fence: root.fence(),
        eligibility: Some(crate::composer::RuntimeComposerEligibility::Available),
        revision: 1,
    })
    .unwrap();
    root.apply(AppAction::SelectRestingPresentation {
        fence: root.fence(),
        raw: false,
    })
    .unwrap();
    assert!(
        root.snapshot().composer_eligible,
        "focused primary Pane must be healthy"
    );
}

fn queue_secondary_loss(root: &mut ApplicationRoot, secondary: PaneId) -> PaneRecoveryTarget {
    let target = root.bind_pane_recovery(secondary, ExecutionId::from_bytes([0x55; 16]));
    root.pane_recovery.expectations.insert(
        secondary,
        PaneRecoveryExpectation {
            runtime_id: 31,
            attachment_id: AttachmentId::from_bytes([0x56; 16]),
        },
    );
    assert!(root.pane_recovery.record_loss(target));
    target
}

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
    let pane_generation = root.pane_recovery_state().generation;
    assert_eq!(root.pane_recovery_target(), Some(target));

    root.begin_recovery(Duration::from_millis(1)).unwrap();
    let primary_generation = root.recovery.state().generation;
    assert_eq!(
        primary_generation, pane_generation,
        "equal numeric generations in separate scopes must remain independent"
    );
    assert_eq!(
        root.begin_next_pane_recovery(Duration::from_millis(2)),
        None
    );
    assert_eq!(root.pane_recovery.active(), None);
    assert_eq!(root.pane_recovery.pending_len(), 1);
    assert_eq!(root.pane_recovery_target(), None);

    // Once the primary episode settles, the unchanged Pane binding is
    // re-queued and gets its own new coordinator generation.
    root.recovery.cancel();
    assert_eq!(
        root.begin_next_pane_recovery(Duration::from_millis(3)),
        Some(target)
    );
    assert_ne!(root.pane_recovery_state().generation, primary_generation);
    assert_eq!(root.pane_recovery_target(), Some(target));
}

#[test]
fn primary_supersession_rejects_late_pane_completion_without_advancing_primary() {
    use std::time::Duration;

    let mut root = ApplicationRoot::new();
    let pane = root.snapshot().shell.focused_pane;
    let target = root.bind_pane_recovery(pane, ExecutionId::from_bytes([0x94; 16]));
    root.pane_recovery.record_loss(target);
    assert_eq!(root.begin_next_pane_recovery(Duration::ZERO), Some(target));

    root.begin_recovery(Duration::from_millis(1)).unwrap();
    let primary_state = root.recovery.state();
    let primary_effects = root.pending_recovery.clone();

    // Swift compares the complete target pin, disconnects an opened stale
    // handle directly, and does not send this completion into Rust.
    assert_ne!(root.pane_recovery.active(), Some(target));
    assert_eq!(root.recovery.state(), primary_state);
    assert_eq!(root.pending_recovery, primary_effects);
    assert!(root.pane_recovery.pending_effects.is_empty());
}

#[test]
fn pane_effect_and_ack_reject_a_stale_numeric_generation() {
    use crate::recovery::RecoveryEffect;
    use std::time::Duration;

    let mut root = ApplicationRoot::new();
    let pane = root.snapshot().shell.focused_pane;
    let target = root.bind_pane_recovery(pane, ExecutionId::from_bytes([0x96; 16]));
    root.pane_recovery.record_loss(target);
    assert_eq!(root.begin_next_pane_recovery(Duration::ZERO), Some(target));
    let generation = root.pane_recovery_state().generation;
    assert!(matches!(
        root.pane_recovery_effect(target, generation),
        Some(RecoveryEffect::PerformAttempt { .. })
    ));
    assert_eq!(
        root.pane_recovery_effect(target, generation.wrapping_sub(1)),
        None
    );
    root.ack_pane_recovery_effect(target, generation.wrapping_sub(1));
    assert!(
        root.pane_recovery_effect(target, generation).is_some(),
        "stale ack must leave the active effect queued"
    );
}

#[test]
fn duplicate_pane_completion_keeps_the_retry_schedule_queued() {
    use crate::recovery::{AttemptOutcome, RecoveryEffect};
    use std::time::Duration;

    let mut root = ApplicationRoot::new();
    let pane = root.snapshot().shell.focused_pane;
    let target = root.bind_pane_recovery(pane, ExecutionId::from_bytes([0x97; 16]));
    root.pane_recovery.record_loss(target);
    assert_eq!(root.begin_next_pane_recovery(Duration::ZERO), Some(target));
    let generation = root.pane_recovery_state().generation;
    root.complete_pane_recovery(
        target,
        generation,
        AttemptOutcome::Retryable,
        Duration::from_millis(1),
        None,
    )
    .unwrap();
    let queued = root.pane_recovery.pending_effects.clone();
    assert!(matches!(
        queued.first(),
        Some(RecoveryEffect::Schedule { .. })
    ));

    root.complete_pane_recovery(
        target,
        generation,
        AttemptOutcome::Retryable,
        Duration::from_millis(2),
        None,
    )
    .unwrap();

    assert_eq!(root.pane_recovery.pending_effects, queued);
    assert!(matches!(
        root.pane_recovery_effect(target, generation),
        Some(RecoveryEffect::Schedule { .. })
    ));
}

#[test]
fn opened_rejection_terminalizes_without_stranding_disposal_when_no_sibling_is_active() {
    use crate::recovery::{AttemptOutcome, RecoveryEffect, RecoveryStage};
    use std::time::Duration;

    let mut root = ApplicationRoot::new();
    let pane = root.snapshot().shell.focused_pane;
    let target = root.bind_pane_recovery(pane, ExecutionId::from_bytes([0x98; 16]));
    root.pane_recovery.expectations.insert(
        pane,
        PaneRecoveryExpectation {
            runtime_id: 1,
            attachment_id: AttachmentId::from_bytes([0x99; 16]),
        },
    );
    root.pane_recovery.record_loss(target);
    assert_eq!(root.begin_next_pane_recovery(Duration::ZERO), Some(target));
    let generation = root.pane_recovery_state().generation;

    root.complete_pane_recovery(
        target,
        generation,
        AttemptOutcome::Opened {
            handle: 0x99,
            adopted: false,
        },
        Duration::from_millis(1),
        None,
    )
    .unwrap();

    assert_eq!(root.pane_recovery.active(), None);
    assert_eq!(root.pane_recovery_state().stage, RecoveryStage::Blocked);
    assert!(root
        .pane_recovery
        .pending_effects
        .iter()
        .all(|effect| !matches!(effect, RecoveryEffect::DisposeHandle(_))));
    assert_eq!(
        root.pane_recovery_target_for_pane(pane),
        Some((target, generation, true))
    );
}

#[test]
fn scheduled_fire_at_the_episode_deadline_releases_the_active_pane() {
    use crate::recovery::{AttemptOutcome, RecoveryEffect, RecoveryStage, EPISODE_DEADLINE};
    use std::time::Duration;

    let mut root = ApplicationRoot::new();
    let pane = root.snapshot().shell.focused_pane;
    let target = root.bind_pane_recovery(pane, ExecutionId::from_bytes([0x9a; 16]));
    root.pane_recovery.record_loss(target);
    assert_eq!(root.begin_next_pane_recovery(Duration::ZERO), Some(target));
    let generation = root.pane_recovery_state().generation;
    root.complete_pane_recovery(
        target,
        generation,
        AttemptOutcome::Retryable,
        Duration::from_millis(1),
        None,
    )
    .unwrap();
    assert!(matches!(
        root.pane_recovery.pending_effects.first(),
        Some(RecoveryEffect::Schedule { .. })
    ));

    assert!(root.fire_pane_recovery(target, generation, EPISODE_DEADLINE));

    assert_eq!(root.pane_recovery_state().stage, RecoveryStage::Exhausted);
    assert_eq!(root.pane_recovery.active(), None);
    assert_eq!(root.pane_recovery.pending_len(), 0);
}

#[test]
fn secondary_episode_does_not_replace_a_usable_primary_coordinator() {
    use crate::recovery::RecoveryStage;
    use std::time::Duration;

    let mut root = ApplicationRoot::new();
    make_primary_usable(&mut root);
    let primary = root.recovery.state();
    let secondary = PaneId::from_bytes([0x61; 16]);
    let target = queue_secondary_loss(&mut root, secondary);

    assert_eq!(
        root.begin_next_pane_recovery(Duration::from_millis(2)),
        Some(target)
    );

    assert_eq!(
        root.recovery.state(),
        primary,
        "secondary recovery changed the primary coordinator"
    );
    assert_eq!(root.recovery.state().stage, RecoveryStage::Usable);
    assert!(
        root.snapshot().composer_eligible,
        "focused healthy primary composer became ineligible"
    );
}

#[test]
fn secondary_execution_ended_does_not_end_focused_healthy_primary() {
    use crate::recovery::{AttemptOutcome, RecoveryStage};
    use std::time::Duration;

    let mut root = ApplicationRoot::new();
    make_primary_usable(&mut root);
    let primary = root.recovery.state();
    let primary_effects = root.pending_recovery.clone();
    let focused = root.snapshot().shell.focused_pane;
    let secondary = PaneId::from_bytes([0x62; 16]);
    let target = queue_secondary_loss(&mut root, secondary);
    assert_eq!(
        root.begin_next_pane_recovery(Duration::from_millis(2)),
        Some(target)
    );
    let pane_generation = root.pane_recovery_state().generation;

    root.complete_pane_recovery(
        target,
        pane_generation,
        AttemptOutcome::ExecutionEnded,
        Duration::from_millis(3),
        None,
    )
    .unwrap();

    assert_eq!(root.snapshot().shell.focused_pane, focused);
    assert_eq!(
        root.recovery.state(),
        primary,
        "secondary ExecutionEnded mutated primary recovery state"
    );
    assert_eq!(
        root.pending_recovery, primary_effects,
        "secondary completion replaced primary effects"
    );
    assert_eq!(root.snapshot().recovery_stage, RecoveryStage::Usable);
    assert!(
        root.snapshot().composer_eligible,
        "focused healthy primary composer was disabled by secondary failure"
    );
}

#[test]
fn secondary_success_preserves_primary_disconnected_or_execution_ended_state() {
    use crate::recovery::{AttemptOutcome, RecoveryStage};
    use std::time::Duration;

    let mut failures = Vec::new();
    for primary_stage in [RecoveryStage::Disconnected, RecoveryStage::ExecutionEnded] {
        let mut root = ApplicationRoot::new();
        if primary_stage == RecoveryStage::ExecutionEnded {
            let primary = PaneId::from_bytes([0x63; 16]);
            root.bind_pane_recovery(primary, ExecutionId::from_bytes([0x64; 16]));
            root.begin_recovery(Duration::ZERO).unwrap();
            let generation = root.recovery.state().generation;
            root.complete_recovery(
                generation,
                AttemptOutcome::ExecutionEnded,
                Duration::from_millis(1),
                None,
            )
            .unwrap();
        }
        assert_eq!(root.recovery.state().stage, primary_stage);
        let primary = root.recovery.state();
        let primary_effects = root.pending_recovery.clone();
        let secondary = PaneId::from_bytes([0x65; 16]);
        let target = queue_secondary_loss(&mut root, secondary);

        assert_eq!(
            root.begin_next_pane_recovery(Duration::from_millis(2)),
            Some(target)
        );
        let pane_generation = root.pane_recovery_state().generation;
        root.complete_pane_recovery(
            target,
            pane_generation,
            AttemptOutcome::Opened {
                handle: 0x66,
                adopted: true,
            },
            Duration::from_millis(3),
            None,
        )
        .unwrap();
        assert!(root.advance_pane_recovery_stage(target, RecoveryStage::RestoringInteraction));
        assert!(root.advance_pane_recovery_stage(target, RecoveryStage::Usable));

        if root.recovery.state() != primary || root.pending_recovery != primary_effects {
            failures.push(format!(
                "primary {primary_stage:?}: state {:?} (expected {:?}), effects {:?} (expected {:?})",
                root.recovery.state(),
                primary,
                root.pending_recovery,
                primary_effects
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "secondary success changed primary state: {failures:?}"
    );
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
    let generation = root.pane_recovery_state().generation;
    let mut now = Duration::ZERO;

    loop {
        assert_eq!(root.pane_recovery.active(), Some(target));
        root.complete_pane_recovery(target, generation, AttemptOutcome::Retryable, now, None)
            .unwrap();
        if root.pane_recovery_state().stage == crate::recovery::RecoveryStage::Exhausted {
            break;
        }
        let delay = root
            .pane_recovery
            .pending_effects
            .iter()
            .find_map(|effect| match effect {
                RecoveryEffect::Schedule { delay, .. } => Some(*delay),
                _ => None,
            });
        now += delay.expect("retry schedule");
        assert!(root.fire_pane_recovery(target, generation, now));
    }

    assert_eq!(root.pane_recovery.active(), None);
    assert!(root.pane_recovery.expectations.contains_key(&pane));
    let exhausted_generation = root.pane_recovery.terminal_stages[&pane].0;
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
    let generation = root.pane_recovery_state().generation;
    let mut now = Duration::ZERO;

    loop {
        root.complete_pane_recovery(failed, generation, AttemptOutcome::Retryable, now, None)
            .unwrap();
        if root.pane_recovery_state().stage == RecoveryStage::Exhausted {
            break;
        }
        let delay = root
            .pane_recovery
            .pending_effects
            .iter()
            .find_map(|effect| match effect {
                RecoveryEffect::Schedule { delay, .. } => Some(*delay),
                _ => None,
            })
            .expect("bounded retry schedule");
        now += delay;
        assert!(root.fire_pane_recovery(failed, generation, now));
    }

    root.finish_pane_recovery_if_terminal();
    assert_eq!(root.pane_recovery.active(), None);
    assert_eq!(root.pane_recovery.pending_len(), 1);
    let exhausted_generation = root.pane_recovery.terminal_stages[&pane].0;
    assert!(root.pane_recovery.expectations.contains_key(&pane));
    assert!(root.pane_recovery.expectations.contains_key(&sibling));
    assert_eq!(root.begin_next_pane_recovery(now), Some(sibling_target));
    assert_eq!(root.pane_recovery.active(), Some(sibling_target));
    assert_eq!(
        root.pane_recovery_target_for_pane(pane),
        Some((failed, exhausted_generation, true)),
        "a later Pane episode must not erase the failed Pane's retry disposition"
    );
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
    let generation = root.pane_recovery_state().generation;

    assert!(!root.abort_pane_recovery_commit(target, generation.wrapping_sub(1)));
    assert_eq!(root.pane_recovery.active(), Some(target));
    assert!(root.abort_pane_recovery_commit(target, generation));
    assert_eq!(root.pane_recovery.active(), None);
    assert_eq!(root.pane_recovery.pending_len(), 1);
    assert!(root.pane_recovery.expectations.contains_key(&pane));
    assert!(root.pane_recovery.expectations.contains_key(&sibling));
    assert_ne!(root.pane_recovery_state().generation, generation);
    assert!(root.retry_pane_recovery(target, generation));
    assert_eq!(root.pane_recovery.pending_len(), 2);
}

#[cfg(target_os = "macos")]
#[test]
fn rejected_focused_pane_commit_publishes_no_candidate_state_and_remains_retryable() {
    use crate::presentation::{PresentationIdentity, PresentationMode, PresentationSession};
    use std::time::Duration;

    let mut root = ApplicationRoot::new();
    let pane = root.snapshot().shell.focused_pane;
    let client = crate::app::provisioning_apply::negotiated_provisioning_client();
    let execution = client.execution_id();
    let candidate_attachment = client.attachment_id();
    root.attach_client(root.fence(), client)
        .expect("install initial focused Pane client");
    let target = root.bind_pane_recovery(pane, execution);
    let candidate_runtime = crate::ffi::with_client(
        root.live_client_handle_for_test().expect("initial handle"),
        |client| client.runtime_id(),
    )
    .expect("initial client is registered");
    root.pane_recovery.expectations.insert(
        pane,
        PaneRecoveryExpectation {
            runtime_id: candidate_runtime,
            attachment_id: AttachmentId::from_bytes([0x33; 16]),
        },
    );
    assert_ne!(
        candidate_attachment,
        root.pane_recovery.expectations[&pane].attachment_id
    );
    assert!(root.pane_recovery.record_loss(target));
    assert_eq!(root.begin_next_pane_recovery(Duration::ZERO), Some(target));
    let generation = root.pane_recovery_state().generation;
    root.complete_pane_recovery(
        target,
        generation,
        crate::recovery::AttemptOutcome::Opened {
            handle: 0,
            adopted: true,
        },
        Duration::from_millis(1),
        None,
    )
    .expect("accepted open enters reconstructing stage");

    // Model the prior handle having been disconnected before the replacement
    // arrives, while retaining the old focused authority until commit succeeds.
    let old_handle = root.live_client_handle_for_test().expect("initial handle");
    crate::ffi::unregister_client(old_handle);
    root.client_handle = None;
    root.pane_client_raws.remove(&pane);
    root.pane_recovery.runtime_ids.remove(&pane);
    let old_authority = super::super::PaneAuthority {
        pane,
        execution,
        attachment: AttachmentId::from_bytes([0x33; 16]),
        controller: true,
        pty_generation: 1,
    };
    root.authority = Some(old_authority);
    root.pane_authorities.insert(pane, old_authority);

    let mut candidate = crate::app::provisioning_apply::negotiated_provisioning_client();
    candidate.cache.generation = 1;
    candidate.cache.rows = 24;
    candidate.cache.columns = 80;
    candidate.cache.cells = vec![
        seyal_protocol::display::DisplayCell::blank();
        usize::from(candidate.cache.rows)
            * usize::from(candidate.cache.columns)
    ];
    let candidate_handle = crate::ffi::register_app_client(candidate)
        .expect("register replacement client")
        .raw();
    // A stale focused presentation identity is a fallible BindIdentity case.
    root.presentation = PresentationSession::new(
        Some(PresentationIdentity::new(ExecutionId::from_bytes([0x99; 16]), 1).unwrap()),
        PresentationMode::Flow,
    );

    assert_eq!(
        root.commit_pane_recovery(target, generation, candidate_handle),
        Err(crate::app::AppError::AlreadyBound)
    );
    assert_eq!(root.pane_client_raws.get(&pane), None);
    assert!(root.client_handle.is_none());
    assert!(!root.extra_pane_clients.contains_key(&pane));
    assert_eq!(root.pane_recovery.runtime_ids.get(&pane), None);
    assert_eq!(root.pane_authorities.get(&pane), Some(&old_authority));
    assert_eq!(root.authority, Some(old_authority));
    assert_eq!(root.pane_recovery.active(), Some(target));

    // Match the FFI rejection path: disconnect the rejected candidate, retire
    // the exact attempt, and prove the same binding can be explicitly retried.
    crate::ffi::unregister_client(candidate_handle);
    assert!(root.abort_pane_recovery_commit(target, generation));
    assert_eq!(root.pane_recovery.active(), None);
    assert!(root.retry_pane_recovery(target, generation));
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
    let generation = root.pane_recovery_state().generation;
    assert_eq!(
        root.pane_recovery_target_for_pane(pane),
        Some((target, generation, false))
    );

    loop {
        root.complete_pane_recovery(target, generation, AttemptOutcome::Retryable, now, None)
            .unwrap();
        if root.pane_recovery_state().stage == RecoveryStage::Exhausted {
            break;
        }
        let delay = root
            .pane_recovery
            .pending_effects
            .iter()
            .find_map(|effect| match effect {
                RecoveryEffect::Schedule { delay, .. } => Some(*delay),
                _ => None,
            })
            .expect("retry schedule");
        now += delay;
        assert!(root.fire_pane_recovery(target, generation, now));
    }

    root.finish_pane_recovery_if_terminal();
    assert_eq!(
        root.pane_recovery_target_for_pane(pane),
        Some((target, generation, true))
    );
    root.bind_pane_recovery(pane, ExecutionId::from_bytes([0x7c; 16]));
    assert_eq!(root.pane_recovery_target_for_pane(pane), None);
}
