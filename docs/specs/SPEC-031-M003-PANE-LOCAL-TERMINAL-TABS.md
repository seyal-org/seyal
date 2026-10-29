# SPEC-031 — M003 pane-local terminal tabs and session lifecycle

- **Status:** Proposed; normative only after ADR-025 is accepted by a non-author maintainer.
- **Date:** 2026-10-29
- **Issue:** #1381 (parent #674)
- **Authority:** Proposed ADR-025. Existing accepted ADRs continue to govern until ADR-025 is accepted.
- **Scope:** Observable behavior for terminal tabs inside Pane regions, independent selection/focus, create/split/close, and Runtime-backed session continuity.
- **Does not own:** PTY/child/VT/TerminalState internals (ADR-004/005); Runtime provisioning wire format (ADR-017 / SPEC-003/004); outer Window/Seyal Tab lifecycle (ADR-018); PaneTree operations outside the new split provisioning outcome (ADR-021 / SPEC-025); durable persistence (ADR-007 / proposed ADR-023).

## 1. Terminology

- **Workspace:** project or context container.
- **Seyal Tab:** outer layout container under a Window. It owns one PaneTree.
- **Pane:** one rectangular leaf region in that PaneTree. It is not a terminal and owns no PTY.
- **Terminal tab:** one terminal session shown inside one Pane. It references one TerminalExecution.
- **TerminalExecution:** Runtime-owned process/PTY/child lifecycle and canonical terminal state.
- **Focused Pane:** the Pane that receives normal keyboard focus and pane-scoped commands.
- **Selected terminal:** the selected terminal tab of a particular Pane.

A `TerminalTabId` is not a `TabId`, `PaneId`, `ExecutionId`, process id, array position, or display name.

## 2. State and invariants

For each Pane:

```text
terminal_tabs: ordered Vec<TerminalTabId>
selected_terminal_tab: Option<TerminalTabId>
```

For each terminal tab:

```text
TerminalTabId → exactly one ExecutionId
```

The authoritative layout and focus state is Rust-owned:

```text
Workspace → Window → Seyal Tab → PaneTree → Pane → ordered terminal tabs
focused_pane: PaneId
```

1. Fresh Workspace state has one Pane, one terminal tab, and one distinct Runtime execution.
2. A Pane may be empty only after its final terminal tab is closed; then its selection is `None` and no terminal input is admissible.
3. Every selected TerminalTabId is a member of its Pane's ordered list. No duplicate terminal tab may occur in two Panes.
4. Every live execution binding is unique across all terminal tabs. One ExecutionId never appears in two panes/tabs.
5. Runtime owns one PTY/child and one canonical TerminalState per TerminalExecution. Client, AppKit, Swift, and Metal never own a duplicate VT/grid or process.
6. Focused Pane and selected terminal are separate facts. The interactive target exists only when the focused Pane has a selected tab with a live binding, current controller attachment and valid input epoch.
7. All rejected state actions leave layout, focus, selections, bindings, and session state unchanged except for a typed last-error result.

## 3. New Terminal

`NewTerminal { focused_pane, containment_generation }` requests one independent execution from the accepted Runtime provisioning path.

On successful create→attach→bind:

- append one fresh TerminalTabId to the focused Pane;
- bind it to the returned unique ExecutionId;
- select the new terminal in that Pane;
- preserve PaneTree topology and every other Pane's identity, selection, focus, execution, geometry, and terminal contents.

On stale Pane/generation, capacity rejection, create failure, attach failure, bind failure, Runtime disconnect, or cancellation:

- do not publish a terminal tab that has no valid execution binding;
- leave pre-existing PaneTree, tabs, selections, focus, and sessions unchanged;
- dispose of a successfully created but never-bound execution only through ADR-017's exact disposition rule;
- surface a bounded typed failure and do not silently retry.

## 4. Split Pane

`SplitFocusedPane { focused_pane, axis, containment_generation }` divides the focused Pane's rectangle and provisions a new terminal tab for the new Pane.

On success:

- old Pane identity, terminal-tab order, selected terminal, execution bindings, and composer drafts survive unchanged;
- new PaneId and TerminalTabId are fresh and distinct;
- the new Pane has exactly one selected terminal tab mapped to a distinct new ExecutionId/PTY;
- the new Pane becomes focused and the new terminal is the interactive target after the focus fence completes.

On stale action, capacity rejection, or create/attach/bind failure, the split and new Pane do not appear. Existing executions are neither reselected nor resized as a side effect of a failed split.

## 5. Select terminal and focus

`SelectTerminalTab { pane, terminal_tab, identity_fence }` changes only the named Pane's selected terminal. It does not change another Pane's selection or implicitly change focused Pane. `FocusPane { pane }` changes only the focused Pane. A pointer activation may dispatch both transitions as one validated interaction; the resulting snapshot still identifies focus and selection separately.

Before admitting input to a newly selected terminal, the client must:

1. fence the prior route and revoke stale responder/input eligibility;
2. validate the current Pane→TerminalTab→Execution binding and attachment/controller tuple;
3. apply that Pane's current accepted geometry to the selected execution if required;
4. activate only the new route.

A stale or ambiguous completion never falls back to the last terminal or nearest Pane. A tab switch does not create, terminate, or replace any execution.

## 6. Input, resize, copy/paste, composer and presentation

- Terminal keyboard input, paste, mouse, direct-terminal keys, composer submission, and terminal-specific actions target only the selected terminal in the focused Pane.
- Copy reads the selected terminal's authoritative display/selection projection; it never reads a sibling terminal or a stale view cache as authority.
- Pane-local composer draft/history and Flow/Raw/TUI state are associated with the terminal tab/execution they serve. A draft cannot be submitted to a newly selected execution unless the user explicitly submits it there under the current revision/fence.
- A user resize command targets the selected terminal of the focused Pane only. When split geometry changes, the accepted geometry transaction is correlated independently to each affected Pane's selected terminal; sizes cannot cross between Panes.
- Inactive terminal tabs retain their last accepted geometry. Selecting one applies the current Pane geometry before enabling input. Rejected/stale resize keeps the last accepted geometry and does not update another execution.
- Flow, Raw and TUI remain mutually exclusive views over the same execution. No tab may create a second terminal parser, grid, transcript authority, renderer, or hidden PTY.

## 7. Close behavior

### 7.1 Close Terminal Tab

Closing a terminal tab is an explicit request to close its terminal session:

1. Rust resolves the exact current TerminalTabId→ExecutionId binding and fences it by containment generation and execution/attachment authority.
2. Rust sends the existing explicit `TerminateExecution` operation for that ExecutionId. This is not a generic Pane/outer-Tab close action.
3. Until Runtime accepts termination and final drain/lifecycle completes, the terminal tab remains addressable in a closing state. If termination is rejected, disconnected, or outcome-unknown, retain the tab and exact execution route with a typed error; do not detach and forget a possibly live process or retry blindly.
4. On finalization, remove only the named terminal tab. If it was selected, choose the tab now at the removed index; if none exists, choose the preceding last tab. Closing an unselected tab leaves selection unchanged.
5. Closing the final terminal tab leaves its Pane present and empty, with `selected_terminal_tab = None`, a visible New Terminal affordance, and no terminal input target.

### 7.2 Close Pane with one or more terminal tabs

Closing a Pane removes the layout region and detaches each of its terminal sessions. It does not terminate any previously bound execution. Each live execution becomes Runtime-owned and unpresented and must remain available through the authoritative session inventory/adoption surface. Closing one Pane cannot change or terminate a sibling execution.

### 7.3 Close final Pane / outer layout

A Seyal Tab may never contain zero Pane leaves. Closing the final Pane follows ADR-018's hierarchical Pane→Seyal Tab→Window behavior. The final Window may close into the existing zero-Window state and re-enter through the existing typed create/activate path. Bound executions remain live and adoptable unless their own terminal-tab close explicitly terminated them.

## 8. Session continuity and restart

- Switching or deselecting a terminal tab preserves its live ExecutionId, child process, cwd as held by that process, canonical terminal state, and retained scrollback within accepted Runtime/VT/history capacity.
- Closing Pane, outer Seyal Tab, Window, or GUI detaches presentation and does not terminate live executions. The same Runtime can enumerate and re-adopt an exact live ExecutionId without creating a new process.
- The live-session inventory/adoption route must expose stable identity, live/ended and attached/detached state without inferring sessions from terminal text. It must not choose an execution by unstable list order. This gate is coordinated with #929 or an accepted replacement child.
- This M003 behavior does not persist Pane/terminal-tab layout or resurrect PTYs across Runtime/OS restart. A restart never claims the old ExecutionId is live. Any durable metadata/layout work must follow accepted M004 persistence authority (#832 / ADR-023) and its migration contract.

## 9. Migration and versioning

- A legacy Pane with one bound ExecutionId migrates to one terminal tab that references the same ExecutionId. No PTY is spawned, killed, or replaced by this migration.
- A legacy unbound Pane migrates to the empty-Pane state. New Workspace creation separately provisions its required initial terminal.
- TerminalTabId uses the normal opaque non-reused identity allocator. It is never derived from labels, list indices, PaneId, ExecutionId, pid, or terminal output.
- Action/snapshot ABI versions and size tags are updated together. Old/new clients fail closed on unsupported versions; no fallback interprets PaneId or TabId as TerminalTabId.
- Runtime protocol messages continue to use Runtime-owned WorkspaceId/ExecutionId/AttachmentId only. TerminalTabId and PaneId stay client-local.
- Durable presentation migration is out of this SPEC until ADR-023 is accepted and explicitly covers the new records.

## 10. Failure, concurrency, security and capacity

- Tests cover spawn/registration/publication failure, create-result loss, attach/bind failure, concurrent Pane/terminal close, stale generation, duplicate/replayed actions, child exit during switch/close, attachment loss, Runtime disconnect and recovery completion.
- Every completion is correlated to exact PaneId, TerminalTabId, ExecutionId, attachment/controller lease and applicable generation. Old completion cannot select, resize, close, or input a new terminal.
- Session adoption attaches to the exact Runtime execution; it never reruns a command or fabricates an ExecutionId/AttachmentId.
- Create is bounded cold control work and cannot stall unrelated PTY→VT progress. Capacity rejection changes no existing terminal state. Metrics distinguish Pane count, terminal-tab count, live executions, attachments, and visible renderer resources.
- Terminal output, OSC title/CWD, display labels and process names are untrusted and never authorize execution creation, selection, filesystem access, or termination.

## 11. Accessibility and visible hierarchy

Every Pane exposes a stable accessibility identifier derived from PaneId and state indicating focus. Its terminal-tab strip exposes each stable TerminalTabId, selected state, and closing/live status. The strip is rendered within that Pane region, not as the outer Seyal Tab strip. New Terminal, Split Pane, Close Terminal, and empty-Pane affordances have stable action identifiers. Accessibility actions route through the same Rust reducer/fencing as pointer and keyboard actions.

Visual styling follows Seyal's current product design system. The cmux screenshot is a layout/behavior reference only; no cmux name or assets are copied. Pixel-level implementation acceptance must be derived from the actual attached reference and the governing Seyal mockup.

## 12. Required implementation tests and evidence

Reducer tests first for the contract's success and rejection cases. Required classes:

- fresh Workspace invariant and migration of bound/unbound legacy Panes;
- New Terminal does not split and yields a distinct ExecutionId/PTY;
- Split creates a visible new Pane with an independent execution and preserves original Pane selection;
- per-Pane independent selection and focus, including non-focused tab selection;
- exact focused+selected input, paste, copy and terminal-action routing;
- selected-terminal and per-Pane geometry routing, stale/rejected resize, and no sibling resize;
- terminal-tab explicit termination success/rejection/unknown outcome/final drain and correct successor selection;
- multi-tab Pane close detaches all exact sessions and preserves siblings; final empty Pane and final Pane/Window/re-entry validity;
- session enumeration/adoption, app detach/crash, Runtime restart truth, child exit while detached, and attachment recovery;
- repeated split/create/switch/resize/close/reopen with no duplicate identities, shared execution, orphan PTY, focus jump or stale completion;
- versioned action/snapshot migration and accessibility identifiers;
- bounded resource/capacity behavior for many Panes, terminal tabs and live executions.

End-to-end acceptance uses the exact packaged Seyal.app, its bundled Runtime, real PTYs and headed XCTest/XCUI. It proves command input and output independently in two Panes and multiple terminal tabs, switching preserves process/state/scrollback, resize affects only intended execution, terminal close ends only its named process, pane close leaves sessions adoptable, and recovery has no focus/input cross-targeting. Record exact source SHA, worktree, Runtime endpoint isolation, test artifact and any blocked external gate.
