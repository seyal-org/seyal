# M001 Core Terminal Reference Screen

**Status:** Frozen reference for design/spec alignment  
**Scope:** Core terminal screen and shared interaction rules  
**Reference image:** `docs/architecture/ui/assets/m001-core-terminal-reference.jpg`

## 1. Purpose

This document freezes the canonical Core Terminal information architecture and interaction rules so later implementation does not drift across mockups.

This is design/spec authority only. It does not authorize implementation ahead of the M001 dependency frontier.

## 2. Canonical object hierarchy

```text
App
└── Workspace
    └── Tab
        └── Pane tree
            └── Pane
                └── TerminalExecution / non-terminal surface
```

Rules:

- one app can contain many Workspaces;
- one Workspace can contain many Tabs;
- one Tab owns a nested split tree;
- each terminal Pane owns at most one `TerminalExecution`;
- `TerminalExecution` remains the sole PTY/child/canonical terminal-state authority;
- UI/Blocks/agents/inspector never create a second VT/grid authority.

## 3. Canonical screen layout

```text
[macOS window chrome]
[workspace-scoped top tab row + split/layout + notifications]
[left context panel] [active tab / pane tree] [right contextual inspector]
```

The design is optimized for terminal power users on a 15-inch display: high information density, minimal permanent chrome, keyboard-first navigation, and no decorative panels.

## 4. Top tab row

### 4.1 Workspace-scoped tabs

The top row shows tabs belonging to the active Workspace.

A tab may show:

- title;
- activity/attention state;
- close affordance where appropriate.

The tab strip must be adaptive:

- preserve the active tab as visible;
- shrink tabs to a documented minimum width;
- when space is still insufficient, use horizontal scrolling/overflow rather than wrapping to multiple rows;
- do not consume vertical terminal space with a second wrapped tab row.

### 4.2 New tab

`+` creates a new tab in the active Workspace.

### 4.3 Split/layout controls

Primary split controls live with the tab/layout chrome and apply to the focused Pane:

- split right;
- split down;
- other layout actions only when they have real implementation semantics.

Pane-local context menus/shortcuts may expose the same actions, but do not duplicate a permanent cluster of split buttons inside every Pane merely for appearance.

### 4.4 Notifications / attention

A compact top-right attention indicator is retained.

It surfaces actionable events without forcing tab switching, including:

- agent approval required;
- command/background task completion;
- failure requiring attention;
- long-running task attention.

The popover behavior is defined by `M001-NOTIFICATIONS-ATTENTION-POPOVER.md`.

### 4.5 Global command palette

The earlier approved references included a global command-palette entry point. That capability is retained as a keyboard-first global navigation/action surface.

It may expose actions such as:

- switch Workspace/Tab/Pane;
- create tab;
- split focused Pane;
- open Sessions/Agents/Resources;
- toggle inspector modes;
- invoke documented product actions.

A permanent toolbar button is not required. The exact shortcut belongs to the native-input/keybinding specification and must not conflict with terminal/application key semantics.

## 5. Left context panel

The left panel remains compact and stable while its content reflects product context.

### 5.1 Workspaces

Show known Workspaces with only useful metadata:

- name;
- project/root path when helpful;
- tab count;
- compact status/attention where backed by real state.

Do not use oversized cards when a dense list communicates the same information better.

### 5.2 Agents

Show current-Workspace agent sessions such as Claude Code, Codex, OpenCode, and Seyal agents.

A row may show:

- provider/session name;
- Running / Waiting / Attention / Idle state;
- compact activity status.

This is the primary compact agent inventory on the Core Terminal screen. The inspector must not repeat the same full agent list.

### 5.3 Tabs — active Workspace

Show a persistent active-Workspace tab inventory with compact activity/attention state.

This complements the top strip:

- top strip = immediate active tabs;
- left list = stable Workspace-local inventory/navigation.

### 5.4 Runtime/connection status

The earlier approved references included connection state. Retain the capability, but not necessarily as a permanently large footer.

Rules:

- local healthy state may be visually minimal;
- remote, detached, reconnecting, degraded, or disconnected state must be clearly surfaced;
- connection state must come from real Runtime/session state, never decorative telemetry.

### 5.5 C03 runtime state mapping (Workspaces / Agents / Tabs)
The left context panel (`C03`) is presentation chrome over the same underlying Workspace/Tab/Pane navigation model.

This section documents the runtime/state inputs that drive *visible* left-pane row content and selection/attention emphasis:

#### 5.5.1 Workspaces rows (C04 rows under C03)
- Primary label: workspace name (`Workspace.name`).
- Secondary context: workspace path/root detail when helpful (`Workspace.detail`).
- Compact meta: tab count (`Workspace.tabs.count`), expressed as `1 tab` vs `N tabs`.
- Status/attention: use the workspace attention flag when backed by real state (`Workspace.attention`), and use real connection/context backing for any additional status/attention markers.
- Selected row emphasis: the active workspace identity (`activeWorkspaceID`).

#### 5.5.2 Agents rows (C04 rows under C03)
- Row set scope: current active workspace only (the left panel is a compact, current-Workspace inventory).
- Provider/session label: agent name (`Agent.name`) as displayed.
- State label: Running / Waiting / Attention / Idle as backed by real agent session state (`Agent.state`).
- Compact meta/status: any compact “activity status” visible in the row, driven by the same agent session state model.
- Attention marker: the row may show a semantic marker when the agent is in Attention state, but must not duplicate the full agent inventory elsewhere (the Inspector is contextual; it must not repeat the full list).
- Selected row emphasis: the selected agent identity (`selectedAgentID`) when an agent is explicitly focused/selected.

#### 5.5.3 Tabs — active Workspace rows (C04 rows under C03)
- Row set scope: active workspace only.
- Tab label: tab title (`Tab.title`).
- Compact meta: pane count (`Tab.paneCount`), expressed as `1 pane` vs `N panes`.
- Attention marker: may show a semantic marker when the tab has attention (`Tab.attention`), but must not recolor the whole row.
- Selected row emphasis: active tab identity (`activeTabID`).

#### 5.5.4 Left panel mode switcher and collapse/hide (C03 container)
- Left panel mode switcher:
  - `Workspaces` mode shows the Workspace inventory plus the current-Workspace compact agent inventory.
  - `Tabs` mode shows the active-Workspace tab inventory.
- Collapse/hide:
  - Hiding the left context panel must reclaim the center Pane width (no empty gutter).
  - Hiding/reopening must not change Workspace/Tab/Pane focus, drafts, selection identities, or execution identity.
  - The selected mode and selection emphasis must be preserved across hide/reopen operations.

### 5.6 C05 shared Tab identity rule (top strip <-> left list)
The “Tabs — active Workspace” rows in the left panel (`C03`) and the active tab selection in the top tab strip (`C05`) represent the same Tab identity.

Selecting a Tab in either location must update the same underlying tab selection identity (active tab) and must not create a second, parallel tab model.

### 5.7 Existing contract guarantees vs remaining gaps
The left pane is constrained by the shared component contracts:

- `C03` guarantees: D1/receded material at rest; optional subtle frost/transparency; dense lists not cards; panel gains only slight D2 definition when actively navigated; may collapse to a thin rail without changing the content model.
- `C04` guarantees: no rounded card per row; one-row/two-line dense variant; hover exposes secondary actions; selected row uses restrained tint/edge/typographic emphasis; attention state may add one semantic marker without recolouring the whole row.
- `C05` guarantees: one row only; active Tab stays visible; tabs compress to documented minimum then scroll/overflow; active state primarily typography + thin seam/indicator; no individual pill cards; `+` and layout actions remain compact; same geometry in single-pane and multipane screens.

Remaining gaps (implementation detail still to be decided):
- Exact activation hysteresis/cancel threshold for left-pane emphasis preview (e.g. pixel distance and/or pointer-up bounds) remains an implementation choice, but must preserve the contract semantics.
- The exact allowed transition shape/duration for emphasis changes (when motion is allowed) is an implementation detail as long as it does not delay input and respects reduced-motion.

### 5.8 Implementation evidence: stable accessibility IDs + emphasis inputs (doc-only mapping)
The current Swift/AppKit implementation (preview scaffold) exposes stable accessibility identifiers and uses explicit state fields to compute “emphasized/active” styling.

#### 5.8.1 Left context panel container controls (C03)
- Left mode switcher: `left-mode` (Workspaces vs Tabs list)
- Left sidebar collapse/hide control: `left-sidebar-collapse`

#### 5.8.2 Left “Workspaces” / “Agents” / “Tabs” rows (C04)
Each row is implemented as a context button produced by `makeContextButton(...)`:
- Workspace row
  - `accessibilityIdentifier`: `workspace.<workspaceID>`
  - Emphasis input: `workspace.id == snapshot.activeWorkspaceID`
  - Dot/status semantics: uses `workspace.attention` for attention coloration; uses an active-workspace success/status color when active
- Agent row
  - `accessibilityIdentifier`: `agent.<agentID>`
  - Emphasis input: `agent.id == state.selectedAgentID`
  - Dot/state semantics: Running/Waiting/Attention/Idle is expressed via the trailing field, with attention coloration driven by `agent.state`
- Tab row in left list (“Tabs — active Workspace”)
  - `accessibilityIdentifier`: `left-tab.<tabID>`
  - Emphasis input: `tab.id == snapshot.activeTabID`
  - Dot semantics: attention coloration is driven by `tab.attention`

Left-list “new tab” action:
- `accessibilityIdentifier`: `left-new-tab`

#### 5.8.3 Top tab strip chips (C05)
The top strip uses tab chips created by `makeTabChip(...)`:
- Tab chip
  - `accessibilityIdentifier`: `tab.<tabID>`
  - Active emphasis input: `tab.id == snapshot.activeTabID`
- Tab close control
  - `accessibilityIdentifier`: `tab.close.<tabID>`
- Top-strip “new tab” action (+)
  - `accessibilityIdentifier`: `new-tab`

## 6. Pane scroll ownership and Block sizing

This section supersedes earlier mockup language that suggested a fixed maximum Block height with internal Block scrolling.

**Canonical rule: the Pane owns normal transcript scrolling. Blocks do not introduce a second nested scrollbar.**

For normal and long-running non-TUI command output:

- Block height is intrinsic to its visible content;
- a Block may grow as output grows;
- the Pane/transcript is the scroll container;
- there is no fixed-height output box with its own internal scroll simply to constrain a long-running process;
- users scroll the Pane to inspect earlier Blocks/output.

This avoids nested scrolling, preserves normal terminal expectations, and makes wheel/trackpad/keyboard navigation predictable.

Implementation may virtualize off-screen history for performance, but virtualization must not change the single-scroll-container interaction model.

## 7. Block model

Normal command execution is represented as a durable Block over real terminal execution.

A Block contains:

### Header

- command text;
- execution state;
- elapsed time;
- timestamp where useful;
- supported actions.

### Body

- actual terminal output/presentation;
- no copied second terminal engine.

### Optional structured enrichment

High-value commands may gain asynchronous structured summaries when backed by real integration/recognizer data, for example:

- Git file changes/diff summary;
- test summary;
- service endpoint/process state;
- Kubernetes resource summary.

Fallback is always correct command + output. Enrichment is additive and must never block PTY → VT → state → render progress.

## 8. Block actions

Reference actions have real intended behavior:

- **Copy** — copy command/output according to target;
- **Rerun** — rerun in the appropriate execution context;
- **Pin** — retain/promote a Block for deliberate quick return/reference;
- **Expand** — focus/enlarge Block inspection without creating terminal authority;
- overflow — only for additional real actions.

Pinning is presentation/workspace metadata. It must not snapshot or duplicate a live grid.

## 9. Long-running foreground commands

A command such as `npm run dev` is a long-running streaming command, not automatically a full-screen TUI.

While it owns the foreground shell:

- output stays in the same Running Block;
- the Running Block grows with output;
- Pane-level scrolling owns navigation through the transcript;
- the Pane may follow the live tail while the user remains at the end;
- scrolling away pauses auto-follow but never terminal progress;
- a compact `Return to live` affordance appears only when useful.

### 9.1 Composer while shell is busy

Do not present a fully active composer that falsely suggests an unrelated command can run in the same occupied shell.

Preferred behavior:

- retract or disable the Pane composer while the foreground process owns the shell;
- replace it with a compact process-running/status surface when useful;
- retain explicit interrupt/stop guidance only when backed by real behavior;
- preserve any existing Pane draft and restore the normal composer when the shell is available again.

**Accepted ADR-026 (#1384):** "busy" follows the ADR-026 §3.2 input target, which qualifies the retraction above. That includes `TrustedShell(session)` (`Submit`) and a clearly labeled `Relay` for an eligible foreground program. Retraction remains correct when the input target is `None`. See `M001-LIVE-TAIL-BEHAVIOR.md` §6.

Parallel commands should run in another Pane/Tab/execution.

Detailed live-tail behavior is defined by `M001-LIVE-TAIL-BEHAVIOR.md`.

## 10. Pane-scoped multiline composer

Every terminal Pane has its own composer state.

The composer is not global and is never shared across panes.

### 10.1 Multiline editing

The earlier approved multiline composer behavior is retained:

- ordinary text entry supports shell commands, scripts, and pipelines;
- the editor expands vertically to a comfortable bounded editing size;
- additional editor content scrolls within the **composer editor only** when necessary; this does not create output/Block nested scrolling;
- `Shift+Return` is the intended default for newline;
- a dedicated execute shortcut/action submits the composed command; exact final keybindings remain configurable/documented by the input spec.

### 10.2 Minimal visible controls

Keep the default composer visually minimal.

Capabilities may include:

- execute;
- history fuzzy search;
- agent invocation;
- product/action invocation;
- shell/context affordance only where the user can actually change it.

Do not permanently display cwd, shell selector, utility buttons, or agent/action chips if the same information is already clear from Pane context and the control has no immediate functional need.

### 10.3 Helper surfaces

History/Agents/Actions open contextually above the focused Pane composer. They are not permanently visible panels.

History behavior is defined in `M001-COMPOSER-HISTORY-FUZZY-SEARCH.md`.

## 11. TUI / alternate-screen takeover

Full-screen TUIs such as Vim/Neovim/htop/tmux use the same `TerminalExecution` and canonical terminal state.

During canonical TUI takeover:

- the application surface expands to the Pane viewport;
- normal Block chrome is not drawn over the TUI;
- the Pane composer is hidden/disabled;
- input goes through the terminal input path directly to the application;
- no client-only fake TUI exit is allowed;
- Pane scrolling is not used to emulate TUI scrolling; the application/terminal mode owns its interaction.

When canonical terminal state exits TUI/alternate-screen mode, Seyal returns to the normal Block/transcript presentation and restores the Pane composer where the shell is available.

See `M001-TUI-TAKEOVER.md`.

## 12. Multipane

A tab may contain one or more panes in a nested split tree.

Rules:

- each terminal Pane has its own `TerminalExecution` and own composer state;
- only one Pane is focused for primary keyboard input at a time;
- focus determines composer/input target, split target, and default inspector context;
- a busy/TUI Pane may have its composer retracted/disabled while other panes remain fully usable;
- do not show a deeply nested pane tree in the left sidebar by default;
- indicate pane count/activity compactly and expose a pane navigator only when useful.

See `M001-MULTIPANE-VIEW.md`.

## 13. Inspector

Inspector context resolves in this priority order:

```text
explicit selection
→ focused object
→ active Pane
→ active Tab
→ active Workspace
```

For a focused terminal Pane, useful data includes:

- Workspace/path/repo context;
- active Tab/Pane identity;
- shell/cwd/execution state;
- foreground process name, PID/PPID, duration, CPU/memory where available;
- process tree;
- backed-by-real-data resources.

### 13.1 Inspector modes

The earlier approved right-side utility concepts are retained as **contextual inspector modes**, not a permanently duplicated utility sidebar.

Potential documented modes:

- Context / Info;
- Process / Runtime;
- Files / Diff / Artifacts;
- Activity / Attention / History.

Each mode/icon must have a real data contract before shipping.

Do not repeat the full agent inventory here. If an agent is selected, show that agent's details only.

## 14. Sessions / Agents / Resources views

Dedicated views are defined separately:

- `M001-SESSIONS-VIEW.md`
- `M001-AGENTS-VIEW.md`
- `M001-RESOURCES-VIEW.md`

They must reuse the same Workspace/Tab/Pane identities and must not create terminal authority merely by opening an inventory view.

## 15. Functional-only UI rule

Every visible region, icon, button, metric, status, and panel must have:

- real product behavior;
- a real backing state/data source;
- or an accepted implementation issue/spec.

Forbidden:

- fake buttons for balance;
- duplicate navigation purely for symmetry;
- placeholder metrics presented as production state;
- always-visible controls with no actionable purpose;
- nested output scroll regions introduced only to make a mockup fit.

## 16. Reconciliation with earlier approved mockups

Older design explorations are not being committed as visual authority, but useful non-conflicting interaction ideas are preserved here.

Retained:

- multiline Pane composer;
- keyboard-first global command palette;
- adaptive/scrollable top tab strip;
- current-Workspace Workspaces/Agents/Tabs context;
- runtime/connection status when meaningful;
- pane-scoped composers in multipane;
- notification/agent-attention popover;
- right-side contextual Files/Activity/Process-style inspector modes;
- Block-based command/output presentation.

Explicitly superseded:

- global/shared composer;
- fixed maximum Block height with internal output scrolling;
- a long-running Node/log process scrolling inside its own nested output box;
- showing an active composer as usable while its shell is occupied;
- duplicate full agent inventories in both left and right sidebars;
- decorative pane/layout/action controls without implementation semantics.

## 17. Companion state specifications

- `M001-SESSIONS-VIEW.md`
- `M001-AGENTS-VIEW.md`
- `M001-RESOURCES-VIEW.md`
- `M001-MULTIPANE-VIEW.md`
- `M001-NOTIFICATIONS-ATTENTION-POPOVER.md`
- `M001-TUI-TAKEOVER.md`
- `M001-BLOCK-DETAILS-INSPECTOR.md`
- `M001-COMPOSER-HISTORY-FUZZY-SEARCH.md`
- `M001-LIVE-TAIL-BEHAVIOR.md`

`M001-CORE-TERMINAL-REFERENCE-INDEX.md` maps the complete set.

Generated mockups are visual references; where a mockup conflicts with these functional rules or accepted terminal architecture, the specifications are authoritative.

## 18. Authority boundary

This reference freezes visual/information architecture only.

It does not override:

- M001 pass ordering;
- accepted Runtime/TerminalExecution/TerminalState ownership;
- Pass-specific implementation authority;
- terminal correctness/performance requirements.
