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
    root.apply(AppAction::ClosePane {
        id: pane,
        containment_generation: root.snapshot().shell.containment_generation,
    })
    .unwrap();
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

#[test]
fn stale_host_close_actions_reject_the_observed_generation() {
    let mut root = ApplicationRoot::new();

    let window_snapshot = root.snapshot().shell;
    let first_window = window_snapshot.active_window.unwrap();
    root.apply_shell(crate::shell::ShellAction::CreateWindow {
        workspace: window_snapshot.active_workspace,
        containment_generation: window_snapshot.containment_generation,
    })
    .unwrap();
    let before_stale_window_close = root.snapshot().shell;
    assert_eq!(
        root.apply(AppAction::CloseWindow {
            id: first_window,
            containment_generation: window_snapshot.containment_generation,
        }),
        Err(AppError::StalePane)
    );
    let after_stale_window_close = root.snapshot().shell;
    assert_eq!(
        after_stale_window_close.windows,
        before_stale_window_close.windows
    );
    assert_eq!(
        after_stale_window_close.containment_generation,
        before_stale_window_close.containment_generation
    );

    let tab_snapshot = root.snapshot().shell;
    let tab = tab_snapshot.active_tab;
    root.apply(AppAction::CreateTab).unwrap();
    let before_stale_tab_close = root.snapshot().shell;
    assert_eq!(
        root.apply(AppAction::CloseTab {
            id: tab,
            containment_generation: tab_snapshot.containment_generation,
        }),
        Err(AppError::StalePane)
    );
    let after_stale_tab_close = root.snapshot().shell;
    assert_eq!(
        after_stale_tab_close.windows,
        before_stale_tab_close.windows
    );
    assert_eq!(
        after_stale_tab_close.containment_generation,
        before_stale_tab_close.containment_generation
    );

    let pane_snapshot = root.snapshot().shell;
    let pane = pane_snapshot.focused_pane;
    root.apply(AppAction::CreateTab).unwrap();
    let before_stale_pane_close = root.snapshot().shell;
    assert_eq!(
        root.apply(AppAction::ClosePane {
            id: pane,
            containment_generation: pane_snapshot.containment_generation,
        }),
        Err(AppError::StalePane)
    );
    let after_stale_pane_close = root.snapshot().shell;
    assert_eq!(
        after_stale_pane_close.windows,
        before_stale_pane_close.windows
    );
    assert_eq!(
        after_stale_pane_close.containment_generation,
        before_stale_pane_close.containment_generation
    );
}

#[test]
fn close_unknown_ids_reject_and_sole_tab_removes_window() {
    // Unknown identities reject without retarget. The sole Tab of the
    // production Window removes that Window (ADR-018 §3.2), replacing the
    // older last-tab refusal.
    let mut root = ApplicationRoot::new();
    let snap = root.snapshot();
    let only_tab = snap.shell.tabs[0].id;
    let only_pane = snap.shell.panes[0].id;

    assert_eq!(
        root.apply(AppAction::CloseTab {
            id: TabId::new(),
            containment_generation: snap.shell.containment_generation,
        }),
        Err(AppError::UnknownChromeTab)
    );
    assert_eq!(
        root.apply(AppAction::ClosePane {
            id: PaneId::new(),
            containment_generation: snap.shell.containment_generation,
        }),
        Err(AppError::UnknownPane)
    );
    assert_eq!(root.snapshot().shell.tabs.len(), 1);
    assert_eq!(root.snapshot().shell.panes.len(), 1);
    assert_eq!(root.snapshot().shell.tabs[0].id, only_tab);
    assert_eq!(root.snapshot().shell.panes[0].id, only_pane);

    root.apply(AppAction::CloseTab {
        id: only_tab,
        containment_generation: snap.shell.containment_generation,
    })
    .expect("sole tab removes window");
    assert!(root.snapshot().shell.windows.is_empty());
    assert_eq!(root.snapshot().shell.active_window, None);
}
