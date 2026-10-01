//! Global keyboard-first command palette (#932).
//!
//! Rust owns open/query/selected-index state. The command list itself is
//! never stored: [`build_commands`] derives it fresh from the current
//! [`ShellSnapshot`]/[`ChromeSnapshot`] every time the palette is opened,
//! filtered, navigated, or run, so it can never present a stale or invented
//! action. Filtering is a plain case-insensitive prefix/substring match, not
//! a general fuzzy-ranking engine (explicitly out of scope). This module
//! owns no Workspace/Tab/Pane/Agent/Attention identity of its own and
//! invents no product action; it only enumerates ones that already exist on
//! [`crate::shell::ShellState`] and [`crate::chrome::ChromeState`]. Hosts
//! dispatch [`PaletteAction`] and render [`PaletteSnapshot`].

use seyal_core::{PaneId, TabId, WorkspaceId};

use crate::chrome::{AgentId, AttentionId, ChromeSnapshot, InspectorMode, LeftPanelMode};
use crate::shell::{ShellSnapshot, SplitAxis};

/// Maximum rows projected to the host for one filter result.
pub const PALETTE_VISIBLE_ROWS: usize = 12;

/// A product action the palette can run. Never stored across snapshots;
/// always resolved from the currently selected, currently filtered row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteCommand {
    CreateTab,
    SplitFocused(SplitAxis),
    SwitchWorkspace(WorkspaceId),
    SwitchTab(TabId),
    FocusPane(PaneId),
    SetLeftPanel(LeftPanelMode),
    SetShellVisibility {
        left: bool,
        inspector: bool,
        tab_strip: bool,
    },
    SetInspectorMode(InspectorMode),
    OpenAttention(AttentionId),
    FocusAgent(AgentId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PaletteEntry {
    label: String,
    category: &'static str,
    command: PaletteCommand,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteError {
    /// A query/move/run/close action arrived while the palette is closed.
    NotOpen,
    /// Run was dispatched with no row at the current selection (empty
    /// filtered list).
    NoSelection,
}

impl PaletteError {
    fn message(self) -> &'static str {
        match self {
            Self::NotOpen => "Command palette is not open.",
            Self::NoSelection => "No command palette row is selected.",
        }
    }
}

impl std::fmt::Display for PaletteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

/// Typed host -> Rust command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteAction {
    Open,
    SetQuery(String),
    MoveSelection(i32),
    Close,
}

/// One read-only projected row. `category` groups rows in the host overlay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteRow {
    pub label: String,
    pub category: &'static str,
}

/// Read-only projection for native hosts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteSnapshot {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    pub rows: Vec<PaletteRow>,
    pub last_error: Option<PaletteError>,
}

/// Authoritative global command-palette state.
#[derive(Clone, Debug, Default)]
pub struct PaletteState {
    open: bool,
    query: String,
    selected: usize,
    last_error: Option<PaletteError>,
}

impl PaletteState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// `row_count` is the filtered row count at the moment of the call; only
    /// [`PaletteAction::MoveSelection`] uses it. The palette does not own
    /// shell/chrome data, so it cannot compute this itself — the caller
    /// (`ApplicationRoot`) derives it from a fresh [`build_commands`] +
    /// [`filter`] pass, the same one it uses to answer the next snapshot.
    pub fn apply(&mut self, action: PaletteAction, row_count: usize) -> Result<(), PaletteError> {
        self.last_error = None;
        match action {
            PaletteAction::Open => {
                if !self.open {
                    self.open = true;
                    self.query.clear();
                    self.selected = 0;
                }
                Ok(())
            }
            PaletteAction::SetQuery(query) => {
                if !self.open {
                    return self.fail(PaletteError::NotOpen);
                }
                if self.query != query {
                    self.query = query;
                    self.selected = 0;
                }
                Ok(())
            }
            PaletteAction::MoveSelection(delta) => {
                if !self.open {
                    return self.fail(PaletteError::NotOpen);
                }
                if row_count == 0 {
                    self.selected = 0;
                    return Ok(());
                }
                let last = (row_count - 1) as i64;
                let next = (self.selected as i64 + delta as i64).clamp(0, last);
                self.selected = next as usize;
                Ok(())
            }
            PaletteAction::Close => {
                self.close();
                Ok(())
            }
        }
    }

    /// Close without going through `apply`; used after a successful Run.
    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.selected = 0;
    }

    /// `allows_tab_creation`/`allows_pane_splitting` are [`crate::shell::ShellState`]
    /// policy, not part of [`ShellSnapshot`]; the caller passes them through
    /// so a palette command that would deterministically fail is never shown
    /// (functional-only rule: omit rather than offer a fake action).
    pub fn snapshot(
        &self,
        shell: &ShellSnapshot,
        chrome: &ChromeSnapshot,
        allows_tab_creation: bool,
        allows_pane_splitting: bool,
    ) -> PaletteSnapshot {
        if !self.open {
            return PaletteSnapshot {
                open: false,
                query: String::new(),
                selected: 0,
                rows: Vec::new(),
                last_error: self.last_error,
            };
        }
        let commands = build_commands(shell, chrome, allows_tab_creation, allows_pane_splitting);
        let filtered = filter(&commands, &self.query);
        let selected = clamp(self.selected, filtered.len());
        let rows = filtered
            .iter()
            .map(|entry| PaletteRow {
                label: entry.label.clone(),
                category: entry.category,
            })
            .collect();
        PaletteSnapshot {
            open: true,
            query: self.query.clone(),
            selected,
            rows,
            last_error: self.last_error,
        }
    }

    /// The command bound to the current selection, or `None` while closed or
    /// with no matching row. Always resolved against a fresh command list.
    pub fn resolve(
        &self,
        shell: &ShellSnapshot,
        chrome: &ChromeSnapshot,
        allows_tab_creation: bool,
        allows_pane_splitting: bool,
    ) -> Option<PaletteCommand> {
        if !self.open {
            return None;
        }
        let commands = build_commands(shell, chrome, allows_tab_creation, allows_pane_splitting);
        let filtered = filter(&commands, &self.query);
        filtered
            .get(clamp(self.selected, filtered.len()))
            .map(|entry| entry.command.clone())
    }

    fn fail(&mut self, error: PaletteError) -> Result<(), PaletteError> {
        self.last_error = Some(error);
        Err(error)
    }
}

fn clamp(selected: usize, row_count: usize) -> usize {
    if row_count == 0 {
        0
    } else {
        selected.min(row_count - 1)
    }
}

fn left_panel_label(mode: LeftPanelMode) -> &'static str {
    match mode {
        LeftPanelMode::Workspaces => "Workspaces",
        LeftPanelMode::Tabs => "Tabs",
    }
}

fn inspector_mode_label(mode: InspectorMode) -> &'static str {
    match mode {
        InspectorMode::Context => "Context",
        InspectorMode::Workspace => "Workspace",
        InspectorMode::Tab => "Tab",
        InspectorMode::Pane => "Pane",
        InspectorMode::Block => "Block",
    }
}

/// Enumerate currently available palette commands from authoritative state.
/// A command whose effect would be a no-op against the current state (the
/// active Workspace/Tab, the focused Pane, the current inspector mode) is
/// omitted rather than shown disabled.
fn build_commands(
    shell: &ShellSnapshot,
    chrome: &ChromeSnapshot,
    allows_tab_creation: bool,
    allows_pane_splitting: bool,
) -> Vec<PaletteEntry> {
    let mut entries = Vec::new();

    if allows_tab_creation {
        entries.push(PaletteEntry {
            label: "New Tab".to_owned(),
            category: "Navigation",
            command: PaletteCommand::CreateTab,
        });
    }
    if allows_pane_splitting {
        entries.push(PaletteEntry {
            label: "Split Pane Right".to_owned(),
            category: "Navigation",
            command: PaletteCommand::SplitFocused(SplitAxis::Right),
        });
        entries.push(PaletteEntry {
            label: "Split Pane Down".to_owned(),
            category: "Navigation",
            command: PaletteCommand::SplitFocused(SplitAxis::Down),
        });
    }

    for workspace in &shell.workspaces {
        if workspace.id == shell.active_workspace {
            continue;
        }
        entries.push(PaletteEntry {
            label: format!("Switch to Workspace: {}", workspace.name),
            category: "Workspace",
            command: PaletteCommand::SwitchWorkspace(workspace.id),
        });
    }

    for tab in &shell.tabs {
        if tab.id == shell.active_tab {
            continue;
        }
        entries.push(PaletteEntry {
            label: format!("Switch to Tab: {}", tab.title),
            category: "Tab",
            command: PaletteCommand::SwitchTab(tab.id),
        });
    }

    for pane in &shell.panes {
        if pane.id == shell.focused_pane {
            continue;
        }
        entries.push(PaletteEntry {
            label: format!("Focus Pane: {}", pane.title),
            category: "Pane",
            command: PaletteCommand::FocusPane(pane.id),
        });
    }

    let other_left_panel = match chrome.left_panel {
        LeftPanelMode::Workspaces => LeftPanelMode::Tabs,
        LeftPanelMode::Tabs => LeftPanelMode::Workspaces,
    };
    entries.push(PaletteEntry {
        label: format!("Show {} in Left Panel", left_panel_label(other_left_panel)),
        category: "View",
        command: PaletteCommand::SetLeftPanel(other_left_panel),
    });

    entries.push(PaletteEntry {
        label: if chrome.left_visible {
            "Hide Left Panel".to_owned()
        } else {
            "Show Left Panel".to_owned()
        },
        category: "View",
        command: PaletteCommand::SetShellVisibility {
            left: !chrome.left_visible,
            inspector: chrome.inspector_visible,
            tab_strip: chrome.tab_strip_visible,
        },
    });
    entries.push(PaletteEntry {
        label: if chrome.inspector_visible {
            "Hide Inspector".to_owned()
        } else {
            "Show Inspector".to_owned()
        },
        category: "View",
        command: PaletteCommand::SetShellVisibility {
            left: chrome.left_visible,
            inspector: !chrome.inspector_visible,
            tab_strip: chrome.tab_strip_visible,
        },
    });
    entries.push(PaletteEntry {
        label: if chrome.tab_strip_visible {
            "Hide Tab Strip".to_owned()
        } else {
            "Show Tab Strip".to_owned()
        },
        category: "View",
        command: PaletteCommand::SetShellVisibility {
            left: chrome.left_visible,
            inspector: chrome.inspector_visible,
            tab_strip: !chrome.tab_strip_visible,
        },
    });

    for mode in [
        InspectorMode::Context,
        InspectorMode::Workspace,
        InspectorMode::Tab,
        InspectorMode::Pane,
        // Block mode is entered only by selecting a Block, never as a
        // palette SetInspectorMode (that would project an empty inspector).
    ] {
        if mode == chrome.inspector_mode {
            continue;
        }
        entries.push(PaletteEntry {
            label: format!("Inspector: {}", inspector_mode_label(mode)),
            category: "View",
            command: PaletteCommand::SetInspectorMode(mode),
        });
    }

    for item in &chrome.attention_items {
        entries.push(PaletteEntry {
            label: format!("Attention: {}", item.title),
            category: "Attention",
            command: PaletteCommand::OpenAttention(item.id.clone()),
        });
    }

    for agent in &chrome.agents {
        if chrome.selected_agent.as_ref() == Some(&agent.id) {
            continue;
        }
        entries.push(PaletteEntry {
            label: format!("Focus Agent: {}", agent.name),
            category: "Agent",
            command: PaletteCommand::FocusAgent(agent.id.clone()),
        });
    }

    entries
}

/// Match strength. Lower sorts first. Plain prefix/substring only — not a
/// general fuzzy-ranking engine (explicitly out of scope for #932).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum MatchClass {
    Prefix,
    Substring,
}

fn classify(query: &str, label: &str) -> Option<MatchClass> {
    if query.is_empty() {
        return Some(MatchClass::Prefix);
    }
    if label.starts_with(query) {
        return Some(MatchClass::Prefix);
    }
    if label.contains(query) {
        return Some(MatchClass::Substring);
    }
    None
}

fn filter<'a>(entries: &'a [PaletteEntry], query: &str) -> Vec<&'a PaletteEntry> {
    let query = query.to_lowercase();
    let mut scored: Vec<(MatchClass, &PaletteEntry)> = entries
        .iter()
        .filter_map(|entry| {
            classify(&query, &entry.label.to_lowercase()).map(|class| (class, entry))
        })
        .collect();
    // Stable sort: ties keep the original build_commands order (category
    // grouping), matching the "no ranking science project" scope.
    scored.sort_by_key(|(class, _)| *class);
    scored
        .into_iter()
        .take(PALETTE_VISIBLE_ROWS)
        .map(|(_, entry)| entry)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::{AgentActivity, AgentRecord, AttentionItem, ChromeAction, ChromeState};
    use crate::shell::{ShellAction, ShellPaneSeed, ShellState, ShellTabSeed, ShellWorkspaceSeed};
    use seyal_core::{PaneId, TabId, WorkspaceId};

    fn seed_shell() -> ShellState {
        let workspace = WorkspaceId::from_bytes([1; 16]);
        let tab = TabId::from_bytes([1; 16]);
        let pane = PaneId::from_bytes([1; 16]);
        ShellState::from_workspaces(
            vec![ShellWorkspaceSeed {
                id: workspace,
                name: "Seyal OSS".into(),
                detail: None,
                attention: false,
                active_tab: tab,
                tabs: vec![ShellTabSeed {
                    id: tab,
                    title: "Core Terminal".into(),
                    attention: false,
                    pane: ShellPaneSeed {
                        id: pane,
                        title: "Pane 1".into(),
                        allows_implicit_execution_bootstrap: true,
                    },
                }],
            }],
            workspace,
            true,
            true,
        )
        .expect("seed shell")
    }

    fn labels(rows: &[PaletteRow]) -> Vec<&str> {
        rows.iter().map(|row| row.label.as_str()).collect()
    }

    #[test]
    fn closed_palette_snapshots_empty_and_every_action_but_open_fails_closed() {
        let shell = seed_shell().snapshot();
        let chrome = ChromeState::new().snapshot(&shell, &[]);
        let mut palette = PaletteState::new();
        let snap = palette.snapshot(&shell, &chrome, true, true);
        assert!(!snap.open);
        assert!(snap.rows.is_empty());
        assert!(palette.resolve(&shell, &chrome, true, true).is_none());
        assert_eq!(
            palette.apply(PaletteAction::SetQuery("x".into()), 0),
            Err(PaletteError::NotOpen)
        );
        assert_eq!(
            palette.apply(PaletteAction::MoveSelection(1), 0),
            Err(PaletteError::NotOpen)
        );
        // Close while already closed is idempotent, not an error.
        assert_eq!(palette.apply(PaletteAction::Close, 0), Ok(()));
    }

    #[test]
    fn open_lists_commands_and_reopen_does_not_reset_an_existing_query() {
        let shell = seed_shell().snapshot();
        let chrome = ChromeState::new().snapshot(&shell, &[]);
        let mut palette = PaletteState::new();
        palette.apply(PaletteAction::Open, 0).unwrap();
        let snap = palette.snapshot(&shell, &chrome, true, true);
        assert!(snap.open);
        assert!(labels(&snap.rows).contains(&"New Tab"));
        assert!(!labels(&snap.rows).contains(&"Switch to Tab: Core Terminal"));
        palette
            .apply(PaletteAction::SetQuery("split".into()), 0)
            .unwrap();
        palette.apply(PaletteAction::Open, 0).unwrap();
        assert_eq!(palette.query(), "split", "reopen is a no-op while open");
    }

    fn entry(label: &str) -> PaletteEntry {
        PaletteEntry {
            label: label.to_owned(),
            category: "Test",
            command: PaletteCommand::CreateTab,
        }
    }

    #[test]
    fn filter_ranks_prefix_before_substring_and_is_case_insensitive() {
        let entries = vec![
            entry("Switch to Tab: Agent Development"),
            entry("Tab Strip: Show"),
            entry("Unrelated"),
        ];
        let filtered = filter(&entries, "tab");
        assert_eq!(
            filtered
                .iter()
                .map(|e| e.label.as_str())
                .collect::<Vec<_>>(),
            vec!["Tab Strip: Show", "Switch to Tab: Agent Development"],
            "prefix match ranks before a later substring hit; non-matches are excluded"
        );
    }

    #[test]
    fn filter_caps_results_at_the_visible_row_limit() {
        let entries: Vec<PaletteEntry> = (0..(PALETTE_VISIBLE_ROWS + 5))
            .map(|i| entry(&format!("Command {i}")))
            .collect();
        assert_eq!(filter(&entries, "").len(), PALETTE_VISIBLE_ROWS);
        assert_eq!(filter(&entries, "command").len(), PALETTE_VISIBLE_ROWS);
    }

    #[test]
    fn build_commands_excludes_no_ops_and_includes_split_and_new_tab() {
        let mut shell = seed_shell();
        shell.apply(ShellAction::CreateTab).unwrap();
        let snap = shell.snapshot();
        let chrome = ChromeState::new().snapshot(&snap, &[]);
        let mut palette = PaletteState::new();
        palette.apply(PaletteAction::Open, 0).unwrap();
        let rows = palette.snapshot(&snap, &chrome, true, true).rows;
        let labels = labels(&rows);
        assert!(labels.contains(&"New Tab"));
        assert!(labels.contains(&"Split Pane Right"));
        assert!(labels.contains(&"Split Pane Down"));
        // CreateTab made "Terminal 2" the active tab; only the now-inactive
        // "Core Terminal" is offered as a switch target.
        assert!(labels.contains(&"Switch to Tab: Core Terminal"));
        assert!(
            !labels.iter().any(|label| label.contains("Terminal 2")),
            "the active Tab is not offered as a switch target"
        );
        assert!(
            !labels.iter().any(|label| label.starts_with("Focus Pane:")),
            "single-Pane Tab has no other Pane to focus"
        );
    }

    #[test]
    fn move_selection_clamps_and_resets_on_query_change() {
        let shell = seed_shell().snapshot();
        let chrome = ChromeState::new().snapshot(&shell, &[]);
        let mut palette = PaletteState::new();
        palette.apply(PaletteAction::Open, 0).unwrap();
        let row_count = palette.snapshot(&shell, &chrome, true, true).rows.len();
        palette
            .apply(PaletteAction::MoveSelection(1), row_count)
            .unwrap();
        assert_eq!(palette.snapshot(&shell, &chrome, true, true).selected, 1);
        palette
            .apply(PaletteAction::MoveSelection(50), row_count)
            .unwrap();
        assert_eq!(
            palette.snapshot(&shell, &chrome, true, true).selected,
            row_count - 1
        );
        palette
            .apply(PaletteAction::MoveSelection(-50), row_count)
            .unwrap();
        assert_eq!(palette.snapshot(&shell, &chrome, true, true).selected, 0);
        palette
            .apply(PaletteAction::SetQuery("split".into()), row_count)
            .unwrap();
        assert_eq!(
            palette.snapshot(&shell, &chrome, true, true).selected,
            0,
            "filter change resets selection"
        );
    }

    #[test]
    fn resolve_tracks_selection_and_closing_stops_resolving() {
        let shell = seed_shell().snapshot();
        let chrome = ChromeState::new().snapshot(&shell, &[]);
        let mut palette = PaletteState::new();
        palette.apply(PaletteAction::Open, 0).unwrap();
        palette
            .apply(PaletteAction::SetQuery("split pane down".into()), 0)
            .unwrap();
        assert_eq!(
            palette.resolve(&shell, &chrome, true, true),
            Some(PaletteCommand::SplitFocused(SplitAxis::Down))
        );
        palette.close();
        assert!(!palette.is_open());
        assert!(palette.resolve(&shell, &chrome, true, true).is_none());
        assert_eq!(palette.query(), "", "close clears the query");
    }

    #[test]
    fn no_match_resolves_to_none() {
        let shell = seed_shell().snapshot();
        let chrome = ChromeState::new().snapshot(&shell, &[]);
        let mut palette = PaletteState::new();
        palette.apply(PaletteAction::Open, 0).unwrap();
        palette
            .apply(PaletteAction::SetQuery("zzz-no-such-command".into()), 0)
            .unwrap();
        assert!(palette
            .snapshot(&shell, &chrome, true, true)
            .rows
            .is_empty());
        assert!(palette.resolve(&shell, &chrome, true, true).is_none());
    }

    #[test]
    fn attention_and_agent_rows_are_sourced_only_from_chrome() {
        let shell = seed_shell().snapshot();
        let mut chrome = ChromeState::new();
        chrome
            .apply(
                ChromeAction::ReplaceAgents {
                    workspace: shell.active_workspace,
                    agents: vec![AgentRecord {
                        id: AgentId::new("agent-1"),
                        name: "Claude".into(),
                        activity: AgentActivity::Running,
                    }],
                },
                &shell,
            )
            .unwrap();
        chrome
            .apply(
                ChromeAction::ReplaceAttention {
                    items: vec![AttentionItem {
                        id: AttentionId::new("att-1"),
                        title: "Build failed".into(),
                        detail: "exit 1".into(),
                        workspace: None,
                        tab: None,
                        agent: None,
                    }],
                },
                &shell,
            )
            .unwrap();
        let snap = chrome.snapshot(&shell, &[]);
        let mut palette = PaletteState::new();
        palette.apply(PaletteAction::Open, 0).unwrap();
        let rows = palette.snapshot(&shell, &snap, true, true).rows;
        assert!(labels(&rows).contains(&"Attention: Build failed"));
        assert!(labels(&rows).contains(&"Focus Agent: Claude"));
        // A selected agent is not offered again.
        chrome
            .apply(
                ChromeAction::SelectAgent {
                    id: AgentId::new("agent-1"),
                },
                &shell,
            )
            .unwrap();
        let after = chrome.snapshot(&shell, &[]);
        let rows = palette.snapshot(&shell, &after, true, true).rows;
        assert!(!labels(&rows).contains(&"Focus Agent: Claude"));
    }

    #[test]
    fn build_commands_omits_create_tab_and_split_when_shell_policy_disallows_them() {
        let shell = ShellState::m001_local("local").snapshot();
        let chrome = ChromeState::new().snapshot(&shell, &[]);
        let mut palette = PaletteState::new();
        palette.apply(PaletteAction::Open, 0).unwrap();
        let rows = palette.snapshot(&shell, &chrome, false, false).rows;
        let labels = labels(&rows);
        assert!(!labels.contains(&"New Tab"));
        assert!(!labels.contains(&"Split Pane Right"));
        assert!(!labels.contains(&"Split Pane Down"));
        assert!(
            !labels.is_empty(),
            "view/inspector toggle commands remain available"
        );
    }
}
