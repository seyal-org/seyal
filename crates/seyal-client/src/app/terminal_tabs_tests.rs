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
fn close_focused_terminal_removes_selected_then_last_closes_window() {
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
    let closed = root.snapshot().shell;
    assert!(
        closed.windows.is_empty(),
        "final terminal in a sole Pane hierarchical-closes the Window"
    );
    assert!(closed.panes.is_empty());
    assert!(
        root.invoke_workspace_command(
            WorkspaceCommand {
                id: WorkspaceCommandId::TerminalCloseFocused,
                ordinal: None,
            },
            BindingContext::APP,
        )
        .is_err(),
        "zero-window composition has no focused terminal to close"
    );
}

#[test]
fn closing_final_terminal_in_a_split_pane_collapses_that_pane() {
    use crate::shell::SplitAxis;

    let mut root = ApplicationRoot::new();
    let first_pane = root.snapshot().shell.focused_pane;
    root.apply(AppAction::SplitFocused {
        axis: SplitAxis::Right,
    })
    .expect("split creates a sibling Pane");
    let shell = root.snapshot().shell;
    assert_eq!(shell.panes.len(), 2);
    let split_pane = shell.focused_pane;
    assert_ne!(split_pane, first_pane);
    let terminal_tab = shell
        .panes
        .iter()
        .find(|row| row.id == split_pane)
        .unwrap()
        .selected_terminal_tab
        .unwrap();

    root.apply(AppAction::CloseTerminalTab {
        pane: split_pane,
        terminal_tab,
        containment_generation: shell.containment_generation,
    })
    .expect("final terminal in a non-sole Pane collapses the Pane");

    let after = root.snapshot().shell;
    assert_eq!(after.panes.len(), 1);
    assert_eq!(after.panes[0].id, first_pane);
    assert!(
        after.panes.iter().all(|row| row.id != split_pane),
        "collapsed Pane must leave the tree"
    );
}
