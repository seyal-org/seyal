# Seyal UI Architecture — Foundation Direction

**Document:** SEYAL-UI-ARCHITECTURE-001
**Date:** 2026-08-23; presentation-mode clarification accepted 2026-09-11
**Status:** Foundation UI architecture; #858 / PR #859 (`8d08f2f`) presentation exclusivity accepted
**Authority:** Subordinate to [`../SEYAL-ARCH-FOUNDATION-RD-001.md`](../SEYAL-ARCH-FOUNDATION-RD-001.md)

This document defines the presentation architecture needed so Seyal can become a futuristic execution workspace without allowing UI work to compromise terminal correctness, persistence, memory, or latency.

---

## 1. Product principle

The UI should feel unlike a conventional terminal because it makes execution context, agents, approvals, history, artifacts, operations and remote machines coherent — not because it adds expensive decoration.

The terminal renderer retains priority over chrome, animation and auxiliary surfaces.

```text
Workspace
  ↓
Window / Tab / Split
  ↓
Flow terminal ─── Raw terminal ─── Live TUI
      │
      ├── virtualized Block history / skeleton
      ├── inline artifacts / diffs
      └── contextual inspectors

Global layer
  ├── Attention Stack
  ├── approvals / questions
  ├── command palette
  ├── search / navigation
  └── notifications
```

---

## 2. Presentation modes

For one terminal Pane, Flow, Raw and Live TUI are **mutually exclusive user-visible presentations** of the same `TerminalExecution`. One canonical terminal authority does not imply one permanently visible/focusable conventional terminal viewport.

A Pane may reuse one Metal compositor/renderer across modes for efficiency, but implementation reuse must not make Raw/TUI pixels or direct terminal input leak through Flow.

### 2.1 Flow mode

Flow mode presents normal shell activity as structured Blocks while preserving the underlying canonical terminal execution.

A Block may visually contain:

- prompt/command identity;
- output range;
- exit state;
- elapsed time;
- cwd/repository context;
- agent/activity association;
- artifacts/diffs/links;
- lightweight actions.

Flow mode does not imply a terminal grid or renderer per Block. A Pane-owned compositor may draw terminal-derived output into visible Block regions, but in Flow there is no independent full-Pane raw terminal viewport behind or beside the transcript. Empty Flow canvas is Seyal UI, not a click-through terminal input surface.

If character-level terminal semantics cannot safely be represented by the structured Flow contract, the Pane transitions to Raw before arbitrary direct terminal input is routed.

### 2.2 Raw mode

Raw mode presents the execution as a conventional full-Pane terminal viewport over canonical terminal state.

Use raw mode when:

- shell integration is absent or unreliable;
- arbitrary character-level terminal semantics are required outside the supported Flow contract;
- the user explicitly chooses it;
- terminal semantics are more important than structured presentation;
- debugging terminal behavior.

Flow and Raw are views of the same `ExecutionId`, but they are not simultaneously visible/interactive layers. Entering Raw yields Flow interaction; leaving Raw re-evaluates whether Flow is currently safe.

### 2.3 Live TUI mode

When canonical terminal state enters alternate screen or another TUI condition requiring full terminal ownership, the pane presents the terminal state full-area.

```text
same ExecutionId
same PTY
same VT
same alternate grid
→ different presentation
```

No Block wrapper may intercept mouse, cursor, keyboard, focus, resize or screen semantics required by the TUI. Flow/Raw presentation yields for the takeover; on exit, current eligibility determines whether the Pane returns to Flow or Raw.

Same-execution continuity does not require one AppKit terminal view object to remain permanently installed underneath all modes.

### 2.4 Transition ownership

Presentation transitions are input-authority transitions as well as layout changes.
Every `Flow ↔ Raw`, `Flow ↔ TUI`, and `Raw ↔ TUI` transition follows this order:

```text
1. Rust freezes the old route and increments the epoch.
2. Native revokes input, mouse capture, first responder, AX focus, and marked text.
3. Stale callbacks fail closed.
4. Rust validates current destination eligibility.
5. Native realizes only that destination.
6. The destination route becomes active.
```

Host completion may gate only local route activation. It must never stall PTY,
VT, or output progress.

Eligibility/admission must be current for the exact execution, attachment and
Controller authority plus the relevant presentation/canonical/integration
generation. Stale callbacks or eligibility never fall through to another mode,
and one physical event is admitted through at most one presentation route.

---

## 3. History, scrolling and Block skeleton

Large histories must remain fast.

```text
canonical logical history
→ stable LineId / chunk identity
→ Block anchors
→ BlockSkeleton
→ viewport virtualization
```

`BlockSkeleton` contains only enough information for efficient navigation/layout, for example:

- BlockId;
- logical line range;
- measured/estimated height;
- collapsed/expanded state;
- status/kind;
- lightweight presentation flags.

It does not copy full output.

The viewport should only materialize/render content close to the visible region. Cold history can be paged from Runtime persistence without allocating render resources for every historical Block.

---

## 4. Workspace object model

ADR-029 replaces the Tab layer below with one pane tree owned by the selected workspace, once SPEC-032 is accepted. Until that spec is accepted, the tree in this section remains the implementation model.

The UI may expose:

```text
Workspace
  ├─ Window(s)
  │    ├─ Tab(s)
  │    │    └─ Split tree
  │    │         └─ PaneView(s)
  │    └─ utility/inspector surfaces
  ├─ Execution registry
  ├─ agent/task activity
  ├─ artifacts
  └─ attention
```

Rules:

- window/tab/split/pane are presentation objects;
- they reference stable runtime identities;
- moving a view must not recreate its PTY;
- closing a view must not implicitly terminate a persistent execution;
- inspectors/artifacts do not receive PTYs unless they explicitly launch terminal execution;
- layout persistence is separate from execution persistence.

---

## 5. Pane types

A pane is a UI container, not synonymous with PTY.

Possible pane presentations:

### Terminal pane

References one `TerminalExecution` and may show Flow/Raw/TUI.

### Agent/activity pane

Shows a structured agent task, plan, tool activity or multi-agent graph. It may reference many terminal executions but owns none.

### Inspector pane

Shows derived metadata such as process/execution details, environment policy, SSH target, logs, performance traces or Block metadata.

### Artifact/diff pane

Shows files, generated artifacts, changes or reviewable diffs.

### Operational pane

Shows structured infrastructure state where a terminal is not the best representation.

This object model avoids the failure mode `pane == PTY`.

---

## 6. Global Attention Stack

Attention is a first-class workspace model rather than a collection of badges.

```text
AttentionItem
  ├─ id
  ├─ kind
  ├─ priority
  ├─ source workspace/execution/agent
  ├─ summary
  ├─ typed actions
  ├─ created/updated time
  └─ resolved/expired state
```

Kinds may include:

- agent approval;
- agent question;
- command failure;
- long-running command completion;
- operation requiring confirmation;
- remote disconnect/reconnect issue;
- security/policy decision;
- result ready for review.

### Popover stack behavior

The user should be able to open one global stack and act without hunting through tabs.

For a typed approval:

```text
Attention Stack
→ inspect summary/context
→ Approve / Reject / Modify
→ typed action sent to owning agent/task
```

No tab switch is required when the action is semantically complete.

### When navigation is mandatory

The stack must focus the exact target execution instead of synthesizing input when:

- password/secret input is required;
- raw terminal text is ambiguous;
- a TUI requires spatial interaction;
- mouse/cursor context matters;
- the protocol cannot prove the requested action is a structured approval.

The UI must never scrape arbitrary terminal text and turn it into a fake trusted approval protocol.

---

## 7. Notifications

OS notifications are projections of `AttentionItem` state.

```text
Runtime AttentionItem
  ├─ in-app stack
  ├─ menu/status surface
  ├─ mobile push (future)
  └─ OS notification
```

Dismissing an OS banner does not erase canonical attention unless the action explicitly resolves it.

The app should support “jump to source” from notifications while typed approvals may be completed directly from an appropriate in-app/mobile surface.

---

## 8. Multi-agent UI

Seyal should make multiple agents understandable without making terminal panes the orchestration model.

Possible future views:

- agent task graph;
- routing decisions;
- active/blocked/waiting agents;
- approvals across all workspaces;
- agent-to-execution associations;
- artifacts/diffs grouped by task;
- attention timeline;
- execution ownership/control state.

The UI consumes orchestration state through typed metadata. It never derives agent authority from whichever tab is currently visible.

---

## 9. Mobile UI role

iOS/Android are remote control surfaces for runtimes on user machines or in Seyal Cloud.

A mobile client should prioritize:

- global Attention Stack;
- approvals/questions;
- workspace navigation;
- terminal viewing/input when needed;
- Blocks/history;
- artifacts/diffs;
- agent/task progress;
- notifications;
- acquire/release terminal control.

Mobile does not need to duplicate desktop window chrome. It should expose the same stable identities through a mobile-appropriate navigation model.

---

## 10. Multi-client control UX

Many clients may observe an execution. Interactive authority must be visible.

Examples:

```text
Desktop Mac      = controller
Phone            = observer
Remote browser   = observer
```

A phone can request control; the Runtime decides according to policy and the current controller is notified.

Resize authority follows the controlling viewport by default. Observers render the canonical terminal dimensions and adapt visually without repeatedly changing PTY winsize.

---

## 11. Futuristic visual direction

The UI should aim for a distinctive execution-workspace identity:

- spatially calm shell surface rather than dense chrome;
- Blocks that emerge from real execution rather than card UI pasted around text;
- contextual metadata revealed progressively;
- global attention instead of tab hunting;
- artifacts/diffs/agent actions integrated with execution history;
- keyboard-first and mouse-first parity;
- smooth continuity between desktop and mobile;
- clear focus/control authority when multiple clients attach;
- motion used to preserve spatial understanding, not as decoration.

Possible visual ideas can be explored through prototypes, but no prototype owns architecture.

---

## 12. Paint priority and performance

Render scheduling priority should roughly be:

```text
P0  focused terminal input/cursor + fresh terminal damage
P1  other visible terminal surfaces
P2  attention/approval interaction response
P3  visible inspectors/artifacts
P4  chrome transitions/animations/decorative effects
P5  off-screen prefetch/background measurement
```

Rules:

- no decorative animation may block PTY/VT/render progress;
- expensive rich Block measurement is asynchronous/incremental;
- hidden panes do not maintain active GPU surfaces;
- viewport virtualization is mandatory for large Block history;
- terminal frame scheduling is not coupled to semantic extraction or agent state.

---

## 13. Accessibility and native behavior

On macOS, the product must support first-class:

- keyboard focus and navigation;
- IME/text composition;
- VoiceOver/accessibility tree;
- selection/copy/paste;
- native menus and shortcuts;
- Retina scaling;
- input method changes;
- multiple windows/spaces/fullscreen behavior.

Accessibility must be designed into Metal-backed terminal presentation; it is not a reason to replace the terminal renderer with a text view. In Flow, accessibility should expose Block/composer structure without also exposing a competing hidden full-Pane raw-terminal focus target.

Rust owns the semantic accessibility snapshot: stable IDs, role, label/value/help,
enabled/selected/focused state, navigation order, and typed actions. AppKit AX
objects only realize that snapshot and post platform notifications. Native must
not invent product roles, labels, or actions.

Product snapshots and terminal prepared frames remain separate transfers.
Candidate-D and `seyal-render` stay damage-driven. Pointer-bearing fields are
borrowed only until the next mutating bridge call; the host copies
synchronously and may retain only its derived copy. A stale generation must
not authorize actions. The published contract does not promise a per-handle
lifetime exception. Pane-sensitive actions carry stable Pane identity plus
the current execution, attachment, and presentation epochs.

---

## 14. UI architecture invariants

1. Presentation never becomes PTY/VT authority.
2. Flow, Raw and TUI are mutually exclusive user-visible views of one terminal execution.
3. Canonical terminal-state authority does not imply a permanently visible/focusable raw terminal viewport in Flow.
4. A Block never implies another terminal engine or independent terminal renderer authority.
5. Pane does not imply PTY.
6. Attention state is global and structured.
7. Typed approvals may be handled without tab navigation.
8. Arbitrary raw terminal prompts are never silently converted into trusted actions.
9. Mobile attaches to existing Runtime authority.
10. Slow clients/animations/inspectors cannot stall the terminal.
11. GPU resources scale with visible content.
12. UI layout persistence and execution persistence remain separate.
13. Futuristic presentation is allowed only inside terminal correctness/performance budgets.
14. If Flow cannot safely preserve required character-level input semantics, switch the Pane to Raw instead of passing input through a hidden terminal surface.
15. Presentation transitions follow the Rust-freeze / native-revoke / fail-closed / Rust-eligibility / native-realize / destination-active order. Host completion may gate only local route activation.
16. Presentation eligibility is fenced to current execution/attachment/controller and relevant canonical/integration generation.
17. Headed product/UI state is Rust-owned; Swift in this repository is only a thin macOS adapter (ADR-015). There is no separate Swift UI product tree. Rejected product-authority Swift may be deleted; required native adapter concerns must be recreated. PR #903 does not restore `Seyal.app` or satisfy the Usable Terminal Gate.

---

## 15. UI work sequence

UI design should proceed alongside terminal milestones, not after the entire engine is complete.

### M001 UI proof

- native macOS window;
- one terminal pane;
- one real Block identity;
- Metal rendering;
- explicit raw/Flow-compatible presentation seam;
- focus/input/accessibility skeleton;
- no fake terminal cards;
- no coexisting raw terminal viewport behind Flow.

### Next UI proof

- virtualized Block history;
- raw/Flow switch or automatic mode transition;
- alternate-screen TUI takeover;
- split/tab/workspace presentation referencing stable executions;
- Attention Stack prototype with typed approval;
- inspector/artifact pane without PTY.

Visual experimentation may run in parallel as non-authoritative prototypes as long as the production path remains aligned with this architecture.
