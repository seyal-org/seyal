//! N2 navigate / palette-by-address coverage extracted from `tests.rs` so the
//! façade suite stays under the handwritten 1_000-LOC hard gate.

use seyal_core::{AttachmentId, ExecutionId, PaneId, TabId, WindowId, WorkspaceId};

use super::{AppAction, AppError, ApplicationRoot, BindingEvidence};
use crate::navigation::ResourceAddress;
use crate::shell::{ShellPaneSeed, ShellState, ShellTabSeed, ShellWindowSeed, ShellWorkspaceSeed};

fn evidence(tag: u8, controller: bool, alternate: bool) -> BindingEvidence {
    BindingEvidence {
        execution: ExecutionId::from_bytes([tag; 16]),
        attachment: AttachmentId::from_bytes([tag.wrapping_add(1); 16]),
        controller,
        pty_generation: 1,
        alternate_screen: alternate,
    }
}

#[test]
fn navigate_preserves_presentation_epoch_and_rejected_leaves_focus() {
    let w1 = WorkspaceId::m001_default();
    let w2 = WorkspaceId::from_bytes([0x22; 16]);
    let t1 = TabId::from_bytes([0x01; 16]);
    let t2 = TabId::from_bytes([0x02; 16]);
    let p1 = PaneId::from_bytes([0x03; 16]);
    let p2 = PaneId::from_bytes([0x04; 16]);
    let win1 = WindowId::from_bytes([0x05; 16]);
    let win2 = WindowId::from_bytes([0x06; 16]);
    let shell = ShellState::from_workspaces(
        vec![
            ShellWorkspaceSeed {
                id: w1,
                name: "A".into(),
                detail: None,
                attention: false,
                active_window: win1,
                windows: vec![ShellWindowSeed {
                    id: win1,
                    active_tab: t1,
                    tabs: vec![ShellTabSeed {
                        id: t1,
                        title: "T1".into(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: p1,
                            title: "P1".into(),
                            allows_implicit_execution_bootstrap: true,
                        },
                    }],
                }],
            },
            ShellWorkspaceSeed {
                id: w2,
                name: "B".into(),
                detail: None,
                attention: false,
                active_window: win2,
                windows: vec![ShellWindowSeed {
                    id: win2,
                    active_tab: t2,
                    tabs: vec![ShellTabSeed {
                        id: t2,
                        title: "T2".into(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: p2,
                            title: "P2".into(),
                            allows_implicit_execution_bootstrap: false,
                        },
                    }],
                }],
            },
        ],
        w1,
        true,
        true,
    )
    .expect("fixture");
    let mut root = ApplicationRoot::with_shell(shell);
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: evidence(1, true, false),
    })
    .unwrap();
    let before = root.snapshot();
    let epoch = before.presentation_epoch;
    let execution = before.execution;
    let attachment = before.attachment;
    let focused = before.shell.focused_pane;

    // Rejected navigate leaves focus unchanged.
    assert_eq!(
        root.apply(AppAction::Navigate {
            fence: root.fence(),
            address: ResourceAddress::Pane {
                workspace: w2,
                tab: t2,
                pane: PaneId::from_bytes([0xee; 16]),
            },
        }),
        Err(AppError::NavigationUnknownPane)
    );
    assert_eq!(root.snapshot().shell.focused_pane, focused);
    assert_eq!(root.snapshot().shell.active_workspace, w1);

    root.apply(AppAction::Navigate {
        fence: root.fence(),
        address: ResourceAddress::Pane {
            workspace: w2,
            tab: t2,
            pane: p2,
        },
    })
    .unwrap();
    let after = root.snapshot();
    assert_eq!(after.shell.active_workspace, w2);
    assert_eq!(after.shell.focused_pane, p2);
    assert_eq!(after.presentation_epoch, epoch);
    assert_eq!(after.execution, execution);
    assert_eq!(after.attachment, attachment);
}

#[test]
fn palette_run_by_address_not_rebinding_ordinal() {
    let w1 = WorkspaceId::m001_default();
    let t1 = TabId::from_bytes([0x11; 16]);
    let t2 = TabId::from_bytes([0x12; 16]);
    let p1 = PaneId::from_bytes([0x13; 16]);
    let p2 = PaneId::from_bytes([0x14; 16]);
    let win1 = WindowId::from_bytes([0x15; 16]);
    let shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: w1,
            name: "A".into(),
            detail: None,
            attention: false,
            active_window: win1,
            windows: vec![ShellWindowSeed {
                id: win1,
                active_tab: t1,
                tabs: vec![
                    ShellTabSeed {
                        id: t1,
                        title: "One".into(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: p1,
                            title: "P1".into(),
                            allows_implicit_execution_bootstrap: true,
                        },
                    },
                    ShellTabSeed {
                        id: t2,
                        title: "Two".into(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: p2,
                            title: "P2".into(),
                            allows_implicit_execution_bootstrap: false,
                        },
                    },
                ],
            }],
        }],
        w1,
        true,
        true,
    )
    .expect("fixture");
    let mut root = ApplicationRoot::with_shell(shell);
    root.apply(AppAction::OpenPalette {
        fence: root.fence(),
    })
    .unwrap();
    root.apply(AppAction::SetPaletteQuery {
        fence: root.fence(),
        query: "Switch to Tab: Two".into(),
    })
    .unwrap();
    let rows = root.snapshot().palette.rows;
    assert_eq!(rows.len(), 1);
    let address = rows[0].address.expect("navigation row carries address");
    assert_eq!(
        address,
        ResourceAddress::Tab {
            workspace: w1,
            tab: t2
        }
    );
    // Shift live ordinals; frozen address still names t2.
    // CreateTab while the palette is open is menu-gated (K7); close first.
    root.apply(AppAction::ClosePalette {
        fence: root.fence(),
    })
    .unwrap();
    root.apply(AppAction::CreateTab).unwrap();
    assert_ne!(root.snapshot().shell.active_tab, t2);
    root.apply(AppAction::Navigate {
        fence: root.fence(),
        address,
    })
    .unwrap();
    assert_eq!(root.snapshot().shell.active_tab, t2);
}
