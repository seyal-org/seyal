# Historical UI visual references

The images in this directory are **historical functional-design inputs**, not current visual implementation authority.

The current Core Terminal direction is defined by:

- `../M001-CORE-TERMINAL-REFERENCE-SCREEN.md`
- `../M001-CORE-TERMINAL-REFERENCE-INDEX.md`
- `../SEYAL-ADAPTIVE-DEPTH-DESIGN-LANGUAGE.md`
- `../SEYAL-UNIVERSAL-COMPONENT-CONTRACT.md`
- `../SEYAL-REFERENCE-SCREEN-CONTRACTS.md`
- the companion state specifications linked from the index

Product constitution, accepted architecture/ADRs/specifications, milestone contracts, terminal correctness, and pass ordering remain higher authority than any mockup.

## Why these references remain

The nine historical images are retained because together they cover important product states that must not be lost during visual redesign:

1. `1-full view terminal.png` — Core Terminal, Blocks, navigation and Inspector composition.
2. `2-session dashboard.png` — session inventory/reconnect management.
3. `3-agent dashboard.png` — cross-workspace agent inventory/orchestration.
4. `4-metrics dashboard.png` — resources/operations/metrics concepts.
5. `5-multi pane view.png` — multipane composition.
6. `6-notifications.png` — global attention/notification stack.
7. `7-TUI.png` — full-screen TUI takeover.
8. `8-TUI within block.png` — historical TUI/Block relationship input; stale behavior is superseded by current TUI specs.
9. `9-search.png` — global search/command and command-history discovery concepts.

Do **not** implement these screenshots literally. Their useful non-conflicting capabilities are carried into the current textual specifications and the replacement reference contracts.

## Replacement reference program

New Adaptive Depth visual references must be generated for all nine states in both dark and light themes.

Target paths are defined in `../SEYAL-REFERENCE-SCREEN-CONTRACTS.md` under `references/adaptive-depth/`.

All replacements must use the same shared component kit from `../SEYAL-UNIVERSAL-COMPONENT-CONTRACT.md`.

In particular:

- the Pane composer must be the same component everywhere;
- top Tabs must retain one treatment everywhere;
- sidebar/context rows must retain one anatomy everywhere;
- Inspector width/section grammar must remain consistent;
- Blocks must use the same Semantic Block + Semantic Seam language;
- attention items must share one anatomy;
- split dividers must use the same Seam state grammar;
- light/dark variants change tokens, not geometry.

If a mockup generator produces a visually attractive screen by redesigning a shared component for that screen, reject the image.

## Retained capabilities from the earlier references

### Multiline Pane composer

Retained:

- one composer state per terminal Pane;
- multiline command/script/pipeline editing;
- `Shift+Return` as intended newline behavior;
- explicit execute action/shortcut;
- composer retracts/disables when the foreground shell cannot accept a new command;
- full-screen TUI takeover hides/disables the composer and uses the same `TerminalExecution`.

The current design intentionally keeps default composer chrome smaller than the old component mockup. cwd/shell/utility controls appear only when they are genuinely actionable and not redundant with Pane context.

### Workspace / Tab / Pane structure

Retained:

- multiple Workspaces;
- Workspace-scoped Tabs;
- Tab-owned nested Pane layout;
- one pane-local composer per available terminal Pane;
- adaptive/scrollable top Tab strip;
- Block-based normal command/output presentation.

### Global command palette

Retained as a keyboard-first global navigation/action surface. A permanent command-palette toolbar button is not required.

### Agent attention

Retained as the structured notification/attention model. Approval/question is one typed attention case among failures, completions, disconnects, policy/security decisions, and review-ready events.

### Right-side utility concepts

Earlier Activity/Files/Agent utility concepts are reconciled into the **contextual Inspector modes**:

- Context / Info;
- Process / Runtime;
- Files / Diff / Artifacts;
- Activity / Attention / History.

Do not duplicate the full agent inventory in both the left panel and inspector.

### Connection/runtime state

Retained as real product state. Healthy local state may be subtle; remote/detached/reconnecting/degraded state must be surfaced clearly.

## Explicitly superseded behavior

The following old-reference details are **not current authority**:

- fixed maximum Block height with internal output scrolling;
- nested scrolling inside a long-running Node/log Block;
- a global/shared command composer;
- a fully active composer while its foreground shell is occupied;
- permanent split-control clusters repeated inside every Pane;
- duplicate full agent lists in left and right sidebars;
- large utility/control rows added only for visual completeness;
- card-heavy visual framing;
- screen-specific component variations;
- neon/glow/gamer visual treatment.

### Current Block scrolling rule

Normal Block/transcript presentation has **one scroll owner: the Pane**.

- Blocks grow intrinsically with output;
- long-running normal-screen output grows its Running Block;
- users scroll the Pane to inspect earlier Blocks/output;
- no fixed max-height output box is introduced just to constrain command output;
- implementation may virtualize off-screen transcript content without exposing nested scroll regions.

### Current TUI rule

A full-screen alternate-screen TUI is not a growing Block with an internal scrollbar.

- the TUI takes over the Pane viewport;
- Block chrome yields;
- composer is hidden/disabled;
- canonical terminal/application input and scrolling semantics win;
- exiting canonical TUI state returns to normal Pane transcript presentation.

### Current split-control rule

Primary split/layout controls live with the active Tab/layout chrome and target the focused Pane. Pane context menus/keyboard shortcuts may expose equivalent actions, but permanent duplicate split controls are not required in every Pane header.

## Configuration and performance

- light and dark appearances remain first-class;
- shell themes own prompt/ANSI terminal-content appearance;
- application chrome/layout/composer settings use typed configuration;
- future optional Lua/customization requires separate capability/security authority;
- configuration parsing, semantic enrichment, agents, persistence, and UI metadata must never synchronously block PTY → VT → damage → render/input paths.

If an old screenshot conflicts with the current frozen specification, **the current specification wins**. Do not create a parallel implementation path to preserve the old mockup.

## Block component board

`block-component/seyal-block-component.webp` is the approved visual for the Seyal Block Component (#1010). Its design authority is `../M003-BLOCK-COMPONENT-DESIGN.md`, which normalizes the board into tokens and records intentional deviations.
