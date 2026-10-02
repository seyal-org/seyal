//! ADR-018 §3.3 app-level adopt / terminate / palette surface.

use seyal_core::{AttachmentId, ExecutionId, WorkspaceId};

use super::{AppAction, AppError, ApplicationRoot, BindingEvidence, NativeEffect};
use crate::palette::PaletteCommand;

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
    // Without a live Runtime accept, terminate must fail closed and keep the id.
    assert_eq!(
        root.apply(AppAction::TerminateExecution { execution }),
        Err(AppError::TerminationNotRequested)
    );
    assert!(!root
        .snapshot()
        .pending_effects
        .iter()
        .any(|effect| matches!(effect, NativeEffect::TerminateExecution { .. })));
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
    root.apply(AppAction::OpenPalette {
        fence: root.fence(),
    })
    .unwrap();
    root.apply(AppAction::SetPaletteQuery {
        fence: root.fence(),
        query: "Terminate Unpresented".to_owned(),
    })
    .unwrap();
    assert_eq!(
        root.apply(AppAction::RunPalette {
            fence: root.fence(),
            address: None,
        }),
        Err(AppError::TerminationNotRequested)
    );
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
    root.apply(AppAction::ClosePane { id: pane })
        .expect("presentation removal");
    assert!(root
        .snapshot()
        .pending_effects
        .iter()
        .all(|effect| !matches!(effect, NativeEffect::TerminateExecution { .. })));
    assert_eq!(root.live_unpresented(), vec![execution]);
    let _ = PaletteCommand::TerminateUnpresented(execution);
}

#[test]
fn close_window_keeps_bound_execution_live_and_enumerable() {
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
        evidence: evidence(execution, AttachmentId::from_bytes([0x88; 16])),
    })
    .unwrap();
    let window = root
        .snapshot()
        .shell
        .active_window
        .expect("product-active window");
    root.apply(AppAction::CloseWindow { id: window })
        .expect("CloseWindow removes presentation only");
    assert!(root
        .snapshot()
        .pending_effects
        .iter()
        .all(|effect| !matches!(effect, NativeEffect::TerminateExecution { .. })));
    assert!(root
        .snapshot()
        .pending_effects
        .iter()
        .any(|effect| matches!(effect, NativeEffect::DestroyWindowRealization { .. })));
    assert_eq!(root.live_unpresented(), vec![execution]);
    assert!(root.snapshot().shell.active_window.is_none());
    assert_eq!(
        root.snapshot().eligibility,
        super::PresentationEligibility::Unbound,
        "B1: after CloseWindow, cleared authority must project Unbound so host recovery cannot open_first"
    );
    assert!(
        root.fence().execution.is_none(),
        "fence must not claim an execution after CloseWindow unbind"
    );

    // Re-entry must not auto-reattach the live-unpresented execution. CreateWindow
    // seeds a bootstrap-gated pane; eligibility stays Unbound until Rust Adopt/Bind.
    root.apply(AppAction::CreateWindow)
        .expect("zero-window re-entry CreateWindow");
    assert_eq!(
        root.live_unpresented(),
        vec![execution],
        "CreateWindow must leave the prior execution live-unpresented"
    );
    assert_eq!(
        root.snapshot().eligibility,
        super::PresentationEligibility::Unbound,
        "CreateWindow must not bind authority without AdoptExecution"
    );
    assert!(
        root.fence().execution.is_none(),
        "re-entry fence must stay execution-free until Adopt/Bind"
    );
    let snap = root.snapshot();
    let focused = snap.shell.focused_pane;
    let pane = snap
        .shell
        .panes
        .iter()
        .find(|pane| pane.id == focused)
        .expect("focused pane after CreateWindow");
    assert!(
        !pane.allows_implicit_bootstrap,
        "B1: CreateWindow pane must gate implicit bootstrap so recovery cannot open_first"
    );
}
