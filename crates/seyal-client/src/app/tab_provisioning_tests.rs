//! Issue #1149 / M003 C2 — headed tab creation on the C1 provisioning route.

use seyal_core::{AttachmentId, ExecutionId};
use seyal_protocol::framing::ErrorCode;
use seyal_runtime::local_ipc::framing::{
    CreateExecutionResult, CreateExecutionResultCode, MessageType, TerminateExecutionResult,
    TerminateExecutionResultCode, HEADER_LEN,
};

use super::provisioning_apply::negotiated_provisioning_client;
use super::{AppAction, AppError, ApplicationRoot, BindingEvidence, PresentationEligibility};
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
    root.complete_create_attach_and_bind(intent.request_id, attachment(execution.to_bytes()[0]))
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
    if client.has_outbound_create(request_id) {
        assert_eq!(
            client.admitted_create_workspace_id(request_id),
            Some(0),
            "M003 create must encode workspace_id 0 (SPEC-004 §18.2)"
        );
    }
    let _ = (HEADER_LEN, MessageType::CreateExecutionRequest);
}

#[test]
fn create_tab_seeds_request_id_past_bootstrap_floor() {
    let mut root = ApplicationRoot::new();
    root.enable_tab_creation_for_test();
    let mut client = negotiated_provisioning_client();
    // Bootstrap create already consumed connection-local id 1.
    client.next_provisioning_request_id = 2;
    root.install_wire_client(client).unwrap();
    root.apply(AppAction::CreateTab).unwrap();
    let pane = root.snapshot().shell.focused_pane;
    let request_id = root
        .provisioning()
        .pending_intent(pane)
        .expect("pending")
        .request_id;
    assert!(
        request_id >= 2,
        "CreateTab must not reuse bootstrap request_id 1; got {request_id}"
    );
}

#[test]
fn interleaved_outstanding_creates_bind_by_request_id_not_focused_pane() {
    let mut root = ApplicationRoot::new();
    root.enable_tab_creation_for_test();
    root.install_wire_client(negotiated_provisioning_client())
        .unwrap();

    root.apply(AppAction::CreateTab).unwrap();
    let pane_a = root.snapshot().shell.focused_pane;
    let intent_a = root.provisioning().pending_intent(pane_a).unwrap().clone();
    root.apply(AppAction::CreateTab).unwrap();
    let pane_b = root.snapshot().shell.focused_pane;
    let intent_b = root.provisioning().pending_intent(pane_b).unwrap().clone();
    assert_ne!(pane_a, pane_b);
    assert_ne!(
        intent_a.request_id, intent_b.request_id,
        "two panes on one wire connection must not both use the same request_id"
    );
    assert!(intent_b.request_id > intent_a.request_id);

    let exec_a = exec(0xA1);
    let exec_b = exec(0xB2);
    for (intent, execution) in [(&intent_a, exec_a), (&intent_b, exec_b)] {
        root.wire_client_mut()
            .unwrap()
            .accept_create_result(CreateExecutionResult {
                execution_id: execution,
                request_id: intent.request_id,
                result_code: CreateExecutionResultCode::Created,
                detail_code: 0,
            })
            .unwrap();
    }
    // Both results queued; absorb drains in arrival order without losing one.
    let first = root.absorb_wire_create_result().unwrap().unwrap();
    let second = root.absorb_wire_create_result().unwrap().unwrap();
    assert_eq!(first.request_id, intent_a.request_id);
    assert_eq!(second.request_id, intent_b.request_id);
    assert!(root.absorb_wire_create_result().unwrap().is_none());

    // Focus is on pane B; completing A's request must bind A, never B.
    assert_eq!(root.snapshot().shell.focused_pane, pane_b);
    root.complete_create_attach_and_bind(intent_a.request_id, attachment(1))
        .unwrap();
    assert_eq!(root.provisioning().recorded_execution(pane_a), Some(exec_a));
    assert_eq!(root.provisioning().recorded_execution(pane_b), None);
    assert_eq!(root.shell.pane_execution(pane_a).unwrap(), Some(exec_a));
    assert_eq!(root.shell.pane_execution(pane_b).unwrap(), None);
    assert!(root.provisioning().pending_intent(pane_b).is_some());

    root.complete_create_attach_and_bind(intent_b.request_id, attachment(2))
        .unwrap();
    assert_eq!(root.provisioning().recorded_execution(pane_b), Some(exec_b));
    assert_eq!(root.shell.pane_execution(pane_b).unwrap(), Some(exec_b));

    assert_eq!(
        root.complete_create_attach_and_bind(intent_a.request_id, attachment(3)),
        Err(AppError::ProvisioningRejected),
        "a completed request id must not bind again"
    );
}

#[test]
fn create_result_is_absorbed_from_registry_client_without_install_wire_client() {
    // Same absorb `poll_client` runs after poll_prepare on the production path.
    let mut root = ApplicationRoot::new();
    root.enable_tab_creation_for_test();
    root.attach_client(root.fence(), negotiated_provisioning_client())
        .expect("register Controller into CLIENTS");
    assert!(root.wire_client().is_none());
    let handle = root.live_client_handle_for_test().unwrap();

    root.apply(AppAction::CreateTab).unwrap();
    let pane = root.snapshot().shell.focused_pane;
    let intent = root.provisioning().pending_intent(pane).unwrap().clone();
    let admitted = crate::ffi::with_client(handle, |client| {
        client.has_pending_create(intent.request_id)
    })
    .unwrap();
    assert!(admitted, "type 36 must be admitted on the registry client");

    let execution = exec(0xC3);
    crate::ffi::with_client_mut(handle, |client| {
        client
            .accept_create_result(CreateExecutionResult {
                execution_id: execution,
                request_id: intent.request_id,
                result_code: CreateExecutionResultCode::Created,
                detail_code: 0,
            })
            .unwrap();
    })
    .unwrap();
    let result = root
        .absorb_wire_create_result()
        .expect("absorb via client_handle")
        .expect("create result present");
    assert_eq!(result.request_id, intent.request_id);
    root.complete_create_attach_and_bind(intent.request_id, attachment(7))
        .unwrap();
    assert_eq!(
        root.provisioning().recorded_execution(pane),
        Some(execution)
    );
}

#[test]
fn production_composition_enables_tab_creation_alongside_pane_splitting() {
    let root = ApplicationRoot::new();
    let snap = root.snapshot();
    // C2b / #1175 + C3 / #1217: CreateTab and SplitFocused share the live C1 path.
    assert!(snap.shell.allows_tab_creation);
    assert!(snap.shell.allows_pane_splitting);
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

    // Make tab_b the authority pane, then close it (detach-only).
    let wire_attachment = root.wire_client().unwrap().attachment_id();
    root.adopt_authority_for_provisioned_pane(BindingEvidence {
        execution: exec_b,
        attachment: wire_attachment,
        controller: true,
        pty_generation: 1,
        alternate_screen: false,
    })
    .unwrap();
    root.apply(AppAction::CloseTab { id: tab_b }).unwrap();
    assert_eq!(root.snapshot().shell.tabs.len(), 2);
    assert!(root.provisioning().is_unreferenced(exec_b));
    assert!(!root.provisioning().is_unreferenced(exec_a));
    assert_eq!(root.provisioning().recorded_execution(pane_a), Some(exec_a));
    assert_eq!(root.provisioning().recorded_execution(pane_b), None);
    assert_eq!(root.provisioning().automatic_retries(), 0);
    assert!(
        root.wire_client().is_some(),
        "detach-only close must keep the shared wire client for remaining tabs"
    );
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
fn terminate_after_attach_seeds_request_id_past_bootstrap_without_manual_seed() {
    // Production attach installs client_handle after Bind. Session must seed
    // from the live client's next id so terminate cannot reuse bootstrap id 1.
    let mut root = ApplicationRoot::new();
    let mut client = negotiated_provisioning_client();
    client.next_provisioning_request_id = 2;
    root.attach_client(root.fence(), client)
        .expect("attach registers Controller and seeds request floor");
    assert!(root.wire_client().is_none());
    assert!(root.live_client_handle_for_test().is_some());

    root.apply(AppAction::TerminateExecution {
        fence: root.fence(),
    })
    .expect("terminate must admit past bootstrap floor");
    let request_id = (1..=16u64)
        .find_map(|id| {
            root.provisioning()
                .pending_terminate_by_request_id(id)
                .map(|intent| intent.request_id)
        })
        .expect("pending terminate");
    assert!(
        request_id >= 2,
        "terminate must not reuse bootstrap create id 1 after attach_client; got {request_id}"
    );
}

#[test]
fn terminate_after_bootstrap_floor_admits_request_id_at_least_two() {
    // Wire-client install path (tests / alternate host): create_tab seeds from
    // wire_client; terminate must still stay past bootstrap id 1.
    let mut root = ApplicationRoot::new();
    root.enable_tab_creation_for_test();
    let mut client = negotiated_provisioning_client();
    client.next_provisioning_request_id = 2;
    root.install_wire_client(client).unwrap();
    // No manual seed: create_tab / terminate must raise the floor from wire.

    drive_create_tab_to_bound(&mut root, exec(0x91));
    let pane = root.snapshot().shell.focused_pane;
    let execution = exec(0x91);
    let wire_attachment = root.wire_client().unwrap().attachment_id();
    root.adopt_authority_for_provisioned_pane(BindingEvidence {
        execution,
        attachment: wire_attachment,
        controller: true,
        pty_generation: 1,
        alternate_screen: false,
    })
    .unwrap();

    root.apply(AppAction::TerminateExecution {
        fence: root.fence(),
    })
    .unwrap();
    let request_id = (1..=16u64)
        .find_map(|id| {
            root.provisioning()
                .pending_terminate_by_request_id(id)
                .map(|intent| intent.request_id)
        })
        .expect("pending terminate");
    assert!(
        request_id >= 2,
        "terminate must not reuse bootstrap create id 1; got {request_id}"
    );
    assert!(
        root.wire_client()
            .unwrap()
            .has_pending_terminate(request_id)
            || root
                .wire_client()
                .unwrap()
                .has_outbound_terminate(request_id)
    );
    let _ = pane;
}

#[test]
fn explicit_terminate_admits_type_38_on_registry_client_without_install_wire_client() {
    // Production path: wire_client is None; admit goes through client_handle.
    // Must not clear/unregister the handle before type 38 is written, and the
    // shared registry client stays after terminate absorb (C2 multi-tab).
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
        "client_handle must stay registered while type 38 is admitted"
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
    assert_eq!(
        root.live_client_handle_for_test(),
        Some(handle),
        "terminate result detaches authority only; shared client_handle stays for CreateTab / remaining tabs"
    );
    assert!(
        crate::ffi::with_client(handle, |_| ()).is_some(),
        "registry entry must remain after terminate absorb"
    );
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
fn explicit_terminate_fails_closed_without_live_client_and_stays_retryable() {
    let mut root = ApplicationRoot::new();
    let client = negotiated_provisioning_client();
    let execution = client.execution_id();
    root.attach_client(root.fence(), client)
        .expect("register Controller");
    let handle = root.live_client_handle_for_test().expect("registry handle");
    crate::ffi::unregister_client(handle);
    let _ = root.poll_client(root.fence());
    assert_eq!(root.live_client_handle_for_test(), None);
    assert_eq!(
        root.provisioning().recorded_execution(root.fence().pane),
        Some(execution)
    );

    let error = root
        .apply(AppAction::TerminateExecution {
            fence: root.fence(),
        })
        .expect_err("terminate must not succeed without a live wire client");
    assert_eq!(error, AppError::NoLiveClient);
    assert_eq!(root.snapshot().last_error, Some(AppError::NoLiveClient));
    assert_eq!(
        root.provisioning().recorded_execution(root.fence().pane),
        Some(execution),
        "binding must remain so terminate can retry after reconnect"
    );
    assert!(
        (1..=16u64).all(|id| {
            root.provisioning()
                .pending_terminate_by_request_id(id)
                .is_none()
        }),
        "failed admit must not leak a PendingKind::Terminate"
    );
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
    root.apply(AppAction::SetPaletteQuery {
        fence: root.fence(),
        query: "Terminate Execution".into(),
    })
    .unwrap();
    assert!(
        root.snapshot()
            .palette
            .rows
            .iter()
            .any(|row| row.label == "Terminate Execution"),
        "P4 terminate is a palette product action, distinct from Close Tab"
    );
}
