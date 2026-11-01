//! CreateWorkspace / SetWorkspaceRoot / root match (ADR-028).

use seyal_core::WorkspaceId;

use super::{ShellError, ShellState};

#[test]
fn create_workspace_binds_root_and_activates() {
    let mut shell = ShellState::m001_local("local");
    let root = std::env::temp_dir().join("seyal-ws-create-bind");
    let _ = std::fs::create_dir_all(&root);
    let path = root.to_string_lossy().into_owned();
    let id = WorkspaceId::from_bytes([0x41; 16]);

    shell
        .create_workspace(id, path.clone(), shell.containment_generation())
        .expect("create");

    assert_eq!(shell.active_workspace_id(), id);
    let bound = shell.workspace_root(id).expect("root");
    assert!(!bound.is_empty());
    assert_eq!(shell.find_workspace_by_root(bound), Some(id));
    let snap = shell.snapshot();
    let row = snap.workspaces.iter().find(|ws| ws.id == id).expect("row");
    assert_ne!(row.name, "Local");
    assert!(
        row.detail.as_ref().is_some_and(|d| d.contains('/')),
        "{row:?}"
    );
}

#[test]
fn create_workspace_rejects_empty_and_duplicate() {
    let mut shell = ShellState::m001_local("local");
    let id = WorkspaceId::from_bytes([0x42; 16]);
    assert_eq!(
        shell.create_workspace(id, String::new(), 0),
        Err(ShellError::InvalidWorkspaceRoot)
    );
    assert_eq!(
        shell.create_workspace(WorkspaceId::m001_default(), "/tmp".into(), 0),
        Err(ShellError::DuplicateWorkspace)
    );
}

#[test]
fn set_workspace_root_updates_default_local() {
    let mut shell = ShellState::m001_local("local");
    let root = std::env::temp_dir().join("seyal-ws-set-root");
    let _ = std::fs::create_dir_all(&root);
    let path = root.to_string_lossy().into_owned();
    let default = WorkspaceId::m001_default();
    shell.set_workspace_root(default, path).expect("set root");
    let bound = shell.workspace_root(default).expect("bound");
    assert_eq!(shell.find_workspace_by_root(bound), Some(default));
}
