use super::*;
use crate::shell::{ShellAction, ShellPaneSeed, ShellState, ShellTabSeed, ShellWorkspaceSeed};
use seyal_core::{BlockId, PaneId, TabId, WorkspaceId};

fn workspace(tag: u8) -> WorkspaceId {
    WorkspaceId::from_bytes([tag; 16])
}

fn tab(tag: u8) -> TabId {
    TabId::from_bytes([tag; 16])
}

fn pane(tag: u8) -> PaneId {
    PaneId::from_bytes([tag; 16])
}

fn seed_shell() -> ShellState {
    let local = workspace(1);
    let other = workspace(2);
    let local_tab = tab(1);
    let agent_tab = tab(2);
    let other_tab = tab(3);
    ShellState::from_workspaces(
        vec![
            ShellWorkspaceSeed {
                id: local,
                name: "Seyal OSS".into(),
                detail: Some("~/Projects/seyal".into()),
                attention: false,
                active_tab: local_tab,
                tabs: vec![
                    ShellTabSeed {
                        id: local_tab,
                        title: "Core Terminal".into(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: pane(1),
                            title: "Pane 1".into(),
                            allows_implicit_execution_bootstrap: true,
                        },
                    },
                    ShellTabSeed {
                        id: agent_tab,
                        title: "Agent Development".into(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: pane(2),
                            title: "Pane 2".into(),
                            allows_implicit_execution_bootstrap: false,
                        },
                    },
                ],
            },
            ShellWorkspaceSeed {
                id: other,
                name: "Payments".into(),
                detail: Some("~/Projects/payments".into()),
                attention: true,
                active_tab: other_tab,
                tabs: vec![ShellTabSeed {
                    id: other_tab,
                    title: "API".into(),
                    attention: false,
                    pane: ShellPaneSeed {
                        id: pane(3),
                        title: "Pane 1".into(),
                        allows_implicit_execution_bootstrap: false,
                    },
                }],
            },
        ],
        local,
        true,
        true,
    )
    .expect("seed")
}

fn claude() -> AgentRecord {
    AgentRecord {
        id: AgentId::new("agent-claude"),
        name: "Claude Code".into(),
        activity: AgentActivity::Running,
    }
}

fn seed_agents(chrome: &mut ChromeState, shell: &ShellSnapshot) {
    chrome
        .apply(
            ChromeAction::ReplaceAgents {
                workspace: shell.active_workspace,
                agents: vec![
                    claude(),
                    AgentRecord {
                        id: AgentId::new("agent-codex"),
                        name: "Codex".into(),
                        activity: AgentActivity::Attention,
                    },
                ],
            },
            shell,
        )
        .unwrap();
}

#[test]
fn inspector_does_not_fabricate_runtime_telemetry() {
    let shell = seed_shell();
    let snap = shell.snapshot();
    let mut chrome = ChromeState::new();
    seed_agents(&mut chrome, &snap);
    let rows = chrome.snapshot(&snap, &[]).inspector_rows;
    let labels: Vec<_> = rows.iter().map(|row| row.label.as_str()).collect();
    assert_eq!(
        labels,
        ["Name", "Path", "Name", "Panes", "Layout", "Pane", "Focus"]
    );
    assert!(rows.iter().all(|row| {
        !row.label.contains("CPU")
            && !row.label.contains("RSS")
            && !row.label.contains("PTY")
            && !row.value.contains("pid")
    }));
    assert_eq!(rows[0].value, "Seyal OSS");
    assert_eq!(rows[1].value, "~/Projects/seyal");
}

#[test]
fn inspector_mode_filters_existing_context_only() {
    let shell = seed_shell();
    let snap = shell.snapshot();
    let mut chrome = ChromeState::new();
    chrome
        .apply(ChromeAction::SetInspectorMode(InspectorMode::Tab), &snap)
        .unwrap();
    let visible = chrome.snapshot(&snap, &[]).visible_inspector_rows;
    assert!(visible.iter().all(|row| row.section == "Tab"));
    assert_eq!(visible.len(), 3);
    assert!(!visible.iter().any(|row| row.section == "Workspace"));
}

#[test]
fn selected_agent_uses_activity_rows_not_runtime_metrics() {
    let shell = seed_shell();
    let snap = shell.snapshot();
    let mut chrome = ChromeState::new();
    seed_agents(&mut chrome, &snap);
    chrome
        .apply(
            ChromeAction::SelectAgent {
                id: AgentId::new("agent-claude"),
            },
            &snap,
        )
        .unwrap();
    let rows = chrome.snapshot(&snap, &[]).inspector_rows;
    assert_eq!(rows[0].section, "Agent");
    assert_eq!(rows[0].value, "Claude Code");
    assert_eq!(rows[1].value, "Running");
    assert!(rows.iter().all(|row| row.label != "CPU"));
}

#[test]
fn left_panel_mode_does_not_invent_workspace_or_tab_identities() {
    let mut shell = seed_shell();
    let before = shell.snapshot();
    let mut chrome = ChromeState::new();
    chrome
        .apply(ChromeAction::SetLeftPanel(LeftPanelMode::Tabs), &before)
        .unwrap();
    let after_shell = shell.snapshot();
    assert_eq!(after_shell.active_workspace, before.active_workspace);
    assert_eq!(after_shell.active_tab, before.active_tab);
    assert_eq!(after_shell.workspaces, before.workspaces);
    assert_eq!(
        chrome.snapshot(&after_shell, &[]).left_panel,
        LeftPanelMode::Tabs
    );
    assert!(chrome.snapshot(&after_shell, &[]).selected_agent.is_none());
    shell.apply(ShellAction::SelectTab { id: tab(2) }).unwrap();
    chrome
        .apply(ChromeAction::ContextNavigated, &shell.snapshot())
        .unwrap();
    assert_eq!(shell.snapshot().active_tab, tab(2));
    assert!(chrome
        .snapshot(&shell.snapshot(), &[])
        .selected_agent
        .is_none());
}

#[test]
fn core_terminal_shell_chrome_is_visible_by_default_and_can_be_hidden() {
    let shell = seed_shell();
    let snap = shell.snapshot();
    let mut chrome = ChromeState::new();
    let initial = chrome.snapshot(&snap, &[]);
    assert!(initial.left_visible);
    assert!(initial.inspector_visible);
    assert!(initial.tab_strip_visible);
    chrome
        .apply(
            ChromeAction::SetShellVisibility {
                left: false,
                inspector: false,
                tab_strip: false,
            },
            &snap,
        )
        .unwrap();
    let hidden = chrome.snapshot(&snap, &[]);
    assert!(!hidden.left_visible);
    assert!(!hidden.inspector_visible);
    assert!(!hidden.tab_strip_visible);
}

#[test]
fn attention_item_navigates_then_dismisses() {
    let mut shell = seed_shell();
    let snap = shell.snapshot();
    let mut chrome = ChromeState::new();
    seed_agents(&mut chrome, &snap);
    chrome
        .apply(
            ChromeAction::ReplaceAttention {
                items: vec![AttentionItem {
                    id: AttentionId::new("attention-preview-tab"),
                    title: "Preview attention item".into(),
                    detail: "Open Agent Development".into(),
                    workspace: Some(workspace(1)),
                    tab: Some(tab(2)),
                    agent: Some(AgentId::new("agent-codex")),
                }],
            },
            &snap,
        )
        .unwrap();
    let effect = chrome
        .apply(
            ChromeAction::OpenAttention {
                id: AttentionId::new("attention-preview-tab"),
            },
            &snap,
        )
        .unwrap();
    assert_eq!(effect.select_workspace, Some(workspace(1)));
    assert_eq!(effect.select_tab, Some(tab(2)));
    if let Some(id) = effect.select_workspace {
        shell.apply(ShellAction::SelectWorkspace { id }).unwrap();
    }
    if let Some(id) = effect.select_tab {
        shell.apply(ShellAction::SelectTab { id }).unwrap();
    }
    let after = chrome.snapshot(&shell.snapshot(), &[]);
    assert!(after.attention_items.is_empty());
    assert_eq!(after.selected_agent, Some(AgentId::new("agent-codex")));
    assert_eq!(shell.snapshot().active_tab, tab(2));
}

#[test]
fn unknown_attention_or_agent_fails_closed() {
    let shell = seed_shell();
    let snap = shell.snapshot();
    let mut chrome = ChromeState::new();
    seed_agents(&mut chrome, &snap);
    assert_eq!(
        chrome.apply(
            ChromeAction::OpenAttention {
                id: AttentionId::new("missing")
            },
            &snap
        ),
        Err(ChromeError::UnknownAttention)
    );
    assert_eq!(
        chrome.apply(
            ChromeAction::SelectAgent {
                id: AgentId::new("ghost")
            },
            &snap
        ),
        Err(ChromeError::UnknownAgent)
    );
    assert!(chrome.snapshot(&snap, &[]).attention_items.is_empty());
    assert!(chrome.snapshot(&snap, &[]).selected_agent.is_none());
}

#[test]
fn attention_unknown_workspace_does_not_dismiss() {
    let shell = seed_shell();
    let snap = shell.snapshot();
    let mut chrome = ChromeState::new();
    chrome
        .apply(
            ChromeAction::ReplaceAttention {
                items: vec![AttentionItem {
                    id: AttentionId::new("bad-workspace"),
                    title: "Ghost".into(),
                    detail: "Missing".into(),
                    workspace: Some(workspace(9)),
                    tab: None,
                    agent: None,
                }],
            },
            &snap,
        )
        .unwrap();
    assert_eq!(
        chrome.apply(
            ChromeAction::OpenAttention {
                id: AttentionId::new("bad-workspace")
            },
            &snap
        ),
        Err(ChromeError::UnknownWorkspace)
    );
    assert_eq!(chrome.snapshot(&snap, &[]).attention_items.len(), 1);
}

fn block(tag: u8, command: &str, state: BlockPresentationState) -> BlockProjection {
    BlockProjection {
        pane: pane(1),
        id: BlockId::from_bytes([tag; 16]),
        command: command.to_owned(),
        state,
        start_line: 10,
        end_line: None,
        exit_status: None,
    }
}

fn values(rows: &[InspectorRow]) -> Vec<(&str, &str)> {
    rows.iter()
        .map(|row| (row.id.as_str(), row.value.as_str()))
        .collect()
}

#[test]
fn block_rows_come_only_from_the_supplied_block_list() {
    let shell = seed_shell();
    let snap = shell.snapshot();
    let mut chrome = ChromeState::new();
    let running = block(7, "cargo test", BlockPresentationState::Running);
    let none = chrome.snapshot(&snap, &[]);
    assert!(none.selected_block.is_none());
    assert!(none.inspector_rows.iter().all(|row| row.section != "Block"));
    assert_eq!(
        chrome.apply(
            ChromeAction::SelectBlock {
                id: running.id,
                blocks: vec![]
            },
            &snap
        ),
        Err(ChromeError::UnknownBlock),
        "empty list fails closed"
    );
    assert_eq!(
        chrome.snapshot(&snap, &[]).inspector_mode,
        InspectorMode::Context
    );
    // Hide the (default-visible) inspector so the reveal below is a real
    // hidden -> visible transition.
    chrome
        .apply(
            ChromeAction::SetShellVisibility {
                left: true,
                inspector: false,
                tab_strip: true,
            },
            &snap,
        )
        .unwrap();
    assert!(!chrome.snapshot(&snap, &[]).inspector_visible);
    chrome
        .apply(
            ChromeAction::SelectBlock {
                id: running.id,
                blocks: vec![running.clone()],
            },
            &snap,
        )
        .unwrap();
    let selected = chrome.snapshot(&snap, std::slice::from_ref(&running));
    assert_eq!(selected.selected_block, Some(running.id));
    assert_eq!(selected.inspector_mode, InspectorMode::Block);
    assert!(
        selected.inspector_visible,
        "selection reveals the inspector"
    );
    assert_eq!(
        values(&selected.visible_inspector_rows),
        vec![
            ("block-command", "cargo test"),
            ("block-state", "Running"),
            ("block-pane", "Pane 1"),
            ("block-workspace", "Seyal OSS"),
        ],
        "exit code and output lines are omitted while unknown"
    );
}

#[test]
fn completed_block_rows_expose_exit_code_and_lines_from_runtime_only() {
    let shell = seed_shell();
    let snap = shell.snapshot();
    let mut chrome = ChromeState::new();
    let mut failed = block(8, "false", BlockPresentationState::Failed);
    failed.end_line = Some(14);
    failed.exit_status = Some(1);
    chrome
        .apply(
            ChromeAction::SelectBlock {
                id: failed.id,
                blocks: vec![failed.clone()],
            },
            &snap,
        )
        .unwrap();
    let rows = chrome
        .snapshot(&snap, std::slice::from_ref(&failed))
        .visible_inspector_rows;
    assert_eq!(
        values(&rows),
        vec![
            ("block-command", "false"),
            ("block-state", "Failed"),
            ("block-exit", "1"),
            ("block-lines", "5"),
            ("block-pane", "Pane 1"),
            ("block-workspace", "Seyal OSS"),
        ]
    );
    assert!(
        rows.iter().all(|row| !matches!(
            row.label.as_str(),
            "Duration" | "Started" | "Finished" | "cwd" | "Shell"
        )),
        "no fabricated telemetry"
    );
}

#[test]
fn completed_unknown_block_rows_render_unknown_never_zero_or_failed() {
    let shell = seed_shell();
    let snap = shell.snapshot();
    let mut chrome = ChromeState::new();
    let mut unknown = block(9, "sleep 1", BlockPresentationState::Unknown);
    unknown.end_line = Some(12);
    unknown.exit_status = None;
    chrome
        .apply(
            ChromeAction::SelectBlock {
                id: unknown.id,
                blocks: vec![unknown.clone()],
            },
            &snap,
        )
        .unwrap();
    let rows = chrome
        .snapshot(&snap, std::slice::from_ref(&unknown))
        .visible_inspector_rows;
    assert_eq!(
        values(&rows),
        vec![
            ("block-command", "sleep 1"),
            ("block-state", "Completed (status unknown)"),
            ("block-exit", "unknown"),
            ("block-lines", "3"),
            ("block-pane", "Pane 1"),
            ("block-workspace", "Seyal OSS"),
        ]
    );
    assert!(
        rows.iter()
            .all(|row| row.value != "0" && row.value != "Failed"),
        "Unknown must never present as exit 0 or Failed"
    );
}

#[test]
fn block_mode_without_a_selected_block_fails_closed_and_projects_context() {
    let shell = seed_shell();
    let snap = shell.snapshot();
    let mut chrome = ChromeState::new();
    assert_eq!(
        chrome.apply(ChromeAction::SetInspectorMode(InspectorMode::Block), &snap),
        Err(ChromeError::UnknownBlock)
    );
    let projected = chrome.snapshot(&snap, &[]);
    assert_eq!(projected.inspector_mode, InspectorMode::Context);
    assert!(
        !projected.visible_inspector_rows.is_empty(),
        "context rows remain; Block mode does not empty the inspector"
    );
    assert!(projected
        .visible_inspector_rows
        .iter()
        .all(|row| row.section != "Block"));
}

#[test]
fn stale_block_selection_falls_back_to_context_and_clear_restores_mode() {
    let shell = seed_shell();
    let snap = shell.snapshot();
    let mut chrome = ChromeState::new();
    chrome
        .apply(ChromeAction::SetInspectorMode(InspectorMode::Pane), &snap)
        .unwrap();
    let first = block(1, "ls", BlockPresentationState::Completed);
    let second = block(2, "pwd", BlockPresentationState::Completed);
    chrome
        .apply(
            ChromeAction::SelectBlock {
                id: first.id,
                blocks: vec![first.clone(), second.clone()],
            },
            &snap,
        )
        .unwrap();
    // Runtime republished without the selected Block.
    let only_second = vec![second.clone()];
    assert!(chrome.selected_block_is_stale(&only_second));
    let stale = chrome.snapshot(&snap, &only_second);
    assert!(stale.selected_block.is_none());
    assert!(stale
        .inspector_rows
        .iter()
        .all(|row| row.section != "Block"));
    assert_eq!(
        stale.inspector_mode,
        InspectorMode::Context,
        "stale Block selection projects Context, not an empty Block inspector"
    );
    assert!(
        !stale.visible_inspector_rows.is_empty(),
        "context rows remain while the selected Block is gone"
    );
    chrome
        .apply(ChromeAction::ClearBlockSelection, &snap)
        .unwrap();
    let cleared = chrome.snapshot(&snap, &only_second);
    assert_eq!(
        cleared.inspector_mode,
        InspectorMode::Pane,
        "previous mode restored"
    );
    assert!(!chrome.selected_block_is_stale(&only_second));
    assert!(cleared
        .visible_inspector_rows
        .iter()
        .all(|row| row.section == "Active Pane"));
}

#[test]
fn block_selection_clears_on_navigation_and_replaces_agent_selection() {
    let shell = seed_shell();
    let snap = shell.snapshot();
    let mut chrome = ChromeState::new();
    seed_agents(&mut chrome, &snap);
    chrome
        .apply(ChromeAction::SelectAgent { id: claude().id }, &snap)
        .unwrap();
    let b = block(3, "make", BlockPresentationState::Running);
    chrome
        .apply(
            ChromeAction::SelectBlock {
                id: b.id,
                blocks: vec![b.clone()],
            },
            &snap,
        )
        .unwrap();
    let selected = chrome.snapshot(&snap, std::slice::from_ref(&b));
    assert!(
        selected.selected_agent.is_none(),
        "one inspector subject at a time"
    );
    assert_eq!(selected.selected_block, Some(b.id));
    chrome.apply(ChromeAction::ContextNavigated, &snap).unwrap();
    let navigated = chrome.snapshot(&snap, std::slice::from_ref(&b));
    assert!(navigated.selected_block.is_none());
    assert_eq!(navigated.inspector_mode, InspectorMode::Context);
}
