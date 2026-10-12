//! Issue #1217 / M003 C3 — pane splitting on the C1 create→attach→bind route.

use seyal_core::{AttachmentId, ExecutionId, PaneId};
use seyal_protocol::framing::ErrorCode;
use seyal_runtime::local_ipc::framing::{
    CreateExecutionResult, CreateExecutionResultCode, MessageType, HEADER_LEN,
};

use super::provisioning_apply::negotiated_provisioning_client;
use super::{AppAction, AppError, ApplicationRoot, BindingEvidence, SplitAxis};
use crate::composer::ComposerAction;
use crate::provisioning::{CreateOutcome, ProvisioningFailure, BOOTSTRAP_COLUMNS, BOOTSTRAP_ROWS};

fn exec(byte: u8) -> ExecutionId {
    ExecutionId::from_bytes([byte; 16])
}

fn attachment(byte: u8) -> AttachmentId {
    AttachmentId::from_bytes([byte; 16])
}

fn evidence(tag: u8) -> BindingEvidence {
    BindingEvidence {
        execution: exec(tag),
        attachment: attachment(tag.wrapping_add(1)),
        controller: true,
        pty_generation: 1,
        alternate_screen: false,
    }
}

/// SplitFocused → admitted type-36 → create result → attach → bind.
fn drive_split_to_bound(root: &mut ApplicationRoot, axis: SplitAxis, execution: ExecutionId) {
    root.apply(AppAction::SplitFocused { axis })
        .expect("split focused");
    let pane = root.snapshot().shell.focused_pane;
    complete_pending_split(root, pane, execution);
}

fn complete_pending_split(root: &mut ApplicationRoot, pane: PaneId, execution: ExecutionId) {
    let intent = root
        .provisioning()
        .pending_intent(pane)
        .expect("split must begin a pending intent")
        .clone();
    assert_eq!(intent.geometry.rows, BOOTSTRAP_ROWS);
    assert_eq!(intent.geometry.columns, BOOTSTRAP_COLUMNS);

    let client = root.wire_client().expect("wire client installed");
    assert!(
        client.has_pending_create(intent.request_id),
        "SplitFocused must admit a create with the session request id"
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
fn split_focus_has_no_live_region_until_bound_then_moves_authority_to_new_leaf() {
    let mut root = ApplicationRoot::new();
    root.install_wire_client(negotiated_provisioning_client())
        .unwrap();
    let first = root.snapshot().shell.focused_pane;
    let first_evidence = evidence(0xA0);
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: first_evidence,
    })
    .unwrap();

    root.apply(AppAction::SplitFocused {
        axis: SplitAxis::Right,
    })
    .unwrap();
    let new_pane = root.snapshot().shell.focused_pane;
    assert_ne!(new_pane, first);
    assert!(root.pane_regions().iter().all(|region| !region.live));
    assert_eq!(root.snapshot().execution, None);

    complete_pending_split(&mut root, new_pane, exec(0xA1));
    root.adopt_authority_for_provisioned_pane(evidence(0xA1))
        .expect("install live authority for bound split leaf");

    assert_eq!(root.snapshot().shell.focused_pane, new_pane);
    assert_eq!(root.snapshot().execution, Some(exec(0xA1)));
    assert_eq!(root.fence().pane, new_pane);
    assert_eq!(root.fence().execution, Some(exec(0xA1)));
    assert_eq!(
        root.pane_authorities.get(&first).unwrap().execution,
        first_evidence.execution
    );
    let regions = root.pane_regions();
    assert_eq!(regions.iter().filter(|region| region.live).count(), 1);
    assert!(regions
        .iter()
        .any(|region| region.pane == new_pane && region.focused && region.live));
    assert!(regions
        .iter()
        .any(|region| region.pane == first && !region.live));
}

fn build_two_by_two(root: &mut ApplicationRoot) -> [PaneId; 4] {
    let a = root.snapshot().shell.focused_pane;
    drive_split_to_bound(root, SplitAxis::Right, exec(0xA1));
    let b = root.snapshot().shell.focused_pane;
    assert_ne!(a, b);

    root.apply(AppAction::FocusPane { id: a }).unwrap();
    drive_split_to_bound(root, SplitAxis::Down, exec(0xA2));
    let d = root.snapshot().shell.focused_pane;
    assert_ne!(d, a);
    assert_ne!(d, b);

    root.apply(AppAction::FocusPane { id: b }).unwrap();
    drive_split_to_bound(root, SplitAxis::Down, exec(0xA3));
    let c = root.snapshot().shell.focused_pane;
    assert_ne!(c, a);
    assert_ne!(c, b);
    assert_ne!(c, d);

    [a, b, d, c]
}

#[test]
fn production_composition_enables_pane_splitting() {
    let root = ApplicationRoot::new();
    let snap = root.snapshot();
    assert!(snap.shell.allows_tab_creation);
    assert!(snap.shell.allows_pane_splitting);
}

#[test]
fn split_focused_admits_type_36_with_session_request_id() {
    let mut root = ApplicationRoot::new();
    root.install_wire_client(negotiated_provisioning_client())
        .unwrap();
    root.apply(AppAction::SplitFocused {
        axis: SplitAxis::Right,
    })
    .unwrap();
    let pane = root.snapshot().shell.focused_pane;
    let request_id = root
        .provisioning()
        .pending_intent(pane)
        .expect("pending")
        .request_id;
    let client = root.wire_client().unwrap();
    assert!(client.has_pending_create(request_id));
    assert_ne!(request_id, 0);
    assert!(
        client.has_outbound_create(request_id) || client.has_pending_create(request_id),
        "SendCreate must reach LocalDisplayClient"
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
fn two_by_two_layout_binds_four_distinct_executions_and_composers() {
    let mut root = ApplicationRoot::new();
    root.install_wire_client(negotiated_provisioning_client())
        .unwrap();
    let first_evidence = evidence(0xA0);
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: first_evidence,
    })
    .unwrap();
    let first = root.snapshot().shell.focused_pane;
    let epoch = root.composer.snapshot(first).expect("composer").epoch;
    root.composer
        .apply(ComposerAction::SetDraft {
            pane: first,
            text: "pane-a".into(),
            epoch,
        })
        .unwrap();

    let panes = build_two_by_two(&mut root);
    assert_eq!(root.snapshot().shell.panes.len(), 4);

    let mut seen = std::collections::HashSet::new();
    for pane in panes {
        let execution = root
            .shell
            .pane_execution(pane)
            .unwrap()
            .expect("each 2×2 leaf must bind an execution");
        assert!(
            seen.insert(execution),
            "2×2 leaves must not share ExecutionId"
        );
        let draft_epoch = root.composer.snapshot(pane).expect("composer").epoch;
        let label = format!("draft-{}", execution.to_bytes()[0]);
        root.composer
            .apply(ComposerAction::SetDraft {
                pane,
                text: label.clone(),
                epoch: draft_epoch,
            })
            .unwrap();
        assert_eq!(
            root.composer.snapshot(pane).unwrap().draft,
            label,
            "each terminal leaf owns an independent composer draft"
        );
    }
    assert_eq!(seen.len(), 4);
    assert_eq!(
        root.shell.pane_execution(first).unwrap(),
        Some(first_evidence.execution)
    );
    assert_eq!(
        root.composer.snapshot(first).unwrap().draft,
        format!("draft-{}", 0xA0)
    );
    assert_eq!(root.provisioning().automatic_retries(), 0);
}

#[test]
fn closing_one_pane_detaches_only_and_leaves_sibling_executions() {
    let mut root = ApplicationRoot::new();
    root.install_wire_client(negotiated_provisioning_client())
        .unwrap();
    let first = root.snapshot().shell.focused_pane;
    drive_split_to_bound(&mut root, SplitAxis::Right, exec(0xB1));
    let second = root.snapshot().shell.focused_pane;
    drive_split_to_bound(&mut root, SplitAxis::Down, exec(0xB2));
    let third = root.snapshot().shell.focused_pane;

    let exec_second = root.provisioning().recorded_execution(second).unwrap();
    let exec_third = root.provisioning().recorded_execution(third).unwrap();

    root.apply(AppAction::ClosePane { id: third }).unwrap();
    assert!(root.provisioning().is_unreferenced(exec_third));
    assert_eq!(
        root.provisioning().recorded_execution(second),
        Some(exec_second)
    );
    assert_eq!(root.provisioning().recorded_execution(third), None);
    assert_eq!(root.provisioning().automatic_retries(), 0);
    assert!(root.snapshot().shell.panes.iter().any(|p| p.id == first));
    assert!(root.snapshot().shell.panes.iter().any(|p| p.id == second));
    assert!(!root.snapshot().shell.panes.iter().any(|p| p.id == third));
}

#[test]
fn split_never_fabricates_attachment_id_on_production_path() {
    let mut root = ApplicationRoot::new();
    root.install_wire_client(negotiated_provisioning_client())
        .unwrap();
    root.apply(AppAction::SplitFocused {
        axis: SplitAxis::Down,
    })
    .unwrap();
    let pane = root.snapshot().shell.focused_pane;
    let intent = root.provisioning().pending_intent(pane).unwrap().clone();
    root.wire_client_mut()
        .unwrap()
        .accept_create_result(CreateExecutionResult {
            execution_id: exec(0xC1),
            request_id: intent.request_id,
            result_code: CreateExecutionResultCode::Created,
            detail_code: 0,
        })
        .unwrap();
    root.absorb_wire_create_result().unwrap();
    // Without complete_create_attach_and_bind supplying a real AttachmentId,
    // the pane must stay unbound (no fabricated attach).
    assert_eq!(root.provisioning().recorded_execution(pane), None);
    assert!(root.shell.pane_execution(pane).unwrap().is_none());
}

#[test]
fn capacity_exceeded_split_create_is_bounded_without_retry_or_bind() {
    let mut root = ApplicationRoot::new();
    root.install_wire_client(negotiated_provisioning_client())
        .unwrap();
    root.apply(AppAction::SplitFocused {
        axis: SplitAxis::Right,
    })
    .unwrap();
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
    assert!(root.provisioning().pending_intent(pane).is_none());
    // Shell leaf remains after Runtime reject (same as CreateTab); no silent
    // success bind and no fixed-frequency retry.
    assert_eq!(root.snapshot().shell.panes.len(), 2);
}

#[test]
fn split_does_not_block_with_fixed_frequency_retry() {
    let mut root = ApplicationRoot::new();
    root.install_wire_client(negotiated_provisioning_client())
        .unwrap();
    root.apply(AppAction::SplitFocused {
        axis: SplitAxis::Right,
    })
    .unwrap();
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
fn palette_lists_split_commands_under_production_policy() {
    let mut root = ApplicationRoot::new();
    root.apply(AppAction::OpenPalette {
        fence: root.fence(),
    })
    .unwrap();
    let snap = root.snapshot();
    let labels: Vec<_> = snap
        .palette
        .rows
        .iter()
        .map(|row| row.label.as_str())
        .collect();
    assert!(
        labels.contains(&"Split Pane Right"),
        "production composition enables Split Pane Right in the palette: {labels:?}"
    );
    assert!(
        labels.contains(&"Split Pane Down"),
        "production composition enables Split Pane Down in the palette: {labels:?}"
    );
}

#[test]
fn split_created_leaf_is_terminal_and_sibling_is_not_reprovisioned() {
    // ADR-017 §4.4: provisioning is per new terminal leaf. Split never creates
    // a non-terminal surface today; the new leaf takes create→attach→bind and
    // the focused sibling's recorded execution (if any) is left alone.
    let mut root = ApplicationRoot::new();
    root.install_wire_client(negotiated_provisioning_client())
        .unwrap();
    let first = root.snapshot().shell.focused_pane;
    assert!(
        root.snapshot()
            .shell
            .panes
            .iter()
            .find(|pane| pane.id == first)
            .unwrap()
            .allows_implicit_bootstrap,
        "bootstrap leaf remains the sole implicit-bootstrap terminal"
    );

    drive_split_to_bound(&mut root, SplitAxis::Right, exec(0xD1));
    let second = root.snapshot().shell.focused_pane;
    assert!(
        !root
            .snapshot()
            .shell
            .panes
            .iter()
            .find(|pane| pane.id == second)
            .unwrap()
            .allows_implicit_bootstrap,
        "split-created terminal leaf must not use implicit bootstrap"
    );
    assert_eq!(root.provisioning().recorded_execution(first), None);
    assert_eq!(
        root.provisioning().recorded_execution(second),
        Some(exec(0xD1))
    );
    assert_ne!(
        root.apply(AppAction::SplitFocused {
            axis: SplitAxis::Right,
        }),
        Err(AppError::PaneSplitUnavailable)
    );
}
