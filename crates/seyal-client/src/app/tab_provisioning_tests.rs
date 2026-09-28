//! Issue #1149 / M003 C2 — headed tab creation on the C1 provisioning route.

use seyal_core::{AttachmentId, ExecutionId};
use seyal_protocol::framing::ErrorCode;

use super::{
    AppAction, AppError, ApplicationRoot, BindingEvidence, PresentationEligibility, SplitAxis,
};
use crate::provisioning::{
    CreateOutcome, ProvisioningEffect, ProvisioningFailure, BOOTSTRAP_COLUMNS, BOOTSTRAP_ROWS,
};
use crate::shell::ShellAction;

fn exec(byte: u8) -> ExecutionId {
    ExecutionId::from_bytes([byte; 16])
}

fn attachment(byte: u8) -> AttachmentId {
    AttachmentId::from_bytes([byte; 16])
}

/// Drive the focused tab's pending create intent through bind into ShellState
/// and ProvisioningSession (no ApplicationRoot authority yet).
fn drive_focused_intent_to_shell_bound(root: &mut ApplicationRoot, execution: ExecutionId) {
    let pane = root.snapshot().shell.focused_pane;
    let intent = root
        .provisioning()
        .pending_intent(pane)
        .expect("create_tab must begin a pending intent")
        .clone();
    let owner = intent.owner;
    let request_id = intent.request_id;
    assert_eq!(intent.geometry.rows, BOOTSTRAP_ROWS);
    assert_eq!(intent.geometry.columns, BOOTSTRAP_COLUMNS);

    let effects = root.provisioning_mut().apply_create_result(
        owner,
        request_id,
        CreateOutcome::Created(execution),
    );
    assert_eq!(
        effects,
        vec![ProvisioningEffect::AttachController { owner, execution }]
    );
    let effects = root.provisioning_mut().apply_attach_success(
        owner,
        request_id,
        attachment(execution.to_bytes()[0]),
    );
    assert_eq!(
        effects,
        vec![ProvisioningEffect::BindPane { pane, execution }]
    );
    root.shell
        .apply(ShellAction::BindExecution { pane, execution })
        .expect("shell bind");
    let more = root.provisioning_mut().apply_bind_success(pane, execution);
    assert!(matches!(
        more.as_slice(),
        [ProvisioningEffect::RequestBootstrapResize { .. }]
    ));
    assert_eq!(
        root.provisioning().recorded_execution(pane),
        Some(execution)
    );
}

#[test]
fn production_composition_allows_tab_creation_not_pane_splitting() {
    let mut root = ApplicationRoot::new();
    let snap = root.snapshot();
    assert!(snap.shell.allows_tab_creation);
    assert!(!snap.shell.allows_pane_splitting);
    assert_eq!(
        root.apply(AppAction::SplitFocused {
            axis: SplitAxis::Right,
        }),
        Err(AppError::PaneSplitUnavailable)
    );
}

#[test]
fn n_created_tabs_produce_n_distinct_execution_bindings() {
    let mut root = ApplicationRoot::new();
    let mut executions = Vec::new();
    let mut panes = Vec::new();

    // Initial tab has no create intent yet (adopt/first-launch is separate).
    // Each CreateTab provisions a distinct leaf.
    for i in 1..=3u8 {
        root.apply(AppAction::CreateTab).expect("create tab");
        let pane = root.snapshot().shell.focused_pane;
        assert!(
            !panes.contains(&pane),
            "each new tab must introduce a distinct PaneId"
        );
        panes.push(pane);
        let execution = exec(i);
        drive_focused_intent_to_shell_bound(&mut root, execution);
        executions.push(execution);
    }

    assert_eq!(root.snapshot().shell.tabs.len(), 4);
    let mut seen = std::collections::HashSet::new();
    for (pane, execution) in panes.iter().zip(executions.iter()) {
        assert_eq!(
            root.provisioning().recorded_execution(*pane),
            Some(*execution)
        );
        assert!(seen.insert(*execution), "ExecutionIds must be distinct");
    }
    assert_eq!(root.provisioning().automatic_retries(), 0);
}

#[test]
fn removing_a_tab_detaches_only_and_leaves_unrelated_executions() {
    let mut root = ApplicationRoot::new();
    root.apply(AppAction::CreateTab).unwrap();
    let pane_a = root.snapshot().shell.focused_pane;
    let tab_a = root.snapshot().shell.active_tab;
    let exec_a = exec(0xA1);
    drive_focused_intent_to_shell_bound(&mut root, exec_a);

    root.apply(AppAction::CreateTab).unwrap();
    let pane_b = root.snapshot().shell.focused_pane;
    let tab_b = root.snapshot().shell.active_tab;
    let exec_b = exec(0xB2);
    drive_focused_intent_to_shell_bound(&mut root, exec_b);
    assert_ne!(tab_a, tab_b);
    assert_ne!(pane_a, pane_b);

    root.apply(AppAction::CloseTab { id: tab_b }).unwrap();
    assert_eq!(root.snapshot().shell.tabs.len(), 2);
    assert!(root.provisioning().is_unreferenced(exec_b));
    assert!(!root.provisioning().is_unreferenced(exec_a));
    assert_eq!(root.provisioning().recorded_execution(pane_a), Some(exec_a));
    assert_eq!(root.provisioning().recorded_execution(pane_b), None);
    assert_eq!(root.provisioning().automatic_retries(), 0);
}

#[test]
fn explicit_terminate_is_distinct_from_removing_chrome() {
    let mut root = ApplicationRoot::new();
    root.apply(AppAction::CreateTab).unwrap();
    let pane = root.snapshot().shell.focused_pane;
    let tab = root.snapshot().shell.active_tab;
    let execution = exec(0x71);

    // Provision to BindPane, then take ApplicationRoot authority via Bind
    // without a prior shell bind so BindExecution succeeds once.
    let intent = root
        .provisioning()
        .pending_intent(pane)
        .expect("pending")
        .clone();
    root.provisioning_mut().apply_create_result(
        intent.owner,
        intent.request_id,
        CreateOutcome::Created(execution),
    );
    root.provisioning_mut()
        .apply_attach_success(intent.owner, intent.request_id, attachment(9));
    root.provisioning_mut().apply_bind_success(pane, execution);

    let fence = root.fence();
    root.apply(AppAction::Bind {
        fence,
        evidence: BindingEvidence {
            execution,
            attachment: attachment(9),
            controller: true,
            pty_generation: 1,
            alternate_screen: false,
        },
    })
    .unwrap();

    let before_tabs = root.snapshot().shell.tabs.len();
    let effects_before = root.provisioning().recorded_execution(pane);
    assert_eq!(effects_before, Some(execution));

    root.apply(AppAction::TerminateExecution {
        fence: root.fence(),
    })
    .unwrap();
    assert_eq!(
        root.snapshot().shell.tabs.len(),
        before_tabs,
        "explicit terminate must not remove tab chrome"
    );
    assert!(root.snapshot().shell.tabs.iter().any(|item| item.id == tab));
    assert_eq!(root.shell.pane_execution(pane).unwrap(), None);
    assert_eq!(root.provisioning().recorded_execution(pane), None);
    assert_eq!(
        root.snapshot().eligibility,
        PresentationEligibility::Unbound
    );
    // Closing chrome afterward is still detach-only for any other bound tab.
    assert_eq!(root.provisioning().automatic_retries(), 0);
}

#[test]
fn newly_bound_pane_keeps_spec_008_presentation_fence() {
    let mut root = ApplicationRoot::new();
    root.apply(AppAction::CreateTab).unwrap();
    let pane = root.snapshot().shell.focused_pane;
    let execution = exec(0xF1);

    let intent = root
        .provisioning()
        .pending_intent(pane)
        .expect("pending")
        .clone();
    root.provisioning_mut().apply_create_result(
        intent.owner,
        intent.request_id,
        CreateOutcome::Created(execution),
    );
    root.provisioning_mut()
        .apply_attach_success(intent.owner, intent.request_id, attachment(3));
    root.provisioning_mut().apply_bind_success(pane, execution);

    let fence = root.fence();
    root.apply(AppAction::Bind {
        fence,
        evidence: BindingEvidence {
            execution,
            attachment: attachment(3),
            controller: true,
            pty_generation: 2,
            alternate_screen: false,
        },
    })
    .unwrap();
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Flow);

    root.apply(AppAction::Refresh {
        fence: root.fence(),
        alternate_screen: true,
    })
    .unwrap();
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Tui);

    // Stale fence after transition is rejected (SPEC-008).
    let mut stale = root.fence();
    stale.presentation_epoch = stale.presentation_epoch.saturating_sub(1);
    assert_eq!(
        root.apply(AppAction::Refresh {
            fence: stale,
            alternate_screen: false,
        }),
        Err(AppError::StalePresentationEpoch)
    );
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Tui);
}

#[test]
fn capacity_exceeded_create_is_bounded_failure_without_retry_or_bind() {
    let mut root = ApplicationRoot::new();
    root.apply(AppAction::CreateTab).unwrap();
    let pane = root.snapshot().shell.focused_pane;
    let intent = root
        .provisioning()
        .pending_intent(pane)
        .expect("pending")
        .clone();

    let effects = root.provisioning_mut().apply_create_result(
        intent.owner,
        intent.request_id,
        CreateOutcome::Failed(ErrorCode::CapacityExceeded),
    );
    assert!(effects.is_empty());
    assert_eq!(
        root.provisioning().last_failure(),
        Some((
            pane,
            ProvisioningFailure::CreateRejected(ErrorCode::CapacityExceeded)
        ))
    );
    assert_eq!(root.provisioning().recorded_execution(pane), None);
    assert_eq!(root.provisioning().automatic_retries(), 0);
    // Tab chrome remains; failure is honest and non-crashing.
    assert_eq!(root.snapshot().shell.tabs.len(), 2);
    assert!(root.provisioning().pending_intent(pane).is_none());
}

#[test]
fn create_tab_does_not_block_with_fixed_frequency_retry() {
    let mut root = ApplicationRoot::new();
    root.apply(AppAction::CreateTab).unwrap();
    let pane = root.snapshot().shell.focused_pane;
    let intent = root.provisioning().pending_intent(pane).unwrap().clone();
    root.provisioning_mut().apply_create_result(
        intent.owner,
        intent.request_id,
        CreateOutcome::Failed(ErrorCode::Backpressure),
    );
    assert_eq!(root.provisioning().automatic_retries(), 0);
    assert!(root.provisioning().pending_intent(pane).is_none());
}

#[test]
fn palette_lists_new_tab_under_production_policy() {
    let mut root = ApplicationRoot::new();
    root.apply(AppAction::OpenPalette {
        fence: root.fence(),
    })
    .unwrap();
    assert!(
        root.snapshot()
            .palette
            .rows
            .iter()
            .any(|row| row.label == "New Tab"),
        "production composition enables CreateTab in the palette"
    );
}
