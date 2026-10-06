//! ADR-018 §3.3 app-level adopt / terminate / palette surface.

use seyal_core::{AttachmentId, ExecutionId, WorkspaceId};

use super::{AppAction, AppError, ApplicationRoot, BindingEvidence};
use crate::palette::PaletteCommand;
use crate::provisioning::ProvisioningEffect;

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
    let mut root = ApplicationRoot::new();
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
    let mut root = ApplicationRoot::new();
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
fn close_actions_do_not_emit_terminate_execution() {
    // Production shell is single-pane; close fails as last-pane before bound
    // checks. Either way no TerminateExecution effect is queued — destruction
    // paths that could unbind are W2b/W4b and do not exist on this branch.
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
    assert!(matches!(
        root.apply(AppAction::ClosePane { id: pane }),
        Err(AppError::CannotCloseLastPane) | Err(AppError::CannotCloseBoundPane)
    ));
    assert!(root.snapshot().pending_effects.is_empty());
    let _ = PaletteCommand::TerminateUnpresented(execution);
}

#[test]
fn palette_adopt_emits_attach_intent_without_binding() {
    let mut root = ApplicationRoot::new();
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
