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
mod tests;
