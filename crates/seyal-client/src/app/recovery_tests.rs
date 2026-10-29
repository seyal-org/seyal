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
fn ended_execution_keeps_flow_transcript_and_hides_the_composer() {
    let mut root = ApplicationRoot::new();
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: BindingEvidence {
            execution: ExecutionId::from_bytes([4; 16]),
            attachment: AttachmentId::from_bytes([5; 16]),
            controller: true,
            pty_generation: 1,
            alternate_screen: false,
        },
    })
    .unwrap();
    root.apply(AppAction::ApplyRuntimeBlocks {
        fence: root.fence(),
        records: Some(vec![
            crate::composer::RuntimeBlockRecord {
                id: seyal_core::BlockId::from_bytes([0x11; 16]),
                command: "printf".into(),
                start_line: 1,
                end_line: Some(2),
                running: false,
                exit_status: Some(0),
                canvas_floor: false,
            },
            crate::composer::RuntimeBlockRecord {
                id: seyal_core::BlockId::from_bytes([0x22; 16]),
                command: "exit 7".into(),
                start_line: 3,
                end_line: Some(3),
                running: true,
                exit_status: None,
                canvas_floor: false,
            },
        ]),
    })
    .unwrap();
    root.apply(AppAction::BeginRecovery {
        now: Duration::ZERO,
    })
    .unwrap();
    let generation = root.snapshot().recovery_generation;
    root.apply(AppAction::CompleteRecovery {
        generation,
        outcome: AttemptOutcome::ExecutionEnded,
        now: Duration::from_millis(1),
        launch: None,
    })
    .unwrap();

    let snapshot = root.snapshot();
    let blocks = snapshot.composer.as_ref().unwrap().blocks.clone();
    assert_eq!(
        blocks[0].state,
        crate::composer::BlockPresentationState::Completed
    );
    assert_eq!(
        blocks[1].state,
        crate::composer::BlockPresentationState::Unknown,
        "an exit block still marked running must not stay / running after the shell ends"
    );
    assert_eq!(snapshot.recovery_stage, RecoveryStage::ExecutionEnded);
    // Keep Flow authority and the shell bind. Unbinding here fail-closes
    // Block projection and blanks retained bodies. New shell releases later.
    assert_eq!(snapshot.eligibility, PresentationEligibility::Flow);
    assert!(root.authority.is_some());
    assert!(!snapshot.composer_eligible);
    assert_eq!(
        snapshot.composer.unwrap().mode,
        crate::composer::ComposerMode::Hidden
    );
}

/// #1379 Defect B: ExecutionEnded latches only the Pane that ended. CreateTab
/// must not inherit a sticky shell-exit flag from the global recovery stage.
#[test]
fn execution_ended_does_not_poison_create_tab_pane_latch() {
    use crate::recovery::{AttemptOutcome, RecoveryStage};
    use std::time::Duration;

    let mut root = ApplicationRoot::new();
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
    let ended_pane = root.snapshot().shell.focused_pane;
    root.apply(AppAction::BeginRecovery {
        now: Duration::ZERO,
    })
    .unwrap();
    let generation = root.snapshot().recovery_generation;
    root.apply(AppAction::CompleteRecovery {
        generation,
        outcome: AttemptOutcome::ExecutionEnded,
        now: Duration::from_millis(1),
        launch: None,
    })
    .unwrap();
    assert!(root.composer.execution_ended(ended_pane));
    assert_eq!(
        root.snapshot().recovery_stage,
        RecoveryStage::ExecutionEnded
    );
    assert!(
        root.authority.is_some(),
        "shell exit keeps Flow authority so completed Blocks still project"
    );

    root.apply(AppAction::CreateTab)
        .expect("CreateTab after shell exit must still admit a new leaf");
    let new_pane = root.snapshot().shell.focused_pane;
    assert_ne!(new_pane, ended_pane, "CreateTab must focus the new leaf");
    assert!(
        root.composer.execution_ended(ended_pane),
        "ended Pane must keep its sticky latch"
    );
    assert!(
        !root.composer.execution_ended(new_pane),
        "new CreateTab leaf must not inherit ExecutionEnded"
    );
    assert!(
        !root.new_shell_available(),
        "CreateTab focus must not leave New shell admitted on a latch-free Pane"
    );
}

/// #1379 Defect B: after shell exit, the same Pane can Bind a fresh
/// ExecutionId (New shell) and the sticky latch clears.
#[test]
fn execution_ended_pane_can_rebind_fresh_execution_and_clear_latch() {
    use crate::recovery::AttemptOutcome;
    use std::time::Duration;

    let mut root = ApplicationRoot::new();
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: BindingEvidence {
            execution: ExecutionId::from_bytes([0x51; 16]),
            attachment: AttachmentId::from_bytes([0x52; 16]),
            controller: true,
            pty_generation: 1,
            alternate_screen: false,
        },
    })
    .unwrap();
    let pane = root.snapshot().shell.focused_pane;
    root.apply(AppAction::BeginRecovery {
        now: Duration::ZERO,
    })
    .unwrap();
    let generation = root.snapshot().recovery_generation;
    root.apply(AppAction::CompleteRecovery {
        generation,
        outcome: AttemptOutcome::ExecutionEnded,
        now: Duration::from_millis(1),
        launch: None,
    })
    .unwrap();
    assert!(root.composer.execution_ended(pane));
    assert!(
        root.retains_flow_history(),
        "shell exit retains Flow history without a host poll-code check"
    );
    assert!(
        root.new_shell_available(),
        "focused ended Pane admits New shell"
    );
    root.composer
        .apply(crate::composer::ComposerAction::SetBusy {
            pane,
            process: Some("exit".to_string()),
        })
        .unwrap();
    root.composer
        .apply(crate::composer::ComposerAction::ApplyRuntimeEligibility {
            pane,
            eligibility: Some(crate::composer::RuntimeComposerEligibility::Busy),
            revision: 40,
        })
        .unwrap();

    root.apply(AppAction::CreateReplacementExecution {
        execution: None,
        attachment: None,
        pty_generation: 0,
    })
    .expect("New shell prepares the ended Pane");
    assert!(root.replacement_bootstrap_pending());
    assert!(
        root.apply(AppAction::Bind {
            fence: root.fence(),
            evidence: BindingEvidence {
                execution: ExecutionId::from_bytes([0x61; 16]),
                attachment: AttachmentId::from_bytes([0x62; 16]),
                controller: true,
                pty_generation: 2,
                alternate_screen: false,
            },
        })
        .is_err(),
        "generic Bind must not steal the focused Pane during New shell"
    );
    let fresh = seyal_core::PaneId::new();
    root.provisioning
        .begin_intent(fresh, None)
        .expect("a new split leaf is not blocked by the released dead binding");
    assert!(
        root.apply(AppAction::CreateReplacementExecution {
            execution: Some(ExecutionId::from_bytes([0x61; 16])),
            attachment: Some(AttachmentId::from_bytes([0x62; 16])),
            pty_generation: 0,
        })
        .is_err(),
        "generation 0 without an adopted client is not a PTY generation"
    );
    root.apply(AppAction::CreateReplacementExecution {
        execution: Some(ExecutionId::from_bytes([0x61; 16])),
        attachment: Some(AttachmentId::from_bytes([0x62; 16])),
        pty_generation: 1,
    })
    .expect("New shell binds the created ExecutionId");
    assert!(!root.replacement_bootstrap_pending());
    assert!(
        !root.composer.execution_ended(pane),
        "successful Create+Bind must clear the shell-exit latch"
    );
    assert!(root.snapshot().composer_eligible);
    assert_eq!(
        root.shell.pane_execution(pane).unwrap(),
        Some(ExecutionId::from_bytes([0x61; 16]))
    );
    root.composer
        .apply(crate::composer::ComposerAction::ApplyRuntimeEligibility {
            pane,
            eligibility: Some(crate::composer::RuntimeComposerEligibility::Available),
            revision: 1,
        })
        .unwrap();
    assert_eq!(
        root.composer.snapshot(pane).unwrap().mode,
        crate::composer::ComposerMode::Available,
        "a new attachment's revision 1 must not stay stale behind the ended shell"
    );
}

/// SPEC-009 §8.2.2: primary shell exit while a sibling is focused latches the
/// primary Pane only. New shell must not release the focused sibling.
#[test]
fn primary_exit_while_sibling_focused_latches_primary_only() {
    use crate::shell::ShellAction;
    use std::time::Duration;

    let mut root = ApplicationRoot::new();
    let primary_execution = ExecutionId::from_bytes([0x11; 16]);
    let sibling_execution = ExecutionId::from_bytes([0x21; 16]);
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: BindingEvidence {
            execution: primary_execution,
            attachment: AttachmentId::from_bytes([0x12; 16]),
            controller: true,
            pty_generation: 1,
            alternate_screen: false,
        },
    })
    .unwrap();
    let primary = root.snapshot().shell.focused_pane;
    root.shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Right,
            containment_generation: root.shell.containment_generation(),
        })
        .expect("split");
    let sibling = root.snapshot().shell.focused_pane;
    assert_ne!(sibling, primary);
    root.shell
        .apply(ShellAction::BindExecution {
            pane: sibling,
            execution: sibling_execution,
        })
        .unwrap();
    root.provisioning
        .record_adopted_binding(sibling, sibling_execution);
    root.install_pane_authority(
        sibling,
        BindingEvidence {
            execution: sibling_execution,
            attachment: AttachmentId::from_bytes([0x22; 16]),
            controller: true,
            pty_generation: 1,
            alternate_screen: false,
        },
    )
    .unwrap();
    root.apply(AppAction::BeginRecovery {
        now: Duration::ZERO,
    })
    .unwrap();
    let generation = root.snapshot().recovery_generation;
    root.apply(AppAction::CompleteRecovery {
        generation,
        outcome: AttemptOutcome::ExecutionEnded,
        now: Duration::from_millis(1),
        launch: None,
    })
    .unwrap();

    assert!(root.composer.execution_ended(primary));
    assert!(!root.composer.execution_ended(sibling));
    assert_eq!(
        root.authority.map(|authority| authority.pane),
        Some(sibling)
    );
    assert_eq!(
        root.shell.pane_execution(primary).unwrap(),
        Some(primary_execution)
    );
    assert_eq!(
        root.shell.pane_execution(sibling).unwrap(),
        Some(sibling_execution)
    );
    assert!(
        root.apply(AppAction::CreateReplacementExecution {
            execution: None,
            attachment: None,
            pty_generation: 0,
        })
        .is_err(),
        "New shell must not prepare a focused healthy sibling"
    );
    assert_eq!(
        root.shell.pane_execution(sibling).unwrap(),
        Some(sibling_execution)
    );
    assert_eq!(
        root.shell.pane_execution(primary).unwrap(),
        Some(primary_execution)
    );
}

/// #1379 / #1171: when the display client is gone, APPLY_RUNTIME_BLOCKS must
/// not wipe completed Flow history (decode reports `records: None`).
#[test]
fn missing_active_client_does_not_wipe_completed_flow_blocks() {
    use crate::composer::RuntimeBlockRecord;
    use seyal_core::BlockId;

    let mut root = ApplicationRoot::new();
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: BindingEvidence {
            execution: ExecutionId::from_bytes([4; 16]),
            attachment: AttachmentId::from_bytes([5; 16]),
            controller: true,
            pty_generation: 1,
            alternate_screen: false,
        },
    })
    .unwrap();
    let block = BlockId::from_bytes([0x71; 16]);
    root.apply(AppAction::ApplyRuntimeBlocks {
        fence: root.fence(),
        records: Some(vec![RuntimeBlockRecord {
            id: block,
            command: "pwd".into(),
            start_line: 1,
            end_line: Some(2),
            running: false,
            exit_status: Some(0),
            canvas_floor: false,
        }]),
    })
    .unwrap();
    assert_eq!(root.snapshot().composer.unwrap().blocks.len(), 1);

    root.apply(AppAction::ApplyRuntimeBlocks {
        fence: root.fence(),
        records: None,
    })
    .unwrap();
    let retained = root.snapshot().composer.unwrap().blocks;
    assert_eq!(retained.len(), 1, "disconnect must retain Flow history");
    assert_eq!(retained[0].id, block);
    assert_eq!(retained[0].command, "pwd");
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

#[test]
fn reconstruction_commit_through_app_root_is_sole_fencing_authority() {
    use crate::recovery::ContinuityIdentity;

    let mut root = ApplicationRoot::new();
    root.apply(AppAction::BeginReconstructionAttempt).unwrap();
    let runtime = ContinuityIdentity { low: 1, high: 2 };
    let execution = ContinuityIdentity { low: 3, high: 4 };
    let first = ContinuityIdentity { low: 5, high: 6 };
    let second = ContinuityIdentity { low: 7, high: 8 };
    root.apply(AppAction::CommitReconstruction {
        runtime,
        execution,
        attachment: first,
        controller_authority_committed: true,
        authoritative_snapshot_committed: true,
    })
    .unwrap();
    assert!(root.reconstruction.can_mutate());
    root.apply(AppAction::DisconnectReconstruction).unwrap();
    root.apply(AppAction::BeginReconstructionAttempt).unwrap();
    root.apply(AppAction::CommitReconstruction {
        runtime,
        execution,
        attachment: second,
        controller_authority_committed: true,
        authoritative_snapshot_committed: true,
    })
    .unwrap();
    root.apply(AppAction::DisconnectReconstruction).unwrap();
    root.apply(AppAction::BeginReconstructionAttempt).unwrap();
    assert_eq!(
        root.apply(AppAction::CommitReconstruction {
            runtime: ContinuityIdentity { low: 99, high: 2 },
            execution,
            attachment: ContinuityIdentity { low: 9, high: 10 },
            controller_authority_committed: true,
            authoritative_snapshot_committed: true,
        }),
        Err(AppError::InvalidPayload)
    );
    assert!(!root.reconstruction.can_mutate());
}

#[test]
fn cancel_recovery_retains_queued_dispose_handle_effects() {
    let mut root = ApplicationRoot::new();
    root.apply(AppAction::BeginRecovery {
        now: Duration::ZERO,
    })
    .unwrap();
    let generation = root.snapshot().recovery_generation;
    // Stale completion against a cancelled episode still emits DisposeHandle;
    // cancel must not drop those disposal effects.
    root.apply(AppAction::AckRecoveryEffect).unwrap();
    root.pending_recovery
        .push(RecoveryEffect::DisposeHandle(42));
    root.pending_recovery.push(RecoveryEffect::Schedule {
        generation,
        delay: Duration::from_millis(10),
    });
    root.apply(AppAction::CancelRecovery).unwrap();
    assert_eq!(
        root.snapshot().recovery_effect,
        Some(RecoveryEffect::DisposeHandle(42))
    );
    assert_eq!(root.snapshot().recovery_stage, RecoveryStage::Disconnected);
}

#[test]
fn claiming_perform_attempt_clears_effect_before_complete() {
    // Host re-entry model: adopt → first frame → reconcile → driveRecovery.
    // Acking PerformAttempt when claimed removes the level-triggered effect so
    // a nested drive cannot open a second attempt; CompleteRecovery still
    // consumes the outstanding attempt bit in RecoveryCoordinator.
    let mut root = ApplicationRoot::new();
    root.apply(AppAction::BeginRecovery {
        now: Duration::ZERO,
    })
    .unwrap();
    let generation = root.snapshot().recovery_generation;
    assert!(matches!(
        root.snapshot().recovery_effect,
        Some(RecoveryEffect::PerformAttempt { .. })
    ));
    root.apply(AppAction::AckRecoveryEffect).unwrap();
    assert_eq!(root.snapshot().recovery_effect, None);
    assert!(root.recovery.has_outstanding_attempt());

    root.apply(AppAction::CompleteRecovery {
        generation,
        outcome: AttemptOutcome::Opened {
            handle: 7,
            adopted: true,
        },
        now: Duration::ZERO,
        launch: None,
    })
    .unwrap();
    assert_eq!(
        root.snapshot().recovery_stage,
        RecoveryStage::Reconstructing
    );
    assert!(!root.recovery.has_outstanding_attempt());

    // Nested/late complete after the attempt was already consumed.
    root.apply(AppAction::CompleteRecovery {
        generation,
        outcome: AttemptOutcome::Opened {
            handle: 8,
            adopted: false,
        },
        now: Duration::from_millis(1),
        launch: None,
    })
    .unwrap();
    assert_eq!(
        root.snapshot().recovery_effect,
        Some(RecoveryEffect::DisposeHandle(8))
    );
    assert_eq!(
        root.snapshot().recovery_stage,
        RecoveryStage::Reconstructing
    );
}

#[test]
fn open_budget_survives_ack_of_perform_attempt() {
    // Host must Ack PerformAttempt to stop re-entry, then still read a
    // non-zero open budget. Reading only from the pending effect returns 0
    // after Ack and exhausts recovery without ever opening.
    let mut root = ApplicationRoot::new();
    root.apply(AppAction::BeginRecovery {
        now: Duration::ZERO,
    })
    .unwrap();
    let before = root.recovery_open_budget_millis();
    assert!(before > 0, "PerformAttempt must publish a remaining budget");
    root.apply(AppAction::AckRecoveryEffect).unwrap();
    assert_eq!(root.snapshot().recovery_effect, None);
    assert_eq!(
        root.recovery_open_budget_millis(),
        before,
        "claimed budget must survive Ack"
    );
    assert!(root.recovery.has_outstanding_attempt());
}

#[test]
fn duplicate_complete_preserves_queued_schedule() {
    let mut root = ApplicationRoot::new();
    root.apply(AppAction::BeginRecovery {
        now: Duration::ZERO,
    })
    .unwrap();
    let generation = root.snapshot().recovery_generation;
    root.apply(AppAction::AckRecoveryEffect).unwrap(); // host claims attempt
    root.apply(AppAction::CompleteRecovery {
        generation,
        outcome: AttemptOutcome::Retryable,
        now: Duration::ZERO,
        launch: None,
    })
    .unwrap();
    assert!(matches!(
        root.snapshot().recovery_effect,
        Some(RecoveryEffect::Schedule { .. })
    ));
    // Duplicate completion for the same generation before the host drains Schedule.
    let _ = root.apply(AppAction::CompleteRecovery {
        generation,
        outcome: AttemptOutcome::Retryable,
        now: Duration::from_millis(1),
        launch: None,
    });
    assert!(
        matches!(
            root.snapshot().recovery_effect,
            Some(RecoveryEffect::Schedule { .. })
        ),
        "duplicate completion dropped the queued Schedule: effect={:?} stage={:?}",
        root.snapshot().recovery_effect,
        root.snapshot().recovery_stage
    );
}
