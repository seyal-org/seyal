//! W5 presentation-tier transitions (ADR-018 §5).

use seyal_core::{ExecutionId, TabId, WindowId, WorkspaceId};

use super::*;

fn two_tab_shell() -> (ShellState, WindowId, TabId, TabId) {
    let window = WindowId::new();
    let first = TabId::new();
    let second = TabId::new();
    let workspace = WorkspaceId::m001_default();
    let shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: workspace,
            name: "Local".to_owned(),
            detail: None,
            attention: false,
            active_window: window,
            windows: vec![ShellWindowSeed {
                id: window,
                active_tab: first,
                tabs: vec![
                    ShellTabSeed {
                        id: first,
                        title: "One".to_owned(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: PaneId::new(),
                            title: "Pane 1".to_owned(),
                            allows_implicit_execution_bootstrap: true,
                        },
                    },
                    ShellTabSeed {
                        id: second,
                        title: "Two".to_owned(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: PaneId::new(),
                            title: "Pane 1".to_owned(),
                            allows_implicit_execution_bootstrap: true,
                        },
                    },
                ],
            }],
        }],
        workspace,
        false,
        true,
        true,
    )
    .expect("two-tab shell");
    (shell, window, first, second)
}

fn leaf_tier(shell: &ShellState, tab: TabId) -> PresentationTier {
    let snap = shell.snapshot();
    snap.windows[0]
        .tabs
        .iter()
        .find(|entry| entry.id == tab)
        .expect("tab")
        .panes[0]
        .presentation_tier
}

#[test]
fn inactive_tab_leaf_is_hidden_active_is_focused() {
    let (mut shell, _window, first, second) = two_tab_shell();
    assert_eq!(leaf_tier(&shell, first), PresentationTier::Focused);
    assert_eq!(leaf_tier(&shell, second), PresentationTier::Hidden);

    shell
        .apply(ShellAction::SelectTab { id: second })
        .expect("select");
    assert_eq!(leaf_tier(&shell, first), PresentationTier::Hidden);
    assert_eq!(leaf_tier(&shell, second), PresentationTier::Focused);
}

#[test]
fn miniaturize_and_occlusion_hide_active_tab_without_containment_bump() {
    let (mut shell, window, first, second) = two_tab_shell();
    let generation = shell.snapshot().containment_generation;
    shell.set_window_miniaturized(window, true).unwrap();
    assert_eq!(leaf_tier(&shell, first), PresentationTier::Hidden);
    assert_eq!(leaf_tier(&shell, second), PresentationTier::Hidden);
    assert_eq!(shell.snapshot().containment_generation, generation);

    shell.set_window_miniaturized(window, false).unwrap();
    assert_eq!(leaf_tier(&shell, first), PresentationTier::Focused);

    shell.set_window_occluded(window, true).unwrap();
    assert_eq!(leaf_tier(&shell, first), PresentationTier::Hidden);
    shell.set_window_occluded(window, false).unwrap();
    assert_eq!(leaf_tier(&shell, first), PresentationTier::Focused);
}

#[test]
fn inverse_occlusion_while_miniaturized_stays_hidden() {
    let (mut shell, window, first, _) = two_tab_shell();
    shell.set_window_miniaturized(window, true).unwrap();
    shell.set_window_occluded(window, false).unwrap();
    assert_eq!(leaf_tier(&shell, first), PresentationTier::Hidden);
}

#[test]
fn unpresented_execution_is_not_a_leaf_tier() {
    let (mut shell, _window, first, _) = two_tab_shell();
    let execution = ExecutionId::from_bytes([7; 16]);
    shell
        .apply(ShellAction::RecordUnpresented {
            execution,
            workspace: WorkspaceId::m001_default(),
        })
        .expect("record");
    assert_eq!(leaf_tier(&shell, first), PresentationTier::Focused);
    assert_eq!(
        shell.live_unpresented(WorkspaceId::m001_default()).len(),
        1,
        "Unpresented is inventory, not a Hidden leaf"
    );
}
