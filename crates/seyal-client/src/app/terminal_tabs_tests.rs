use crate::app::{AppAction, ApplicationRoot};
use crate::keybinding::{BindingContext, WorkspaceCommand, WorkspaceCommandId};

#[test]
fn new_terminal_action_adds_tab_to_focused_pane_without_splitting() {
    let mut root = ApplicationRoot::new();
    let before = root.snapshot().shell;
    let pane = before.focused_pane;
    let before_panes = before.panes.len();
    let before_terminals = before
        .panes
        .iter()
        .find(|row| row.id == pane)
        .unwrap()
        .terminal_tabs
        .len();

    root.apply(AppAction::NewTerminal)
        .expect("create terminal tab");

    let after = root.snapshot().shell;
    let focused = after.panes.iter().find(|row| row.id == pane).unwrap();
    assert_eq!(after.panes.len(), before_panes);
    assert_eq!(focused.terminal_tabs.len(), before_terminals + 1);
    assert_eq!(
        focused.selected_terminal_tab,
        focused.terminal_tabs.last().map(|row| row.id)
    );
    assert_eq!(
        after.focused_pane, pane,
        "terminal selection does not move pane focus"
    );
}

#[test]
fn close_focused_terminal_removes_only_selected_terminal_and_keeps_pane() {
    let mut root = ApplicationRoot::new();
    let before = root.snapshot().shell;
    let pane = before.focused_pane;
    let window_count = before.windows.len();
    let first_terminal = before
        .panes
        .iter()
        .find(|row| row.id == pane)
        .unwrap()
        .selected_terminal_tab
        .unwrap();

    root.apply(AppAction::NewTerminal)
        .expect("add second terminal to the Pane");
    let selected_terminal = root
        .snapshot()
        .shell
        .panes
        .iter()
        .find(|row| row.id == pane)
        .unwrap()
        .selected_terminal_tab
        .unwrap();

    root.invoke_workspace_command(
        WorkspaceCommand {
            id: WorkspaceCommandId::TerminalCloseFocused,
            ordinal: None,
        },
        BindingContext::APP,
    )
    .expect("close selected terminal");

    let after = root.snapshot().shell;
    let remaining_pane = after.panes.iter().find(|row| row.id == pane).unwrap();
    assert_eq!(after.windows.len(), window_count);
    assert_eq!(after.panes.len(), before.panes.len());
    assert_eq!(remaining_pane.terminal_tabs.len(), 1);
    assert_eq!(remaining_pane.terminal_tabs[0].id, first_terminal);
    assert_ne!(remaining_pane.terminal_tabs[0].id, selected_terminal);
    assert_eq!(remaining_pane.selected_terminal_tab, Some(first_terminal));

    root.invoke_workspace_command(
        WorkspaceCommand {
            id: WorkspaceCommandId::TerminalCloseFocused,
            ordinal: None,
        },
        BindingContext::APP,
    )
    .expect("close the remaining selected terminal");
    assert!(root
        .snapshot()
        .shell
        .panes
        .iter()
        .find(|row| row.id == pane)
        .unwrap()
        .terminal_tabs
        .is_empty());

    assert_eq!(
        root.invoke_workspace_command(
            WorkspaceCommand {
                id: WorkspaceCommandId::TerminalCloseFocused,
                ordinal: None,
            },
            BindingContext::APP,
        ),
        Err(crate::app::AppError::ActionUnavailable),
        "cmd+w with an empty Pane must not fall back to closing its layout"
    );
    let empty = root.snapshot().shell;
    assert_eq!(empty.windows.len(), window_count);
    assert_eq!(empty.panes.len(), before.panes.len());
}
