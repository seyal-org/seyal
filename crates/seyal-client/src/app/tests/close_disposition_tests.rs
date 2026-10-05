//! Disposition close must clear presentation identity so a surviving pane can rebind.

use super::super::*;
use crate::shell::{ShellPaneSeed, ShellTabSeed, ShellWindowSeed, ShellWorkspaceSeed, SplitAxis};
use seyal_core::WindowId;

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
fn closing_bound_authority_pane_clears_presentation_for_rebind() {
    let tab = TabId::new();
    let pane = PaneId::new();
    let window = WindowId::new();
    let workspace = WorkspaceId::m001_default();
    let shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: workspace,
            name: "Local".to_owned(),
            detail: Some("local".to_owned()),
            attention: false,
            active_window: window,
            windows: vec![ShellWindowSeed {
                id: window,
                active_tab: tab,
                tabs: vec![ShellTabSeed {
                    id: tab,
                    title: "Terminal".to_owned(),
                    attention: false,
                    pane: ShellPaneSeed {
                        id: pane,
                        title: "Pane 1".to_owned(),
                        allows_implicit_execution_bootstrap: true,
                    },
                }],
            }],
        }],
        workspace,
        true,
        false,
        false,
    )
    .expect("fixture");
    let mut root = ApplicationRoot::with_shell(shell);

    let first = evidence(4, true, false);
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: first,
    })
    .unwrap();
    assert_eq!(
        root.presentation
            .snapshot()
            .identity
            .map(|id| id.execution_id),
        Some(first.execution)
    );

    root.apply(AppAction::SplitFocused {
        axis: SplitAxis::Right,
    })
    .unwrap();
    // Focus moved to the new unbound leaf; close the original bound authority pane.
    root.apply(AppAction::ClosePane { id: pane }).unwrap();
    assert!(root.authority.is_none());
    assert_eq!(root.presentation.snapshot().identity, None);

    let second = evidence(5, true, false);
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: second,
    })
    .expect("rebind after authority pane close must succeed");
    assert_eq!(root.snapshot().execution, Some(second.execution));
}
