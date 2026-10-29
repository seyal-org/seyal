use crate::app::{AppAction, ApplicationRoot};

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
