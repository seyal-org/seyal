//! W4a: extra window creation stays off until close exists (ADR-018 §3.3a).

use seyal_core::{PaneId, TabId, WindowId, WorkspaceId};

use super::{
    ShellAction, ShellNativeEffect, ShellPaneSeed, ShellState, ShellTabSeed,
    ShellWindowSeed, ShellWorkspaceSeed,
};

fn occupied_and_empty() -> (ShellState, WorkspaceId) {
    let occupied = WorkspaceId::m001_default();
    let empty = WorkspaceId::from_bytes([0x44; 16]);
    let window = WindowId::new();
    let tab = TabId::new();
    let shell = ShellState::from_workspaces(
        vec![
            ShellWorkspaceSeed {
                id: occupied,
                name: "Local".into(),
                detail: None,
                attention: false,
                active_window: window,
                windows: vec![ShellWindowSeed {
                    id: window,
                    active_tab: tab,
                    tabs: vec![ShellTabSeed {
                        id: tab,
                        title: "Terminal".into(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: PaneId::new(),
                            title: "Pane 1".into(),
                            allows_implicit_execution_bootstrap: true,
                        },
                    }],
                }],
            },
            ShellWorkspaceSeed {
                id: empty,
                name: "Empty".into(),
                detail: None,
                attention: false,
                active_window: WindowId::new(),
                windows: Vec::new(),
            },
        ],
        occupied,
        false,
        false,
    )
    .expect("fixture");
    (shell, empty)
}

#[test]
fn production_default_admits_extra_window_under_w4b() {
    let mut shell = ShellState::m001_local("local");
    assert!(shell.allows_window_creation(), "W4b admits CreateWindow");
    let before = shell.containment_fingerprint();
    let generation = shell.containment_generation();
    let windows = shell.snapshot().windows.len();
    assert_eq!(windows, 1);
    assert!(shell
        .apply(ShellAction::CreateWindow {
            workspace: WorkspaceId::m001_default(),
            containment_generation: generation,
        })
        .is_ok());
    assert_eq!(shell.snapshot().windows.len(), windows + 1);
    assert!(shell.containment_generation() > generation);
    let _ = shell.take_effects();
    assert_ne!(shell.containment_fingerprint(), before);
    // Tab creation is separately admitted under W4b.
    assert!(shell.allows_tab_creation());
}

#[test]
fn empty_workspace_create_window_succeeds_when_admission_is_off() {
    let (mut shell, empty) = occupied_and_empty();
    assert!(!shell.allows_window_creation());
    let generation = shell.containment_generation();
    shell
        .apply(ShellAction::CreateWindow {
            workspace: empty,
            containment_generation: generation,
        })
        .expect("zero-window re-entry");
    let snap = shell.snapshot();
    assert_eq!(snap.active_workspace, empty);
    assert_eq!(snap.windows.len(), 2);
    assert_eq!(snap.containment_generation, generation + 1);
    assert!(shell
        .take_effects()
        .iter()
        .any(|effect| matches!(effect, ShellNativeEffect::RealizeWindow { .. })));
}

#[test]
fn admitted_create_window_adds_a_second_window() {
    let mut shell = ShellState::m001_local("local");
    shell.set_allows_window_creation(true);
    let generation = shell.containment_generation();
    shell
        .apply(ShellAction::CreateWindow {
            workspace: WorkspaceId::m001_default(),
            containment_generation: generation,
        })
        .expect("admission on");
    assert_eq!(shell.snapshot().windows.len(), 2);
    assert!(shell
        .take_effects()
        .iter()
        .any(|effect| matches!(effect, ShellNativeEffect::RealizeWindow { .. })));
}
