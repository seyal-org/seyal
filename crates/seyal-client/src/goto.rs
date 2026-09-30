//! Navigation-only goto / quick-switcher surface (SPEC-022 §7 / N4).
//!
//! Target-kind scopes (Workspaces / Tabs / Panes / Sessions) over address-carrying
//! rows. Enumeration is bounded with honest truncation reporting. Run carries the
//! stored [`ResourceAddress`]; the N1 resolver remains the only resolver. This
//! module does not own focus history, a second overlay, or command verbs.

use crate::navigation::ResourceAddress;
use crate::shell::NavigationInventory;

/// Maximum candidates projected for one scope after filtering (SPEC-022 R7.6).
pub const GOTO_ENUMERATION_BOUND: usize = 64;

/// Target-kind scope. Scopes are never blended (ADR-019 §8 / SPEC-022 R7.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum GotoScope {
    Workspaces = 0,
    Tabs = 1,
    Panes = 2,
    Sessions = 3,
}

impl GotoScope {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Workspaces),
            1 => Some(Self::Tabs),
            2 => Some(Self::Panes),
            3 => Some(Self::Sessions),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Workspaces => "Workspaces",
            Self::Tabs => "Tabs",
            Self::Panes => "Panes",
            Self::Sessions => "Sessions",
        }
    }

    /// Host-facing placeholder text for the goto overlay (ADR-015: Rust-owned).
    pub fn placeholder(self, truncated: bool) -> String {
        if truncated {
            format!("Go to {} (truncated)…", self.as_str())
        } else {
            format!("Go to {}…", self.as_str())
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Workspaces => Self::Tabs,
            Self::Tabs => Self::Panes,
            Self::Panes => Self::Sessions,
            Self::Sessions => Self::Workspaces,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GotoError {
    NotOpen,
    NoSelection,
    UnsupportedScope,
}

impl GotoError {
    fn message(self) -> &'static str {
        match self {
            Self::NotOpen => "Goto surface is not open.",
            Self::NoSelection => "No goto row is selected.",
            Self::UnsupportedScope => "Unsupported goto scope.",
        }
    }
}

impl std::fmt::Display for GotoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GotoAction {
    Open {
        scope: GotoScope,
    },
    SetScope(GotoScope),
    /// Advance to the next target-kind scope (Workspaces→Tabs→Panes→Sessions).
    CycleScope,
    SetQuery(String),
    MoveSelection(i32),
    Close,
}

/// One navigation-only projected row (SPEC-022 R7.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GotoRow {
    pub address: ResourceAddress,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GotoSnapshot {
    pub open: bool,
    pub scope: GotoScope,
    pub query: String,
    pub selected: usize,
    pub rows: Vec<GotoRow>,
    /// True when the live inventory for this scope exceeded
    /// [`GOTO_ENUMERATION_BOUND`] after filtering (SPEC-022 R7.6).
    pub truncated: bool,
    pub last_error: Option<GotoError>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GotoEntry {
    address: ResourceAddress,
    label: String,
}

/// Authoritative goto surface state.
#[derive(Clone, Debug)]
pub struct GotoState {
    open: bool,
    scope: GotoScope,
    query: String,
    selected: usize,
    last_error: Option<GotoError>,
    /// Frozen on Open / SetScope / SetQuery. Run reads from this list.
    projected: Vec<GotoEntry>,
    truncated: bool,
}

impl Default for GotoState {
    fn default() -> Self {
        Self::new()
    }
}

impl GotoState {
    pub fn new() -> Self {
        Self {
            open: false,
            scope: GotoScope::Panes,
            query: String::new(),
            selected: 0,
            last_error: None,
            projected: Vec::new(),
            truncated: false,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn scope(&self) -> GotoScope {
        self.scope
    }

    pub fn apply(&mut self, action: GotoAction, row_count: usize) -> Result<(), GotoError> {
        self.last_error = None;
        match action {
            GotoAction::Open { scope } => {
                self.open = true;
                self.scope = scope;
                self.query.clear();
                self.selected = 0;
                self.projected.clear();
                self.truncated = false;
                Ok(())
            }
            GotoAction::SetScope(scope) => {
                if !self.open {
                    return self.fail(GotoError::NotOpen);
                }
                if self.scope != scope {
                    self.scope = scope;
                    self.selected = 0;
                    self.projected.clear();
                    self.truncated = false;
                }
                Ok(())
            }
            GotoAction::CycleScope => {
                if !self.open {
                    return self.fail(GotoError::NotOpen);
                }
                self.scope = self.scope.next();
                self.selected = 0;
                self.projected.clear();
                self.truncated = false;
                Ok(())
            }
            GotoAction::SetQuery(query) => {
                if !self.open {
                    return self.fail(GotoError::NotOpen);
                }
                if self.query != query {
                    self.query = query;
                    self.selected = 0;
                    self.projected.clear();
                    self.truncated = false;
                }
                Ok(())
            }
            GotoAction::MoveSelection(delta) => {
                if !self.open {
                    return self.fail(GotoError::NotOpen);
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
            GotoAction::Close => {
                self.close();
                Ok(())
            }
        }
    }

    /// Rebuild the frozen projection from authoritative inventory.
    pub fn rebuild(&mut self, inventory: &NavigationInventory) {
        if !self.open {
            self.projected.clear();
            self.truncated = false;
            return;
        }
        let (rows, truncated) = enumerate(self.scope, inventory, &self.query);
        self.projected = rows;
        self.truncated = truncated;
        self.selected = clamp(self.selected, self.projected.len());
    }

    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.selected = 0;
        self.projected.clear();
        self.truncated = false;
        self.last_error = None;
    }

    pub fn snapshot(&self) -> GotoSnapshot {
        if !self.open {
            return GotoSnapshot {
                open: false,
                scope: self.scope,
                query: String::new(),
                selected: 0,
                rows: Vec::new(),
                truncated: false,
                last_error: self.last_error,
            };
        }
        let selected = clamp(self.selected, self.projected.len());
        GotoSnapshot {
            open: true,
            scope: self.scope,
            query: self.query.clone(),
            selected,
            rows: self
                .projected
                .iter()
                .map(|entry| GotoRow {
                    address: entry.address,
                    label: entry.label.clone(),
                })
                .collect(),
            truncated: self.truncated,
            last_error: self.last_error,
        }
    }

    /// Address bound to the current frozen selection.
    pub fn selected_address(&self) -> Option<ResourceAddress> {
        if !self.open {
            return None;
        }
        self.projected
            .get(clamp(self.selected, self.projected.len()))
            .map(|entry| entry.address)
    }

    fn fail(&mut self, error: GotoError) -> Result<(), GotoError> {
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

fn enumerate(
    scope: GotoScope,
    inventory: &NavigationInventory,
    query: &str,
) -> (Vec<GotoEntry>, bool) {
    let candidates: Vec<GotoEntry> = match scope {
        GotoScope::Workspaces => inventory
            .workspaces
            .iter()
            .map(|item| GotoEntry {
                address: ResourceAddress::Workspace { workspace: item.id },
                label: workspace_label(item),
            })
            .collect(),
        GotoScope::Tabs => inventory
            .tabs
            .iter()
            .map(|item| GotoEntry {
                address: ResourceAddress::Tab {
                    workspace: item.workspace,
                    tab: item.id,
                },
                label: tab_label(item),
            })
            .collect(),
        GotoScope::Panes => inventory
            .panes
            .iter()
            .map(|item| GotoEntry {
                address: ResourceAddress::Pane {
                    workspace: item.workspace,
                    tab: item.tab,
                    pane: item.id,
                },
                label: pane_label(item),
            })
            .collect(),
        GotoScope::Sessions => inventory
            .sessions
            .iter()
            .map(|item| GotoEntry {
                address: ResourceAddress::Execution {
                    execution: item.execution,
                },
                label: session_label(item),
            })
            .collect(),
    };
    filter_and_bound(&candidates, query)
}

fn filter_and_bound(entries: &[GotoEntry], query: &str) -> (Vec<GotoEntry>, bool) {
    let query = query.to_lowercase();
    let matched: Vec<GotoEntry> = entries
        .iter()
        .filter(|entry| query.is_empty() || entry.label.to_lowercase().contains(query.as_str()))
        .cloned()
        .collect();
    let truncated = matched.len() > GOTO_ENUMERATION_BOUND;
    let rows = matched.into_iter().take(GOTO_ENUMERATION_BOUND).collect();
    (rows, truncated)
}

fn workspace_label(item: &crate::shell::WorkspaceNavItem) -> String {
    let mut label = item.name.clone();
    if let Some(detail) = item.detail.as_ref().filter(|d| !d.is_empty()) {
        label.push_str(" · ");
        label.push_str(detail);
    }
    push_badge(&mut label, item.active, "active");
    push_badge(&mut label, item.attention, "attention");
    label
}

fn tab_label(item: &crate::shell::TabNavItem) -> String {
    let mut label = format!("{} · {}", item.title, item.workspace_name);
    push_badge(&mut label, item.active, "active");
    push_badge(&mut label, item.attention, "attention");
    label
}

fn pane_label(item: &crate::shell::PaneNavItem) -> String {
    let mut label = format!("{} · {}", item.title, item.tab_title);
    push_badge(&mut label, item.focused, "focused");
    if item.execution.is_some() {
        push_badge(&mut label, true, "bound");
    } else {
        push_badge(&mut label, true, "unbound");
    }
    label
}

fn session_label(item: &crate::shell::SessionNavItem) -> String {
    let mut label = format!("{} · {}", item.pane_title, item.workspace_name);
    // No fabricated "live" badge — SessionNavItem has no Runtime liveness
    // authority (SPEC-022 R7.1). Focused is projected from shell focus.
    push_badge(&mut label, item.focused, "focused");
    label
}

fn push_badge(label: &mut String, present: bool, badge: &str) {
    if present {
        label.push_str(" · ");
        label.push_str(badge);
    }
}
