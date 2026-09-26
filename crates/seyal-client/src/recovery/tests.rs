use super::*;

fn drive_retryable(coordinator: &mut RecoveryCoordinator) -> Vec<Duration> {
    let mut now = Duration::ZERO;
    let mut delays = Vec::new();
    let mut effects = coordinator.begin_episode(now);
    let mut attempts = 0;
    loop {
        let next = match effects.as_slice() {
            [RecoveryEffect::PerformAttempt { generation, .. }] => {
                let generation = *generation;
                attempts += 1;
                coordinator.complete_attempt(generation, AttemptOutcome::Retryable, now, None)
            }
            [RecoveryEffect::Schedule { generation, delay }] => {
                let generation = *generation;
                let delay = *delay;
                delays.push(delay);
                now += delay;
                coordinator.scheduled_fire(generation, now)
            }
            [] => break,
            other => panic!("unexpected effects {other:?}"),
        };
        effects = next;
        if attempts > 20 {
            panic!("did not converge");
        }
    }
    assert_eq!(attempts, MAXIMUM_ATTEMPTS);
    delays
}

#[test]
fn seven_attempt_schedule_exhausts() {
    let mut coordinator = RecoveryCoordinator::default();
    let delays = drive_retryable(&mut coordinator);
    assert_eq!(delays.as_slice(), &RETRY_DELAYS);
    assert_eq!(coordinator.attempt_count(), MAXIMUM_ATTEMPTS);
    assert_eq!(coordinator.state().stage, RecoveryStage::Exhausted);
    assert!(!coordinator.has_scheduled_attempt());
}

#[test]
fn endpoint_missing_launches_once_controller_busy_never_launches() {
    let mut missing = RecoveryCoordinator::default();
    let mut now = Duration::ZERO;
    let mut launches = 0;
    let mut effects = missing.begin_episode(now);
    for _ in 0..20 {
        let next = match effects.as_slice() {
            [RecoveryEffect::PerformAttempt { generation, .. }] => {
                let generation = *generation;
                missing.complete_attempt(
                    generation,
                    AttemptOutcome::EndpointMissing,
                    now,
                    Some(LaunchResult::Started),
                )
            }
            [RecoveryEffect::LaunchHelper { .. }, RecoveryEffect::Schedule { generation, delay }] =>
            {
                let generation = *generation;
                let delay = *delay;
                launches += 1;
                now += delay;
                missing.scheduled_fire(generation, now)
            }
            [RecoveryEffect::Schedule { generation, delay }] => {
                let generation = *generation;
                let delay = *delay;
                now += delay;
                missing.scheduled_fire(generation, now)
            }
            [] => break,
            other => panic!("unexpected {other:?}"),
        };
        effects = next;
    }
    assert_eq!(launches, 1);
    assert_eq!(missing.state().stage, RecoveryStage::Exhausted);

    let mut busy = RecoveryCoordinator::default();
    now = Duration::ZERO;
    effects = busy.begin_episode(now);
    let RecoveryEffect::PerformAttempt {
        generation: busy_generation,
        ..
    } = effects[0]
    else {
        panic!("expected attempt");
    };
    effects = busy.complete_attempt(busy_generation, AttemptOutcome::ControllerBusy, now, None);
    assert_eq!(busy.state().stage, RecoveryStage::WaitingForController);
    while let Some((generation, delay)) = match effects.as_slice() {
        [RecoveryEffect::Schedule { generation, delay }] => Some((*generation, *delay)),
        _ => None,
    } {
        now += delay;
        effects = busy.scheduled_fire(generation, now);
        if let [RecoveryEffect::PerformAttempt { generation, .. }] = effects.as_slice() {
            let generation = *generation;
            effects = busy.complete_attempt(generation, AttemptOutcome::ControllerBusy, now, None);
        }
    }
    assert_eq!(launches, 1);
    assert_eq!(busy.attempt_count(), MAXIMUM_ATTEMPTS);
    assert_eq!(busy.state().stage, RecoveryStage::Exhausted);
}

#[test]
fn begin_episode_replaces_generation_and_cancels_prior_schedule() {
    let mut coordinator = RecoveryCoordinator::default();
    let first = coordinator.begin_episode(Duration::ZERO);
    let RecoveryEffect::PerformAttempt {
        generation: first_generation,
        ..
    } = first[0]
    else {
        panic!("expected attempt");
    };
    coordinator.complete_attempt(
        first_generation,
        AttemptOutcome::Retryable,
        Duration::ZERO,
        None,
    );
    assert!(coordinator.has_scheduled_attempt());
    let second = coordinator.begin_episode(Duration::ZERO);
    let RecoveryEffect::PerformAttempt {
        generation: second_generation,
        ..
    } = second[0]
    else {
        panic!("expected attempt");
    };
    assert!(second_generation > first_generation);
    assert_eq!(coordinator.attempt_count(), 1);
    assert!(!coordinator.has_scheduled_attempt());
    assert!(coordinator
        .scheduled_fire(first_generation, Duration::from_millis(10))
        .is_empty());
    let stale = coordinator.complete_attempt(
        first_generation,
        AttemptOutcome::Connected,
        Duration::from_millis(10),
        None,
    );
    assert!(stale.is_empty());
    assert_eq!(coordinator.state().generation, second_generation);
    assert_eq!(coordinator.state().stage, RecoveryStage::Discovering);

    let opened = coordinator.complete_attempt(
        first_generation,
        AttemptOutcome::Opened {
            handle: 77,
            adopted: true,
        },
        Duration::from_millis(10),
        None,
    );
    assert_eq!(opened, vec![RecoveryEffect::DisposeHandle(77)]);
    assert_eq!(coordinator.state().stage, RecoveryStage::Discovering);
}

#[test]
fn rejected_opened_handle_is_disposed_and_blocked() {
    let mut coordinator = RecoveryCoordinator::default();
    let started = coordinator.begin_episode(Duration::ZERO);
    let RecoveryEffect::PerformAttempt { generation, .. } = started[0] else {
        panic!("expected attempt");
    };
    let effects = coordinator.complete_attempt(
        generation,
        AttemptOutcome::Opened {
            handle: 103,
            adopted: false,
        },
        Duration::ZERO,
        None,
    );
    assert_eq!(effects, vec![RecoveryEffect::DisposeHandle(103)]);
    assert_eq!(coordinator.state().stage, RecoveryStage::Blocked);
    assert!(!coordinator.has_scheduled_attempt());
}

#[test]
fn helper_missing_blocks_without_scheduling() {
    let mut coordinator = RecoveryCoordinator::default();
    let started = coordinator.begin_episode(Duration::ZERO);
    let RecoveryEffect::PerformAttempt { generation, .. } = started[0] else {
        panic!("expected attempt");
    };
    let effects = coordinator.complete_attempt(
        generation,
        AttemptOutcome::EndpointMissing,
        Duration::ZERO,
        Some(LaunchResult::HelperMissing),
    );
    assert_eq!(effects, vec![RecoveryEffect::LaunchHelper { generation }]);
    assert_eq!(coordinator.state().stage, RecoveryStage::Blocked);
    assert!(coordinator.blocked_launch());
    assert!(!coordinator.has_scheduled_attempt());
}

#[test]
fn stale_launch_helper_carries_superseded_generation() {
    let mut coordinator = RecoveryCoordinator::default();
    let started = coordinator.begin_episode(Duration::ZERO);
    let RecoveryEffect::PerformAttempt {
        generation: first_generation,
        ..
    } = started[0]
    else {
        panic!("expected attempt");
    };
    let launched = coordinator.complete_attempt(
        first_generation,
        AttemptOutcome::EndpointMissing,
        Duration::ZERO,
        Some(LaunchResult::Started),
    );
    assert!(launched.contains(&RecoveryEffect::LaunchHelper {
        generation: first_generation
    }));
    let second = coordinator.begin_episode(Duration::from_millis(10));
    let RecoveryEffect::PerformAttempt {
        generation: second_generation,
        ..
    } = second[0]
    else {
        panic!("expected attempt");
    };
    assert_ne!(first_generation, second_generation);
    assert!(!launched.iter().any(|effect| matches!(
        effect,
        RecoveryEffect::LaunchHelper { generation } if *generation == second_generation
    )));
}

#[test]
fn reconstruction_pins_runtime_execution_and_requires_fresh_attachment() {
    let runtime = ContinuityIdentity { low: 1, high: 2 };
    let execution = ContinuityIdentity { low: 3, high: 4 };
    let first = ContinuityIdentity { low: 5, high: 6 };
    let second = ContinuityIdentity { low: 7, high: 8 };
    let mut state = ReconstructionState::default();
    state.begin_attempt();
    assert!(!state.can_mutate());
    assert!(state.commit(runtime, execution, first, true, true));
    assert!(state.can_mutate());
    state.disconnect();
    state.begin_attempt();
    assert!(state.commit(runtime, execution, second, true, true));
    state.disconnect();
    state.begin_attempt();
    assert!(!state.commit(
        ContinuityIdentity { low: 99, high: 2 },
        execution,
        ContinuityIdentity { low: 9, high: 10 },
        true,
        true
    ));
    assert_eq!(state.stage, ReconstructionStage::BlockedIdentityMismatch);
    assert!(!state.can_mutate());
}

#[test]
fn reconstruction_rejects_interrupted_snapshot_and_old_attachment() {
    let runtime = ContinuityIdentity { low: 1, high: 2 };
    let execution = ContinuityIdentity { low: 3, high: 4 };
    let attachment = ContinuityIdentity { low: 5, high: 6 };
    let mut state = ReconstructionState::default();
    state.begin_attempt();
    assert!(!state.commit(runtime, execution, attachment, true, false));
    assert_eq!(
        state.stage,
        ReconstructionStage::AwaitingAuthoritativeSnapshot
    );
    assert!(state.commit(runtime, execution, attachment, true, true));
    state.disconnect();
    state.begin_attempt();
    assert!(!state.commit(runtime, execution, attachment, true, true));
    assert_eq!(state.stage, ReconstructionStage::BlockedIdentityMismatch);
}

#[test]
fn advance_presentation_stage_after_connect() {
    let mut c = RecoveryCoordinator::default();
    let now = Duration::ZERO;
    let _ = c.begin_episode(now);
    let generation = c.state().generation;
    let _ = c.complete_attempt(generation, AttemptOutcome::Connected, now, None);
    assert_eq!(c.state().stage, RecoveryStage::Reconstructing);
    assert!(c.advance_presentation_stage(RecoveryStage::RestoringInteraction));
    assert_eq!(c.state().stage, RecoveryStage::RestoringInteraction);
    assert!(c.advance_presentation_stage(RecoveryStage::Usable));
    assert_eq!(c.state().stage, RecoveryStage::Usable);
    assert!(!c.advance_presentation_stage(RecoveryStage::Discovering));
}

#[test]
fn cancel_clears_active_episode() {
    let mut c = RecoveryCoordinator::default();
    let _ = c.begin_episode(Duration::ZERO);
    assert!(c.is_active());
    c.cancel();
    assert!(!c.is_active());
    assert_eq!(c.state().stage, RecoveryStage::Disconnected);
}

#[test]
fn classify_open_result_maps_bridge_failure_classes() {
    assert_eq!(
        classify_open_result(1, true),
        AttemptOutcome::EndpointMissing
    );
    assert_eq!(
        classify_open_result(2, true),
        AttemptOutcome::EndpointMissing
    );
    assert_eq!(
        classify_open_result(3, true),
        AttemptOutcome::ControllerBusy
    );
    assert_eq!(classify_open_result(4, true), AttemptOutcome::Retryable);
    assert_eq!(classify_open_result(1, false), AttemptOutcome::Blocked);
    assert_eq!(classify_open_result(3, false), AttemptOutcome::Blocked);
}

#[test]
fn advance_presentation_stage_rejects_unconnected_and_terminal_stages() {
    let mut c = RecoveryCoordinator::default();
    assert!(!c.advance_presentation_stage(RecoveryStage::Usable));
    let _ = c.begin_episode(Duration::ZERO);
    assert!(!c.advance_presentation_stage(RecoveryStage::RestoringInteraction));
    assert!(!c.advance_presentation_stage(RecoveryStage::Usable));
    assert_eq!(c.state().stage, RecoveryStage::Discovering);
    c.cancel();
    assert!(!c.advance_presentation_stage(RecoveryStage::Usable));
    assert_eq!(c.state().stage, RecoveryStage::Disconnected);
}

#[test]
fn cancel_mid_open_disposes_late_opened_handle() {
    let mut c = RecoveryCoordinator::default();
    let started = c.begin_episode(Duration::ZERO);
    let RecoveryEffect::PerformAttempt { generation, .. } = started[0] else {
        panic!("expected attempt");
    };
    c.cancel();
    let effects = c.complete_attempt(
        generation,
        AttemptOutcome::Opened {
            handle: 101,
            adopted: true,
        },
        Duration::from_millis(5),
        None,
    );
    assert_eq!(effects, vec![RecoveryEffect::DisposeHandle(101)]);
    assert_eq!(c.state().stage, RecoveryStage::Disconnected);
}

#[test]
fn helper_missing_reported_after_claimed_launch_blocks() {
    let mut c = RecoveryCoordinator::default();
    let started = c.begin_episode(Duration::ZERO);
    let RecoveryEffect::PerformAttempt { generation, .. } = started[0] else {
        panic!("expected attempt");
    };
    let claimed = c.complete_attempt(
        generation,
        AttemptOutcome::EndpointMissing,
        Duration::ZERO,
        None,
    );
    assert_eq!(
        claimed,
        vec![
            RecoveryEffect::LaunchHelper { generation },
            RecoveryEffect::Schedule {
                generation,
                delay: RETRY_DELAYS[0],
            },
        ]
    );
    let blocked = c.complete_attempt(
        generation,
        AttemptOutcome::EndpointMissing,
        Duration::from_millis(1),
        Some(LaunchResult::HelperMissing),
    );
    assert!(blocked.is_empty());
    assert_eq!(c.state().stage, RecoveryStage::Blocked);
    assert!(c.blocked_launch());
    assert!(!c.has_scheduled_attempt());
    assert!(!c.is_active());
}
