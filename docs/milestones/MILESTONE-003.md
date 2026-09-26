# Milestone 003 — Core Seyal Workspace

**Status:** **In progress.** Not Done. This file is the M003 implementation contract. It does **not** claim beta, M003 complete, or market-ready.

**GitHub epic:** [#665](https://github.com/seyal-org/seyal/issues/665)  
**Hierarchy umbrella:** [#674](https://github.com/seyal-org/seyal/issues/674)  
**Presentation umbrella:** [#675](https://github.com/seyal-org/seyal/issues/675)  
**Config umbrella:** [#676](https://github.com/seyal-org/seyal/issues/676)  
**Shell-integration spike:** [#686](https://github.com/seyal-org/seyal/issues/686)

**Entry gate:** M001 Done. M002.1–M002.9 terminal contracts are closed on the permanent path. Remaining M002 close-out (#824/#672, #673 PHYSICAL_ARM64, #837, then freeze-SHA validation and #664) may run **in parallel** and is **not** an M003 product blocker. #673 does not wait on #824 closure.

**Authority:** This document is subordinate to the accepted Seyal foundation architecture, accepted ADRs (especially ADR-007, ADR-009, ADR-015), SPEC-006 / SPEC-008 / SPEC-009, and [`docs/architecture/ui/SEYAL-UI-ARCHITECTURE-001.md`](../architecture/ui/SEYAL-UI-ARCHITECTURE-001.md). It narrows those authorities into an M003 contract. It does not reopen architecture, invent a second terminal engine, or override GitHub Issue acceptance with prose.

**Why this file exists:** M001 has `MILESTONE-001.md`. M003 previously lived only as epic #665 plus umbrellas and leftover M001.1 host slices after parent [#878](https://github.com/seyal-org/seyal/issues/878) closed. That gap let chrome/multipane work look like either “still M001.1” or “implement #674 wholesale.” After this freeze, M003 work is only the chain in §5.

---

## 1. Goal

M003 delivers Seyal's native local workspace around the **same** production terminal path:

```text
PTY → VT/parser → TerminalState/grid → damage → projection → Metal
```

Windows, tabs, splits, navigation, pane-scoped Flow/Raw/TUI presentation, local config/themes/fonts/keybindings, and a trusted shell-integration boundary must form a coherent headed workspace **without** changing `TerminalExecution` ownership.

Roadmap exit ([`docs/product/ROADMAP.md`](../product/ROADMAP.md)):

> macOS windows/tabs/splits/navigation, raw/Block/composer presentation, local config/themes/fonts/keybindings and trusted shell-integration boundary form a coherent local workspace. Alpha/beta quality; still not market-ready.

M003 is **not** market-ready. That remains M004 / #666. M003 is **not** the M002 terminal-engine close-out and **not** the agent platform.

---

## 2. Architecture slice

Keep one `TerminalExecution` → one PTY → one authoritative `TerminalState`. Tabs, splits, and windows are presentation/domain structure. A terminal leaf references at most one existing `TerminalExecution` and owns no second PTY, VT, or grid.

```text
Runtime
  -> TerminalExecution registry / PTY / VT / TerminalState
  -> WorkspaceId association (ADR-007; not a PTY owner)

Rust product (ADR-015)
  -> ShellState: Workspace / Tab / PaneTree / focus
  -> chrome, composer, Flow/Raw/TUI policy, recovery policy
  -> typed actions in; deterministic snapshots out

Thin AppKit host
  -> NSWindow / NSEvent / IME / AX / clipboard / Metal drawable
  -> realizes snapshots; no writable product model
```

Portable product authority stays in Rust (`seyal-client` / application root). Swift is only the macOS adapter. Flow, Raw, and TUI remain mutually exclusive presentations of the same execution (ADR-009 / SPEC-008). Closing or reparenting chrome must not terminate a still-owned execution.

No synchronous agent, persistence, cloud, licensing, telemetry, Lua, or Block semantic work on the terminal hot path. No JSON, per-cell, or per-glyph FFI on that path.

---

## 3. In scope

M003 implements and proves, on the permanent path:

- native macOS windows, tabs, nested splits, focus, and navigation
- Workspace / Tab / Pane identities over the existing Rust `ShellState` (do not port `SeyalShellState`)
- pane-scoped composer plus mutually exclusive Flow / Raw / TUI
- minimum Command Block lifecycle as a projection of Runtime `BlockTimeline`
- local TOML config, themes, fonts, keybindings, and startup shell/CWD policy
- trusted shell-integration seam where signals are available; raw mode remains fully functional without it
- headed tests that closing chrome does not kill unrelated live executions
- workspace/tab/pane scaling evidence that distinguishes presentations from real PTYs

---

## 4. Explicit non-goals

Do **not** pull these into M003. They are not “remaining M003 work.”

```text
M002 terminal-engine close-out (#824 / #673 / #837 / #664)
mutating TerminalState / VT / Unicode / reflow as an M003 PR
durable disk recovery / signing / notarization / market release (M004)
agent runtime / memory / context production (M005)
full workflow / coding surfaces (M006)
graphics / image protocols (#689)
Linux/Windows GUI, remote-product attach (M007)
plugins, collaboration, enterprise, cloud
second terminal model, per-Block PTY, or popup/untracked PTYs
implementing umbrella #674 as one PR
stealing actively claimed/assigned work (for example #922, #865, or M002 #673)
reviving superseded historical Cursor branches as production evidence or creating a second branch for the same active Issue
reopening merged #932 / #933 / #935
```

A FAIL or INCONCLUSIVE **mandatory** criterion on a remaining §5 issue may create one scoped child. That is defect closure, not a new product slice.

---

## 5. Finding-set freeze and remaining chain

Approved umbrellas are **#674 / #675 / #676**, plus refinement/spike **#686**. Headed leftover slices after M001.1 parent #878 closed now live under this contract. Snapshot **2026-09-25** (live GitHub remains authority over stale prose). This refresh defines the executable development frontier rather than treating every open M003-related Issue as Ready.

### 5.1 Already on `master` (do not reopen)

| Slice | Issue | Status |
|---|---|---|
| Rust Workspace/Tab/Pane domain | #879 / PR #892 | **Closed** |
| Rust theme/config semantics | #740 | **Closed** |
| Rust Flow/Raw/TUI policy | #861 | **Closed** |
| Rust Block/composer product state | #881 | **Closed** |
| Thin AppKit host over Rust snapshots | #883 / PR #910 / #914 | **Closed** |
| Command palette | #932 / PR #940 | **Closed** |
| Composer history fuzzy recall | #933 / PR #938 | **Closed** |
| Block details inspector | #935 / PR #939 | **Closed** |
| Retained Flow/long-output/TUI workload fixtures | #1005 / PR #1052 | **Closed** |

These landings are **foundation**, not M003 Done. The headed host is still a one-live-Metal-leaf adapter until #923 / #936.

### 5.2 Workspace/hierarchy frontier

| Slice | Issue | Status |
|---|---|---|
| Workspace chrome (left / tabs / inspector) | #922 | **Active** — owned by @crdileep82; implementation PR #1017 open; do not duplicate |
| Split-tree projection; one live Metal leaf | #923 | **Blocked only on consumable #922 head**; may develop stacked before #922 merge |
| Split drag-resize ratios | #928 | **Blocked only on a consumable #923 head**; may develop stacked before #923 merges |
| Pane/tab execution provisioning contract | #994 | **Active refinement** — owned by @mahboobmonnamd; open PR #1056 (Proposed ADR-017); defines the missing product→Runtime execution-creation seam |
| Multiple live Metal surfaces per Tab | #936 | **Blocked** on #923 plus accepted provisioning work derived from #994 |
| Adaptive Depth chrome fidelity | #934 | **Blocked only on a consumable #922 head**; presentation-only, may develop stacked once #922 publishes it (design-doc PR #1018 by @crdileep82 open) |

Historical `cursor/issue-922-*` / `cursor/issue-923-*` branches are not current production authority. #923 has an explicit 2026-09-21 product-owner disposition that its old Cursor branch is abandoned/superseded. Do not resume old branches implicitly.

Agent inventory/Agents-center/attention product work (#926/#927/#930) is not part of the M003 critical path; `docs/product/ROADMAP.md` assigns the agent-native local substrate and Attention foundations to M005 (#667/#680). The specific Sessions/Resources **center views** (#929/#931) are likewise not M003 close blockers. This does **not** defer #674's required stable local `Session` identity or hierarchy semantics; it only avoids making those optional center surfaces a prerequisite. #937 is historical orchestration, not an implementation pickup.

### 5.3 Umbrellas and executable parallel lanes

| Slice | Issue | Status |
|---|---|---|
| Native hierarchy / windows / tabs / splits / navigation | #674 | **Open umbrella** — not a pickup |
| Pane input, Blocks, selection, same-execution presentation | #675 | **Open umbrella** — not a pickup |
| Local config / themes / fonts / keybindings / launch policy | #676 | **Open umbrella** — not a pickup |
| Trusted shell-integration / semantic command boundaries | #686 | **In Progress** — human owner @mahboobmonnamd; ADR-009 duration amendment accepted on `master` via merged #1022; remaining open work is SPEC-008 alignment, unchecked spike acceptance, and any follow-on implementation-Issue refinement — no production code in #686 |
| Flow compositor Block-region drawing | #865 | **Active** — owned by @mahboobmonnamd; implementation PR #1058 open on legacy `issue/865`; migration of `issue/865` / `cursor/live-tail-865-c8cd` to `mahboobmonnamd/issue/865` under #997 still pending; not available for pickup |
| Flow composer input/IME/focus fence | #866 | **Blocked on #865** |
| Raw/TUI full-Pane takeover | #867 | **Blocked on #866** |
| Headed Flow/Raw/TUI workload matrix | #868 | **Blocked on #867** |
| Renderer qualification / regression | #869 | **Refinement / after #868** |
| Production startup config realization | #993 | **Active** — owned by @anulalbs; implementation PR #1006 open; do not duplicate |
| Pane/tab execution provisioning contract | #994 | **Active refinement** — owned by @mahboobmonnamd; open PR #1056 |
| Native window/tab lifecycle contract | #1000 | **Active refinement** — owned by @mahboobmonnamd; open PR #1055 (Proposed ADR-018) |
| Local resource addressing / goto / focus history | #1004 | **Active refinement** — owned by @mahboobmonnamd; open PR #1057 (Proposed ADR-019 / SPEC-022) |
| VF-5 Block transcript presentation (Block Component) | #1010 | **Active** — owned by @crdileep82; open PRs #1044 / #1059; not available for pickup |
| ADR-009 prompt-anchor amendment | #1041 | **Active refinement** — owned by @crdileep82; open PR #1045; not available for pickup |
| Composer shell context / Git branch switching | #1036 | **Active docs** — owned by @mahboobmonnamd; open PR #1031; not available for pickup |

**Executable frontier (2026-09-25):**

```text
Hierarchy lane
  #922 active → publish refreshed consumable head/PR
       └─ #923 may develop stacked before #922 merge → #928
  #994 active under @mahboobmonnamd / PR #1056 ─┐
                                                └→ provisioning implementation → #936
  #1000 active under @mahboobmonnamd / PR #1055 (window/tab lifecycle)
  #1004 active under @mahboobmonnamd / PR #1057 (addressing/focus history)
  #1001 active (ADR-021/SPEC-025 Proposed via merged #1053) → acceptance before PaneTree op children

Presentation lane
  #865 active under @mahboobmonnamd / PR #1058 (legacy-ref migration pending) → #866 → #867 → #868 → #869
  #1005 fixtures landed on master (PR #1052); later #868 consumes them
  #1010 active under @crdileep82 / PRs #1044 / #1059 (VF-5)
  #1041 active under @crdileep82 / PR #1045 (ADR-009 prompt-anchor)
  #1036 active under @mahboobmonnamd / PR #1031 (Composer shell context)

Configuration lane
  #993 active under @anulalbs / PR #1006
  #1002 active under @mahboobmonnamd / PR #1051 (keybinding schema)
  #1003 active under @mahboobmonnamd / PR #1050 (startup shell/env/CWD)

Shell-metadata decision
  #686 active under @mahboobmonnamd; duration amendment accepted via merged #1022;
    remaining open #686 work (SPEC-008 / spike acceptance / implementation-Issue refinement)
    only gates behavior that still needs that remaining contract work
```

#993 is already active under @anulalbs / PR #1006 and is not available for pickup. #1001 / #1002 / #1003 are active under @mahboobmonnamd (merged #1053; open #1051 / #1050) and are not available for pickup. #1005 is Done on `master` via #1052. #994, #1000, and #1004 are active refinements under @mahboobmonnamd (open PRs #1056 / #1055 / #1057 on `mahboobmonnamd/issue/<n>`) and are not available for pickup; #686 remains active under @mahboobmonnamd with the duration amendment already accepted via merged #1022. #865 is owned by @mahboobmonnamd with implementation PR #1058 open; its legacy `issue/865` / `cursor/live-tail-865-c8cd` refs still await migration to `mahboobmonnamd/issue/865`, and it is not available for pickup. #1010 / #1041 are owned by @crdileep82 (open PRs noted above). #1036 is owned by @mahboobmonnamd via PR #1031. None may edit M002 terminal-state/VT/Unicode/reflow authorities. #923 does not wait for #922 merge; it waits only until #922 publishes a refreshed, consumable action/snapshot head that can be used as the stack base. #936 cannot absorb or invent the execution-provisioning protocol; that boundary is owned by #994 and its eventual accepted implementation child.

Contributor ownership follows #997 / merged PR #998: work remains human-owned, with agents as delegated tools/co-authors. #989 / PR #990 were closed as superseded. New branches use the human owner's namespace per the merged `ISSUE-PROTOCOL.md`.

### 5.4 Groomed contributor frontier

Current contributor-ready M003 work is maintained in #999. Repeated references below are summaries of the same Issues, not duplicate pickup authority.

**Start now — currently empty (live GitHub, 2026-09-25)**
- No unclaimed, unblocked M003 Issue exists at this snapshot. Every refinement lane is owned (below) and the remaining production slices are stacked on a consumable upstream head. Under §5.5 this is a scarcity signal: #999 must replenish the pool before new contributors arrive.

**Active / already owned**
- #994 — advanced/core — human owner @mahboobmonnamd; TerminalExecution provisioning contract in open PR #1056 (Proposed ADR-017), not available for pickup.
- #1000 — advanced/core — human owner @mahboobmonnamd; native window/tab lifecycle contract in open PR #1055 (Proposed ADR-018), not available for pickup.
- #1004 — standard — human owner @mahboobmonnamd; local resource addressing/goto/focus-history semantics in open PR #1057 (Proposed ADR-019 / SPEC-022), not available for pickup.
- #865 — advanced/core — human owner @mahboobmonnamd; Flow live-tail implementation PR #1058 open on legacy `issue/865` pending migration to `mahboobmonnamd/issue/865`, not available for pickup.
- #993 — standard — human owner @anulalbs; production config/theme/font startup implementation in PR #1006, not available for pickup.
- #1001 — standard — human owner @mahboobmonnamd; ADR-021 / SPEC-025 Proposed on `master` via merged #1053; remaining acceptance/DoD still open; not available for pickup.
- #1002 — standard — human owner @mahboobmonnamd; keybinding/chord schema refinement in open PR #1051, not available for pickup.
- #1003 — advanced/core — human owner @mahboobmonnamd; startup shell/environment/CWD launch-policy refinement in open PR #1050, not available for pickup.
- #686 — advanced/core — human owner @mahboobmonnamd; ADR-009 duration amendment accepted on `master` via merged #1022; remaining open work is SPEC-008 alignment, unchecked spike acceptance, and any follow-on implementation-Issue refinement — not available for pickup.

**Done / landed (do not reopen as start-now)**
- #1005 — starter — retained Flow/long-output/TUI workload fixtures landed via merged #1052; later #868 consumes them.

**Near-ready stacked work**
- #923 / #934 after a refreshed consumable #922 head; merge need not wait for #922.
- #928 after a consumable #923 head; merge need not wait for #923.

**Not active M003 contributor work**
- #926/#927/#930 agent product surfaces are deferred to M005.
- #929 Sessions center is blocked on authoritative Runtime session/execution inventory and is not an active contributor pickup.
- #931 resource center is deferred until resource authority exists.
- #921 duplicate and #937 stale orchestration are closed.

### 5.5 Development-capacity rule

For the active M003 milestone, planning must maintain a continuously groomed **open-source contributor pool** instead of preparing work only when a known developer becomes idle.

- Do **not** size the Ready queue to a fixed team count. Seyal must be able to absorb additional contributors without waiting for maintainers to invent work after they arrive.
- Maintain a healthy surplus of startable Issues across production, testing, performance, documentation/tooling, and bounded refinement/R&D. The queue should be replenished before it becomes scarce, based on observed contributor demand and completion rate rather than a fixed developer number.
- Distinguish **start dependency** from **merge dependency**. A stable upstream branch/PR may be consumed by a stacked downstream PR; merge order is preserved without forcing idle time.
- Use **Blocked** only for a real missing contract/authority, conflicting ownership, or unavailable required interface — not merely because an upstream PR has not merged.
- Umbrella Issues (#674/#675/#676) are planning parents, never execution locks.
- Ready-for-Refinement is valid active work when its output is the accepted contract required for a later production slice; it must not contain production implementation.
- Keep multiple startable items in each independent active lane where architecture permits: hierarchy, presentation, configuration, validation/performance, documentation/tooling, and architecture/refinement.
- Classify contributor suitability explicitly: **starter**, **standard**, **advanced/core**. Core terminal/runtime authority changes remain advanced and tightly reviewed; OSS contributors should still have meaningful production work outside those protected seams.
- A contributor-facing issue must be self-contained enough that a new contributor can understand scope, dependencies, tests, and success criteria without private context.
- A stale historical branch or closed PR must receive an explicit handoff/disposition; it must not silently reserve a ticket forever.

This contributor pool is limited to work whose architecture/dependency entry conditions are already satisfied. It does not authorize beginning blocked future-milestone implementation merely to manufacture tickets.

After this freeze, **do not create new M003 implementation Issues** except:

1. a Ready child that decomposes #674, #675, or #676 into one independently reviewable outcome, or
2. a production defect that makes a remaining §5/#6 criterion `FAIL` or `INCONCLUSIVE`, or
3. an amended frozen finding from independent review of those remaining Issues.

---

## 6. Remaining issue contracts

### 6.1 Parallel with late M002

Roadmap:

> M003 may develop in parallel with late M002 only at stable boundaries; terminal-state/VT/Unicode/reflow changes remain owned by the terminal lane.

Stable boundary for this freeze: M002.1–M002.9 are closed. Remaining M002 is workload-matrix and measured performance (#824, #673, #837). M003 PRs must not:

- change parser / `TerminalState` / HistoryStore / Unicode / reflow / terminfo / keyboard-protocol contracts
- steal #824, #673, or #837
- claim M002 technical preview as a result of workspace chrome

Lane B (native workspace) and lane A/C (terminal + performance) may run together. They must not edit the same authoritative subsystem.

### 6.2 #674 — umbrella, not a pickup

#674 remains the hierarchy/navigation parent. It is too large for one production PR (windows, tabs, nested splits, move/reparent, zoom, resource addressing, palette richness already partly landed). Mark children **Ready** only after `docs/engineering/ISSUE-PROTOCOL.md` checkboxes pass. If a child needs a reusable windows/tabs/splits behavioral contract that SPEC-008 does not own, stop and refine a specification **before** coding; do not invent that spec in an implementation PR.

The window/tab half of that missing contract is refined by #1000: [`../architecture/ADR-018-NATIVE-WINDOW-TAB-LIFECYCLE.md`](../architecture/ADR-018-NATIVE-WINDOW-TAB-LIFECYCLE.md) (**Proposed**) with child decomposition in [`../engineering/M003-WINDOW-TAB-LIFECYCLE-DECOMPOSITION.md`](../engineering/M003-WINDOW-TAB-LIFECYCLE-DECOMPOSITION.md). A child must not code window/tab lifecycle behavior before ADR-018 acceptance.

The pane move/reparent/zoom/equalize/directional-focus half of that missing contract is refined by #1001 (active under @mahboobmonnamd): [`../architecture/ADR-021-PANE-TREE-OPERATIONS.md`](../architecture/ADR-021-PANE-TREE-OPERATIONS.md) (**Proposed** on `master` via merged #1053) with observable behavior in [`../specs/SPEC-025-M003-PANE-TREE-OPERATIONS.md`](../specs/SPEC-025-M003-PANE-TREE-OPERATIONS.md) and child decomposition in [`../engineering/M003-PANE-TREE-OPERATIONS-DECOMPOSITION.md`](../engineering/M003-PANE-TREE-OPERATIONS-DECOMPOSITION.md). Focus-history retention and Resource Addressing are #1004 (ADR-019 **Proposed** on `master` via merged #1057). Window/tab lifecycle remains #1000 (open PR #1055). Execution provisioning remains #994 (open PR #1056). ADR-021 / SPEC-025 are not Accepted until acceptance is recorded; a child must not code PaneTree operation behavior before ADR-021 acceptance.

### 6.3 #923 — split-tree projection after #922

Project the Tab `PaneTree` into visible Pane regions. Only the focused Pane hosts the live terminal/Metal/composer surface in that slice. Multiple simultaneous live PTY/Metal surfaces stay #936. The historical Cursor branch has been explicitly abandoned/superseded. #923 does **not** require #922 to merge: once #922 publishes a refreshed/current consumable branch or PR with stable action/snapshot fields, #923 may start stacked on that head and rebase after #922 lands.

### 6.4 #686 — shell-metadata refinement

#686 remains active decision/refinement work owned by @mahboobmonnamd. The accepted zsh path already exists; Bash/fish remain Unsupported/Raw. The ADR-009 duration amendment is **accepted on `master` via merged #1022** (superseded predecessor #991 is historical only). Remaining open #686 work is SPEC-008 alignment to that accepted duration/trust boundary, closing the Issue's unchecked spike acceptance criteria, and refining any separate implementation Issue before production code — do not treat #1022 as in-flight. Do not block unrelated Flow compositor or config work on #686. Only behavior that still needs that remaining contract work waits on #686.

### 6.5 #675 / #865–#867 — same-execution presentation

Blocks, composer, Raw, and TUI are projections of one `TerminalExecution`. Do not start #675 as a bundle. #865's technical scope is Ready because its compositor work is pane-local and #861 is complete; it does not require #923. #865 is now owned by @mahboobmonnamd with implementation PR #1058 open and is not available for pickup. Legacy `issue/865` and `cursor/live-tail-865-c8cd` are pre-#997 ownership artifacts still awaiting migration to `mahboobmonnamd/issue/865`; closed PR #874 is historical only. #866 follows #865, and #867 follows #866. Their final multi-pane integration is validated later with the hierarchy lane; none may create a second terminal authority.

### 6.6 #676 / #993 — configuration frontier

#740 already owns Rust TOML/theme/config semantics. #993 is the first M003 production child and is now active under @anulalbs / PR #1006: it wires those existing semantics into actual app startup and thin-host visual realization. It is intentionally independent from #922/#923. Keybinding/chord schema (#1002 / open PR #1051) and startup shell/env/CWD launch policy (#1003 / open PR #1050) are already active under @mahboobmonnamd; do not duplicate them or implement #676 wholesale.

### 6.7 #994 — execution provisioning before #936

Current production `ShellState` deliberately keeps tab creation and pane splitting fail-closed until a distinct execution route exists. Runtime can create multiple executions internally, but the headed product lacks an accepted client→Runtime provisioning contract. #994 owns that refinement. If a new IPC/public protocol shape is required, its ADR/spec must land separately before production work. #936 must consume the accepted seam; it must not invent execution creation inside renderer/AppKit code.

### 6.8 Closed M001.1 corrective work

#890 and #904 are closed. They are historical corrective authority/evidence, not remaining M003 work and not implementation pickups.

### 6.7 #676 / #1003 — startup launch policy (refinement)

Umbrella #676 remains unassigned and is not a pickup. The startup shell/environment/CWD half of that umbrella is refined by #1003: [`../architecture/ADR-020-STARTUP-SHELL-ENV-CWD-LAUNCH-POLICY.md`](../architecture/ADR-020-STARTUP-SHELL-ENV-CWD-LAUNCH-POLICY.md) (**Proposed**) and [`../specs/SPEC-023-M003-STARTUP-LAUNCH-POLICY.md`](../specs/SPEC-023-M003-STARTUP-LAUNCH-POLICY.md) (**Proposed**), with child drafts in [`../engineering/M003-LAUNCH-POLICY-DECOMPOSITION.md`](../engineering/M003-LAUNCH-POLICY-DECOMPOSITION.md). The typed `EffectiveLaunchPolicy` is the input the #994 / proposed ADR-017 (PR #1056) provisioning seam resolves for profile `0`. No launch-policy child is Ready before ADR-020 / SPEC-023 acceptance; provisioning consume paths also wait on ADR-017. Themes/fonts/keybindings and broader TOML schema remain separate #676 follow-ons. Do not assign #676 from this refinement.

---

## 7. IME and input honesty

SPEC-006 / ADR-011 / ADR-015 remain input/IME authority. M003 presentation transitions follow the Rust-freeze / native-revoke fence in SPEC-008. Native owns only marked text and disposable editor cache. Do not pull multilingual IME breadth (#836) into M003. Do not route Flow keystrokes through a hidden raw terminal viewport.

---

## 8. Final milestone validation

M003 may be marked Done only when every mandatory criterion below is `PASS` on **one freeze SHA**, using `.agents/skills/milestone-validation/SKILL.md` and the M001 Pass 10 verdict model (`PASS` / `FAIL` / `INCONCLUSIVE` / `PLATFORM_LIMITED` / `N/A`). `PLATFORM_LIMITED` is not automatic PASS.

### 8.1 Evidence classes

Label every performance/presentation claim: `CI` | `controlled-host` | `PLATFORM_LIMITED`. Foundation `make bench` with `SEYAL_REQUIRE_DISPLAY_LINK_BENCHMARK=0` is `CI` only.

### 8.2 Mandatory close-out criteria

**Architecture**

- one authoritative `TerminalState` per `TerminalExecution`; no second VT/grid/renderer
- tabs/splits/windows never own a PTY; a terminal leaf binds at most one execution
- Rust owns portable product/UI; Swift remains a thin host (ADR-015)
- Flow/Raw/TUI are mutually exclusive; Flow has no coexisting raw input viewport
- closing/reparenting chrome does not terminate an unrelated live execution
- no agent/persistence/cloud/licensing on the terminal hot path
- OSS has no commercial dependency

**Workspace**

- #674 children covering windows/tabs/nested splits/focus and the #994-derived execution-provisioning seam are Done or explicitly classified
- #675 / #865–#867 presentation rows are usable with real shell/TUI workloads, or classified
- #676 children, beginning with #993, prove config/themes/fonts/keybindings/launch policy are usable without an account
- #686 remaining open decision output (beyond the duration amendment already accepted via merged #1022) is accepted before any #675 semantic-boundary claim that still depends on that remaining contract work
- #936 multi-live Metal uses distinct Runtime-owned executions through the accepted provisioning seam and is Done or explicitly deferred with an owning follow-up

**Performance / resources**

- window/tab/pane create and focus/switch latency recorded
- idle CPU/RSS with hidden/occluded panes recorded
- 1/10/50/100 presentation scaling distinguished from real PTY count
- M002 hot-path and history-cap gates are not silently weakened

**Engineering**

- exact-head `make bootstrap`, `make build`, `make test`, `make check`, `make bench`
- native XCTest / XCUI required by the remaining headed slices
- clean-checkout demo
- independent review of the freeze SHA with no unresolved red/orange blocker
- non-goals in §4 still deferred

### 8.3 Demo (clean checkout, freeze SHA)

```sh
make bootstrap
make build
make test
make check
make bench
```

Plus the headed window/tab/split/focus procedure recorded by the remaining #674/#923/#936 evidence files, and the Flow/Raw/TUI procedure from #868 when that Issue is in the freeze set.

---

## 9. What “M003 Done” means

Epic #665 reaches Done when the §8 ledger is `PASS` on the freeze SHA and the §5 umbrellas/required children are Done or explicitly classified. It authorizes **beta** in the roadmap release-channel sense. It does **not** authorize M004 launch claims, M002 technical-preview claims, or agent production.

M002 performance/workload close-out may still be open at that moment only if every M003 merge stayed off the terminal-state/VT/Unicode/reflow contracts. If an M003 change violated that seam, stop and restore the terminal lane before claiming either milestone.
