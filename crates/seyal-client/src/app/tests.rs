use super::*;
use crate::presentation::InputRoute;

fn evidence(tag: u8, controller: bool, alternate: bool) -> BindingEvidence {
    BindingEvidence {
        execution: ExecutionId::from_bytes([tag; 16]),
        attachment: AttachmentId::from_bytes([tag.wrapping_add(1); 16]),
        controller,
        pty_generation: 1,
        alternate_screen: alternate,
    }
}

/// Runtime published `Available` for the bound attachment: the only way
/// the composer becomes submittable.
fn runtime_available(root: &mut ApplicationRoot, revision: u64) {
    root.apply(AppAction::ApplyRuntimeComposerStatus {
        fence: root.fence(),
        eligibility: Some(RuntimeComposerEligibility::Available),
        revision,
    })
    .unwrap();
}

#[test]
fn bound_composer_is_busy_until_runtime_publishes_eligibility() {
    use crate::composer::ComposerMode;

    let mut root = ApplicationRoot::new();
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: evidence(8, true, false),
    })
    .unwrap();
    let bound = root.snapshot().composer.unwrap();
    assert!(matches!(bound.mode, ComposerMode::Busy { .. }));
    assert!(!bound.can_submit);
    root.apply(AppAction::SetComposerDraft {
        fence: root.fence(),
        text: "echo hi".into(),
        composer_epoch: bound.epoch,
    })
    .unwrap();
    let epoch = root.snapshot().composer.unwrap().epoch;
    assert_eq!(
        root.apply(AppAction::SubmitComposer {
            fence: root.fence(),
            composer_epoch: epoch,
        }),
        Err(AppError::ComposerSubmitDisabled)
    );
    // A status relayed against a stale execution fence fails closed.
    let mut stale = root.fence();
    stale.execution = Some(ExecutionId::from_bytes([0x99; 16]));
    assert_eq!(
        root.apply(AppAction::ApplyRuntimeComposerStatus {
            fence: stale,
            eligibility: Some(RuntimeComposerEligibility::Available),
            revision: 1,
        }),
        Err(AppError::StaleExecution)
    );
    assert!(!root.snapshot().composer.unwrap().can_submit);
    runtime_available(&mut root, 1);
    let ready = root.snapshot().composer.unwrap();
    assert_eq!(ready.mode, ComposerMode::Available);
    assert!(ready.can_submit);
    assert_eq!(ready.draft, "echo hi");
    // Runtime says busy again (command running): draft preserved.
    root.apply(AppAction::ApplyRuntimeComposerStatus {
        fence: root.fence(),
        eligibility: Some(RuntimeComposerEligibility::Busy),
        revision: 2,
    })
    .unwrap();
    let busy = root.snapshot().composer.unwrap();
    assert!(!busy.can_submit);
    assert_eq!(busy.draft, "echo hi");
}

#[test]
fn new_root_is_one_unbound_pane() {
    let root = ApplicationRoot::new();
    let snap = root.snapshot();
    assert_eq!(snap.shell.panes.len(), 1);
    assert!(snap.execution.is_none());
    assert_eq!(snap.eligibility, PresentationEligibility::Unbound);
    assert!(!snap.composer_eligible);
    assert_eq!(snap.generation, 1);
    assert_eq!(root.snapshot(), root.snapshot());
    assert!(snap.chrome.left_visible);
    assert!(snap.chrome.inspector_visible);
    assert!(snap.chrome.tab_strip_visible);
}

#[test]
fn create_tab_and_split_focused_fail_closed_under_m001_default_policy() {
    // AppAction::CreateTab/SplitFocused (#922) route straight to the same
    // ShellState that already disallows composition growth until a
    // distinct execution route exists; the direct action must fail the
    // same way the palette-mediated path already does, not silently
    // no-op.
    let mut root = ApplicationRoot::new();
    assert_eq!(
        root.apply(AppAction::CreateTab),
        Err(AppError::TabCreationUnavailable)
    );
    assert_eq!(
        root.apply(AppAction::SplitFocused {
            axis: SplitAxis::Right,
        }),
        Err(AppError::PaneSplitUnavailable)
    );
}

#[test]
fn close_tab_and_close_pane_fail_closed_when_only_one_exists() {
    // The M001 production shell starts with exactly one Tab and one
    // Pane, so ShellState's "cannot close last" guard rejects CloseTab/
    // ClosePane before an id is even looked up (shell.rs close_tab/
    // close_pane), whether the id is real or not. This exercises the
    // new close_tab_error/close_pane_error mapping surfaces that
    // distinct cause rather than collapsing it to an unknown-id error.
    let mut root = ApplicationRoot::new();
    let snap = root.snapshot();
    let only_tab = snap.shell.tabs[0].id;
    let only_pane = snap.shell.panes[0].id;

    assert_eq!(
        root.apply(AppAction::CloseTab { id: TabId::new() }),
        Err(AppError::CannotCloseLastTab)
    );
    assert_eq!(
        root.apply(AppAction::ClosePane { id: PaneId::new() }),
        Err(AppError::CannotCloseLastPane)
    );
    assert_eq!(
        root.apply(AppAction::CloseTab { id: only_tab }),
        Err(AppError::CannotCloseLastTab)
    );
    assert_eq!(
        root.apply(AppAction::ClosePane { id: only_pane }),
        Err(AppError::CannotCloseLastPane)
    );
    // A rejected mutation does not remove the only Tab/Pane.
    let after = root.snapshot();
    assert_eq!(after.shell.tabs.len(), 1);
    assert_eq!(after.shell.panes.len(), 1);
    assert_eq!(after.shell.tabs[0].id, only_tab);
    assert_eq!(after.shell.panes[0].id, only_pane);
}

#[test]
fn unbound_cannot_authorize_flow_or_composer() {
    let mut root = ApplicationRoot::new();
    let fence = root.fence();
    assert_eq!(
        root.apply(AppAction::SubmitInput {
            fence,
            text: "echo".into(),
        }),
        Err(AppError::UnboundUnauthorized)
    );
    let snap = root.snapshot();
    assert_eq!(snap.eligibility, PresentationEligibility::Unbound);
    assert!(!snap.composer_eligible);
    assert!(!snap
        .accessibility
        .iter()
        .any(|node| node.role == AccessibilityRole::Composer));
    assert_eq!(
        root.presentation.snapshot().input_route,
        InputRoute::Composer
    );
    assert_ne!(snap.eligibility, PresentationEligibility::Flow);
}

#[test]
fn bind_then_focus_keeps_one_execution() {
    let mut root = ApplicationRoot::new();
    let fence = root.fence();
    let bound = evidence(1, true, false);
    root.apply(AppAction::Bind {
        fence,
        evidence: bound,
    })
    .unwrap();
    let snap = root.snapshot();
    assert_eq!(snap.execution, Some(bound.execution));
    assert_eq!(snap.eligibility, PresentationEligibility::Flow);
    assert!(snap.composer_eligible);
    let focused = snap.pane;
    root.apply(AppAction::Focus {
        fence: root.fence(),
    })
    .unwrap();
    assert_eq!(root.snapshot().pane, focused);
    assert_eq!(root.snapshot().execution, Some(bound.execution));
}

#[test]
fn alternate_screen_evidence_derives_tui_not_host_policy() {
    let mut root = ApplicationRoot::new();
    let fence = root.fence();
    root.apply(AppAction::Bind {
        fence,
        evidence: evidence(2, true, true),
    })
    .unwrap();
    let snap = root.snapshot();
    assert_eq!(snap.eligibility, PresentationEligibility::Tui);
    assert!(!snap.composer_eligible);
    assert!(snap
        .accessibility
        .iter()
        .any(|node| node.role == AccessibilityRole::Terminal));
}

#[test]
fn refresh_alternate_screen_takeover_is_not_latched_at_bind() {
    let mut root = ApplicationRoot::new();
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: evidence(2, true, false),
    })
    .unwrap();
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Flow);
    assert!(root.snapshot().composer_eligible);
    root.apply(AppAction::Refresh {
        fence: root.fence(),
        alternate_screen: true,
    })
    .unwrap();
    let tui = root.snapshot();
    assert_eq!(tui.eligibility, PresentationEligibility::Tui);
    assert!(!tui.composer_eligible);
    root.apply(AppAction::Refresh {
        fence: root.fence(),
        alternate_screen: false,
    })
    .unwrap();
    let flow = root.snapshot();
    assert_eq!(flow.eligibility, PresentationEligibility::Flow);
    assert!(flow.composer_eligible);
}

#[test]
fn stale_identities_fail_closed_and_are_not_retried() {
    let mut root = ApplicationRoot::new();
    let unbound = root.fence();
    root.apply(AppAction::Bind {
        fence: unbound,
        evidence: evidence(3, true, false),
    })
    .unwrap();
    let generation = root.snapshot().generation;
    assert_eq!(
        root.apply(AppAction::Focus { fence: unbound }),
        Err(AppError::StaleExecution)
    );
    assert_eq!(root.snapshot().generation, generation);
    assert_eq!(root.snapshot().last_error, Some(AppError::StaleExecution));

    let current = root.fence();
    let mut stale_pane = current;
    stale_pane.pane = PaneId::from_bytes([0xff; 16]);
    assert_eq!(
        root.apply(AppAction::Focus { fence: stale_pane }),
        Err(AppError::UnknownPane)
    );

    let mut stale_attach = current;
    stale_attach.attachment = Some(AttachmentId::from_bytes([0xab; 16]));
    assert_eq!(
        root.apply(AppAction::Refresh {
            fence: stale_attach,
            alternate_screen: false,
        }),
        Err(AppError::StaleAttachment)
    );

    let mut stale_controller = current;
    stale_controller.controller = false;
    assert_eq!(
        root.apply(AppAction::Refresh {
            fence: stale_controller,
            alternate_screen: false,
        }),
        Err(AppError::StaleController)
    );

    let mut stale_epoch = current;
    stale_epoch.presentation_epoch = current.presentation_epoch.wrapping_add(9);
    assert_eq!(
        root.apply(AppAction::Refresh {
            fence: stale_epoch,
            alternate_screen: false,
        }),
        Err(AppError::StalePresentationEpoch)
    );
    assert_eq!(
        root.snapshot().execution,
        Some(evidence(3, true, false).execution)
    );
}

#[test]
fn observer_and_flow_cannot_submit_direct_input() {
    let mut root = ApplicationRoot::new();
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: evidence(4, false, false),
    })
    .unwrap();
    assert_eq!(
        root.apply(AppAction::SubmitInput {
            fence: root.fence(),
            text: "x".into(),
        }),
        Err(AppError::NotController)
    );

    let mut controller = ApplicationRoot::new();
    controller
        .apply(AppAction::Bind {
            fence: controller.fence(),
            evidence: evidence(5, true, false),
        })
        .unwrap();
    assert_eq!(
        controller.apply(AppAction::SubmitInput {
            fence: controller.fence(),
            text: "x".into(),
        }),
        Err(AppError::DirectInputUnauthorized)
    );
}

#[test]
fn tui_controller_without_client_is_authorized_but_has_no_second_pty() {
    let mut root = ApplicationRoot::new();
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: evidence(6, true, true),
    })
    .unwrap();
    assert_eq!(
        root.apply(AppAction::SubmitInput {
            fence: root.fence(),
            text: "x".into(),
        }),
        Err(AppError::NoLiveClient)
    );
    assert_eq!(root.snapshot().shell.panes.len(), 1);
    assert_eq!(
        root.snapshot().execution.unwrap(),
        evidence(6, true, true).execution
    );
}

#[test]
fn quit_freezes_and_emits_one_native_effect() {
    let mut root = ApplicationRoot::new();
    root.apply(AppAction::Quit).unwrap();
    let snap = root.snapshot();
    assert!(snap.frozen);
    assert_eq!(
        snap.pending_effect,
        NativeEffect::BoundedDetachThenTerminate
    );
    assert_eq!(
        root.apply(AppAction::Focus {
            fence: root.fence()
        }),
        Err(AppError::Frozen)
    );
    root.apply(AppAction::AckEffect).unwrap();
    assert_eq!(root.snapshot().pending_effect, NativeEffect::None);
}

#[test]
fn unknown_pane_does_not_route_across_identities() {
    let mut root = ApplicationRoot::new();
    let mut fence = root.fence();
    fence.pane = PaneId::new();
    assert_eq!(
        root.apply(AppAction::Focus { fence }),
        Err(AppError::UnknownPane)
    );
}

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
fn composer_history_is_fenced_and_never_submits() {
    let mut root = ApplicationRoot::new();
    // Unbound: fence is valid, composer is not Available.
    assert_eq!(
        root.apply(AppAction::OpenComposerHistory {
            fence: root.fence()
        }),
        Err(AppError::ComposerHistoryUnavailable)
    );
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: evidence(8, true, false),
    })
    .unwrap();
    runtime_available(&mut root, 1);
    let epoch = root.snapshot().composer.unwrap().epoch;
    root.apply(AppAction::SetComposerDraft {
        fence: root.fence(),
        text: "make check".into(),
        composer_epoch: epoch,
    })
    .unwrap();
    root.apply(AppAction::SubmitComposer {
        fence: root.fence(),
        composer_epoch: epoch,
    })
    .unwrap();
    let request_id = root
        .snapshot()
        .composer
        .unwrap()
        .pending_request_id
        .unwrap();
    root.apply(AppAction::ApplyComposerResult {
        fence: root.fence(),
        request_id,
        accepted: true,
    })
    .unwrap();
    assert_eq!(root.snapshot().composer.unwrap().history_count, 1);

    let mut stale = root.fence();
    stale.execution = Some(ExecutionId::from_bytes([0x99; 16]));
    assert_eq!(
        root.apply(AppAction::OpenComposerHistory { fence: stale }),
        Err(AppError::StaleExecution)
    );
    assert!(root.snapshot().composer.unwrap().history.is_none());

    root.apply(AppAction::OpenComposerHistory {
        fence: root.fence(),
    })
    .unwrap();
    root.apply(AppAction::SetComposerHistoryFilter {
        fence: root.fence(),
        query: "mk".into(),
    })
    .unwrap();
    root.apply(AppAction::MoveComposerHistorySelection {
        fence: root.fence(),
        delta: 1,
    })
    .unwrap();
    let overlay = root.snapshot().composer.unwrap().history.unwrap();
    assert_eq!(overlay.rows, vec!["make check"]);
    assert_eq!(overlay.selected, 0);
    let composer_epoch = root.snapshot().composer.unwrap().epoch;
    root.apply(AppAction::SelectComposerHistory {
        fence: root.fence(),
        composer_epoch,
    })
    .unwrap();
    let after = root.snapshot().composer.unwrap();
    assert_eq!(after.draft, "make check");
    assert!(after.history.is_none());
    assert!(after.pending_request_id.is_none());
    assert!(after.can_submit);

    root.apply(AppAction::OpenComposerHistory {
        fence: root.fence(),
    })
    .unwrap();
    root.apply(AppAction::Quit).unwrap();
    assert!(root.snapshot().frozen);
    assert_eq!(
        root.apply(AppAction::CloseComposerHistory {
            fence: root.fence()
        }),
        Err(AppError::Frozen)
    );
    assert!(
        root.snapshot().composer.unwrap().history.is_none(),
        "frozen root projects no open overlay"
    );
}

#[test]
fn palette_open_filter_run_is_fenced_and_omits_disallowed_commands() {
    let mut root = ApplicationRoot::new();
    // Unbound: require_fence passes (the Pane itself exists), so the
    // palette is usable before any Runtime attach.
    root.apply(AppAction::OpenPalette {
        fence: root.fence(),
    })
    .unwrap();
    let opened = root.snapshot();
    assert!(opened.palette.open);
    assert!(
        !opened.palette.rows.iter().any(|row| row.label == "New Tab"),
        "M001 default shell policy disallows tab creation; the command is omitted, not disabled"
    );
    assert!(!opened.palette.rows.is_empty());

    // A stale fence rejects every subsequent palette action and leaves
    // state untouched.
    let mut stale = root.fence();
    stale.execution = Some(ExecutionId::from_bytes([0x77; 16]));
    assert_eq!(
        root.apply(AppAction::MovePaletteSelection {
            fence: stale,
            delta: 1
        }),
        Err(AppError::StaleExecution)
    );
    assert!(root.snapshot().palette.open);

    // A query matching nothing fails Run closed without dismissing.
    root.apply(AppAction::SetPaletteQuery {
        fence: root.fence(),
        query: "zzz-no-such-command".into(),
    })
    .unwrap();
    assert!(root.snapshot().palette.rows.is_empty());
    assert_eq!(
        root.apply(AppAction::RunPalette {
            fence: root.fence()
        }),
        Err(AppError::PaletteNoSelection)
    );
    assert!(
        root.snapshot().palette.open,
        "failed Run does not close the palette"
    );

    // Filter to exactly one row and run it: the resolved command applies
    // through the same path as a direct SetShellVisibility action, and
    // the palette closes itself afterward. Core Terminal chrome is
    // visible by default, so the available toggle command is "Hide".
    root.apply(AppAction::SetPaletteQuery {
        fence: root.fence(),
        query: "Hide Inspector".into(),
    })
    .unwrap();
    let filtered = root.snapshot().palette;
    assert_eq!(filtered.rows.len(), 1);
    assert_eq!(filtered.rows[0].label, "Hide Inspector");
    assert!(root.snapshot().chrome.inspector_visible);
    root.apply(AppAction::RunPalette {
        fence: root.fence(),
    })
    .unwrap();
    let after = root.snapshot();
    assert!(!after.palette.open, "Run closes the palette");
    assert_eq!(after.palette.query, "");
    assert!(
        !after.chrome.inspector_visible,
        "the resolved command actually ran"
    );
}

#[test]
fn palette_move_selection_and_close_are_fenced_and_reversible() {
    let mut root = ApplicationRoot::new();
    assert_eq!(
        root.apply(AppAction::SetPaletteQuery {
            fence: root.fence(),
            query: "x".into()
        }),
        Err(AppError::PaletteNotOpen)
    );
    root.apply(AppAction::OpenPalette {
        fence: root.fence(),
    })
    .unwrap();
    let row_count = root.snapshot().palette.rows.len();
    assert!(
        row_count >= 2,
        "enough commands to exercise selection movement"
    );
    root.apply(AppAction::MovePaletteSelection {
        fence: root.fence(),
        delta: 1,
    })
    .unwrap();
    assert_eq!(root.snapshot().palette.selected, 1);
    root.apply(AppAction::MovePaletteSelection {
        fence: root.fence(),
        delta: -100,
    })
    .unwrap();
    assert_eq!(root.snapshot().palette.selected, 0);
    root.apply(AppAction::ClosePalette {
        fence: root.fence(),
    })
    .unwrap();
    let closed = root.snapshot();
    assert!(!closed.palette.open);
    assert!(closed.palette.rows.is_empty());
    assert_eq!(
        root.apply(AppAction::ClosePalette {
            fence: root.fence()
        }),
        Ok(()),
        "closing an already-closed palette is idempotent"
    );
}

#[test]
fn block_selection_is_fenced_and_sourced_from_runtime_blocks() {
    use crate::chrome::InspectorMode;
    use seyal_core::BlockId;

    let mut root = ApplicationRoot::new();
    let known = BlockId::from_bytes([0x51; 16]);
    let unknown = BlockId::from_bytes([0x52; 16]);
    // Unbound: fence valid, but there is no Block list yet.
    assert_eq!(
        root.apply(AppAction::SelectBlock {
            fence: root.fence(),
            id: known,
        }),
        Err(AppError::UnknownBlock)
    );
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: evidence(8, true, false),
    })
    .unwrap();
    root.apply(AppAction::ApplyRuntimeBlocks {
        fence: root.fence(),
        records: vec![RuntimeBlockRecord {
            id: known,
            command: "git status".into(),
            start_line: 1,
            end_line: Some(3),
            running: false,
            exit_status: Some(0),
        }],
    })
    .unwrap();
    assert_eq!(
        root.apply(AppAction::SelectBlock {
            fence: root.fence(),
            id: unknown,
        }),
        Err(AppError::UnknownBlock)
    );
    let mut stale = root.fence();
    stale.execution = Some(ExecutionId::from_bytes([0x99; 16]));
    assert_eq!(
        root.apply(AppAction::SelectBlock {
            fence: stale,
            id: known,
        }),
        Err(AppError::StaleExecution)
    );
    assert!(root.snapshot().chrome.selected_block.is_none());

    root.apply(AppAction::SelectBlock {
        fence: root.fence(),
        id: known,
    })
    .unwrap();
    let snap = root.snapshot();
    assert_eq!(snap.chrome.selected_block, Some(known));
    assert_eq!(snap.chrome.inspector_mode, InspectorMode::Block);
    assert!(snap.chrome.inspector_visible);
    let rows = &snap.chrome.visible_inspector_rows;
    assert_eq!(rows[0].value, "git status");
    assert_eq!(rows[1].value, "Completed");
    assert_eq!(rows[2].value, "0");
    assert_eq!(rows[3].value, "3");

    // Runtime republishes without that Block: selection clears itself.
    root.apply(AppAction::ApplyRuntimeBlocks {
        fence: root.fence(),
        records: vec![],
    })
    .unwrap();
    let after = root.snapshot();
    assert!(after.chrome.selected_block.is_none());
    assert_eq!(after.chrome.inspector_mode, InspectorMode::Context);
    assert!(after
        .chrome
        .inspector_rows
        .iter()
        .all(|row| row.section != "Block"));
}

#[test]
fn composer_submit_busy_and_stale_request_are_rust_owned() {
    use crate::composer::{BlockPresentationState, ComposerMode};
    use seyal_core::BlockId;

    let mut root = ApplicationRoot::new();
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: evidence(8, true, false),
    })
    .unwrap();
    runtime_available(&mut root, 1);
    let epoch = root.snapshot().composer.as_ref().unwrap().epoch;
    root.apply(AppAction::SetComposerDraft {
        fence: root.fence(),
        text: "echo hi".into(),
        composer_epoch: epoch,
    })
    .unwrap();
    let ready = root.snapshot().composer.unwrap();
    assert_eq!(ready.mode, ComposerMode::Available);
    assert!(ready.can_submit);
    root.apply(AppAction::SubmitComposer {
        fence: root.fence(),
        composer_epoch: ready.epoch,
    })
    .unwrap();
    let busy = root.snapshot().composer.unwrap();
    assert!(!busy.can_submit);
    assert!(matches!(busy.mode, ComposerMode::Busy { .. }));
    let request_id = busy.pending_request_id.unwrap();
    assert_eq!(
        root.apply(AppAction::ApplyComposerResult {
            fence: root.fence(),
            request_id: request_id.wrapping_add(3),
            accepted: true,
        }),
        Err(AppError::StaleComposerRequest)
    );
    assert_eq!(root.snapshot().composer.unwrap().draft, "echo hi");
    root.apply(AppAction::ApplyComposerResult {
        fence: root.fence(),
        request_id,
        accepted: false,
    })
    .unwrap();
    assert_eq!(root.snapshot().composer.unwrap().draft, "echo hi");
    assert!(root.snapshot().composer.unwrap().can_submit);

    let block = BlockId::from_bytes([0x44; 16]);
    root.apply(AppAction::ApplyRuntimeBlocks {
        fence: root.fence(),
        records: vec![RuntimeBlockRecord {
            id: block,
            command: "echo hi".into(),
            start_line: 1,
            end_line: Some(1),
            running: false,
            exit_status: Some(0),
        }],
    })
    .unwrap();
    let projected = &root.snapshot().composer.unwrap().blocks;
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].id, block);
    assert_eq!(projected[0].state, BlockPresentationState::Completed);
    assert_eq!(projected[0].pane, root.snapshot().pane);
}

#[test]
fn chrome_inspector_and_attention_do_not_invent_identities() {
    use crate::chrome::{AgentActivity, AgentRecord, AttentionItem, InspectorMode, LeftPanelMode};

    let mut root = ApplicationRoot::new();
    let workspace = root.snapshot().shell.active_workspace;
    let tab = root.snapshot().shell.active_tab;
    root.apply(AppAction::ReplaceChrome {
        fence: root.fence(),
        agents: vec![AgentRecord {
            id: AgentId::new("agent-1"),
            name: "Reviewer".into(),
            activity: AgentActivity::Attention,
        }],
        attention: vec![AttentionItem {
            id: AttentionId::new("att-1"),
            title: "Need review".into(),
            detail: "diff".into(),
            workspace: Some(workspace),
            tab: Some(tab),
            agent: Some(AgentId::new("agent-1")),
        }],
    })
    .unwrap();
    root.apply(AppAction::SetLeftPanel {
        mode: LeftPanelMode::Tabs,
    })
    .unwrap();
    root.apply(AppAction::SetInspectorMode {
        mode: InspectorMode::Workspace,
    })
    .unwrap();
    let chrome = root.snapshot().chrome;
    assert_eq!(chrome.left_panel, LeftPanelMode::Tabs);
    assert_eq!(chrome.inspector_mode, InspectorMode::Workspace);
    assert!(chrome
        .inspector_rows
        .iter()
        .all(|row| row.id != "runtime-telemetry"));
    assert_eq!(chrome.attention_items.len(), 1);
    root.apply(AppAction::OpenAttention {
        fence: root.fence(),
        id: AttentionId::new("att-1"),
    })
    .unwrap();
    let after = root.snapshot();
    assert!(after.chrome.attention_items.is_empty());
    assert_eq!(after.shell.active_workspace, workspace);
    assert_eq!(after.shell.active_tab, tab);
    assert_eq!(
        root.apply(AppAction::OpenAttention {
            fence: root.fence(),
            id: AttentionId::new("missing"),
        }),
        Err(AppError::UnknownAttention)
    );
}
