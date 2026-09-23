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
stealing assigned issues (#686, #865, #890, #904, #824, …)
creating issue/922 or issue/923 while cursor/issue-922-* or cursor/issue-923-* exist
  without an explicit resume or abandon decision
reopening merged #932 / #933 / #935
```

A FAIL or INCONCLUSIVE **mandatory** criterion on a remaining §5 issue may create one scoped child. That is defect closure, not a new product slice.

---

## 5. Finding-set freeze and remaining chain

Approved umbrellas are **#674 / #675 / #676**, plus spike **#686**. Headed leftover slices after M001.1 parent #878 closed now live under this contract. Snapshot **2026-09-16** (live GitHub is authority over stale issue-body prose, including #937 merge-order text).

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

These landings are **foundation**, not M003 Done. The headed host is still a one-live-Metal-leaf adapter until #923 / #936.

### 5.2 Remaining headed leftovers (under M003 after #878 closed)

| Slice | Issue | Status |
|---|---|---|
| Workspace chrome (left / tabs / inspector) | #922 (duplicate title #921) | **Open**; Cursor branch `cursor/issue-922-workspace-chrome-c6e6` exists |
| Split-tree projection; one live Metal leaf | #923 | **Open**; Cursor branch `cursor/issue-923-multipane-c6e6` exists |
| Split drag-resize ratios | #928 | **Open** — after #923 |
| Attention bell + popover | #926 | **Open**; Cursor branch `cursor/issue-926-attention-popover-c6e6` exists |
| Left Workspaces agents inventory | #927 | **Open** |
| Agents center view | #930 | **Open** — after #927 |
| Adaptive Depth chrome fidelity | #934 | **Open** |
| Multiple live Metal surfaces per Tab | #936 | **Open — Blocked** on #674 and #923 |
| Sessions center | #929 | **Open — Blocked** on session inventory authority |
| Resources center | #931 | **Open — Blocked** on resource inventory authority |
| Chrome merge-order map | #937 | **Open** orchestration; stale vs merged #932/#933/#935 |

Do **not** create `issue/922` or `issue/923` while those Cursor branches exist unless the owner explicitly resumes or abandons them.

### 5.3 Umbrellas, presentation, and spike

| Slice | Issue | Status |
|---|---|---|
| Native hierarchy / windows / tabs / splits / navigation | #674 | **Open** umbrella — **not** one Ready implementation PR |
| Pane input, Blocks, selection, same-execution presentation | #675 | **Open** — needs #674 pane/focus model; #686 where semantic boundaries are required |
| Local config / themes / fonts / keybindings / launch policy | #676 | **Open** |
| Trusted shell-integration / semantic command boundaries | #686 | **Open** spike; assignee **@mahboobmonnamd** — do not steal |
| Flow compositor Block-region drawing | #865 | **Open**; assignee **@mahboobmonnamd** — do not steal |
| Flow composer input/IME/focus fence | #866 | **Open** |
| Raw/TUI full-Pane takeover | #867 | **Open** |
| Headed Flow/Raw/TUI workload matrix | #868 | **Open** — after presentation slices |
| Renderer qualification / regression | #869 | **Open** — after presentation slices |

**Start order (mandatory for new pickups):**

```text
this freeze (#971)
  → settle Cursor-branch resume-or-abandon for #922/#923
  → #923 split-tree projection (one live Metal leaf)
  → decompose #674 into Ready children (do not implement the umbrella)
  → #936 multi-live Metal only after that decomposition and #923
  → #675 / #866 / #867 behind the pane model and #686 where required
  → #676 config/themes/keybindings
  → #868 / #869 headed acceptance
  → milestone-validation on one freeze SHA
  → mark epic #665 Done
```

#928, #926, #927, #934 may proceed only when they do not collide with an active #922/#923 claim and do not mutate terminal-state contracts.

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

### 6.3 #923 — first headed slice after branch resolution

Project the Tab `PaneTree` into visible Pane regions. Only the focused Pane hosts the live terminal/Metal/composer surface in that slice. Multiple simultaneous live PTY/Metal surfaces stay #936. Resume the existing Cursor branch only with an explicit resume request and sole assignee; otherwise abandon that branch before creating `issue/923`.

### 6.4 #686 — spike, assigned

#686 answers how trusted prompt/CWD/command-start/end signals work for zsh/bash/fish without Warpify-style injection. It unblocks #675 semantic Block boundaries. It is a spike/ADR-or-spec output, not a silent production hook. Keep the current assignee.

### 6.5 #675 / #865–#867 — same-execution presentation

Blocks, composer, Raw, and TUI are projections of one `TerminalExecution`. Do not start #675 as a bundle. Prefer the existing presentation Issues (#865–#867) once the pane/focus model exists. Do not steal #865.

### 6.6 #890 / #904 — not M003 product slices

These remain M001.1 recovery/architecture Issues assigned to **@mahboobmonnamd**. This freeze does not implement, close, or reassign them.

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

- #674 children covering windows/tabs/nested splits/focus are Done or explicitly classified
- #675 / #865–#867 presentation rows are usable with real shell/TUI workloads, or classified
- #676 config/themes/fonts/keybindings/launch policy are usable without an account
- #686 decision output is accepted before #675 semantic-boundary claims depend on it
- #936 multi-live Metal is Done or explicitly deferred with an owning follow-up

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
