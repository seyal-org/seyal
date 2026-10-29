# M001 Multipane View

**Status:** Frozen UI reference specification  
**Parent:** `M001-CORE-TERMINAL-REFERENCE-SCREEN.md`  
**Scope:** Tab-owned split layout and per-Pane terminal interaction

## 1. Purpose

The Multipane view defines how one Workspace Tab presents multiple panes without losing terminal ownership, composer isolation, inspector context, or 15-inch power-user density.

## 2. Canonical ownership

```text
Workspace
└── Tab
    └── Split tree
        ├── Pane
        │   └── TerminalExecution / non-terminal surface
        └── Pane
            └── TerminalExecution / non-terminal surface
```

Rules:

- the Tab owns the split tree/layout;
- a terminal Pane owns at most one `TerminalExecution`;
- `TerminalExecution` owns one PTY/child/canonical terminal state;
- splitting never mirrors or duplicates a VT engine;
- non-terminal panes do not receive hidden PTYs merely because they exist.

## 3. Layout controls

Primary split/layout controls live in the active Tab's top layout chrome and target the focused Pane:

- split right;
- split down;
- supported layout selector/actions.

Pane-level context menus/keyboard shortcuts may expose equivalent actions when useful, but do not repeat permanent split-button clusters inside every Pane purely for appearance.

## 4. Pane chrome

Each Pane remains compact.

Useful context may include:

- execution/process title;
- cwd/path where needed;
- Running/Attention state;
- focused-state accent;
- minimal overflow actions.

Avoid repeating Workspace/global Tab navigation inside each Pane.

## 5. Per-Pane composer

Every terminal Pane owns an independent multiline composer state.

Therefore a 2x2 terminal layout has four composer states, one per terminal Pane.

Rules:

- focused Pane composer is interaction-dominant;
- inactive available composers may remain visible but subdued;
- drafts/history context remain pane-local;
- typing never routes to a different Pane because it was previously active.

### 5.1 Busy or TUI Pane

A Pane whose shell is occupied by a long-running foreground process must not present a misleading fully active composer.

- retract or disable that Pane's composer;
- preserve its draft;
- optionally show compact real running-process status;
- restore the composer when the shell becomes available.

**Contingent on acceptance of Proposed ADR-026 (#1384):** "busy" follows the ADR-026 §3.2 input target. That includes `TrustedShell(session)` (`Submit`) and a clearly labeled `Relay` for an eligible foreground program. Retraction remains correct when the input target is `None`. See `M001-LIVE-TAIL-BEHAVIOR.md` §6.

During full-screen TUI takeover, that Pane's composer is hidden/disabled.

Other panes remain independently usable.

## 6. Pane-level scrolling

For normal Block/transcript presentation, **each Pane owns one transcript scrollbar**.

- Blocks grow intrinsically with output;
- long-running normal-screen command output grows its Running Block;
- there is no fixed-height nested output scrollbar inside each Block;
- the user scrolls the Pane to navigate Blocks/output;
- implementation may virtualize off-screen content without exposing nested scroll regions.

During TUI takeover, application/terminal semantics own the Pane surface instead of the normal transcript scrolling model.

## 7. Block/raw/TUI coexistence

Different panes may simultaneously show:

- normal Blocks;
- long-running streaming output;
- raw terminal;
- full-screen TUI takeover;
- future non-terminal surfaces.

No Pane presentation changes another Pane's terminal authority.

## 8. Focus

Exactly one Pane receives primary keyboard focus at a time.

Focus determines:

- active composer/input target;
- split target;
- inspector default context;
- pane-scoped action target.

Mouse and keyboard focus navigation must update the same canonical UI focus state.

## 9. Inspector

The inspector follows the focused Pane unless the user explicitly selects another object.

Useful focused-terminal context includes:

- Pane identity;
- layout/focus state;
- shell/cwd;
- execution state;
- foreground process;
- real process/resource data.

Changing Pane focus updates inspector context without destroying any Pane state.

## 10. Left-panel pane tree

Do not render a deeply nested Pane tree in the left sidebar by default.

For 15-inch layouts:

- keep Workspaces, Agents, and Tabs stable;
- show Pane count/activity compactly where useful;
- expose a Pane navigator/tree only when invoked or complexity requires it.

## 11. Top tab overflow

Multipane must not force the top Workspace Tab strip to wrap vertically.

- keep active Tab visible;
- shrink to a documented minimum width;
- then scroll/overflow horizontally.

## 12. Resize

Dragging split boundaries changes Pane geometry.

For terminal panes, resize eventually flows through the authoritative Runtime/PTY/terminal-state transaction. UI must not claim geometry that Runtime/PTY rejected.

## 13. Non-terminal panes

Future panes may host agent/activity/diff/artifact/inspector-like surfaces without owning a PTY.

The layout model must explicitly distinguish terminal from non-terminal surfaces.

## 14. Performance

Multipane must scale without:

- one renderer thread/loop per Pane;
- duplicate terminal state;
- full-Pane redraws when damage is localized;
- synchronous cross-Pane coordination for terminal progress;
- nested output scroll containers that multiply layout work.

Focus/layout/composer metadata must never become a dependency of PTY → VT → canonical-state progress.
