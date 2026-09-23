//! Portable shell chrome: agents, inspector, attention, left-panel mode, and
//! which shell regions are visible. Left panel, tab strip, and inspector are
//! visible by default per the frozen Core Terminal reference
//! (`docs/architecture/ui/M001-CORE-TERMINAL-REFERENCE-SCREEN.md`); the
//! earlier receded-by-default first UI is superseded.
//!
//! This module derives inspector/attention projections from authoritative
//! [`crate::shell::ShellSnapshot`], the composer's Runtime-projected Block
//! list, plus activity rows supplied by the host. It does not own
//! Workspace/Tab/Pane/Block identities, fabricate Runtime telemetry, or
//! implement an agent provider. Hosts dispatch [`ChromeAction`] and render
//! [`ChromeSnapshot`].

use std::collections::HashMap;
use std::fmt;

use seyal_core::{BlockId, TabId, WorkspaceId};

use crate::composer::{BlockPresentationState, BlockProjection};
use crate::shell::{LayoutDescription, ShellSnapshot};

/// Product chrome for the left context list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeftPanelMode {
    Workspaces,
    Tabs,
}

/// Product inspector filter. Filtering never invents rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectorMode {
    Context,
    Workspace,
    Tab,
    Pane,
    /// Selected Block details (#935). Rows exist only while a Block from the
    /// authoritative list is selected.
    Block,
}

/// Display state for one agent row. This is not Runtime telemetry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentActivity {
    Running,
    Waiting,
    Attention,
    Idle,
}

impl AgentActivity {
    fn label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Waiting => "Waiting",
            Self::Attention => "Attention",
            Self::Idle => "Idle",
        }
    }
}

/// Host-facing agent identity. Not a Runtime/Execution/Workspace authority.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AgentId(String);

impl AgentId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Host-facing attention identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AttentionId(String);

impl AttentionId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentRecord {
    pub id: AgentId,
    pub name: String,
    pub activity: AgentActivity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttentionItem {
    pub id: AttentionId,
    pub title: String,
    pub detail: String,
    pub workspace: Option<WorkspaceId>,
    pub tab: Option<TabId>,
    pub agent: Option<AgentId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InspectorRow {
    pub id: String,
    pub section: String,
    pub label: String,
    pub value: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChromeError {
    UnknownAgent,
    UnknownAttention,
    UnknownWorkspace,
    UnknownTab,
    UnknownBlock,
}

impl ChromeError {
    fn message(self) -> &'static str {
        match self {
            Self::UnknownAgent => "Unknown agent.",
            Self::UnknownAttention => "Unknown attention item.",
            Self::UnknownWorkspace => "Attention target Workspace does not exist.",
            Self::UnknownTab => "Attention target Tab does not exist.",
            Self::UnknownBlock => "Block is not in the focused Pane's Block list.",
        }
    }
}

impl fmt::Display for ChromeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChromeAction {
    SetLeftPanel(LeftPanelMode),
    SetInspectorMode(InspectorMode),
    SelectAgent {
        id: AgentId,
    },
    OpenAttention {
        id: AttentionId,
    },
    ReplaceAgents {
        workspace: WorkspaceId,
        agents: Vec<AgentRecord>,
    },
    ReplaceAttention {
        items: Vec<AttentionItem>,
    },
    /// Host applied a Workspace/Tab/Pane navigation action. Clears agent and
    /// Block selection without inventing new composition identities.
    ContextNavigated,
    /// Bind the inspector to one Block from `blocks`. Fails closed for an
    /// unknown identity. Switches to [`InspectorMode::Block`] and reveals the
    /// inspector so the selection is visible.
    SelectBlock {
        id: BlockId,
        blocks: Vec<BlockProjection>,
    },
    /// Return the inspector to focused-Pane context and the previous mode.
    ClearBlockSelection,
    /// Shell-region visibility. Visible is the Core Terminal default.
    SetShellVisibility {
        left: bool,
        inspector: bool,
        tab_strip: bool,
    },
}

/// Navigation the host must apply to [`crate::shell::ShellState`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ChromeEffect {
    pub select_workspace: Option<WorkspaceId>,
    pub select_tab: Option<TabId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChromeSnapshot {
    pub left_panel: LeftPanelMode,
    pub inspector_mode: InspectorMode,
    pub left_visible: bool,
    pub inspector_visible: bool,
    pub tab_strip_visible: bool,
    pub selected_agent: Option<AgentId>,
    pub selected_block: Option<BlockId>,
    pub agents: Vec<AgentRecord>,
    pub inspector_rows: Vec<InspectorRow>,
    pub visible_inspector_rows: Vec<InspectorRow>,
    pub attention_items: Vec<AttentionItem>,
    pub last_error: Option<ChromeError>,
}

/// Authoritative chrome/inspector/attention product state.
#[derive(Clone, Debug)]
pub struct ChromeState {
    left_panel: LeftPanelMode,
    inspector_mode: InspectorMode,
    left_visible: bool,
    inspector_visible: bool,
    tab_strip_visible: bool,
    selected_agent: Option<AgentId>,
    selected_block: Option<BlockId>,
    /// Mode to restore when the Block selection clears.
    mode_before_block: Option<InspectorMode>,
    agents: HashMap<WorkspaceId, Vec<AgentRecord>>,
    attention: Vec<AttentionItem>,
    last_error: Option<ChromeError>,
}

impl Default for ChromeState {
    fn default() -> Self {
        Self {
            left_panel: LeftPanelMode::Workspaces,
            inspector_mode: InspectorMode::Context,
            left_visible: true,
            inspector_visible: true,
            tab_strip_visible: true,
            selected_agent: None,
            selected_block: None,
            mode_before_block: None,
            agents: HashMap::new(),
            attention: Vec::new(),
            last_error: None,
        }
    }
}

impl ChromeState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply(
        &mut self,
        action: ChromeAction,
        shell: &ShellSnapshot,
    ) -> Result<ChromeEffect, ChromeError> {
        self.last_error = None;
        match action {
            ChromeAction::SetLeftPanel(mode) => {
                self.left_panel = mode;
                self.selected_agent = None;
                Ok(ChromeEffect::default())
            }
            ChromeAction::SetInspectorMode(mode) => {
                if mode == InspectorMode::Block && self.selected_block.is_none() {
                    // Block mode is entered only through SelectBlock. A mode
                    // switch with no selected Block would project an empty
                    // inspector with no recovery path.
                    return self.fail(ChromeError::UnknownBlock);
                }
                self.inspector_mode = mode;
                Ok(ChromeEffect::default())
            }
            ChromeAction::SelectAgent { id } => {
                if !self
                    .agents_for(shell.active_workspace)
                    .iter()
                    .any(|agent| agent.id == id)
                {
                    return self.fail(ChromeError::UnknownAgent);
                }
                self.selected_agent = Some(id);
                Ok(ChromeEffect::default())
            }
            ChromeAction::OpenAttention { id } => self.open_attention(&id, shell),
            ChromeAction::ReplaceAgents { workspace, agents } => {
                self.agents.insert(workspace, agents);
                if let Some(selected) = &self.selected_agent
                    && !self
                        .agents_for(shell.active_workspace)
                        .iter()
                        .any(|agent| agent.id == *selected)
                {
                    self.selected_agent = None;
                }
                Ok(ChromeEffect::default())
            }
            ChromeAction::ReplaceAttention { items } => {
                self.attention = items;
                Ok(ChromeEffect::default())
            }
            ChromeAction::ContextNavigated => {
                self.selected_agent = None;
                self.clear_block_selection();
                Ok(ChromeEffect::default())
            }
            ChromeAction::SelectBlock { id, blocks } => {
                if !blocks.iter().any(|block| block.id == id) {
                    return self.fail(ChromeError::UnknownBlock);
                }
                if self.selected_block.is_none() {
                    self.mode_before_block = Some(self.inspector_mode);
                }
                self.selected_block = Some(id);
                self.selected_agent = None;
                self.inspector_mode = InspectorMode::Block;
                self.inspector_visible = true;
                Ok(ChromeEffect::default())
            }
            ChromeAction::ClearBlockSelection => {
                self.clear_block_selection();
                Ok(ChromeEffect::default())
            }
            ChromeAction::SetShellVisibility {
                left,
                inspector,
                tab_strip,
            } => {
                self.left_visible = left;
                self.inspector_visible = inspector;
                self.tab_strip_visible = tab_strip;
                Ok(ChromeEffect::default())
            }
        }
    }

    /// `blocks` is the focused Pane's authoritative Block list from the
    /// composer projection. A selected Block absent from it is treated as
    /// unselected: rows fall back to Pane context and no Block rows are shown.
    pub fn snapshot(&self, shell: &ShellSnapshot, blocks: &[BlockProjection]) -> ChromeSnapshot {
        let selected_block = self.selected_block_in(blocks);
        let inspector_mode =
            if self.inspector_mode == InspectorMode::Block && selected_block.is_none() {
                InspectorMode::Context
            } else {
                self.inspector_mode
            };
        let inspector_rows = self.inspector_rows(shell, selected_block);
        let visible_inspector_rows = filter_rows(&inspector_rows, inspector_mode);
        ChromeSnapshot {
            left_panel: self.left_panel,
            inspector_mode,
            left_visible: self.left_visible,
            inspector_visible: self.inspector_visible,
            tab_strip_visible: self.tab_strip_visible,
            selected_agent: self.selected_agent.clone(),
            selected_block: selected_block.map(|block| block.id),
            agents: self.agents_for(shell.active_workspace).to_vec(),
            inspector_rows,
            visible_inspector_rows,
            attention_items: self.attention.clone(),
            last_error: self.last_error,
        }
    }

    fn open_attention(
        &mut self,
        id: &AttentionId,
        shell: &ShellSnapshot,
    ) -> Result<ChromeEffect, ChromeError> {
        let index = self
            .attention
            .iter()
            .position(|item| item.id == *id)
            .ok_or_else(|| {
                self.last_error = Some(ChromeError::UnknownAttention);
                ChromeError::UnknownAttention
            })?;
        let item = self.attention[index].clone();
        if let Some(workspace) = item.workspace
            && !shell.workspaces.iter().any(|row| row.id == workspace)
        {
            return self.fail(ChromeError::UnknownWorkspace);
        }
        if let Some(tab) = item.tab {
            let tab_known = match item.workspace {
                Some(workspace) if workspace != shell.active_workspace => true,
                _ => shell.tabs.iter().any(|row| row.id == tab),
            };
            if !tab_known && item.workspace.is_none() {
                return self.fail(ChromeError::UnknownTab);
            }
        }
        if let Some(agent) = &item.agent {
            let workspace = item.workspace.unwrap_or(shell.active_workspace);
            if !self
                .agents_for(workspace)
                .iter()
                .any(|row| row.id == *agent)
            {
                return self.fail(ChromeError::UnknownAgent);
            }
            self.selected_agent = Some(agent.clone());
        } else {
            self.selected_agent = None;
        }
        self.attention.remove(index);
        Ok(ChromeEffect {
            select_workspace: item.workspace,
            select_tab: item.tab,
        })
    }

    /// Whether the selected Block is still in the authoritative list.
    pub fn selected_block_is_stale(&self, blocks: &[BlockProjection]) -> bool {
        self.selected_block.is_some() && self.selected_block_in(blocks).is_none()
    }

    fn selected_block_in<'a>(&self, blocks: &'a [BlockProjection]) -> Option<&'a BlockProjection> {
        let id = self.selected_block?;
        blocks.iter().find(|block| block.id == id)
    }

    fn clear_block_selection(&mut self) {
        if self.selected_block.take().is_some() {
            self.inspector_mode = self
                .mode_before_block
                .take()
                .unwrap_or(InspectorMode::Context);
        }
    }

    fn inspector_rows(
        &self,
        shell: &ShellSnapshot,
        selected_block: Option<&BlockProjection>,
    ) -> Vec<InspectorRow> {
        let workspace = shell
            .workspaces
            .iter()
            .find(|row| row.id == shell.active_workspace);
        let tab = shell.tabs.iter().find(|row| row.id == shell.active_tab);
        let pane = shell.panes.iter().find(|row| row.id == shell.focused_pane);
        if let Some(block) = selected_block {
            return block_rows(
                block,
                workspace.map(|item| item.name.as_str()),
                pane.map(|item| item.title.as_str()),
            );
        }
        if let Some(selected) = &self.selected_agent
            && let Some(agent) = self
                .agents_for(shell.active_workspace)
                .iter()
                .find(|row| row.id == *selected)
        {
            return vec![
                row("agent-name", "Agent", "Name", agent.name.clone()),
                row(
                    "agent-state",
                    "Agent",
                    "State",
                    agent.activity.label().to_owned(),
                ),
                row(
                    "agent-workspace",
                    "Workspace",
                    "Name",
                    workspace
                        .map(|item| item.name.clone())
                        .unwrap_or_else(|| "—".to_owned()),
                ),
            ];
        }
        vec![
            row(
                "workspace-name",
                "Workspace",
                "Name",
                workspace
                    .map(|item| item.name.clone())
                    .unwrap_or_else(|| "—".to_owned()),
            ),
            row(
                "workspace-path",
                "Workspace",
                "Path",
                workspace
                    .and_then(|item| item.detail.clone())
                    .unwrap_or_else(|| "—".to_owned()),
            ),
            row(
                "tab-name",
                "Tab",
                "Name",
                tab.map(|item| item.title.clone())
                    .unwrap_or_else(|| "—".to_owned()),
            ),
            row(
                "tab-panes",
                "Tab",
                "Panes",
                tab.map(|item| item.pane_count.to_string())
                    .unwrap_or_else(|| "—".to_owned()),
            ),
            row(
                "tab-layout",
                "Tab",
                "Layout",
                layout_label(shell.layout).to_owned(),
            ),
            row(
                "pane-name",
                "Active Pane",
                "Pane",
                pane.map(|item| item.title.clone())
                    .unwrap_or_else(|| "—".to_owned()),
            ),
            row("pane-focus", "Active Pane", "Focus", "Focused".to_owned()),
        ]
    }

    fn agents_for(&self, workspace: WorkspaceId) -> &[AgentRecord] {
        self.agents.get(&workspace).map_or(&[], Vec::as_slice)
    }

    fn fail<T>(&mut self, error: ChromeError) -> Result<T, ChromeError> {
        self.last_error = Some(error);
        Err(error)
    }
}

fn row(id: &str, section: &str, label: &str, value: String) -> InspectorRow {
    InspectorRow {
        id: id.to_owned(),
        section: section.to_owned(),
        label: label.to_owned(),
        value,
    }
}

/// Block detail rows from Runtime-projected metadata only (spec §3/§11).
/// Fields Runtime does not publish (duration, timestamps, cwd, shell) are
/// omitted rather than fabricated.
fn block_rows(
    block: &BlockProjection,
    workspace: Option<&str>,
    pane: Option<&str>,
) -> Vec<InspectorRow> {
    let state = match block.state {
        BlockPresentationState::Running => "Running",
        BlockPresentationState::Completed => "Completed",
        BlockPresentationState::Failed => "Failed",
        BlockPresentationState::Unknown => "Completed (status unknown)",
    };
    let mut rows = vec![
        row("block-command", "Block", "Command", block.command.clone()),
        row("block-state", "Block", "State", state.to_owned()),
    ];
    if let Some(exit_status) = block.exit_status {
        rows.push(row(
            "block-exit",
            "Block",
            "Exit code",
            exit_status.to_string(),
        ));
    } else if block.state == BlockPresentationState::Unknown {
        rows.push(row(
            "block-exit",
            "Block",
            "Exit code",
            "unknown".to_owned(),
        ));
    }
    if let Some(end_line) = block.end_line
        && end_line >= block.start_line
    {
        rows.push(row(
            "block-lines",
            "Block",
            "Output lines",
            (end_line - block.start_line + 1).to_string(),
        ));
    }
    rows.push(row(
        "block-pane",
        "Block",
        "Pane",
        pane.map(str::to_owned).unwrap_or_else(|| "—".to_owned()),
    ));
    rows.push(row(
        "block-workspace",
        "Block",
        "Workspace",
        workspace
            .map(str::to_owned)
            .unwrap_or_else(|| "—".to_owned()),
    ));
    rows
}

fn filter_rows(rows: &[InspectorRow], mode: InspectorMode) -> Vec<InspectorRow> {
    match mode {
        InspectorMode::Context => rows.to_vec(),
        InspectorMode::Block => rows
            .iter()
            .filter(|row| row.section == "Block")
            .cloned()
            .collect(),
        InspectorMode::Workspace => rows
            .iter()
            .filter(|row| row.section == "Workspace")
            .cloned()
            .collect(),
        InspectorMode::Tab => rows
            .iter()
            .filter(|row| row.section == "Tab")
            .cloned()
            .collect(),
        InspectorMode::Pane => rows
            .iter()
            .filter(|row| row.section == "Active Pane")
            .cloned()
            .collect(),
    }
}

fn layout_label(layout: LayoutDescription) -> &'static str {
    match layout {
        LayoutDescription::Single => "Single pane",
        LayoutDescription::SplitRight => "Split right",
        LayoutDescription::SplitDown => "Split down",
    }
}

#[cfg(test)]
mod tests {
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
}
