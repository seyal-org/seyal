//! ADR-018 §3.3 app-level adopt / terminate / palette surface.

use seyal_core::{AttachmentId, ExecutionId, WorkspaceId};

use super::{AppAction, AppError, ApplicationRoot, BindingEvidence};
use crate::palette::PaletteCommand;
use crate::provisioning::ProvisioningEffect;

fn root_with_bootstrap_effects_drained() -> ApplicationRoot {
    let mut root = ApplicationRoot::new();
    while !root.snapshot().pending_effects.is_empty() {
        root.apply(AppAction::AckEffect).unwrap();
    }
    root
}

fn evidence(execution: ExecutionId, attachment: AttachmentId) -> BindingEvidence {
    BindingEvidence {
        execution,
        attachment,
        controller: true,
        pty_generation: 1,
        alternate_screen: false,
    }
}

#[test]
fn adopt_keeps_execution_id_and_uses_fresh_attachment() {
    let mut root = ApplicationRoot::new();
    let workspace = WorkspaceId::m001_default();
    let execution = ExecutionId::from_bytes([0xab; 16]);
    let first_attachment = AttachmentId::from_bytes([0x01; 16]);
    let second_attachment = AttachmentId::from_bytes([0x02; 16]);
    assert_ne!(first_attachment, second_attachment);

    root.apply(AppAction::RecordUnpresented {
        execution,
        workspace,
    })
    .unwrap();
    let fence = root.fence();
    root.apply(AppAction::Adopt {
        fence,
        evidence: evidence(execution, first_attachment),
    })
    .expect("adopt");

    let snap = root.snapshot();
    assert_eq!(snap.execution, Some(execution));
    assert_eq!(snap.attachment, Some(first_attachment));
    assert!(root.live_unpresented().is_empty());
    assert_eq!(
        root.provisioning().recorded_execution(snap.pane),
        Some(execution),
        "adopt must record the binding for P4 terminate / bound close"
    );

    // Already bound: reject with typed AlreadyBound (invariant 4).
    assert_eq!(
        root.apply(AppAction::Adopt {
            fence: root.fence(),
            evidence: evidence(execution, second_attachment),
        }),
        Err(AppError::AlreadyBound)
    );
}

#[test]
fn terminate_queues_adr005_effect_not_from_close() {
    let mut root = root_with_bootstrap_effects_drained();
    let workspace = WorkspaceId::m001_default();
    let execution = ExecutionId::new();
    root.apply(AppAction::RecordUnpresented {
        execution,
        workspace,
    })
    .unwrap();
    root.apply(AppAction::TerminateUnpresented { execution })
        .unwrap();
    assert!(
        root.snapshot().pending_effects.is_empty(),
        "dispose must not emit NativeEffect TerminateExecution"
    );
    assert!(
        matches!(
            root.pending_wire_effects(),
            [ProvisioningEffect::AttachController { execution: parked, .. }]
                if *parked == execution
        ),
        "§6.3 dispose parks AttachController, not a host NativeEffect"
    );
    assert_eq!(root.live_unpresented(), vec![execution]);
}

#[test]
fn palette_lists_unpresented_without_auto_select() {
    let mut root = ApplicationRoot::new();
    let workspace = WorkspaceId::m001_default();
    let low = ExecutionId::from_bytes([0x01; 16]);
    let high = ExecutionId::from_bytes([0xfe; 16]);
    root.apply(AppAction::RecordUnpresented {
        execution: high,
        workspace,
    })
    .unwrap();
    root.apply(AppAction::RecordUnpresented {
        execution: low,
        workspace,
    })
    .unwrap();

    let fence = root.fence();
    root.apply(AppAction::OpenPalette { fence }).unwrap();
    root.apply(AppAction::SetPaletteQuery {
        fence: root.fence(),
        query: "Adopt Unpresented".to_owned(),
    })
    .unwrap();
    let rows = root.snapshot().palette.rows;
    let adopt: Vec<_> = rows
        .iter()
        .filter(|row| row.label.starts_with("Adopt Unpresented:"))
        .collect();
    assert_eq!(adopt.len(), 2);
    // Deterministic order follows ExecutionId ascending (low then high).
    assert!(adopt[0].label.ends_with("01010101"));
    assert!(adopt[1].label.ends_with("fefefefe"));
}

#[test]
fn palette_terminate_dispatches_typed_action() {
    let mut root = root_with_bootstrap_effects_drained();
    let workspace = WorkspaceId::m001_default();
    let execution = ExecutionId::from_bytes([0x33; 16]);
    root.apply(AppAction::RecordUnpresented {
        execution,
        workspace,
    })
    .unwrap();
    let fence = root.fence();
    root.apply(AppAction::OpenPalette { fence }).unwrap();
    root.apply(AppAction::SetPaletteQuery {
        fence: root.fence(),
        query: "Terminate Unpresented".to_owned(),
    })
    .unwrap();
    root.apply(AppAction::RunPalette {
        fence: root.fence(),
        address: None,
    })
    .unwrap();
    assert!(root.snapshot().pending_effects.is_empty());
    assert!(matches!(
        root.pending_wire_effects(),
        [ProvisioningEffect::AttachController { execution: parked, .. }]
            if *parked == execution
    ));
    assert_eq!(root.live_unpresented(), vec![execution]);
}

#[test]
fn sync_drops_retired_and_bound() {
    let mut root = ApplicationRoot::new();
    let workspace = WorkspaceId::m001_default();
    let kept = ExecutionId::from_bytes([0x10; 16]);
    let bound = ExecutionId::from_bytes([0x20; 16]);
    root.apply(AppAction::RecordUnpresented {
        execution: ExecutionId::from_bytes([0x30; 16]),
        workspace,
    })
    .unwrap();
    root.apply(AppAction::RecordUnpresented {
        execution: bound,
        workspace,
    })
    .unwrap();
    // Bind removes bound ids from the catalog; sync must not resurrect them.
    root.apply(AppAction::Adopt {
        fence: root.fence(),
        evidence: evidence(bound, AttachmentId::from_bytes([0x99; 16])),
    })
    .unwrap();
    root.apply(AppAction::SyncLiveUnpresented {
        entries: vec![(kept, workspace), (bound, workspace)],
    })
    .unwrap();
    assert_eq!(root.live_unpresented(), vec![kept]);
}

#[test]
fn runtime_inventory_reconciles_unpresented_catalog_and_keeps_workspace_scope() {
    let mut root = ApplicationRoot::new();
    let workspace = WorkspaceId::m001_default();
    let other_workspace = WorkspaceId::from_bytes([0x44; 16]);
    let live_here = ExecutionId::from_bytes([0x11; 16]);
    let exited_here = ExecutionId::from_bytes([0x22; 16]);
    let live_elsewhere = ExecutionId::from_bytes([0x33; 16]);

    root.sync_live_unpresented(vec![
        (live_here, workspace),
        (exited_here, workspace),
        (live_elsewhere, other_workspace),
    ])
    .unwrap();

    // Runtime reports two live executions and omits the naturally exited one.
    root.reconcile_live_unpresented(&[live_here, live_elsewhere]);

    assert_eq!(root.live_unpresented(), vec![live_here]);
    assert_eq!(
        root.shell.live_unpresented(other_workspace),
        vec![live_elsewhere],
        "reconciliation keeps the Workspace association while removing exited ids"
    );
}

#[test]
fn runtime_inventory_imports_survivors_into_a_fresh_shell_catalog() {
    let mut root = ApplicationRoot::new();
    let first = ExecutionId::from_bytes([0x51; 16]);
    let second = ExecutionId::from_bytes([0x52; 16]);

    // A restarted GUI has no prior shell catalog. Runtime still owns these
    // live executions, and its current M001 association is the stable default
    // Workspace (SPEC-004 §18.2; ADR-007 §2).
    root.reconcile_live_unpresented(&[second, first]);

    assert_eq!(root.live_unpresented(), vec![first, second]);
}

#[test]
fn close_actions_do_not_emit_terminate_execution() {
    let mut root = ApplicationRoot::new();
    let execution = ExecutionId::new();
    let workspace = WorkspaceId::m001_default();
    root.apply(AppAction::RecordUnpresented {
        execution,
        workspace,
    })
    .unwrap();
    root.apply(AppAction::Adopt {
        fence: root.fence(),
        evidence: evidence(execution, AttachmentId::from_bytes([0x77; 16])),
    })
    .unwrap();
    let pane = root.fence().pane;
    root.apply(AppAction::ClosePane {
        id: pane,
        containment_generation: root.snapshot().shell.containment_generation,
    })
    .expect("presentation removal");
    assert!(root
        .snapshot()
        .pending_effects
        .iter()
        .all(|effect| !matches!(
            effect,
            super::NativeEffect::BoundedDetachThenTerminate { .. }
        )));
    assert_eq!(root.live_unpresented(), vec![execution]);
    let _ = PaletteCommand::TerminateUnpresented(execution);
}

#[test]
fn close_window_releases_bound_execution_into_live_unpresented_catalog() {
    let mut root = ApplicationRoot::new();
    let execution = ExecutionId::from_bytes([0x78; 16]);
    let window = root
        .snapshot()
        .shell
        .active_window
        .expect("initial product window");
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: evidence(execution, AttachmentId::from_bytes([0x79; 16])),
    })
    .expect("bind execution");
    let generation = root.snapshot().shell.containment_generation;

    root.apply(AppAction::CloseWindow {
        id: window,
        containment_generation: generation,
    })
    .expect("close presentation");

    assert!(root.snapshot().shell.windows.is_empty());
    assert_eq!(root.live_unpresented(), vec![execution]);
    assert!(root.provisioning().is_unreferenced(execution));
    assert!(root
        .snapshot()
        .pending_effects
        .iter()
        .all(|effect| !matches!(
            effect,
            super::NativeEffect::BoundedDetachThenTerminate { .. }
        )));
}

#[test]
fn palette_adopt_emits_attach_intent_without_binding() {
    let mut root = root_with_bootstrap_effects_drained();
    let workspace = WorkspaceId::m001_default();
    let execution = ExecutionId::from_bytes([0xcd; 16]);
    root.apply(AppAction::RecordUnpresented {
        execution,
        workspace,
    })
    .unwrap();
    let pane = root.snapshot().shell.focused_pane;
    assert!(root
        .snapshot()
        .shell
        .panes
        .iter()
        .find(|item| item.id == pane)
        .unwrap()
        .execution
        .is_none());

    let fence = root.fence();
    root.apply(AppAction::OpenPalette { fence }).unwrap();
    root.apply(AppAction::SetPaletteQuery {
        fence: root.fence(),
        query: "Adopt Unpresented".to_owned(),
    })
    .unwrap();
    root.apply(AppAction::RunPalette {
        fence: root.fence(),
        address: None,
    })
    .unwrap();

    assert!(
        root.snapshot().pending_effects.is_empty(),
        "adopt must not emit NativeEffect RequestAdoptAttach"
    );
    assert!(matches!(
        root.pending_wire_effects(),
        [ProvisioningEffect::AttachController { execution: parked, .. }]
            if *parked == execution
    ));
    // Catalog and leaf binding unchanged until BindPane after a real AttachmentId.
    assert_eq!(root.live_unpresented(), vec![execution]);
    assert!(root
        .snapshot()
        .shell
        .panes
        .iter()
        .find(|item| item.id == pane)
        .unwrap()
        .execution
        .is_none());
}

#[test]
fn adopt_rejects_cross_workspace_and_retired() {
    let mut root = ApplicationRoot::new();
    let local = WorkspaceId::m001_default();
    let other = WorkspaceId::from_bytes([0x22; 16]);
    let execution = ExecutionId::from_bytes([0x55; 16]);
    root.apply(AppAction::RecordUnpresented {
        execution,
        workspace: other,
    })
    .unwrap();
    assert_eq!(
        root.apply(AppAction::Adopt {
            fence: root.fence(),
            evidence: evidence(execution, AttachmentId::from_bytes([0x11; 16])),
        }),
        Err(AppError::CrossWorkspaceAdopt)
    );
    assert_eq!(
        root.apply(AppAction::Adopt {
            fence: root.fence(),
            evidence: evidence(
                ExecutionId::from_bytes([0x66; 16]),
                AttachmentId::from_bytes([0x12; 16])
            ),
        }),
        Err(AppError::ExecutionNotUnpresented)
    );
    let _ = local;
}

#[test]
fn failed_unpresented_record_does_not_change_provisioning_ledger() {
    let mut root = ApplicationRoot::new();
    let workspace = WorkspaceId::m001_default();
    let execution = ExecutionId::from_bytes([0xa5; 16]);

    root.apply(AppAction::RecordUnpresented {
        execution,
        workspace,
    })
    .expect("catalog the live execution");
    root.apply(AppAction::Adopt {
        fence: root.fence(),
        evidence: evidence(execution, AttachmentId::from_bytes([0xb6; 16])),
    })
    .expect("bind the same execution");

    assert!(!root.provisioning().is_unreferenced(execution));
    assert_eq!(
        root.apply(AppAction::RecordUnpresented {
            execution,
            workspace,
        }),
        Err(AppError::AlreadyBound)
    );
    assert!(
        !root.provisioning().is_unreferenced(execution),
        "a rejected catalog update must not leave a phantom unreferenced ledger entry"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn poll_client_drains_runtime_inventory_before_live_client_lookup() {
    let mut root = ApplicationRoot::new();
    let live = ExecutionId::from_bytes([0x61; 16]);
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    tx.send(Ok(vec![live])).unwrap();
    root.pending_unpresented_inventory = Some(rx);

    assert_eq!(root.poll_client(root.fence()), Err(AppError::NoLiveClient));

    assert!(root.pending_unpresented_inventory.is_none());
    assert_eq!(root.live_unpresented(), vec![live]);
}
