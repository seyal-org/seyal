# Seyal Universal Component Contract

**Status:** Proposed visual/component authority  
**Parent:** `SEYAL-ADAPTIVE-DEPTH-DESIGN-LANGUAGE.md`  
**Scope:** Shared first-party UI components used by all Seyal reference screens

## 1. Purpose

This document prevents screen-by-screen visual drift.

Every Seyal reference screen must be composed from the same component contracts. A screen may change a component's **state, content, visibility, or density variant** only where this document explicitly allows it. It must not redesign that component for a particular mockup.

Historical screenshots under `docs/architecture/ui/references/` remain functional-coverage inputs. They are not permission to create nine unrelated visual systems.

## 2. Universal visual rule

Seyal uses:

- **Zero-Chrome at rest** — structural UI should disappear until it carries meaning;
- **Adaptive Depth** — material appears in proportion to operational relevance;
- **Semantic Seams** — boundaries encode execution/context/state rather than decorative cards;
- **Focus Gravity** — the active object becomes perceptually strongest without geometry movement;
- **Terminal Truth** — terminal/TUI content stays crisp and visually authoritative.

The product should feel like one continuous operational canvas, not a collection of cards and dashboards.

## 3. Cross-screen invariants

1. The same component keeps the same visual grammar in every screen and theme.
2. Light and dark themes change tokens, not structure.
3. Terminal content does not become translucent to create a glass effect.
4. Normal Blocks are not rounded cards.
5. Pane boundaries are seams, not framed windows.
6. Frost is reserved for utility/editing/overlay surfaces.
7. Attention uses semantic colour only while attention exists.
8. No gamer/neon/glow styling.
9. No component may add synchronous work to terminal I/O, VT mutation, damage tracking, or rendering.
10. Any component rendered in a reference image must map to a real behavior/data contract or an explicitly proposed capability.

## 4. Shared tokens

Tokens are semantic, not platform-material names.

### Surface roles

- `surface.truth` — terminal transcript/grid/TUI; effectively opaque.
- `surface.utility.receded` — sidebar/Inspector at rest; low-contrast material.
- `surface.utility.active` — focused composer/Inspector/helper surface.
- `surface.overlay` — popover/search/attention stack.
- `surface.attention` — temporary attention emphasis; never a permanent full panel fill.

### Seam roles

- `seam.rest` — barely visible structural separator.
- `seam.hover` — modest contrast increase.
- `seam.focus` — focused object relationship.
- `seam.running` — compact running state marker where useful.
- `seam.attention` — warning/failure/approval relationship.

### Text roles

- `text.primary`
- `text.secondary`
- `text.muted`
- `text.terminal` — terminal-owned rendering/theme.
- `text.attention`

### State colour roles

- `state.focus`
- `state.success`
- `state.warning`
- `state.danger`
- `state.info`
- `state.agent-active`

Do not assign provider-specific decorative colours as a universal identity system.

## 5. C01 — UI Container

Purpose: top-level application framing.

Contract:

- native window frame/chrome remains minimal;
- application content reaches close to the frame edges;
- no large permanent toolbar band;
- global actions use compact icons/keyboard-first commands;
- healthy local runtime state is visually quiet;
- remote/degraded/attention state may promote a compact indicator.

Allowed state variation: platform window controls, light/dark tokens, compact/fullscreen geometry.

Forbidden variation: screen-specific header redesigns.

## 6. C02 — Global Utility Rail

Purpose: stable entry points for major non-terminal views and settings.

Contract:

- narrow vertical rail when present;
- icon-first with accessible labels/tooltips;
- selected state uses one restrained focus treatment;
- notification/attention entry may show a compact count/dot;
- no decorative grouping cards.

The same rail treatment is used on Sessions, Agents, Resources and Core Terminal screens.

## 7. C03 — Left Context Panel

Purpose: current work context adjacent to the terminal/work surface.

Contract:

- D1/receded material at rest;
- optional subtle frost/transparency;
- dense lists, not cards;
- typography/alignment carry hierarchy;
- panel gains only slight D2 definition when actively navigated;
- may collapse to a thin rail without changing the content model.

Sections may include Workspaces, Sessions, Agents, Tabs or source navigation according to screen scope.

## 8. C04 — Context Row

Used for Workspace, Session, Agent, Tab or Resource rows.

Fixed anatomy:

```text
[state glyph] primary label                    [optional compact meta]
              secondary context
```

Contract:

- no rounded card per row;
- one-row or two-line dense variant;
- hover exposes secondary actions;
- selected row uses restrained tint/edge/typographic emphasis;
- attention state may add one semantic marker without recolouring the whole row.

Interaction contract (must match `/apple-design` feel):
- kill latency: active emphasis must update on pointer-down (press), not only on click/release completion.
- direct-manipulation selection: pointer-up commits the corresponding selection only if the pointer is still within the activation region/hysteresis.
- cancel-by-dragging-away: if the pointer drags away beyond the activation hysteresis before pointer-up, cancel the commit and return the row to the last committed selection emphasis.
- interruptibility: emphasis preview must be cancelable/redirectable at any time (for example, if the side panel collapses/hides mid-press or focus changes) without input being locked out.
- reduced-motion: selection emphasis changes must respect reduced-motion preferences (no slides, no delayed motion); prefer an instant or short cross-fade of emphasis state only when motion is allowed.

## 9. C05 — Top Tab Strip

Purpose: Workspace-scoped Tab navigation.

Contract:

- one row only;
- active Tab stays visible;
- tabs compress to documented minimum then scroll/overflow;
- active state primarily typography + thin seam/indicator;
- no individual pill cards;
- `+` and layout actions remain compact;
- same geometry in single-pane and multipane screens.

Interaction contract (must match `/apple-design` feel):
- kill latency: active emphasis must update on pointer-down, not only on click/release completion.
- commit/cancel semantics: pointer-up commits the active Tab only if within activation hysteresis; dragging away before release cancels the commit and returns to the last committed active Tab.
- interruptibility: switching tabs must not lock input; in-flight emphasis previews must be cancellable/redirectable immediately.
- reduced-motion: emphasis updates must respect reduced-motion preferences (no animated movement that delays input); keep transitions confined to short opacity/typography/seam emphasis changes when motion is allowed.

## 10. C06 — Terminal Pane

Purpose: presentation viewport for one terminal execution.

Contract:

- terminal canvas is `surface.truth`;
- no permanent rounded outer card;
- focused Pane is communicated by `seam.focus` and subtle chrome gravity, never by reducing terminal-text clarity in other panes;
- Pane-local chrome is minimal and appears only when needed;
- a terminal Pane owns its own composer state;
- TUI takeover replaces normal Block/composer presentation inside the same Pane.

## 11. C07 — Semantic Block

Purpose: durable command/execution presentation over real terminal execution.

Fixed anatomy:

```text
command / prompt        status · duration · optional actions
terminal output
semantic seam
```

Contract:

- default Block has no card background and no persistent shadow;
- adjacent Blocks read as one transcript;
- Block identity comes from command header + semantic seam + spacing rhythm;
- actions appear on hover/focus/keyboard invocation;
- selected Block may gain slight D1/D2 definition without becoming a heavy card;
- failed/attention Block promotes only its seam/status, not its entire background;
- Blocks grow intrinsically; Pane owns transcript scrolling.

Block actions retain the same order/placement across screens: Copy, Rerun, Pin, Expand/Inspect, Overflow where implemented.

## 12. C08 — Semantic Seam

Purpose: Seyal's primary visual signature.

A seam is both separator and state carrier.

Contract:

- thin and low contrast at rest;
- can expose compact execution metadata/actions when the related object is focused/hovered;
- can communicate focus, running or attention through restrained token change;
- never glows or pulses;
- never becomes a thick decorative border.

The same seam grammar is reused for Block boundaries, Pane splits, Inspector relationships, agent relationships and attention edges.

## 13. C09 — Pane Composer

Purpose: pane-scoped multiline command editing.

This component is **identical in every terminal screen**.

Detailed authority and interaction rules are defined by `SEYAL-COMPOSER-COMPONENT-SPEC.md`.

Fixed placement: bottom edge of its owning available terminal Pane.

Fixed anatomy:

```text
[optional cwd] [optional git branch ▼]          [compact context/state]
[input/editor]                                 [contextual actions] [execute]
```

Contract:

- D1 at rest, D2 when editing;
- restrained frost/transparency is allowed;
- modest radius; not a floating oversized pill/card;
- same horizontal padding, height rhythm and icon placement everywhere;
- shell prompt rendering remains shell/terminal truth and is never reconstructed by Composer;
- optional cwd/Git affordances are Seyal UI, not a second prompt;
- composer state, eligibility and command construction/submission are Rust-owned (ADR-015 / ADR-009); native hosts render snapshots and forward typed actions only;
- cwd/Git chips appear only from pane/execution-scoped metadata, never by parsing arbitrary prompt text; untrusted cwd (for example OSC 7) is display-only;
- Git branch chip may open a keyboard-first branch helper only when the repository comes from a trusted cwd source; a switch is a Rust composer submission under ADR-009 after revalidation and leaves the draft untouched (see `SEYAL-COMPOSER-COMPONENT-SPEC.md`, Proposed);
- no auto-stash/reset/force checkout behavior;
- shell-aware completion may be surfaced only by delegating to a supported shell/completion integration; history/Agents/Actions remain separate Seyal suggestion modes;
- auto-expands vertically for multiline editing;
- only composer editor may internally scroll when draft is very tall;
- retracts/disables while shell foreground execution owns input;
- hidden during full-screen TUI takeover;
- helper surfaces open above this same component;
- context discovery/completion/Git work remains outside PTY/VT/render hot paths.

Allowed content variation: draft text, cwd/Git availability, dirty/read-only state, disabled/running guidance, helper mode, and enabled actions backed by capability.

Forbidden variation: different composer design per screen/pane, fake prompt reconstruction, prompt scraping as authority, or local Git state presented as remote execution truth.

## 14. C10 — Inspector

Purpose: persistent contextual value-add surface.

Fixed placement: right side when open.

Contract:

- D1 at rest, D2 when user focuses/explicitly selects inspected content;
- restrained frosted/tonal material;
- modes use a consistent top selector/tab treatment;
- sections use typography + seams, not nested cards;
- context resolution follows explicit selection → focused object → active Pane → active Tab → active Workspace;
- the full agent inventory is not duplicated here;
- width family is stable across Core Terminal, Multipane and management screens.

Allowed content variation: selected object and mode.

Forbidden variation: turning Inspector into a different dashboard per screen.

## 15. C11 — Attention Stack

Purpose: actionable global events without forced navigation.

Contract:

- anchored from the stable attention entry point;
- overlay/frosted surface, not a permanent fourth column;
- items are dense and vertically stacked;
- each item shows only source, concise context, time/state and required actions;
- actionable item gets `seam.attention`/semantic accent;
- read and resolved are distinct;
- stack geometry and item anatomy remain identical across screens.

## 16. C12 — Search / Command Surface

Purpose: global cross-product navigation/actions and pane-local history helper.

Shared visual grammar:

- frosted/tonal overlay;
- one search field;
- compact scope selector;
- dense rows;
- keyboard selection indicator;
- result metadata aligned consistently;
- no grid of result cards.

Global palette and pane-history helper share component language but not data semantics.

## 17. C13 — Split Divider

Purpose: multipane geometry boundary.

Contract:

- uses `seam.rest` at rest;
- `seam.hover` near pointer/resize interaction;
- focused Pane relationship may use `seam.focus` on the relevant edge;
- no thick border around each Pane;
- no rounded Pane container.

## 18. C14 — TUI Takeover Surface

Purpose: direct full-Pane terminal application presentation.

Contract:

- same Terminal Pane and terminal authority;
- TUI occupies available Pane viewport;
- Block seams/header disappear over the TUI;
- composer disappears;
- utility panes outside the terminal Pane may remain visible if layout permits;
- no decorative `TUI mode` banner consuming permanent terminal area.

## 19. C15 — Management Data Surface

Used by Sessions, Agents and Resources dedicated views.

Contract:

- dense table/list/inspection layout;
- flat rows and typography-led grouping;
- no SaaS-style card grid as default;
- selected row can project details into Inspector or adjacent details surface using the same Inspector grammar;
- tables/charts/metrics appear only when real data exists;
- empty space stays empty rather than being filled with decorative cards.

## 20. C16 — Status / State Indicator

Contract:

- small semantic glyph/dot/text;
- never colour-only;
- state vocabulary must map to real runtime/agent/execution state;
- same shape and placement rules across left rows, Blocks, Sessions and Agents;
- attention may promote visibility temporarily.

## 21. C17 — Action Button

Contract:

- buttons are rare in the primary terminal workspace;
- neutral actions use low-material text/icon treatment;
- primary/destructive controls appear mainly in overlays or management details when action consequence warrants it;
- same height/radius/typography across Approve, Reject, Reconnect, Terminate and similar actions;
- destructive action is visually distinct but not oversized.

## 22. Theme parity

Dark and light themes use identical component geometry and state logic.

Dark:

- truth surface remains deep neutral, not coloured gaming black/blue;
- frosted surfaces use restrained luminance separation;
- semantic colours stay low-saturation except attention.

Light:

- truth surface remains crisp and readable;
- utility surfaces use slight tonal/frost separation rather than grey card stacks;
- borders remain minimal.

## 23. Zero-Chrome acceptance test

A reference screen fails if, at rest:

- every region has a visible container;
- every Block looks independently boxed;
- every Pane has a full border/header;
- the Inspector is more visually prominent than the terminal without explicit user focus;
- the composer looks like a floating SaaS input widget;
- decorative frost/blur is visible everywhere;
- the screen still looks like a generic modern terminal after labels are removed.

A screen passes when the terminal/work content appears primary and the Seyal UI becomes visible mainly through semantic seams, focus, context and action.
