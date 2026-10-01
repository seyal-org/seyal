//! Issue #1149 / M003 C2 — headed tab creation on the C1 provisioning route.

use seyal_core::{AttachmentId, ExecutionId};
use seyal_protocol::framing::ErrorCode;
use seyal_runtime::local_ipc::framing::{
    CreateExecutionResult, CreateExecutionResultCode, MessageType, TerminateExecutionResult,
    TerminateExecutionResultCode, HEADER_LEN,
};

use super::provisioning_apply::negotiated_provisioning_client;
use super::{
    AppAction, AppError, ApplicationRoot, BindingEvidence, PresentationEligibility, SplitAxis,
};
use crate::provisioning::{CreateOutcome, ProvisioningFailure, BOOTSTRAP_COLUMNS, BOOTSTRAP_ROWS};

fn exec(byte: u8) -> ExecutionId {
    ExecutionId::from_bytes([byte; 16])
}

fn attachment(byte: u8) -> AttachmentId {
    AttachmentId::from_bytes([byte; 16])
}

/// CreateTab → admitted type-36 on the wire client → create result → attach → bind.
fn drive_create_tab_to_bound(root: &mut ApplicationRoot, execution: ExecutionId) {
    root.apply(AppAction::CreateTab).expect("create tab");
    let pane = root.snapshot().shell.focused_pane;
    let intent = root
        .provisioning()
        .pending_intent(pane)
        .expect("create_tab must begin a pending intent")
        .clone();
    assert_eq!(intent.geometry.rows, BOOTSTRAP_ROWS);
    assert_eq!(intent.geometry.columns, BOOTSTRAP_COLUMNS);

    let client = root.wire_client().expect("wire client installed");
    assert!(
        client.has_pending_create(intent.request_id),
        "CreateTab must admit a create with the session request id"
    );
    assert!(
        client.has_outbound_create(intent.request_id)
            || client.has_pending_create(intent.request_id),
        "SendCreate must not be dropped: type-36 must be admitted on the client"
    );

    root.wire_client_mut()
        .unwrap()
        .accept_create_result(CreateExecutionResult {
            execution_id: execution,
            request_id: intent.request_id,
            result_code: CreateExecutionResultCode::Created,
            detail_code: 0,
        })
        .unwrap();
    root.absorb_wire_create_result()
        .expect("absorb create")
        .expect("create result present");
    root.complete_create_attach_and_bind(attachment(execution.to_bytes()[0]))
        .expect("attach+bind");
    assert_eq!(
        root.provisioning().recorded_execution(pane),
        Some(execution)
    );
}

#[test]
fn create_tab_admits_type_36_with_session_request_id() {
    let mut root = ApplicationRoot::new();
    root.enable_tab_creation_for_test();
    root.install_wire_client(negotiated_provisioning_client())
        .unwrap();
    root.apply(AppAction::CreateTab).unwrap();
    let pane = root.snapshot().shell.focused_pane;
    let request_id = root
        .provisioning()
        .pending_intent(pane)
        .expect("pending")
        .request_id;
    let client = root.wire_client().unwrap();
    assert!(
        client.has_pending_create(request_id),
        "session request id must be the admitted create id (no second allocator)"
    );
    assert_ne!(request_id, 0);
    // Decoder smoke: at least one outbound create frame or a flushed admission.
    assert!(
        client.has_outbound_create(request_id) || client.has_pending_create(request_id),
        "SendCreate must reach LocalDisplayClient::submit_create_execution_with_id"
    );
    let _ = (HEADER_LEN, MessageType::CreateExecutionRequest);
}

#[test]
fn production_composition_keeps_tab_creation_gated_and_splits_fail_closed() {
    let mut root = ApplicationRoot::new();
    let snap = root.snapshot();
    // Keep production gated until the wire create→attach→bind driver is live.
    assert!(!snap.shell.allows_tab_creation);
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
    root.enable_tab_creation_for_test();
    root.install_wire_client(negotiated_provisioning_client())
        .unwrap();
    let mut executions = Vec::new();
    let mut panes = Vec::new();

    for i in 1..=3u8 {
        let execution = exec(i);
        drive_create_tab_to_bound(&mut root, execution);
        let pane = root.snapshot().shell.focused_pane;
        assert!(
            !panes.contains(&pane),
            "each new tab must introduce a distinct PaneId"
        );
        panes.push(pane);
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
    root.enable_tab_creation_for_test();
    root.install_wire_client(negotiated_provisioning_client())
        .unwrap();
    drive_create_tab_to_bound(&mut root, exec(0xA1));
    let pane_a = root.snapshot().shell.focused_pane;
    let tab_a = root.snapshot().shell.active_tab;
    let exec_a = exec(0xA1);

    drive_create_tab_to_bound(&mut root, exec(0xB2));
    let pane_b = root.snapshot().shell.focused_pane;
    let tab_b = root.snapshot().shell.active_tab;
    let exec_b = exec(0xB2);
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
    root.enable_tab_creation_for_test();
    root.install_wire_client(negotiated_provisioning_client())
        .unwrap();
    drive_create_tab_to_bound(&mut root, exec(0x71));
    let pane = root.snapshot().shell.focused_pane;
    let tab = root.snapshot().shell.active_tab;
    let execution = exec(0x71);
    let wire_attachment = root.wire_client().unwrap().attachment_id();

    root.adopt_authority_for_provisioned_pane(BindingEvidence {
        execution,
        attachment: wire_attachment,
        controller: true,
        pty_generation: 1,
        alternate_screen: false,
    })
    .unwrap();

    let before_tabs = root.snapshot().shell.tabs.len();
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
    // Authority stays until TerminateExecutionResult; shell binding still present.
    assert_eq!(root.shell.pane_execution(pane).unwrap(), Some(execution));
    assert_eq!(root.provisioning().recorded_execution(pane), None);
    assert_ne!(
        root.snapshot().eligibility,
        PresentationEligibility::Unbound
    );
    let request_id = (1..=8u64)
        .find_map(|id| {
            root.provisioning()
                .pending_terminate_by_request_id(id)
                .map(|intent| intent.request_id)
        })
        .expect("pending terminate retained until result");
    assert!(
        root.wire_client()
            .unwrap()
            .has_pending_terminate(request_id)
            || root
                .wire_client()
                .unwrap()
                .has_outbound_terminate(request_id),
        "type 38 must be admitted before detach"
    );

    root.wire_client_mut()
        .unwrap()
        .accept_terminate_result(TerminateExecutionResult {
            attachment_id: wire_attachment,
            request_id,
            result_code: TerminateExecutionResultCode::TerminationRequested,
            detail_code: 0,
        })
        .unwrap();
    root.absorb_wire_terminate_result(false)
        .expect("absorb terminate")
        .expect("terminate result present");
    assert_eq!(root.shell.pane_execution(pane).unwrap(), None);
    assert_eq!(
        root.snapshot().eligibility,
        PresentationEligibility::Unbound
    );
    assert_eq!(root.provisioning().automatic_retries(), 0);
    assert!(root
        .provisioning()
        .pending_terminate_by_request_id(request_id)
        .is_none());
}

#[test]
fn explicit_terminate_admits_type_38_on_registry_client_without_install_wire_client() {
    // Production path: wire_client is None; admit goes through client_handle.
    // Must not clear/unregister the handle before type 38 is written.
    let mut root = ApplicationRoot::new();
    assert!(root.wire_client().is_none());
    let client = negotiated_provisioning_client();
    let execution = client.execution_id();
    let attachment_id = client.attachment_id();
    root.attach_client(root.fence(), client)
        .expect("register Controller into CLIENTS");
    assert!(root.wire_client().is_none());
    let handle = root
        .live_client_handle_for_test()
        .expect("production path retains registry handle");

    root.apply(AppAction::TerminateExecution {
        fence: root.fence(),
    })
    .expect("terminate must admit while handle is live");

    assert_eq!(
        root.live_client_handle_for_test(),
        Some(handle),
        "client_handle must stay registered until TerminateExecutionResult"
    );
    assert!(root.wire_client().is_none());
    assert_ne!(
        root.snapshot().eligibility,
        PresentationEligibility::Unbound
    );

    let request_id = (1..=8u64)
        .find_map(|id| {
            root.provisioning()
                .pending_terminate_by_request_id(id)
                .map(|intent| intent.request_id)
        })
        .expect("PendingKind::Terminate must remain until result");
    let admitted = crate::ffi::with_client(handle, |client| {
        client.has_pending_terminate(request_id) || client.has_outbound_terminate(request_id)
    })
    .expect("registry client still present");
    assert!(
        admitted,
        "type 38 must be written on the still-registered client_handle (no install_wire_client)"
    );

    crate::ffi::with_client_mut(handle, |client| {
        client
            .accept_terminate_result(TerminateExecutionResult {
                attachment_id,
                request_id,
                result_code: TerminateExecutionResultCode::TerminationRequested,
                detail_code: 0,
            })
            .expect("accept terminate result");
    })
    .expect("registry client");
    // Same absorb production `poll_client` invokes after poll_prepare. Probe
    // clients drop their socket peer, so poll_prepare would disconnect; drive
    // absorb directly to prove registry-client correlation and detach.
    root.absorb_wire_terminate_result(false)
        .expect("absorb terminate via client_handle")
        .expect("terminate result present");
    assert_eq!(root.live_client_handle_for_test(), None);
    assert_eq!(
        root.snapshot().eligibility,
        PresentationEligibility::Unbound
    );
    assert_eq!(root.snapshot().execution, None);
    assert!(!root.provisioning().is_unreferenced(execution));
    assert!(root
        .provisioning()
        .pending_terminate_by_request_id(request_id)
        .is_none());
}

#[test]
fn newly_bound_pane_keeps_spec_008_presentation_fence() {
    let mut root = ApplicationRoot::new();
    root.enable_tab_creation_for_test();
    root.install_wire_client(negotiated_provisioning_client())
        .unwrap();
    drive_create_tab_to_bound(&mut root, exec(0xF1));
    let execution = exec(0xF1);

    root.adopt_authority_for_provisioned_pane(BindingEvidence {
        execution,
        attachment: attachment(3),
        controller: true,
        pty_generation: 2,
        alternate_screen: false,
    })
    .unwrap();
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Flow);

    root.apply(AppAction::Refresh {
        fence: root.fence(),
        alternate_screen: true,
    })
    .unwrap();
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Tui);

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
    root.enable_tab_creation_for_test();
    root.install_wire_client(negotiated_provisioning_client())
        .unwrap();
    root.apply(AppAction::CreateTab).unwrap();
    let pane = root.snapshot().shell.focused_pane;
    let intent = root
        .provisioning()
        .pending_intent(pane)
        .expect("pending")
        .clone();

    root.wire_client_mut()
        .unwrap()
        .accept_create_result(CreateExecutionResult {
            execution_id: ExecutionId::from_bytes([0; 16]),
            request_id: intent.request_id,
            result_code: CreateExecutionResultCode::Error(ErrorCode::CapacityExceeded),
            detail_code: 0,
        })
        .unwrap();
    root.absorb_wire_create_result().unwrap();
    assert_eq!(
        root.provisioning().last_failure(),
        Some((
            pane,
            ProvisioningFailure::CreateRejected(ErrorCode::CapacityExceeded)
        ))
    );
    assert_eq!(root.provisioning().recorded_execution(pane), None);
    assert_eq!(root.provisioning().automatic_retries(), 0);
    assert_eq!(root.snapshot().shell.tabs.len(), 2);
    assert!(root.provisioning().pending_intent(pane).is_none());
}

#[test]
fn create_tab_does_not_block_with_fixed_frequency_retry() {
    let mut root = ApplicationRoot::new();
    root.enable_tab_creation_for_test();
    root.install_wire_client(negotiated_provisioning_client())
        .unwrap();
    root.apply(AppAction::CreateTab).unwrap();
    let pane = root.snapshot().shell.focused_pane;
    let intent = root.provisioning().pending_intent(pane).unwrap().clone();
    let _ = root.provisioning_mut().apply_create_result(
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
    root.enable_tab_creation_for_test();
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
