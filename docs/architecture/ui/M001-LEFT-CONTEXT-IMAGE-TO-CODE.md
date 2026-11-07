# M001 Left Context (C03 + C05) - Image-to-Code Implementation Spec

**Status:** Proposed implementation spec (doc-only)
**Scope:** Core Terminal left pane (`C03`) and shared Tab selection (`C05`).
**Primary authorities:**
- `M001-CORE-TERMINAL-REFERENCE-SCREEN.md` (left context panel information architecture)
- `SEYAL-UNIVERSAL-COMPONENT-CONTRACT.md` (`C03`, `C04`, `C05` visual + interaction contracts)
- `M001-UI-SHELL-SCAFFOLD.md` (native behavior, hide/reopen rules, geometry intent)

> Note on visual authority: the checked-in `01-core terminal.png` is a historical reference. This doc describes implementable contracts and code-aligned component inventory, not a pixel-golden completion claim.

## 0. Default chrome refinement — PR #1456

**Status:** Proposed product-approved default for M003, superseding the
historical always-visible left Workspaces/Tabs switcher and outer Workspace
Tab top strip described below.

- Default C03 shows **Workspaces**, **Layouts**, and backed **Agents**;
  the layout rows navigate the active Workspace's outer Rust `TabId` entries,
  including the sole initial layout.
- Do not render a permanent left **Tabs** mode control. The existing terminal
  inventory remains accessible on demand via the command palette; Workspaces
  can restore the default view.
- Every Pane retains one 30-point non-wrapping `TerminalTabId` strip, with
  quiet labels, selected 2-point seam, focused-pane actions, and stable close
  slots. Overflow must reveal the selected tab.
- No second terminal-looking outer strip is permitted. The Workspaces header
  offers **New Layout** (Rust `CreateTab`); each Pane offers **New Terminal**
  (Rust `NewTerminal`).
- Native rendered screenshots and interaction tests, not concept art, establish
  visual/behavioral acceptance. All unchanged sections below remain design
  background unless they conflict with this refined navigation decision.

## 1. Source-reference inventory (states)
This spec targets the “Reference 01 — Core Terminal / full terminal view” left-pane states:
- Left panel visible
  - Mode = `Workspaces` (Workspace inventory + compact current-Workspace agent inventory)
  - Mode = `Tabs` (active-Workspace tab inventory + `+ New Tab`)
- Left panel hidden/collapsed
- Top tab strip
  - Active tab highlight
  - Tab close affordance only when multiple tabs exist
  - `+` new-tab affordance

## 2. Forensic visual decomposition (component regions)
The left pane is composed from the shared component kit:

```text
LeftPane(C03)
├── Header
│   ├── Mode switcher (Workspaces <-> Tabs)
│   └── Collapse control (hide/reopen)
└── Content stack
    ├── Section titles (Workspaces / Agents / Tabs)
    ├── Context rows (C04)
    │   ├── Workspace row (primary name + secondary path, trailing tab count)
    │   ├── Agent row (primary provider name, trailing state label)
    │   └── Tab row (primary title, trailing pane count)
    └── `+ New Tab` action (button)

TopTabStrip(C05)
└── Horizontal tab chips + `+` new-tab action (scroll behavior supports overflow)
```

## 3. Measurement & token table (geometry that must stay stable)
Values below are implementation-aligned tokens currently used in the macOS shell scaffold. Production must preserve the stable geometry relationships, even if the exact underlying AppKit primitive changes.

| Token/Metric | Value | Contract intent |
|---|---:|---|
| `Layout.leftContextWidth` | `220` | Fixed left panel rail width when visible |
| Left panel preview target width | `~236` points @ `1280x800` | Ensures reference density matches screenshot program |
| Header padding (content stack edge insets) | `top/left/bottom/right = 12` | Keeps header + rows visually aligned |
| Header spacing | `4` | Tight adjacency between mode switcher and collapse |
| C04 row corner radius | `7` | Shared “row card” rounding (restrained) |
| C04 row emphasized background | `focusSoft` | D2 definition without heavy borders/cards |
| C05 tab chip corner radius | `7` | Matches row rounding language |
| C05 active accent line height | `2` | Thin focus indicator, not a thick border |

## 4. Component contracts + runtime-state mapping

### 4.1 C03 - Left Context Panel
**Purpose:** Work context adjacent to the terminal/work surface.

**Runtime inputs (authoritative sources):**
- `leftPanelMode`
  - `.workspaces`: render Workspaces rows + compact current-Workspace agent inventory
  - `.tabs`: render active-Workspace tab inventory
- Workspace list and active workspace identity
  - `workspaces`
  - `activeWorkspaceID`
- Agents list (current workspace only)
  - `activeWorkspace.agents`
- Terminal-tab list (active workspace only; pane-local ADR-025 identities)
  - Rust-projected `TerminalTab` rows for the active Workspace (grouped by Pane when helpful)
  - focused `PaneId` + that Pane's selected `TerminalTabId`
- Hide/collapse visibility
  - visibility toggle (`isLeftContextVisible` in the scaffold) must not affect focus/selection state.

### 4.2 C04 - Context Row (used for left Workspaces/Agents/Tabs rows)
**Purpose:** Dense, state-carrying list row for Workspace/Agent/Tab/Resource entities.

**Runtime mapping:**
- `primary` label: derived from row model (`Workspace.name`, `Agent.name`, `TerminalTab.title`)
- `secondary` label:
  - Workspace row: `Workspace.detail` when helpful
  - Agent row: omitted in the left list variant
  - Terminal-tab row: omitted in the left list variant (Pane grouping headers may carry a Pane label)
- `trailing` meta:
  - Workspace row: tab count string (`1 tab` vs `N tabs`)
  - Agent row: `agent.state.rawValue`
  - Terminal-tab row: optional compact status only when Rust projects it; do not invent pane-count as a substitute for terminal identity
- `attention` + status color:
  - Workspace row: uses `workspace.attention` for warning coloration
  - Agent row: uses `agent.state` for the warning/severity cue
  - Terminal-tab row: uses that terminal tab's attention when backed by real state
- `emphasized`:
  - Workspace row emphasized if `workspace.id == activeWorkspaceID`
  - Agent row emphasized if `agent.id == selectedAgentID`
  - Terminal-tab row emphasized if it is the selected `TerminalTabId` of the focused Pane

### 4.3 C05 - Top Tab Strip
**Purpose:** Workspace-scoped tab navigation.

**Runtime mapping:**
- Chip set: `snapshot.tabs` for the active workspace
- Active tab:
  - Active emphasis input: `tab.id == activeTabID`
- Close affordance:
  - Close control is hidden when there is only one tab (`tabs.count <= 1`)

### 4.4 Shared identity rule (C03 Tabs list <-> pane terminal strips)
Selecting a terminal tab in the left Tabs list or on a Pane strip updates the same Rust-owned `TerminalTabId` selection (and focuses the owning Pane when needed). The host must not create a second terminal-tab model. Outer Seyal Workspace `TabId` remains a distinct layout-container identity.

## 5. Interactions, focus paths, keyboard & accessibility

### 5.1 Pointer interaction semantics (must follow `/apple-design`)
Applies to `C04` rows and `C05` tab chips:
- kill latency: emphasis updates on pointer-down
- pointer-up commits selection only if still inside activation hysteresis
- drag-away cancels commit and returns to the last committed selection
- interruptibility: any in-flight emphasis preview must be cancelable immediately (including side-panel hide/reopen)
- reduced-motion: no animated movement that delays input; prefer instant or short cross-fade emphasis state changes

### 5.2 Keyboard / native shortcut expectations (scaffold contract)
- Workspace selection uses native macOS command-key equivalents (`M001-UI-SHELL-SCAFFOLD.md`).
- Tab selection uses `⌘1...⌘9` mapping for direct selection.
- Sidebar hide/reopen uses `⌘0` toggling, while Inspector uses `⌥⌘0`.

### 5.3 Accessibility identifier mapping (implementation-aligned)
These identifiers must remain stable so UI tests and accessibility tooling can target controls:

Left panel container controls:
- `left-mode` (mode switcher)
- `left-sidebar-collapse` (collapse/hide)

Left C04 rows + actions:
- Workspace row: `workspace.<workspaceID>`
- Agent row: `agent.<agentID>`
- Tab row: `left-tab.<tabID>`
- `+ New Tab` in left list: `left-new-tab`

Top C05 tab strip controls:
- Tab chip: `tab.<tabID>`
- Tab close: `tab.close.<tabID>`
- `+ New Tab` in top strip: `new-tab`

## 6. Visual states & transition rules

### 6.1 C04 row states
- Rest: emphasized=false, attention marker only when backed by real attention state.
- Hover: may expose secondary actions if the row model supports them.
- Press/preview: emphasized preview must appear immediately on pointer-down.
- Selected: emphasized=true (selected row emphasis), communicated via restrained tint/background + typography emphasis.

### 6.2 C05 tab chip states
- Rest: typography + thin seam emphasis only on active.
- Press/preview: active chip emphasis updates immediately on pointer-down.
- Selected: active chip shows elevated background + thin focus accent line.

### 6.3 Transition & reduced-motion
- side-panel show/hide and emphasis changes must not delay input
- when reduced-motion is enabled, emphasis changes must be instant or cross-fade without motion/slide.

## 7. Scrolling, clipping & layout boundaries
- Left panel content currently uses a vertical stack; if overflow occurs, behavior is implementation-defined but must not create nested output scrolling.
- Top tab strip uses horizontal overflow behavior; it must remain a single navigation row (no wrapping into multiple rows).

## 8. Intentional deviations allowed for production
This spec allows changing underlying AppKit primitives (NSButton vs custom view, segmented control vs radio group) as long as:
- contract-level semantics stay identical (C03/C04/C05)
- accessibility IDs remain stable
- the selection/press/cancel semantics match the interaction contract

## 9. Unknowns / open design decisions
- Exact numeric activation hysteresis for cancel-by-dragging (pixel distance or pointer-up bounds).
- Whether hover exposes secondary actions in the left pane today (C04 contract allows it; current scaffold may not implement extra actions).
- Exact emphasis transition duration/curve when motion is allowed (must remain short and non-delaying).
- Precise clipping/overflow behavior at very small window sizes.

## 10. Visual regression matrix (implementation QA checklist)
Capture at least:
- Dark + light theme: left mode = Workspaces, with one active workspace emphasized and at least one agent attention row
- Dark + light theme: left mode = Tabs, with one active tab emphasized
- Dark + light theme: left panel hidden/collapsed and center Pane width reclaimed
- Top tab strip:
  - multiple tabs: active + close visible
  - single tab: close hidden

## 11. Implementation dependency graph (order of work)
1. Verify component hierarchy and geometry relationships for `C03/C04/C05` (no extra columns/gutters; left remains thin/secondary).
2. Implement pointer-down emphasis + commit/cancel semantics for row/chip selection.
3. Wire selection to the single underlying identity per model (`activeWorkspaceID`, `selectedAgentID`, `activeTabID`).
4. Ensure accessibility identifiers remain stable.
5. Run interaction QA with reduced-motion enabled and validate no motion delays input.

