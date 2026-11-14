# M001 UI Shell Scaffold Boundary

> Historical M001 authority: [`M001-FLOW-BLOCKS-DEFAULT-AND-TUI-TAKEOVER.md`](M001-FLOW-BLOCKS-DEFAULT-AND-TUI-TAKEOVER.md)
> established Flow/Blocks as the production default.
>
> **Proposed #858 supersession:** any wording in this scaffold that can be read
> as a permanent visible/focusable terminal surface underneath Flow is
> superseded by ADR-009/SPEC-008 on merge. In corrected Flow, the Pane transcript
> and composer are the user-visible interaction surface and terminal-derived
> pixels are composed only into Block output regions. Raw/TUI are mutually
> exclusive full-Pane presentations. A direct-terminal input path requires the
> Pane to transition to Raw/TUI first, with the source first-responder/IME/mouse/
> input route revoked before the destination route is enabled.

**Status:** Implementation boundary for the pre-Pass-6 native shell scaffold; presentation target is historical/subordinate to ADR-009/SPEC-008
**Authority:** subordinate to `M001-CORE-TERMINAL-REFERENCE-SCREEN.md`, `SEYAL-UI-ARCHITECTURE-001.md`, accepted ADRs/specs, M001 pass ordering, and accepted Runtime/TerminalExecution ownership

## Purpose

Allow native macOS shell/component work to proceed without pretending that Pass 6 renderer or live Runtime integration is complete.

The scaffold may establish:

- shared native design tokens;
- the Core Terminal shell regions;
- a data-driven Block presentation container;
- one Pane-owned transcript scroll surface;
- a Pane-scoped composer presentation seam;
- contextual left panel and inspector presentation seams;
- attention-popover presentation seam;
- native AppKit menu/key-equivalent routing for presentation-only Workspace/Tab/window navigation;
- transient Command-hold shortcut-discovery overlays that do not alter layout;
- the permanent Metal renderer/presenter integration seam without requiring it to be a visible/focusable Raw terminal viewport during Flow;
- deterministic preview fixtures behind an explicit preview-only launch path.

## Hard boundary

This scaffold must not implement or invent:

- Pass 6 terminal rendering;
- live Runtime attachment;
- an alternate PTY, VT, grid, terminal model, or copied terminal authority;
- per-cell Swift/Rust callbacks;
- a temporary `NSTextView`/SwiftUI terminal renderer;
- shell integration or command intelligence;
- agent approval mutations;
- production notification state;
- terminal-input keybinding behavior that forwards or transforms PTY/TUI input;
- multiple-pane Runtime semantics.

The production application path is the Flow/Blocks shell when trusted structured
presentation is safe. Metal remains the permanent renderer technology, but the
renderer is not required to be a conventional terminal viewport in Flow. Under
the proposed #858 correction, full-Pane direct terminal interaction belongs only
to Raw/TUI presentation. A fixture-only preview is not a production fallback and
must not remain the default launch path.

## Frozen visual contract

The shell preview must follow the frozen Core Terminal visual direction together with the current functional specifications. The visual target is a dense, dark developer workspace designed for a 15-inch display, not a generic AppKit form using adaptive system colors.

At the canonical `1280x800` preview size:

- the workspace-scoped tab strip spans the full content width beneath native window chrome;
- a compact left context panel occupies approximately 236 points;
- the left panel uses a **Workspaces / Tabs** switcher instead of stacking both inventories permanently;
- **Workspaces** shows all known Workspaces plus agent sessions for the active Workspace;
- **Tabs** shows the active Workspace's tab inventory and new-tab action;
- the focused terminal Pane owns the dominant center width;
- a contextual Inspector occupies approximately 292 points on the right and is pinned to the content trailing edge with no unused strip beside it;
- the Inspector retains the frozen design's slim vertical tool rail on its far-right edge;
- the Inspector rail is functional rather than decorative: current preview modes filter existing context into **Context**, **Workspace**, **Tab**, and **Pane** views only;
- both the left context panel and right Inspector can be hidden and reopened; when either is hidden the center Pane reclaims that width rather than preserving an empty gutter;
- left-pane hide/reopen must be immediate on press (no input delay/lockout) and must cancel/redirect any in-flight emphasis preview without changing Workspace/Tab/Pane selection state;
- reduced-motion must be respected for any emphasis/visibility state changes associated with side-panel collapse/expand;
- persistent top-chrome toggles reopen a hidden side panel, while each visible side panel also exposes its own collapse control;
- each Pane has compact, functional Pane-local split and close controls in its header;
- the Pane contains its own minimal composer at the bottom;
- the palette is low-contrast charcoal/navy with restrained purple focus and semantic status accents;
- typography is dense and terminal output remains monospaced;
- unsupported process/resource/history inspector modes from concept art remain omitted until backed by accepted behavior/data.

Where visual concept art conflicts with the current Block sizing, busy-composer, TUI takeover, ownership, or functional-only rules, the current specifications win.

The XCTest layout contract and XCUIAutomation suite are part of this visual contract. XCUI launches the actual `Seyal.app` preview, asserts the visible hierarchy and left-center-right ordering, exercises the Workspaces/Tabs switcher, Inspector rail, side-panel hide/reopen behavior, native navigation shortcuts, hierarchical close behavior, shortcut-hint presentation, and Pane-local lifecycle controls, and records a rendered screenshot in the test result bundle for design review. A true pixel-golden comparison must not be claimed until the approved source reference image is deliberately added to the repository as a test asset.

## Preview-only path

The UI shell may be launched explicitly for design/decomposition review using a debug-only preview path. Preview fixtures:

- are deterministic;
- may model disposable Workspace/Tab/Pane/focus/draft interaction state;
- are never Runtime authority;
- must not fabricate PTY/process/terminal/resource telemetry;
- must not be reused as terminal output/state storage.

The normal preview therefore leaves renderer/Runtime terminal authority unattached rather than filling it with fake command output.

## Workspace and Tab navigation

ADR-029's accepted sidebar lists open workspaces in one window and switches that window's pane tree in place. That replacement is implementation authority only after SPEC-032 is accepted. Until then the model below remains in force.

The frozen compact navigation model is:

```text
left context
├── Workspaces
│   ├── Workspace inventory
│   └── active-Workspace agent sessions
└── Tabs
    └── active-Workspace tab inventory
```

The same Tab identity is used by the top strip and the left Tabs view. Switching Workspace replaces the Workspace-scoped tab inventory rather than showing tabs from multiple Workspaces together.

The left context panel is presentation chrome, not execution authority. Hiding it must not change Workspace, Tab, Pane, draft, focus, or execution identity. Reopening it restores the same navigation state.

## Native navigation shortcuts

macOS shell navigation uses native `NSMenuItem` key equivalents rather than `keyDown` interception. Shortcuts therefore remain discoverable in the menu bar, participate in normal AppKit command routing/accessibility, and do not consume plain Control/Option terminal input that a future PTY/TUI path may require.

The preview shortcut contract is:

| Scope | Previous / Next | Direct selection |
| --- | --- | --- |
| Workspace | `⌃⌘[` / `⌃⌘]` | `⌃⌘1` … `⌃⌘9` |
| Tab | `⇧⌘[` / `⇧⌘]` | `⌘1` … `⌘9` |
| Window | `⇧⌘\`` / `⌘\`` | `⌥⌘1` … `⌥⌘9` |

Additional native UI commands:

- `⌘T` creates a preview Tab;
- `⌘W` closes the focused context from the inside out: if the active Tab has more than one Pane it closes the focused Pane; once one Pane remains it closes the active Tab if another Tab exists; once the Window contains one Tab with one Pane it closes the Window;
- `⌘0` toggles the left navigation sidebar;
- `⌥⌘0` toggles Inspector.

A Tab never enters a zero-Pane state. Repeated `⌘W` therefore peels **Pane → Tab → Window** across invocations instead of bulk-deleting panes or bypassing the active Pane.

`⌘1…9` for Tabs and `⌥⌘1…9` for windows intentionally follow established macOS terminal conventions. Workspace shortcuts occupy a separate Control+Command layer so Workspace navigation never conflicts with Tab/window numbering.

Window switching is presentation-level AppKit behavior over existing visible titled windows. It does not create or own Runtime sessions. Keyboard navigation must route through the same shell state/actions as mouse navigation; it must not create a duplicate Workspace/Tab model or reconstruct the shell in a way that loses Pane drafts, focus, Inspector mode, or sidebar visibility.

## Command-hold shortcut discovery

Holding **Command only** intentionally for 300 ms shows compact shortcut labels over currently reachable controls. This is a transient discovery layer, not another toolbar or navigation model.

Rules:

- hints are scoped to the current key Window;
- the 300 ms delay prevents normal Command chords from flashing the overlay;
- releasing Command hides the hints;
- pressing any non-modifier key before or after presentation cancels/hides them;
- adding Shift, Option, or Control to the held Command cancels the Command-only discovery state;
- app deactivation or key-window loss cancels/hides the hints;
- the overlay is hit-test transparent and must not alter Auto Layout or move any shell content;
- only controls backed by real shortcuts receive hints;
- the labels reflect the current shortcut map dynamically, including numbered Workspaces/Tabs, `⌘T`, `⌘W`, `⌘0`, and `⌥⌘0` where their target is visible.

The discovery overlay may observe local key/modifier events only to determine visibility and must return those events unchanged. Actual shortcut execution remains native `NSMenuItem` key-equivalent routing.

## Inspector navigation

The frozen Inspector includes a narrow vertical mode rail on the far-right edge. The rail switches the detail surface immediately to its left.

For the pre-Pass-6 preview, only modes backed by existing deterministic UI context are allowed:

- **Context** — all currently available Inspector context;
- **Workspace** — Workspace rows only;
- **Tab** — active Tab rows only;
- **Pane** — focused Pane rows only.

No process, resource, metrics, history, or other Runtime-dependent Inspector item may appear merely to match concept-art icons. Those modes are introduced only after accepted data ownership and behavior exist.

Hiding the Inspector hides the detail surface and its mode rail together. Its top-chrome toggle remains available to reopen it, and the selected Inspector mode is retained across hide/reopen operations.

## Pane-local layout controls

The top layout controls continue to target the focused Pane. Every Pane also exposes compact local controls so multipane layouts remain understandable without relying on implicit focus:

- **Split** opens Split Right / Split Down for that Pane;
- **Close Pane** removes that Pane when the Tab has more than one Pane;
- the last Pane cannot be closed through the preview UI;
- closing a Pane collapses its parent split and preserves surviving Pane draft/focus state.

These controls manipulate preview layout metadata only. They do not create a PTY or claim Runtime resize/lifecycle authority.

## Scroll ownership

Normal transcript mode has one output scroll owner per Pane.

`BlockView` must not own an `NSScrollView` or another nested output-scroll surface. Blocks size intrinsically from their presented body. The Pane transcript may scroll and later virtualize off-screen content without changing this interaction model.

The Pane-local multiline composer may use its own bounded editor scroll surface as allowed by the composer specification; that editor is not terminal output scrolling.

## Block ownership

`BlockView` accepts a presentation model and a body view. It does not own terminal cells, VT state, PTY state, execution lifecycle, or a copied output transcript.

The renderer can provide terminal-derived Block output without changing Block ownership or exposing a full-Pane Raw viewport in Flow.

## TUI / Raw seam

The shell architecture must allow a mutually exclusive presentation change for the
same execution: normal Flow transcript, full-Pane Raw direct-terminal fallback,
or canonical full-Pane TUI takeover. The scaffold does not implement the final
state machine itself.

A transition must release the previous mode's first responder, IME/preedit and
mouse/input route before the destination route can admit input. Same-execution
continuity does not require one visible/focusable AppKit terminal view to remain
installed under every presentation.

## Functional-only rule

Production UI must not show unsupported controls or fabricated metrics merely because they exist in a mockup. Preview interaction controls are acceptable only where they manipulate real preview UI state and are covered by tests. Runtime-dependent controls remain hidden until the Runtime contract exists.

## Acceptance

The scaffold is acceptable only if:

- default M001 app behavior is unchanged unless superseded by accepted later architecture/specification;
- the shell is native Swift/AppKit;
- Metal remains the permanent terminal rendering technology;
- no SwiftUI terminal implementation exists;
- `NSTextView` is used only for the Pane-local multiline composer and never as a terminal renderer;
- `BlockView` contains no nested output scroll view;
- one Pane transcript scroll surface owns normal output navigation per Pane;
- Flow does not expose a coexisting focusable/click-through Raw terminal viewport;
- deterministic shell construction participates in the native smoke test;
- the canonical `1280x800` preview satisfies the frozen three-column layout contract with Inspector flush to the trailing edge;
- the Workspaces/Tabs switcher follows the frozen compact navigation model;
- native Workspace/Tab/window shortcuts use AppKit menu key equivalents and route through the same preview state/actions as pointer navigation;
- `⌘W` follows the explicit focused Pane → active Tab → Window close sequence without producing a zero-Pane Tab;
- a Command-only 300 ms hold exposes hit-test-transparent, no-layout-shift shortcut labels and cancels on key/modifier/focus loss;
- shortcut routing preserves existing shell presentation state such as sidebar visibility rather than replacing the shell with a second state owner;
- the Inspector exposes the frozen right-edge navigation rail using only functional modes backed by current preview context;
- left context and Inspector hide/reopen controls are functional and the center Pane reclaims hidden side-panel width;
- each Pane's split and close controls mutate the intended Pane's preview layout state;
- XCUIAutomation launches the real preview and validates those interactions;
- a rendered screenshot is attached to the XCUI result for design review;
- all existing repository tests/checks remain green.
