//! W4a: extra window creation stays off until close exists (ADR-018 §3.3a).

use seyal_core::{PaneId, TabId, WindowId, WorkspaceId};

use super::{
    ShellAction, ShellError, ShellNativeEffect, ShellPaneSeed, ShellState, ShellTabSeed,
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
        false,
    )
    .expect("fixture");
    (shell, empty)
}

#[test]
fn production_default_rejects_extra_window_without_mutation() {
    let mut shell = ShellState::m001_local("local");
    assert!(!shell.allows_window_creation());
    let generation = shell.containment_generation();
    let windows = shell.snapshot().windows.len();
    assert_eq!(windows, 1);
    assert_eq!(
        shell.apply(ShellAction::CreateWindow {
            workspace: WorkspaceId::m001_default(),
            containment_generation: generation,
        }),
        Err(ShellError::WindowCreationUnavailable)
    );
    assert_eq!(
        shell.last_error(),
        Some(ShellError::WindowCreationUnavailable)
    );
    assert_eq!(
        shell.last_error().expect("error").error_number(),
        20,
        "shell snapshot error number"
    );
    assert_eq!(shell.snapshot().windows.len(), windows);
    assert_eq!(shell.containment_generation(), generation);
    assert!(shell.take_effects().is_empty());
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
    assert_eq!(snap.windows.len(), 2);
    assert_eq!(snap.containment_generation, generation + 1);
    assert!(
        snap.windows.iter().any(|window| window.workspace == empty),
        "new window must land on the empty workspace"
    );
    assert!(shell
        .take_effects()
        .iter()
        .any(|effect| matches!(effect, ShellNativeEffect::RealizeWindow { .. })));
}

#[test]
fn admitted_create_window_adds_a_second_window() {
    let mut shell = ShellState::m001_local("local");
    shell.set_allows_window_creation_for_test(true);
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
