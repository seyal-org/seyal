//! K8: `goto.open` dispatch opens the existing N4 surface.

use crate::goto::GotoScope;
use crate::keybinding::{WorkspaceCommand, WorkspaceCommandId};

use super::ApplicationRoot;

#[test]
fn goto_open_command_opens_existing_goto_surface() {
    let mut root = ApplicationRoot::new();
    assert!(!root.snapshot().goto.open);

    let route = root.keybinding_route_context(false);
    root.invoke_workspace_command(
        WorkspaceCommand {
            id: WorkspaceCommandId::GotoOpen,
            ordinal: None,
        },
        route,
    )
    .expect("goto.open");

    let goto = root.snapshot().goto;
    assert!(goto.open);
    assert_eq!(goto.scope, GotoScope::Panes);
}
