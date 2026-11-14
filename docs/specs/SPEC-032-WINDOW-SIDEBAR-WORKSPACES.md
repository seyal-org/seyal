# SPEC-032 — Window sidebar of open workspaces

- **Status:** Proposed. Normative only when this specification is merged as Accepted by a non-author maintainer. Until then ADR-018 containment remains the implementation authority.
- **Date:** 2026-11-14
- **Issue:** #1487
- **Authority:** Accepted ADR-029. This spec names the reducer and migration steps ADR-029 §2.4 requires.
- **Does not own:** `CreateWorkspace` minting or bound-root rules (ADR-028); PTY, child, VT, or `TerminalState` (ADR-004 / ADR-005); pane-local terminal-tab binding (proposed ADR-025 / SPEC-031); durable layout restore (ADR-007 P4 / proposed ADR-023).

## 1. State

Rust `ShellState` is the only owner of this state.

For each `WindowId`:

```text
open_workspaces: ordered Vec<WorkspaceId>   // no duplicates
selected_workspace: Option<WorkspaceId>     // member of open_workspaces, or None only while the list is empty
```

For each `WorkspaceId`:

```text
pane_tree: one PaneTree
presenting_window: Option<WindowId>         // at most one
```

For each Pane, the ordered terminal surfaces and selected surface stay the pane-local terminal tabs. A `TerminalTabId` is not a sidebar row.

`TabId` is not part of this containment. A pane address is `WorkspaceId` + `PaneId`.

## 2. Invariants

1. A non-empty window has exactly one selected workspace, and that id is in `open_workspaces`.
2. A window shows only the selected workspace's pane tree.
3. `presenting_window` is the only window whose `open_workspaces` contains that workspace.
4. One `ExecutionId` is bound to at most one terminal surface in the whole composition.
5. `WorkspaceId` on an execution does not change because a sidebar row was added, selected, or removed.
6. Sidebar index, title, directory, branch, ports, and notification text are labels. They are not identities and they authorize nothing.

## 3. Reducer

Every action names the `WindowId` and carries the current containment generation when it changes membership. A stale generation or an unknown id leaves open lists, selection, pane trees, bindings, and the input route unchanged.

### 3.1 SelectWorkspace

1. Reject if the workspace is not in that window's `open_workspaces`.
2. If it is already selected, the action is a no-op.
3. Fence and revoke the previous workspace's input route.
4. Set `selected_workspace` to the named workspace.
5. Admit input only to that workspace's focused pane and selected terminal surface.

Selecting does not create a window, raise a different window, clone a pane tree, or move an execution.

### 3.2 OpenWorkspace

1. Reject if `presenting_window` is already another window. The effect is to order that window front and select the workspace there, not to copy it.
2. If the workspace is already in this window's list, run SelectWorkspace.
3. Otherwise append it to `open_workspaces`, set it selected, and set `presenting_window` to this window.
4. Fence the previous route before admitting the new one.

`CreateWorkspace` remains ADR-028. After a successful create, OpenWorkspace adds the new id to the current window and selects it. Create does not move existing executions.

### 3.3 NewWindow

Create a `WindowId` whose `open_workspaces` contains one new workspace, selected, with one pane tree. Do not copy another window's open list, pane tree, or execution bindings.

### 3.4 RemoveWorkspace

1. Remove only the named id from that window's `open_workspaces`.
2. Clear `presenting_window` for that workspace.
3. Detach presentation. Do not delete the workspace and do not terminate executions.
4. If the removed id was selected, select the workspace now at the removed index, or the new last workspace if none follows.
5. If the list is empty, the window follows ADR-018 zero-window re-entry when that removal also closes the window.

### 3.5 Migration

Run once when this spec becomes the implementation authority.

1. A workspace with one Seyal Tab keeps that tab's pane tree as `pane_tree`.
2. A workspace with more than one Seyal Tab keeps the active tab's pane tree.
3. Every execution bound only to a non-active tab stays live, becomes unpresented, and keeps its `WorkspaceId`.
4. Non-active tab layouts are discarded. They are not minted as new workspaces.
5. The workspace's existing presenting window, if any, lists that one workspace as selected. No second window gains a copy.

## 4. Host

Swift renders `open_workspaces` and `selected_workspace` from the Rust snapshot and forwards SelectWorkspace, OpenWorkspace, NewWindow, and RemoveWorkspace. Swift does not mint `WorkspaceId`, reorder the list, or choose the selected workspace.

Accessibility ids are the `WorkspaceId`, `PaneId`, and `TerminalTabId`. Titles and indexes are not ids.

## 5. Tests

- Select, open, new-window, and remove, including stale generation and unknown ids.
- A switch cannot deliver the next input, paste, or composer submission to the previously selected workspace.
- One workspace pane tree cannot be open in two windows.
- Migration for one tab and for several tabs, including unpresented executions and unchanged `WorkspaceId`.
- Sidebar title, directory, branch, port text, and terminal text cannot authorize focus, spawn, or a path.

## 6. Out of scope

Browser panels, workspace groups, notification rings, freeform canvas, socket control, cmux assets, and durable sidebar restore across quit or relaunch.
