//! CreateWorkspace / SetWorkspaceRoot / root match (ADR-028).

use seyal_core::WorkspaceId;

use super::{ShellAction, ShellError, ShellState, SplitAxis};

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

#[test]
fn rename_workspace_and_pane_update_labels_without_identity_change() {
    let mut shell = ShellState::m001_local("local");
    let workspace = WorkspaceId::m001_default();
    let pane = shell.snapshot().focused_pane;
    let before_root = shell.workspace_root(workspace).map(str::to_owned);

    shell
        .rename_workspace(workspace, "  Core  ")
        .expect("rename workspace");
    shell.rename_pane(pane, "Build").expect("rename pane");

    let snap = shell.snapshot();
    let row = snap
        .workspaces
        .iter()
        .find(|ws| ws.id == workspace)
        .expect("workspace row");
    assert_eq!(row.name, "Core");
    assert_eq!(
        snap.panes
            .iter()
            .find(|p| p.id == pane)
            .map(|p| p.title.as_str()),
        Some("Build")
    );
    assert_eq!(
        shell.workspace_root(workspace).map(str::to_owned),
        before_root,
        "rename must not change the bound root"
    );
    assert_eq!(
        shell.rename_workspace(workspace, "   "),
        Err(ShellError::InvalidDisplayName)
    );
    assert_eq!(
        shell.rename_pane(pane, ""),
        Err(ShellError::InvalidDisplayName)
    );
}

#[test]
fn rename_pane_updates_only_selected_terminal_and_sibling_pane() {
    let mut shell = ShellState::m001_local("local");
    let first = shell.snapshot().focused_pane;
    shell
        .apply(ShellAction::SplitPane {
            id: first,
            axis: SplitAxis::Right,
            containment_generation: shell.containment_generation(),
        })
        .expect("split");
    let second = shell.snapshot().focused_pane;
    assert_ne!(first, second);

    shell
        .apply(ShellAction::NewTerminalTab {
            pane: second,
            containment_generation: shell.containment_generation(),
        })
        .expect("second terminal in focused pane");
    let before = shell.snapshot();
    let second_pane = before
        .panes
        .iter()
        .find(|pane| pane.id == second)
        .expect("second pane");
    assert_eq!(second_pane.terminal_tabs.len(), 2);
    let selected = second_pane.selected_terminal_tab.expect("selected");
    let other = second_pane
        .terminal_tabs
        .iter()
        .find(|terminal| terminal.id != selected)
        .expect("sibling terminal")
        .id;
    let other_title = second_pane
        .terminal_tabs
        .iter()
        .find(|terminal| terminal.id == other)
        .map(|terminal| terminal.title.clone())
        .expect("other title");
    let first_title = before
        .panes
        .iter()
        .find(|pane| pane.id == first)
        .map(|pane| pane.title.clone())
        .expect("first title");

    shell
        .rename_pane(second, "Build")
        .expect("rename focused pane");

    let after = shell.snapshot();
    let renamed = after
        .panes
        .iter()
        .find(|pane| pane.id == second)
        .expect("renamed pane");
    assert_eq!(renamed.title, "Build");
    assert_eq!(
        renamed
            .terminal_tabs
            .iter()
            .find(|terminal| terminal.id == selected)
            .map(|terminal| terminal.title.as_str()),
        Some("Build"),
        "selected terminal label must update"
    );
    assert_eq!(
        renamed
            .terminal_tabs
            .iter()
            .find(|terminal| terminal.id == other)
            .map(|terminal| terminal.title.as_str()),
        Some(other_title.as_str()),
        "sibling terminal label must stay unchanged"
    );
    assert_eq!(
        after
            .panes
            .iter()
            .find(|pane| pane.id == first)
            .map(|pane| pane.title.as_str()),
        Some(first_title.as_str()),
        "sibling pane label must stay unchanged"
    );
}
