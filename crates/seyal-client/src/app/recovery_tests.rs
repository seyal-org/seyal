//! RecoveryCoordinator apply paths through ApplicationRoot.

use super::*;

#[test]
fn recovery_retry_ladder_and_stale_generation_fail_closed() {
    let mut root = ApplicationRoot::new();
    root.apply(AppAction::BeginRecovery {
        now: Duration::ZERO,
    })
    .unwrap();
    let first = root.snapshot();
    assert_eq!(first.recovery_stage, RecoveryStage::Discovering);
    assert_eq!(first.recovery_attempts, 1);
    assert_eq!(
        first.recovery_effect,
        Some(RecoveryEffect::PerformAttempt {
            generation: first.recovery_generation,
            remaining: Duration::from_secs(1),
        })
    );

    root.apply(AppAction::CompleteRecovery {
        generation: first.recovery_generation,
        outcome: AttemptOutcome::ControllerBusy,
        now: Duration::ZERO,
        launch: None,
    })
    .unwrap();
    let scheduled = root.snapshot();
    assert_eq!(
        scheduled.recovery_stage,
        RecoveryStage::WaitingForController
    );
    assert_eq!(
        scheduled.recovery_effect,
        Some(RecoveryEffect::Schedule {
            generation: first.recovery_generation,
            delay: Duration::from_millis(10),
        })
    );

    root.apply(AppAction::BeginRecovery {
        now: Duration::from_millis(5),
    })
    .unwrap();
    let second = root.snapshot();
    assert_ne!(second.recovery_generation, first.recovery_generation);
    assert_eq!(
        root.apply(AppAction::FireScheduledRecovery {
            generation: first.recovery_generation,
            now: Duration::from_millis(15),
        }),
        Err(AppError::StaleRecoveryGeneration)
    );
    assert_eq!(
        root.apply(AppAction::CompleteRecovery {
            generation: first.recovery_generation,
            outcome: AttemptOutcome::Opened {
                handle: 9,
                adopted: true,
            },
            now: Duration::from_millis(15),
            launch: None,
        }),
        Err(AppError::StaleRecoveryGeneration)
    );
    assert_eq!(
        root.snapshot().recovery_effect,
        Some(RecoveryEffect::DisposeHandle(9))
    );
    assert_eq!(root.snapshot().recovery_stage, RecoveryStage::Discovering);
    assert_eq!(
        root.snapshot().recovery_generation,
        second.recovery_generation
    );
    root.apply(AppAction::AckRecoveryEffect).unwrap();
    assert_eq!(
        root.snapshot().recovery_effect,
        second.recovery_effect,
        "stale disposal must not drop the current episode's attempt"
    );
}

#[test]
fn recovery_cancel_and_presentation_advance_through_app_root() {
    let mut root = ApplicationRoot::new();
    root.apply(AppAction::BeginRecovery {
        now: Duration::ZERO,
    })
    .unwrap();
    assert_eq!(
        root.apply(AppAction::AdvanceRecoveryStage {
            stage: RecoveryStage::Usable,
        }),
        Err(AppError::InvalidPayload)
    );
    let generation = root.snapshot().recovery_generation;
    root.apply(AppAction::CompleteRecovery {
        generation,
        outcome: AttemptOutcome::Opened {
            handle: 5,
            adopted: true,
        },
        now: Duration::from_millis(1),
        launch: None,
    })
    .unwrap();
    assert_eq!(
        root.snapshot().recovery_stage,
        RecoveryStage::Reconstructing
    );
    root.apply(AppAction::AdvanceRecoveryStage {
        stage: RecoveryStage::RestoringInteraction,
    })
    .unwrap();
    root.apply(AppAction::AdvanceRecoveryStage {
        stage: RecoveryStage::Usable,
    })
    .unwrap();
    assert_eq!(root.snapshot().recovery_stage, RecoveryStage::Usable);

    root.apply(AppAction::BeginRecovery {
        now: Duration::from_millis(10),
    })
    .unwrap();
    let active = root.snapshot();
    assert!(active.recovery_effect.is_some());
    root.apply(AppAction::CancelRecovery).unwrap();
    let cancelled = root.snapshot();
    assert_eq!(cancelled.recovery_stage, RecoveryStage::Disconnected);
    assert_eq!(cancelled.recovery_effect, None);
    assert_ne!(cancelled.recovery_generation, active.recovery_generation);
    assert_eq!(
        root.apply(AppAction::FireScheduledRecovery {
            generation: active.recovery_generation,
            now: Duration::from_millis(20),
        }),
        Err(AppError::StaleRecoveryGeneration)
    );
}

#[test]
fn recovery_endpoint_missing_launches_once_then_seven_attempts() {
    use crate::recovery::{EPISODE_DEADLINE, MAXIMUM_ATTEMPTS, RETRY_DELAYS};

    let mut root = ApplicationRoot::new();
    root.apply(AppAction::BeginRecovery {
        now: Duration::ZERO,
    })
    .unwrap();
    let generation = root.snapshot().recovery_generation;
    let mut now = Duration::ZERO;
    let mut launches = 0u32;
    for _ in 0..MAXIMUM_ATTEMPTS {
        root.apply(AppAction::AckRecoveryEffect).unwrap();
        root.apply(AppAction::CompleteRecovery {
            generation,
            outcome: AttemptOutcome::EndpointMissing,
            now,
            launch: Some(LaunchResult::Started),
        })
        .unwrap();
        if matches!(
            root.snapshot().recovery_effect,
            Some(RecoveryEffect::LaunchHelper { .. })
        ) {
            launches += 1;
            root.apply(AppAction::AckRecoveryEffect).unwrap();
        }
        if let Some(RecoveryEffect::Schedule { delay, .. }) = root.snapshot().recovery_effect {
            now += delay;
            if now >= EPISODE_DEADLINE {
                break;
            }
            root.apply(AppAction::AckRecoveryEffect).unwrap();
            root.apply(AppAction::FireScheduledRecovery { generation, now })
                .unwrap();
        }
    }
    assert_eq!(launches, 1);
    assert_eq!(root.snapshot().recovery_attempts, MAXIMUM_ATTEMPTS);
    assert_eq!(root.snapshot().recovery_stage, RecoveryStage::Exhausted);
    assert_eq!(RETRY_DELAYS.len() as u32 + 1, MAXIMUM_ATTEMPTS);
}
