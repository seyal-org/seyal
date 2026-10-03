//! Global keyboard-first command palette (#932 / SPEC-022 N2).
//!
//! Rust owns open/query/selected-index state and a **frozen** projection of
//! rows captured on Open / SetQuery. Navigation rows carry
//! [`ResourceAddress`]; Run uses that stored address (or a host-echoed one),
//! never a freshly rebuilt ordinal. Verb/chrome rows keep a stored
//! [`PaletteCommand`]. Filtering is a plain case-insensitive prefix/substring
//! match, not a general fuzzy-ranking engine.

use crate::chrome::{AgentId, AttentionId, ChromeSnapshot, InspectorMode, LeftPanelMode};
use crate::navigation::ResourceAddress;
use crate::shell::{ShellSnapshot, SplitAxis};

/// Maximum rows projected to the host for one filter result.
pub const PALETTE_VISIBLE_ROWS: usize = 12;

/// A non-navigation product action the palette can run. Navigation targets use
/// [`ResourceAddress`] on the row instead (SPEC-022 R7.2 / R7.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteCommand {
    CreateTab,
    SplitFocused(SplitAxis),
    SetLeftPanel(LeftPanelMode),
    SetShellVisibility {
        left: bool,
        inspector: bool,
        tab_strip: bool,
    },
    SetInspectorMode(InspectorMode),
    OpenAttention(AttentionId),
    FocusAgent(AgentId),
    /// Explicit non-TUI presentation (#867). `raw` latches Raw across TUI.
    SelectResting {
        raw: bool,
    },
}

/// What Run executes for the current selection (never a re-resolved ordinal).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteRunTarget {
    Navigate(ResourceAddress),
    Command(PaletteCommand),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PaletteEntry {
    label: String,
    category: &'static str,
    address: Option<ResourceAddress>,
    command: Option<PaletteCommand>,
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

/// One read-only projected row. Navigation rows carry `address`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteRow {
    pub label: String,
    pub category: &'static str,
    pub address: Option<ResourceAddress>,
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
    /// Frozen on Open / SetQuery. Run reads from this list, never rebuilds.
    projected: Vec<PaletteEntry>,
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

    /// `row_count` is the frozen projected count; only
    /// [`PaletteAction::MoveSelection`] uses it.
    pub fn apply(&mut self, action: PaletteAction, row_count: usize) -> Result<(), PaletteError> {
        self.last_error = None;
        match action {
            PaletteAction::Open => {
                if !self.open {
                    self.open = true;
                    self.query.clear();
                    self.selected = 0;
                    self.projected.clear();
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
                    self.projected.clear();
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

    /// Rebuild the frozen projection from authoritative shell/chrome state.
    /// Called after Open and SetQuery so Run never re-resolves by ordinal.
    /// `explicit_raw` is `Some` only for a bound Pane: `Some(true)` offers
    /// return to Flow; `Some(false)` offers explicit Raw; `None` omits both.
    pub fn rebuild(
        &mut self,
        shell: &ShellSnapshot,
        chrome: &ChromeSnapshot,
        allows_tab_creation: bool,
        allows_pane_splitting: bool,
        explicit_raw: Option<bool>,
    ) {
        if !self.open {
            self.projected.clear();
            return;
        }
        let commands = build_commands(
            shell,
            chrome,
            allows_tab_creation,
            allows_pane_splitting,
            explicit_raw,
        );
        self.projected = filter(&commands, &self.query);
        self.selected = clamp(self.selected, self.projected.len());
    }

    /// Close without going through `apply`; used after a successful Run.
    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.selected = 0;
        self.projected.clear();
    }

    pub fn snapshot(&self) -> PaletteSnapshot {
        if !self.open {
            return PaletteSnapshot {
                open: false,
                query: String::new(),
                selected: 0,
                rows: Vec::new(),
                last_error: self.last_error,
            };
        }
        let selected = clamp(self.selected, self.projected.len());
        let rows = self
            .projected
            .iter()
            .map(|entry| PaletteRow {
                label: entry.label.clone(),
                category: entry.category,
                address: entry.address,
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

    /// Target bound to the current frozen selection, or `None` while closed /
    /// with no matching row. Does **not** rebuild against live state.
    pub fn selected_target(&self) -> Option<PaletteRunTarget> {
        if !self.open {
            return None;
        }
        let entry = self
            .projected
            .get(clamp(self.selected, self.projected.len()))?;
        if let Some(address) = entry.address {
            return Some(PaletteRunTarget::Navigate(address));
        }
        entry.command.clone().map(PaletteRunTarget::Command)
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
    explicit_raw: Option<bool>,
) -> Vec<PaletteEntry> {
    let mut entries = Vec::new();

    match explicit_raw {
        Some(false) => entries.push(PaletteEntry {
            label: "Use Raw Terminal".to_owned(),
            category: "Terminal",
            address: None,
            command: Some(PaletteCommand::SelectResting { raw: true }),
        }),
        Some(true) => entries.push(PaletteEntry {
            label: "Return to Flow".to_owned(),
            category: "Terminal",
            address: None,
            command: Some(PaletteCommand::SelectResting { raw: false }),
        }),
        None => {}
    }

    if allows_tab_creation {
        entries.push(PaletteEntry {
            label: "New Tab".to_owned(),
            category: "Navigation",
            address: None,
            command: Some(PaletteCommand::CreateTab),
        });
    }
    if allows_pane_splitting {
        entries.push(PaletteEntry {
            label: "Split Pane Right".to_owned(),
            category: "Navigation",
            address: None,
            command: Some(PaletteCommand::SplitFocused(SplitAxis::Right)),
        });
        entries.push(PaletteEntry {
            label: "Split Pane Down".to_owned(),
            category: "Navigation",
            address: None,
            command: Some(PaletteCommand::SplitFocused(SplitAxis::Down)),
        });
    }

    for workspace in &shell.workspaces {
        if workspace.id == shell.active_workspace {
            continue;
        }
        entries.push(PaletteEntry {
            label: format!("Switch to Workspace: {}", workspace.name),
            category: "Workspace",
            address: Some(ResourceAddress::Workspace {
                workspace: workspace.id,
            }),
            command: None,
        });
    }

    for tab in &shell.tabs {
        if tab.id == shell.active_tab {
            continue;
        }
        entries.push(PaletteEntry {
            label: format!("Switch to Tab: {}", tab.title),
            category: "Tab",
            address: Some(ResourceAddress::Tab {
                workspace: shell.active_workspace,
                tab: tab.id,
            }),
            command: None,
        });
    }

    for pane in &shell.panes {
        if pane.id == shell.focused_pane {
            continue;
        }
        entries.push(PaletteEntry {
            label: format!("Focus Pane: {}", pane.title),
            category: "Pane",
            address: Some(ResourceAddress::Pane {
                workspace: shell.active_workspace,
                tab: shell.active_tab,
                pane: pane.id,
            }),
            command: None,
        });
    }

    let other_left_panel = match chrome.left_panel {
        LeftPanelMode::Workspaces => LeftPanelMode::Tabs,
        LeftPanelMode::Tabs => LeftPanelMode::Workspaces,
    };
    entries.push(PaletteEntry {
        label: format!("Show {} in Left Panel", left_panel_label(other_left_panel)),
        category: "View",
        address: None,
        command: Some(PaletteCommand::SetLeftPanel(other_left_panel)),
    });

    entries.push(PaletteEntry {
        label: if chrome.left_visible {
            "Hide Left Panel".to_owned()
        } else {
            "Show Left Panel".to_owned()
        },
        category: "View",
        address: None,
        command: Some(PaletteCommand::SetShellVisibility {
            left: !chrome.left_visible,
            inspector: chrome.inspector_visible,
            tab_strip: chrome.tab_strip_visible,
        }),
    });
    entries.push(PaletteEntry {
        label: if chrome.inspector_visible {
            "Hide Inspector".to_owned()
        } else {
            "Show Inspector".to_owned()
        },
        category: "View",
        address: None,
        command: Some(PaletteCommand::SetShellVisibility {
            left: chrome.left_visible,
            inspector: !chrome.inspector_visible,
            tab_strip: chrome.tab_strip_visible,
        }),
    });
    entries.push(PaletteEntry {
        label: if chrome.tab_strip_visible {
            "Hide Tab Strip".to_owned()
        } else {
            "Show Tab Strip".to_owned()
        },
        category: "View",
        address: None,
        command: Some(PaletteCommand::SetShellVisibility {
            left: chrome.left_visible,
            inspector: chrome.inspector_visible,
            tab_strip: !chrome.tab_strip_visible,
        }),
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
            address: None,
            command: Some(PaletteCommand::SetInspectorMode(mode)),
        });
    }

    for item in &chrome.attention_items {
        entries.push(PaletteEntry {
            label: format!("Attention: {}", item.title),
            category: "Attention",
            address: None,
            command: Some(PaletteCommand::OpenAttention(item.id.clone())),
        });
    }

    for agent in &chrome.agents {
        if chrome.selected_agent.as_ref() == Some(&agent.id) {
            continue;
        }
        entries.push(PaletteEntry {
            label: format!("Focus Agent: {}", agent.name),
            category: "Agent",
            address: None,
            command: Some(PaletteCommand::FocusAgent(agent.id.clone())),
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

fn filter(entries: &[PaletteEntry], query: &str) -> Vec<PaletteEntry> {
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
        .map(|(_, entry)| entry.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::{AgentActivity, AgentRecord, AttentionItem, ChromeAction, ChromeState};
    use crate::shell::{
        ShellAction, ShellPaneSeed, ShellState, ShellTabSeed, ShellWindowSeed, ShellWorkspaceSeed,
    };
    use seyal_core::{PaneId, TabId, WindowId, WorkspaceId};

    fn seed_shell() -> ShellState {
        let workspace = WorkspaceId::from_bytes([1; 16]);
        let tab = TabId::from_bytes([1; 16]);
        let pane = PaneId::from_bytes([1; 16]);
        let window = WindowId::new();
        ShellState::from_workspaces(
            vec![ShellWorkspaceSeed {
                id: workspace,
                name: "Seyal OSS".into(),
                detail: None,
                attention: false,
                active_window: window,
                windows: vec![ShellWindowSeed {
                    id: window,
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
            }],
            workspace,
            true,
            true,
        )
        .expect("fixture")
    }

    fn labels(rows: &[PaletteRow]) -> Vec<&str> {
        rows.iter().map(|row| row.label.as_str()).collect()
    }

    fn open_rebuilt(
        palette: &mut PaletteState,
        shell: &ShellSnapshot,
        chrome: &ChromeSnapshot,
        tabs: bool,
        splits: bool,
    ) {
        palette.apply(PaletteAction::Open, 0).unwrap();
        palette.rebuild(shell, chrome, tabs, splits, None);
    }

    #[test]
    fn closed_palette_rejects_query_and_move() {
        let shell = seed_shell().snapshot();
        let chrome = ChromeState::new().snapshot(&shell, &[]);
        let mut palette = PaletteState::new();
        let snap = palette.snapshot();
        assert!(!snap.open);
        assert!(snap.rows.is_empty());
        assert!(palette.selected_target().is_none());
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
        let _ = chrome;
    }

    #[test]
    fn open_lists_commands_and_reopen_does_not_reset_an_existing_query() {
        let shell = seed_shell().snapshot();
        let chrome = ChromeState::new().snapshot(&shell, &[]);
        let mut palette = PaletteState::new();
        open_rebuilt(&mut palette, &shell, &chrome, true, true);
        let snap = palette.snapshot();
        assert!(snap.open);
        assert!(labels(&snap.rows).contains(&"New Tab"));
        assert!(!labels(&snap.rows).contains(&"Switch to Tab: Core Terminal"));
        palette
            .apply(PaletteAction::SetQuery("split".into()), 0)
            .unwrap();
        palette.rebuild(&shell, &chrome, true, true, None);
        palette.apply(PaletteAction::Open, 0).unwrap();
        assert_eq!(palette.query(), "split", "reopen is a no-op while open");
    }

    fn entry(label: &str) -> PaletteEntry {
        PaletteEntry {
            label: label.to_owned(),
            category: "Test",
            address: None,
            command: Some(PaletteCommand::CreateTab),
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
        open_rebuilt(&mut palette, &snap, &chrome, true, true);
        let rows = palette.snapshot().rows;
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
        let tab_row = rows
            .iter()
            .find(|row| row.label == "Switch to Tab: Core Terminal")
            .expect("tab row");
        assert!(matches!(tab_row.address, Some(ResourceAddress::Tab { .. })));
    }

    #[test]
    fn move_selection_clamps_and_resets_on_query_change() {
        let shell = seed_shell().snapshot();
        let chrome = ChromeState::new().snapshot(&shell, &[]);
        let mut palette = PaletteState::new();
        open_rebuilt(&mut palette, &shell, &chrome, true, true);
        let row_count = palette.snapshot().rows.len();
        palette
            .apply(PaletteAction::MoveSelection(1), row_count)
            .unwrap();
        assert_eq!(palette.snapshot().selected, 1);
        palette
            .apply(PaletteAction::MoveSelection(50), row_count)
            .unwrap();
        assert_eq!(palette.snapshot().selected, row_count - 1);
        palette
            .apply(PaletteAction::MoveSelection(-50), row_count)
            .unwrap();
        assert_eq!(palette.snapshot().selected, 0);
        palette
            .apply(PaletteAction::SetQuery("split".into()), row_count)
            .unwrap();
        palette.rebuild(&shell, &chrome, true, true, None);
        assert_eq!(
            palette.snapshot().selected,
            0,
            "filter change resets selection"
        );
    }

    #[test]
    fn selected_target_tracks_selection_and_closing_clears_it() {
        let shell = seed_shell().snapshot();
        let chrome = ChromeState::new().snapshot(&shell, &[]);
        let mut palette = PaletteState::new();
        open_rebuilt(&mut palette, &shell, &chrome, true, true);
        palette
            .apply(PaletteAction::SetQuery("split pane down".into()), 0)
            .unwrap();
        palette.rebuild(&shell, &chrome, true, true, None);
        assert_eq!(
            palette.selected_target(),
            Some(PaletteRunTarget::Command(PaletteCommand::SplitFocused(
                SplitAxis::Down
            )))
        );
        palette.close();
        assert!(!palette.is_open());
        assert!(palette.selected_target().is_none());
        assert_eq!(palette.query(), "", "close clears the query");
    }

    #[test]
    fn no_match_has_no_selected_target() {
        let shell = seed_shell().snapshot();
        let chrome = ChromeState::new().snapshot(&shell, &[]);
        let mut palette = PaletteState::new();
        open_rebuilt(&mut palette, &shell, &chrome, true, true);
        palette
            .apply(PaletteAction::SetQuery("zzz-no-such-command".into()), 0)
            .unwrap();
        palette.rebuild(&shell, &chrome, true, true, None);
        assert!(palette.snapshot().rows.is_empty());
        assert!(palette.selected_target().is_none());
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
        open_rebuilt(&mut palette, &shell, &snap, true, true);
        let rows = palette.snapshot().rows;
        assert!(labels(&rows).contains(&"Attention: Build failed"));
        assert!(labels(&rows).contains(&"Focus Agent: Claude"));
        // A selected agent is not offered again after a rebuild.
        chrome
            .apply(
                ChromeAction::SelectAgent {
                    id: AgentId::new("agent-1"),
                },
                &shell,
            )
            .unwrap();
        let after = chrome.snapshot(&shell, &[]);
        palette.rebuild(&shell, &after, true, true, None);
        let rows = palette.snapshot().rows;
        assert!(!labels(&rows).contains(&"Focus Agent: Claude"));
    }

    #[test]
    fn build_commands_omits_create_tab_and_split_when_shell_policy_disallows_them() {
        let shell = ShellState::m001_local("local").snapshot();
        let chrome = ChromeState::new().snapshot(&shell, &[]);
        let mut palette = PaletteState::new();
        open_rebuilt(&mut palette, &shell, &chrome, false, false);
        let rows = palette.snapshot().rows;
        let labels = labels(&rows);
        assert!(!labels.contains(&"New Tab"));
        assert!(!labels.contains(&"Split Pane Right"));
        assert!(!labels.contains(&"Split Pane Down"));
        assert!(
            !labels.is_empty(),
            "view/inspector toggle commands remain available"
        );
    }

    #[test]
    fn frozen_projection_ignores_live_shell_changes_until_rebuild() {
        let mut shell = seed_shell();
        shell.apply(ShellAction::CreateTab).unwrap();
        let snap = shell.snapshot();
        let chrome = ChromeState::new().snapshot(&snap, &[]);
        let mut palette = PaletteState::new();
        open_rebuilt(&mut palette, &snap, &chrome, true, true);
        palette
            .apply(PaletteAction::SetQuery("Switch to Tab: Core".into()), 0)
            .unwrap();
        palette.rebuild(&snap, &chrome, true, true, None);
        let address = match palette.selected_target() {
            Some(PaletteRunTarget::Navigate(address)) => address,
            other => panic!("expected navigate target, got {other:?}"),
        };
        // Select the other tab in live shell so a fresh ordinal rebuild would
        // omit "Core Terminal" and put a different row at index 0.
        let core = match address {
            ResourceAddress::Tab { tab, .. } => tab,
            _ => panic!("expected tab address"),
        };
        let other = snap
            .tabs
            .iter()
            .map(|tab| tab.id)
            .find(|id| *id != core)
            .expect("other tab");
        shell
            .apply(ShellAction::SelectTab { id: other })
            .expect("select");
        // Frozen selection still carries the original address.
        assert_eq!(
            palette.selected_target(),
            Some(PaletteRunTarget::Navigate(address))
        );
    }
}
