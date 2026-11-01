//! ApplicationRoot CreateWorkspace / SetWorkspaceRoot (ADR-028).

use seyal_core::WorkspaceId;

use super::ApplicationRoot;

#[test]
fn create_workspace_with_minted_id_projects_root() {
    let mut app = ApplicationRoot::new();
    let root = std::env::temp_dir().join("seyal-app-ws-create");
    let _ = std::fs::create_dir_all(&root);
    let path = root.to_string_lossy().into_owned();
    let id = WorkspaceId::from_bytes([0x71; 16]);
    app.create_workspace_with_minted_id_for_test(id, path)
        .expect("create");
    assert_eq!(app.shell.active_workspace_id(), id);
    assert!(app.shell.workspace_root(id).is_some());
}

#[test]
fn set_workspace_root_action_binds_default() {
    let mut app = ApplicationRoot::new();
    let root = std::env::temp_dir().join("seyal-app-ws-set");
    let _ = std::fs::create_dir_all(&root);
    let path = root.to_string_lossy().into_owned();
    app.set_workspace_root(WorkspaceId::m001_default(), path)
        .expect("set");
    assert!(app
        .shell
        .workspace_root(WorkspaceId::m001_default())
        .is_some());
}
