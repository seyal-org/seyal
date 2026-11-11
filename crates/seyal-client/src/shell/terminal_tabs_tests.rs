use super::*;
use seyal_core::TerminalTabId;

fn active_shell() -> ShellState {
    ShellState::m001_local("/tmp/seyal-terminal-tabs")
}

#[test]
fn fresh_workspace_has_one_pane_and_one_selected_terminal_tab() {
    let shell = active_shell();
    let snapshot = shell.snapshot();
    assert_eq!(snapshot.panes.len(), 1);
    assert_eq!(snapshot.panes[0].terminal_tabs.len(), 1);
    assert_eq!(
        snapshot.panes[0].selected_terminal_tab,
        Some(snapshot.panes[0].terminal_tabs[0].id)
    );
    assert_eq!(snapshot.panes[0].terminal_tabs[0].execution, None);
}

#[test]
fn new_terminal_adds_a_distinct_tab_without_splitting_and_selects_it() {
    let mut shell = active_shell();
    let before = shell.snapshot();
    let pane = before.focused_pane;
    let original = before.panes[0].selected_terminal_tab.unwrap();
    shell
        .apply(ShellAction::NewTerminalTab {
            pane,
            containment_generation: before.containment_generation,
        })
        .unwrap();

    let after = shell.snapshot();
    assert_eq!(after.panes.len(), 1);
    assert_eq!(after.tree, before.tree);
    assert_eq!(after.panes[0].terminal_tabs.len(), 2);
    let selected = after.panes[0].selected_terminal_tab.unwrap();
    assert_ne!(selected, original);
    assert_eq!(after.panes[0].terminal_tabs[1].id, selected);
}

#[test]
fn selection_is_pane_local_and_does_not_change_pane_focus() {
    let mut shell = active_shell();
    let first_pane = shell.snapshot().focused_pane;
    shell
        .apply(ShellAction::NewTerminalTab {
            pane: first_pane,
            containment_generation: shell.containment_generation(),
        })
        .unwrap();
    let first_tabs = shell.snapshot().panes[0].terminal_tabs.clone();
    let first_selection = shell.snapshot().panes[0].selected_terminal_tab;

    shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Right,
            containment_generation: shell.containment_generation(),
        })
        .unwrap();
    let second_pane = shell.snapshot().focused_pane;
    let second_original = shell.snapshot().panes[1].selected_terminal_tab.unwrap();
    shell
        .apply(ShellAction::NewTerminalTab {
            pane: second_pane,
            containment_generation: shell.containment_generation(),
        })
        .unwrap();
    let second_selected = shell.snapshot().panes[1].selected_terminal_tab.unwrap();
    shell
        .apply(ShellAction::SelectTerminalTab {
            pane: first_pane,
            terminal_tab: first_tabs[0].id,
        })
        .unwrap();

    let after = shell.snapshot();
    assert_eq!(after.focused_pane, second_pane);
    let first = after
        .panes
        .iter()
        .find(|pane| pane.id == first_pane)
        .unwrap();
    let second = after
        .panes
        .iter()
        .find(|pane| pane.id == second_pane)
        .unwrap();
    assert_eq!(first.selected_terminal_tab, Some(first_tabs[0].id));
    assert_eq!(second.selected_terminal_tab, Some(second_selected));
    assert_ne!(second_selected, second_original);
    assert_ne!(first.selected_terminal_tab, first_selection);
}

#[test]
fn each_terminal_tab_has_an_independent_execution_binding() {
    let mut shell = active_shell();
    let pane = shell.snapshot().focused_pane;
    let first_terminal = shell.snapshot().panes[0].selected_terminal_tab.unwrap();
    let first_execution = ExecutionId::new();
    shell
        .apply(ShellAction::BindTerminalExecution {
            pane,
            terminal_tab: first_terminal,
            execution: first_execution,
        })
        .unwrap();
    shell
        .apply(ShellAction::NewTerminalTab {
            pane,
            containment_generation: shell.containment_generation(),
        })
        .unwrap();
    let second_terminal = shell.snapshot().panes[0].selected_terminal_tab.unwrap();
    let second_execution = ExecutionId::new();
    shell
        .apply(ShellAction::BindTerminalExecution {
            pane,
            terminal_tab: second_terminal,
            execution: second_execution,
        })
        .unwrap();
    assert_ne!(first_terminal, second_terminal);
    assert_ne!(first_execution, second_execution);
    let snapshot = shell.snapshot();
    let pane = &snapshot.panes[0];
    assert_eq!(pane.terminal_tabs[0].execution, Some(first_execution));
    assert_eq!(pane.terminal_tabs[1].execution, Some(second_execution));
}

#[test]
fn closing_the_final_terminal_leaves_a_valid_empty_pane() {
    let mut shell = active_shell();
    let pane = shell.snapshot().focused_pane;
    let terminal_tab = shell.snapshot().panes[0].selected_terminal_tab.unwrap();
    shell
        .apply(ShellAction::CloseTerminalTab {
            pane,
            terminal_tab,
            containment_generation: shell.containment_generation(),
        })
        .unwrap();
    let after = shell.snapshot();
    assert_eq!(after.panes.len(), 1);
    assert_eq!(after.panes[0].selected_terminal_tab, None);
    assert!(after.panes[0].terminal_tabs.is_empty());
}

#[test]
fn bound_execution_cannot_be_shared_between_terminal_tabs_or_panes() {
    let mut shell = active_shell();
    let first_pane = shell.snapshot().focused_pane;
    let first_terminal = shell.snapshot().panes[0].selected_terminal_tab.unwrap();
    let execution = ExecutionId::new();
    shell
        .apply(ShellAction::BindTerminalExecution {
            pane: first_pane,
            terminal_tab: first_terminal,
            execution,
        })
        .unwrap();
    shell
        .apply(ShellAction::NewTerminalTab {
            pane: first_pane,
            containment_generation: shell.containment_generation(),
        })
        .unwrap();
    let second_terminal = shell.snapshot().panes[0].selected_terminal_tab.unwrap();
    assert_eq!(
        shell.apply(ShellAction::BindTerminalExecution {
            pane: first_pane,
            terminal_tab: second_terminal,
            execution,
        }),
        Err(ShellError::ExecutionAlreadyBound)
    );
}

#[test]
fn terminal_tab_identity_is_not_an_outer_tab_or_pane_identity() {
    let shell = active_shell();
    let snapshot = shell.snapshot();
    let terminal: TerminalTabId = snapshot.panes[0].terminal_tabs[0].id;
    assert_ne!(terminal.to_bytes(), snapshot.active_tab.to_bytes());
    assert_ne!(terminal.to_bytes(), snapshot.panes[0].id.to_bytes());
}
