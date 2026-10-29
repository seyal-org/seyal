# ADR-025 — Pane-local terminal tabs and session lifecycle

- **Status:** Proposed; not implementation authority until accepted by a non-author maintainer.
- **Date:** 2026-10-29
- **Issue:** #1381 (parent #674)
- **Scope:** Rust-owned terminal-tab identity and selection within Pane regions; create, split, close, session retention, recovery, and migration semantics.
- **Depends on:** Accepted ADR-005, ADR-007, ADR-009, ADR-015, ADR-017, ADR-018, ADR-021; SPEC-003, SPEC-004, SPEC-006, SPEC-008, SPEC-009, SPEC-025.
- **Coordinates with:** #1175 / C2b, #1217 / C3, #936 / multi-live Pane surfaces, #929 / live-session inventory and adoption, #832 / Proposed ADR-023 M004 persistence, and the SPEC-004 per-attachment delivery contract.
- **Companion specification:** Proposed SPEC-031.

## 1. Context

Seyal currently models `Workspace → Window → Tab → PaneTree → Pane`. A Pane is a leaf region in a split layout and may reference at most one `ExecutionId`. The requested product has a second, pane-local strip: each Pane region may present several terminal sessions and select one independently. This is not the existing Window/Workspace Tab, and it is not a stack of surfaces sharing one PTY.

The requested operations are distinct:

- **New Terminal** provisions a new Runtime-owned execution and adds its terminal tab to the focused Pane without changing the split tree.
- **Split Pane** divides the focused Pane region and provisions a new, independent terminal execution for the new region.
- **Select Terminal Tab** selects one tab in one Pane while preserving other Panes' selections and every session's identity and state.

Adding a terminal-tab list to the existing Pane binding would silently contradict accepted authority. ADR-021 allows at most one `ExecutionId` per Pane. ADR-009 gives a Pane one composer and one focused execution route. ADR-017 defines provisioning for a terminal leaf. ADR-018 makes closing Pane/Tab/Window presentation detach-only. ADR-007 keeps live execution lifetime independent of GUI presentation and forbids claiming a prior PTY is live after Runtime restart.

This ADR proposes a distinct terminal-session binding and lifecycle. It does not move PTY, child, VT, canonical `TerminalState`, or execution-registry ownership out of Runtime.

## 2. Decision

A terminal tab is a stable Rust-owned presentation identity for one Runtime-owned terminal execution. A Pane remains a rectangular layout region and owns no execution infrastructure.

```text
Workspace
└── Window
    └── Seyal Tab
        └── PaneTree
            └── Pane (layout region; focused independently)
                ├── ordered TerminalTabIds
                └── selected TerminalTabId, or none when empty

TerminalTabId → exactly one ExecutionId reference
ExecutionId → one Runtime-owned TerminalExecution
TerminalExecution → one PTY/child lifecycle + one canonical TerminalState
```

`TerminalTabId` is not `TabId`, `PaneId`, `ExecutionId`, a process ID, or a display label. Rust allocates opaque, process-unique, non-reused identity. The Runtime does not store or resolve `PaneId` or `TerminalTabId`; it continues to own `ExecutionId` and `WorkspaceId` association.

### 2.1 Ownership and uniqueness

1. Rust `ShellState` owns PaneTree, pane membership/order of terminal tabs, each Pane's selected terminal tab, focused Pane, terminal-tab identity, presentation state, and the mapping from `TerminalTabId` to `ExecutionId`.
2. A `TerminalTab` is a reference, not an execution owner. It contains no PTY, child handle, parser, grid, VT state, scrollback authority, or renderer authority.
3. One `TerminalExecution` owns exactly one PTY/child and one canonical `TerminalState`. One `ExecutionId` may be bound to at most one live terminal tab. Two Panes or terminal tabs must never share an `ExecutionId` or PTY.
4. `focused_pane` and each Pane's `selected_terminal_tab` are separate state. The effective interactive target is resolved by Rust from both, then fenced by the exact `ExecutionId`, current attachment/controller lease, and input/presentation epoch. Missing or stale components fail closed.
5. Pane-local composer draft/history, Flow/Raw/TUI mode, terminal-specific inspector context, and scroll position are terminal-tab/execution scoped where they depend on a particular execution. Pane geometry, split position, and pane focus remain Pane scoped. ADR-009's one-composer/one-route rule is amended only as required by this split; the composer never becomes a second terminal input path.
6. Accessibility exposes stable Pane and TerminalTab identities plus focused and selected state. Display names and indexes are never authority.

### 2.2 Creation and split

- A fresh Workspace has one Pane containing one selected terminal tab backed by one newly provisioned execution.
- `NewTerminal` provisions through the accepted ADR-017 Runtime create→attach→bind path. On success, Rust appends/selects the new tab in the focused Pane. PaneTree and other Panes' focus, selections, bindings, and content do not change.
- `SplitPane` divides the focused Pane region and provisions a new selected terminal tab with a fresh execution for the new Pane. The original Pane retains its terminal-tab list and selected tab. The split action never reuses an existing execution.
- Both operations are transactional at the product boundary. Stale containment or Pane identities fail before mutation. Spawn, attach, or bind failure leaves the old layout and sessions intact; a successfully created but never-bound execution follows ADR-017's exact disposition path. A failed split cannot leave an unbound extra Pane as though it succeeded.
- Shell, environment, `TERM`, initial CWD and initial size remain under ADR-020 / ADR-017 launch-policy ownership. Terminal output, OSC text, titles, and labels never select launch policy or filesystem paths.

### 2.3 Focus, selection, input, resize and presentation

- Selecting a terminal tab changes only that Pane's `selected_terminal_tab`; it never changes another Pane's selected tab. Pane focus is a separate typed transition. A pointer activation may commit focus and selection together, but the Rust snapshot records both facts independently.
- Input, paste, copy, mouse, composer submission, and terminal-specific commands resolve only to the selected terminal of the focused Pane. The host cannot choose or substitute an `ExecutionId`.
- A user-requested terminal resize targets only the selected terminal in the focused Pane. Pane geometry changes are separately correlated: each affected Pane's selected terminal receives only that Pane's accepted geometry; no terminal receives a sibling Pane's dimensions. Inactive terminal tabs retain their last accepted geometry until selected, then receive the Pane's current geometry before interactive input is admitted.
- Switching never creates, terminates, or replaces an execution. It fences the old input route before enabling the new one. Every session continues through the permanent Runtime→PTY→VT→TerminalState path. Inactive sessions may suspend replaceable display delivery only through an accepted SPEC-004 capability; their canonical state and process progress continue. If that capability is unavailable, use the accepted bounded attachment policy and fail creation honestly at capacity.
- Flow, Raw and TUI remain exclusive presentations of the same selected execution. No alternate parser, copied VT grid, fake terminal data, or temporary renderer is permitted.

### 2.4 Close and session lifetime

This proposal distinguishes a terminal-session close from closing layout presentation:

- **Close Terminal Tab** is the user's explicit request to close that terminal session. Rust resolves the exact `TerminalTabId → ExecutionId` and issues the existing typed, Controller-fenced `TerminateExecution` operation. It does not infer a target from the focused view after asynchronous work begins. The selected tab remains reachable in a closing state until Runtime accepts termination and the existing final-drain/lifecycle contract completes. If termination is rejected or its outcome is unknown, Rust retains the binding and surfaces a typed failure; it must not detach and forget a possibly live process. On successful finalization, Rust removes only that terminal tab and selects the next tab at the removed position, or the preceding last tab when none follows. Closing an unselected tab leaves the Pane's current selection unchanged.
- **Closing the final terminal tab in a Pane** leaves a valid empty Pane with no selected terminal and no input target. The Pane offers New Terminal. It does not produce a zero-Pane Seyal Tab.
- **Close Pane**, including one with several terminal tabs, is layout presentation close. Rust detaches each bound execution and removes the Pane; every previously bound execution stays live and becomes an unpresented session. No execution is implicitly terminated. A live-session inventory/adoption route is mandatory before shipping this close behavior, consistent with ADR-018 §3.3 and #929.
- **Close outer Seyal Tab, Window, or final Pane through hierarchical close** follows ADR-018: presentation is destroyed, its bound executions remain live and re-adoptable, and zero-window re-entry remains possible. No close can terminate a sibling or retarget a different execution.
- Explicit Close Terminal is the only new session-ending gesture in this model. It is represented as the existing exact-ID terminate operation, not as a generic Pane/Tab/Window close with a hidden termination side effect. The UI must communicate that Close Terminal ends its process; any separate detach action preserves it.

### 2.5 Default session management and persistence boundary

Seyal's default session behavior is Runtime-owned live-session continuity, similar to tmux while the resident Runtime remains alive:

- Switching terminal tabs, hiding/closing a Pane, closing an outer Tab/Window, quitting the GUI, or crashing the GUI does not by itself terminate a still-live execution. An unpresented session remains enumerable by exact `ExecutionId` and can be explicitly adopted into a new Pane; adoption never spawns a replacement or guesses from list order.
- Closing a terminal tab explicitly terminates that named session as specified in §2.4.
- Runtime restart/reboot does not preserve a live PTY/child. Old executions are reported as ended/unavailable, never resurrected or presented as live. A new terminal gets a new `ExecutionId`.
- M003 adds no durable layout/session database. Runtime-live inventory and same-Runtime adoption are required for the session-management behavior above. Durable Workspace/Pane/TerminalTab layout and session metadata are coordinated with M004 #832 / Proposed ADR-023; this ADR does not create a second store or pre-empt that decision.

### 2.6 Compatibility and migration

The current one-binding Pane is represented as one terminal tab without spawning or replacing the existing execution. An unbound Pane migrates to the valid empty-Pane state. `TerminalTabId` is freshly allocated and never derived from a Pane index, ExecutionId bytes, process id, terminal text, or display label. Versioned client snapshots/actions add TerminalTab identity and per-Pane selection; unknown versions/sizes fail closed. No protocol message may carry PaneId or TerminalTabId to Runtime. If a persisted layout format is introduced by M004, its migration must be owned and accepted under ADR-023 before persisted fields are written.

### 2.7 Failures, recovery, capacity and security

- Stale Pane, TerminalTab, Execution, attachment, controller, containment generation or input epoch fails closed and is atomic with respect to product state.
- Create/attach/bind, close/terminate, final drain, and session-adoption each have explicit success, rejection, timeout/outcome-unknown, Runtime disconnect and child-exit outcomes. Unknown termination outcome retains a route to the exact execution; no blind retry may create or kill a different process.
- Recovery reattaches to the exact existing `ExecutionId` and binding generation. A stale completion cannot select a sibling tab, change another Pane's focus, or admit input.
- Capacity is bounded by accepted SPEC-003/SPEC-004 limits. A rejected create leaves all existing tabs and executions unchanged. Scaling measures distinguish Pane count, terminal-tab count, live ExecutionId count, attachment count, and visible renderer count.
- Terminal content and process output are untrusted data, never authority. Same-UID peer authentication, Runtime capability checks, one Controller lease per execution, and terminal input fencing remain in force.

## 3. Alternatives considered

1. **Put several tab labels on the existing one-execution Pane.** Rejected: it cannot preserve independent processes, PTYs, scrollback, and identities; it is decorative chrome, not the requested behavior.
2. **Make every terminal session an outer Seyal Tab.** Rejected as a substitute: it does not provide each Pane its own terminal-tab strip or independent per-Pane selection.
3. **Share one ExecutionId/PTY between panes or terminal tabs.** Rejected: it violates the single-controller and canonical-state boundaries and cross-targets input/resize.
4. **Let Swift own per-pane terminal selection or layout.** Rejected by ADR-015.
5. **Terminate sessions whenever a Pane/outer Tab closes.** Rejected: it contradicts ADR-007/018 and breaks detached live-session continuity. Explicit Close Terminal remains a separate exact-session operation.
6. **Persist/recreate live processes across Runtime restart.** Rejected: a process/PTY cannot be resurrected. Durable metadata is M004 and must never claim a dead process is live.

## 4. Consequences and implementation gates

- ADR-021's one-execution-per-Pane wording and ADR-009's Pane composer/input route need scoped amendments. ADR-017 provisioning remains the Runtime creation authority. ADR-018's generic layout-close behavior remains detach-only; the new Close Terminal operation is explicit termination. SPEC-004/006/008/009 require scoped amendments as the accepted API and state transitions dictate.
- #1175 and #1217 provide the existing per-execution provisioning path; #936 provides multi-live Pane renderer/resize/recovery work. This feature must not duplicate those implementations.
- The implementation child cannot become Ready until this ADR/spec is accepted, exact live-session enumeration/adoption has an accepted path (#929 or a scoped replacement), provisioning/attachment dependencies are current, and the attachment/capacity contract supports the required tab count.
- The implementation is a separate Issue and PR, test-first, on the exact current OSS base, and must pass real headed XCUI with real Runtime/PTYS. A build or Rust reducer test alone is insufficient.

## 5. Acceptance

This ADR becomes Accepted only after a separate Architecture/R&D PR receives independent non-author maintainer approval and is merged under #1381. Until then it is a proposal and authorizes no production implementation.
