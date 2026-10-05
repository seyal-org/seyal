//! W7 orthogonal lifecycle matrix cells for ADR-018 / AGENTS.md.
//!
//! Each test names an independent fact combination. Impossible-by-construction
//! states are asserted rather than silently omitted (green-CI rule). Full matrix
//! ledger: `docs/evidence/m003-w7-1221-adversarial-matrix.md`.

use seyal_core::{ExecutionId, TabId, WindowId, WorkspaceId};

use super::*;

fn two_window_shell() -> (ShellState, WindowId, WindowId, TabId, TabId, PaneId, PaneId) {
    let w1 = WindowId::new();
    let w2 = WindowId::new();
    let t1 = TabId::new();
    let t2 = TabId::new();
    let p1 = PaneId::new();
    let p2 = PaneId::new();
    let shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: WorkspaceId::m001_default(),
            name: "Local".into(),
            detail: None,
            attention: false,
            active_window: w1,
            windows: vec![
                ShellWindowSeed {
                    id: w1,
                    active_tab: t1,
                    tabs: vec![ShellTabSeed {
                        id: t1,
                        title: "One".into(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: p1,
                            title: "P1".into(),
                            allows_implicit_execution_bootstrap: false,
                        },
                    }],
                },
                ShellWindowSeed {
                    id: w2,
                    active_tab: t2,
                    tabs: vec![ShellTabSeed {
                        id: t2,
                        title: "Two".into(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: p2,
                            title: "P2".into(),
                            allows_implicit_execution_bootstrap: false,
                        },
                    }],
                },
            ],
        }],
        WorkspaceId::m001_default(),
        true,
        true,
        true,
    )
    .expect("fixture");
    (shell, w1, w2, t1, t2, p1, p2)
}

fn leaf_tier(shell: &ShellState, pane: PaneId) -> PresentationTier {
    shell
        .snapshot()
        .windows
        .iter()
        .flat_map(|window| window.tabs.iter())
        .flat_map(|tab| tab.panes.iter())
        .find(|entry| entry.id == pane)
        .expect("pane")
        .presentation_tier
}

fn containment(shell: &ShellState) -> u64 {
    shell.snapshot().containment_generation
}

/// Documented impossible: a closed window cannot host a Focused/Visible/Hidden leaf.
#[test]
fn closed_window_has_no_leaf_presentation_tiers() {
    let (mut shell, w1, _w2, _t1, _t2, p1, p2) = two_window_shell();
    let execution = ExecutionId::from_bytes([0x11; 16]);
    shell
        .apply(ShellAction::BindExecution {
            pane: p1,
            execution,
        })
        .unwrap();
    shell
        .apply(ShellAction::CloseWindow {
            id: w1,
            containment_generation: containment(&shell),
        })
        .unwrap();
    let snap = shell.snapshot();
    assert!(!snap.windows.iter().any(|window| window.id == w1));
    assert!(
        snap.windows
            .iter()
            .flat_map(|window| window.tabs.iter())
            .flat_map(|tab| tab.panes.iter())
            .all(|pane| pane.id != p1),
        "closed window leaves cannot remain Focused/Visible/Hidden"
    );
    assert_eq!(
        shell.live_unpresented(WorkspaceId::m001_default()),
        vec![execution]
    );
    assert_eq!(leaf_tier(&shell, p2), PresentationTier::Focused);
}

/// Window open × execution alive × attached (bound) × Focused.
#[test]
fn open_alive_bound_focused_is_represented() {
    let (mut shell, _w1, _w2, _t1, _t2, p1, _) = two_window_shell();
    let execution = ExecutionId::from_bytes([0x22; 16]);
    shell
        .apply(ShellAction::BindExecution {
            pane: p1,
            execution,
        })
        .unwrap();
    assert_eq!(leaf_tier(&shell, p1), PresentationTier::Focused);
    assert_eq!(
        shell.snapshot().windows[0].tabs[0].panes[0].execution,
        Some(execution)
    );
}

/// Window open × execution unbound × Hidden (inactive tab).
#[test]
fn open_unbound_hidden_inactive_tab_is_represented() {
    let (mut shell, window, _, _, _, _, _) = two_window_shell();
    let window = shell.snapshot().active_window.unwrap_or(window);
    shell
        .apply(ShellAction::CreateTab {
            window,
            containment_generation: containment(&shell),
        })
        .unwrap();
    let snap = shell.snapshot();
    let host = snap
        .windows
        .iter()
        .find(|entry| entry.id == window)
        .expect("window");
    assert_eq!(host.tabs.len(), 2);
    let focused_pane = host
        .tabs
        .iter()
        .find(|tab| tab.id == host.active_tab)
        .unwrap()
        .panes[0]
        .id;
    let hidden_pane = host
        .tabs
        .iter()
        .find(|tab| tab.id != host.active_tab)
        .unwrap()
        .panes[0]
        .id;
    assert_eq!(leaf_tier(&shell, focused_pane), PresentationTier::Focused);
    assert_eq!(leaf_tier(&shell, hidden_pane), PresentationTier::Hidden);
    assert!(host
        .tabs
        .iter()
        .flat_map(|tab| tab.panes.iter())
        .find(|pane| pane.id == hidden_pane)
        .unwrap()
        .execution
        .is_none());
}

/// Window open × occluded → Hidden without containment bump.
#[test]
fn open_occluded_hidden_without_containment_bump() {
    let (mut shell, w1, _, _, _, p1, _) = two_window_shell();
    let before = containment(&shell);
    shell.set_window_occluded(w1, true).unwrap();
    assert_eq!(leaf_tier(&shell, p1), PresentationTier::Hidden);
    assert_eq!(containment(&shell), before);
}

/// Shell close is not quit: zero-window re-entry still admits CreateWindow.
#[test]
fn shell_close_does_not_encode_quitting_state() {
    let (mut shell, w1, _, _, _, p1, _) = two_window_shell();
    let execution = ExecutionId::from_bytes([0x33; 16]);
    shell
        .apply(ShellAction::BindExecution {
            pane: p1,
            execution,
        })
        .unwrap();
    shell
        .apply(ShellAction::CloseWindow {
            id: w1,
            containment_generation: containment(&shell),
        })
        .unwrap();
    shell
        .apply(ShellAction::CreateWindow {
            workspace: WorkspaceId::m001_default(),
            containment_generation: containment(&shell),
        })
        .expect("zero-window re-entry is not quit");
    assert!(shell.snapshot().active_window.is_some());
}

/// Impossible-by-construction: Unpresented is inventory, never a live leaf tier.
#[test]
fn unpresented_is_inventory_not_leaf_tier() {
    let (mut shell, _, _, _, _, p1, _) = two_window_shell();
    let execution = ExecutionId::from_bytes([0x44; 16]);
    shell
        .apply(ShellAction::RecordUnpresented {
            execution,
            workspace: WorkspaceId::m001_default(),
        })
        .unwrap();
    assert_eq!(leaf_tier(&shell, p1), PresentationTier::Focused);
    assert_eq!(
        shell.live_unpresented(WorkspaceId::m001_default()),
        vec![execution]
    );
    assert!(shell
        .snapshot()
        .windows
        .iter()
        .flat_map(|window| window.tabs.iter())
        .flat_map(|tab| tab.panes.iter())
        .all(|pane| pane.presentation_tier != PresentationTier::Unpresented));
}

/// Controller/Observer is an AppFence/attachment fact — shell bindings carry
/// only ExecutionId (impossible for shell to encode Role).
#[test]
fn shell_binding_does_not_encode_controller_observer_role() {
    let (mut shell, _, _, _, _, p1, _) = two_window_shell();
    let execution = ExecutionId::from_bytes([0x55; 16]);
    shell
        .apply(ShellAction::BindExecution {
            pane: p1,
            execution,
        })
        .unwrap();
    let snap = shell.snapshot();
    let pane = snap.windows[0].tabs[0]
        .panes
        .iter()
        .find(|entry| entry.id == p1)
        .unwrap();
    assert_eq!(pane.execution, Some(execution));
    let rendered = format!("{pane:?}");
    assert!(
        !rendered.contains("Controller") && !rendered.contains("Observer"),
        "PaneLeafSnapshot must not encode attachment role"
    );
}

/// Inverse: presentation removal of a Tab leaves the Window open and does not
/// emit TerminateExecution (execution becomes Unpresented inventory).
#[test]
fn tab_close_leaves_window_open_without_terminate() {
    let (mut shell, w1, _, t1, _, p1, _) = two_window_shell();
    shell
        .apply(ShellAction::CreateTab {
            window: w1,
            containment_generation: containment(&shell),
        })
        .unwrap();
    let execution = ExecutionId::from_bytes([0x66; 16]);
    shell
        .apply(ShellAction::BindExecution {
            pane: p1,
            execution,
        })
        .unwrap();
    shell
        .apply(ShellAction::CloseTab {
            id: t1,
            containment_generation: containment(&shell),
        })
        .unwrap();
    assert_eq!(shell.snapshot().active_window, Some(w1));
    assert!(shell
        .snapshot()
        .windows
        .iter()
        .any(|window| window.id == w1));
    assert_eq!(
        shell.live_unpresented(WorkspaceId::m001_default()),
        vec![execution]
    );
    assert!(shell
        .take_effects()
        .iter()
        .all(|effect| !matches!(effect, ShellNativeEffect::TerminateExecution { .. })));
}
