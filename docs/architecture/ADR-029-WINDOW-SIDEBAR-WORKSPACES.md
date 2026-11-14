# ADR-029 — Window sidebar of open workspaces

- **Status:** Accepted on merge of PR #1488 by non-author maintainer @mahboobmonnamd under #1487. An author or agent comment is not that acceptance. The merged file was misnumbered ADR-028; this record renames it to ADR-029. ADR-028 remains the workspace-root decision under #1409. ADR-027 remains reserved for #1385.
- **Date:** 2026-11-14
- **Issue:** #1487
- **Companion specification:** [`SPEC-032`](../specs/SPEC-032-WINDOW-SIDEBAR-WORKSPACES.md). Reducer and migration steps in that spec are normative when its acceptance PR is merged by a non-author maintainer. Until then ADR-018 containment remains the implementation authority.
- **Numbering note:** ADR-029. Do not reuse ADR-028.
- **Scope:** Presentation containment inside one window: an ordered sidebar of open workspaces, in-place selection, and one pane tree per workspace.
- **Depends on:** Accepted ADR-005, ADR-007, ADR-009, ADR-015, ADR-017, ADR-018, ADR-019, ADR-021, and ADR-028 (workspace root and `CreateWorkspace` only). Proposed ADR-025 / SPEC-031 for pane-local terminal surfaces.
- **Does not own:** browser panels, workspace groups, notification rings, freeform canvas, a socket API, a second terminal engine, or durable layout persistence (ADR-007 P4 / proposed ADR-023).

## 1. Context

cmux's published window hierarchy is the requested product model ([concepts](https://cmux.com/docs/concepts)):

```text
Window
  └── Workspace          sidebar row; several may be open; one is selected
        └── Pane         split region
              └── Surface    tab inside that pane
```

A window has its own sidebar. Selecting a row swaps that window's panes. Each pane has its own surface tabs. The product request is that Seyal follow this containment, including several projects open in one sidebar.

Seyal's accepted containment is the opposite shape:

```text
Workspace (ADR-007 durable domain)
└── Window                 bound to that one Workspace for its lifetime (ADR-018 §1.1)
    └── Seyal Tab
        └── PaneTree
            └── Pane
                  └── terminal tab     proposed ADR-025; cmux's Surface
```

ADR-018 §2.2 and §7 make `ActivateWorkspace` raise or create that workspace's window. ADR-018 §11.4 records the deliberate rejection of switching a workspace inside the current window. ADR-018 §10.A rejected a different proposal, "a workspace owns tabs and a window shows a subset of those tabs," because one tab would then be visible in two windows.

The production shell starts as one workspace, so the sidebar cannot show multiple open projects. That is a product gap. It is not evidence that the current code should be restyled around ADR-018. The containment itself has to change.

Pane-local terminal tabs already correspond to cmux surfaces. This ADR does not reopen that binding.

## 2. Decision

One window is a presentation frame over an ordered set of open workspaces. The sidebar lists those workspaces. Exactly one is selected while the window is non-empty. Selection replaces the visible pane tree in that same window.

```text
Window
  └── ordered open WorkspaceIds
        └── selected Workspace
              └── one PaneTree
                    └── Pane
                          └── ordered terminal surfaces
```

There is no Seyal Tab between a workspace and its pane tree. A workspace owns one pane tree. cmux's Surface remains the pane-local terminal tab from proposed ADR-025: one `TerminalTabId` references one `ExecutionId`.

### 2.1 What stays

1. `WorkspaceId` remains the ADR-007 durable domain identity. It owns execution association, retention, and security scope. A sidebar row is a presentation of that identity, not a second object and not a display-name key.
2. One `TerminalExecution` owns one PTY, one child lifecycle, and one canonical `TerminalState`. The window, the sidebar, and the pane tree own none of those.
3. One `ExecutionId` is bound to at most one live terminal surface across the whole composition.
4. Closing a window, or removing a workspace from a sidebar, detaches presentation. It does not delete the workspace and does not terminate executions. Explicit close of a terminal surface remains the only session-ending gesture defined by ADR-025.
5. Rust owns sidebar membership, order, selection, pane-tree structure, and focus. Swift realizes the sidebar and the window. Swift does not mint `WorkspaceId` or choose which workspace is selected.
6. Flow, Raw, and TUI stay mutually exclusive views of the selected execution. This ADR adds no browser pane, no second VT, and no second renderer.

### 2.2 Sidebar behavior

- **Open workspace.** Adds an existing or newly created `WorkspaceId` to the current window's open list and selects it. The previous workspace stays in the list, unselected, with its executions still live.
- **Select workspace.** Changes which open workspace this window shows. The host does not raise a different window for that selection. The previous input route is fenced and revoked before the selected workspace's route is admitted. A stale selection changes nothing.
- **New window.** Creates a window whose sidebar contains one new workspace, selected. It does not clone another window's open list or another workspace's pane tree.
- **Already open elsewhere.** A workspace's pane tree is presented in at most one window. Selecting it orders that window front and selects it there. It is not copied into a second window.
- **Empty window.** A window may temporarily list zero workspaces only while the last sidebar row is being removed. It then follows ADR-018's zero-window re-entry if that removal also closes the window. A window never shows two workspaces' panes at once.

`ActivateWorkspace` as specified in ADR-018 §2.2 is replaced by the open and select actions above. There is one activation path.

### 2.3 Identity

Sidebar index, title, working directory, git branch, port list, and notification text are derived labels. They are not `WorkspaceId`, `WindowId`, `PaneId`, or `ExecutionId`. Untrusted terminal text never authorizes opening a workspace, selecting one, spawning a process, or choosing a directory.

`TabId` leaves the user-facing containment. ADR-019's closed `ResourceAddress` set loses `Tab`. A pane address is `Workspace + Pane`. That address amendment is part of accepting this ADR, not an implementation choice. Extending addresses with a new sidebar-row id is rejected: the sidebar row is the `WorkspaceId`.

### 2.4 Migration of existing Seyal Tabs

A workspace that already has one Seyal Tab keeps that tab's pane tree as the workspace pane tree.

A workspace that already has more than one Seyal Tab keeps the active tab's pane tree. Every execution bound only to a non-active tab stays live and becomes unpresented. Those tabs' layouts are not preserved and are not turned into extra workspaces. Rehoming an execution into a newly minted workspace would change ADR-007 ownership and is out of scope.

The reducer steps for that migration are SPEC-032. They run only after SPEC-032 is accepted. Until then the current ADR-018 containment remains in force.

## 3. Security and hot path

- An in-place switch is a selection. It must not read or write another workspace's terminal bytes, and it must not run on the PTY to VT path.
- Input, paste, and composer submission after the switch resolve only to the selected workspace's focused pane and selected terminal surface.
- A rejected or stale switch leaves the previous workspace selected and its route unchanged.
- Sidebar metadata is display only. It cannot satisfy a path, spawn, or focus check.
- Several open workspaces may keep live executions. Their inactive presentation cost is reported separately from PTY count. Opening a workspace is bounded cold control work and must not stall unrelated terminal progress.

## 4. Out of scope

The following cmux behavior is not this decision:

- embedded browser surfaces;
- workspace groups, pins, colors, and per-group spawn;
- notification rings and git/port decoration beyond untrusted labels;
- freeform canvas layout;
- socket or CLI remote control of the sidebar;
- copying cmux assets or names into Seyal.

Durable restoration of the sidebar across quit or relaunch waits on accepted M004 persistence. This ADR does not write layout to disk.

## 5. Alternatives

### A. Keep ADR-018 and only list workspaces in the sidebar

Rejected. Selection would still raise another window. The sidebar would not switch the layout the user is looking at, which is the cmux behavior this issue asks for.

### B. A new "project" object beside Workspace

Rejected. The sidebar project and the ADR-007 workspace would be two identities for one domain object. Display names would become a second key.

### C. Use Seyal Tabs as the sidebar rows

Rejected. Those rows are layouts inside one project. cmux's sidebar row is the project, and it owns the pane tree directly. Keeping both layers preserves the container this decision removes.

### D. ADR-018 §10.A, revived

Not this decision. §10.A put the same tabs into more than one window. Here a workspace pane tree has one presenting window, and the sidebar rows are workspaces, not shared tabs.

## 6. Acceptance consequences

On acceptance, and only then:

- ADR-018 §1.1, §2.2, §7, and §11.4 are amended to the containment in §2. The one-workspace-per-window lifetime binding is removed.
- ADR-019 drops `Tab` from `ResourceAddress`. ADR-021's pane-tree operations are scoped to the workspace pane tree rather than to a Seyal Tab.
- The UI shell scaffold's Workspaces / Tabs switcher and `ui/SEYAL-UI-ARCHITECTURE-001.md` §4 are updated to the sidebar model.
- SPEC-032 defines the reducer, the migration in §2.4, accessibility identifiers, and the tests in §3.
- An implementation issue may become Ready only after SPEC-032 is accepted. That issue is not this ADR's pull request and not the numbering-correction pull request.

This acceptance authorizes no production Rust or Swift in the architecture record. Implementation waits on a Ready issue.
