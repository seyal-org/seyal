# Milestone 003 — Core Seyal Workspace

**Status:** **In progress.** Not Done. This file is the M003 implementation contract. It does **not** claim beta, M003 complete, or market-ready.

**GitHub epic:** [#665](https://github.com/seyal-org/seyal/issues/665)  
**Hierarchy umbrella:** [#674](https://github.com/seyal-org/seyal/issues/674)  
**Presentation umbrella:** [#675](https://github.com/seyal-org/seyal/issues/675)  
**Config umbrella:** [#676](https://github.com/seyal-org/seyal/issues/676)  
**Shell-integration spike:** [#686](https://github.com/seyal-org/seyal/issues/686) — **Closed** (not a remaining pickup)

**Entry gate:** M001 Done. M002.1–M002.9 terminal contracts are closed on the permanent path. Remaining M002 close-out (#824/#672, #673 PHYSICAL_ARM64, #837, then freeze-SHA validation and #664) may run **in parallel** and is **not** an M003 product blocker. #673 does not wait on #824 closure.

**Authority:** This document is subordinate to the accepted Seyal foundation architecture, accepted ADRs (especially ADR-007, ADR-009, ADR-015, and M003 ADR-017 / ADR-018 / ADR-019 / ADR-020 / ADR-021), SPEC-006 / SPEC-008 / SPEC-009 / SPEC-022 / SPEC-023 / SPEC-024 / SPEC-025, and [`docs/architecture/ui/SEYAL-UI-ARCHITECTURE-001.md`](../architecture/ui/SEYAL-UI-ARCHITECTURE-001.md). It narrows those authorities into an M003 contract. It does not reopen architecture, invent a second terminal engine, or override GitHub Issue acceptance with prose. It does **not** claim M003 Done.

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

Approved umbrellas are **#674 / #675 / #676**. Spike **#686** is **Closed**. Headed leftover slices after M001.1 parent #878 closed now live under this contract. Snapshot **2026-10-05** (live GitHub remains authority over stale prose). This refresh defines the executable development frontier. It does **not** claim M003 Done, beta, or market-ready.

### 5.1 Already on `master` (do not reopen)

These landings are **foundation and accepted contracts plus landed children**, not M003 Done. The headed host is still a one-live-Metal-leaf adapter until #923 / #936, and tab/pane create remains fail-closed until C2b / C3.

**M001.1 / domain foundations**

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
| Trusted shell-integration spike | #686 / PRs #1022, #1201 | **Closed** |

**Accepted M003 contracts (not implemented-behavior claims)**

| Slice | Issue | Status |
|---|---|---|
| Execution provisioning / disposition | #994 / ADR-017 | **Closed**; ADR Accepted on merge of PR #1088 |
| Native window/tab lifecycle | #1000 / ADR-018 | **Closed**; ADR Accepted on merge of PR #1208 |
| Resource addressing / goto / focus-history semantics | #1004 / ADR-019 / SPEC-022 | **Closed**; Accepted on merge of PR #1208 |
| Startup shell/env/CWD launch policy | #1003 / ADR-020 / SPEC-023 | **Closed**; ADR Accepted on merge of PR #1089 |
| Keybinding/chord schema | #1002 / SPEC-024 | **Closed** |
| PaneTree operations contract | #1001 / ADR-021 / SPEC-025 | **Closed**; Accepted on merge of PR #1208 |

**Landed implementation children (L0–L5, K1–K6, N1/N2/N4, W1, C1/C2a, P3/P4/M1)**

| Slice | Issue | Status |
|---|---|---|
| L0 LaunchPolicyRejected (SPEC-004 code 17) | #1113 | **Closed** |
| L1 EffectiveLaunchPolicy type and resolver | #1097 | **Closed** |
| L2 Runtime create applies EffectiveLaunchPolicy | #1102 | **Closed** |
| L3 launch-policy failure/warning UX | #1119 | **Closed** |
| L4 config-declared shell, cwd, login bit | #1214 | **Closed** |
| L5 launch-policy failure injection / streaming | #1166 | **Closed** |
| K1 keybinding schema / KeybindingTable | #1096 | **Closed** |
| K2 defaults, reserved Command, conflicts | #1106 | **Closed** |
| K3 routing gate and Raw/TUI non-interception | #1110 | **Closed** |
| K4 chord prefix state machine | #1124 | **Closed** |
| K5 menu and accessibility shortcut projection | #1125 | **Closed** |
| K6 headed keybinding evidence | #1138 | **Closed** |
| K8 (goto) bind `goto.open` | #1127 | **Closed** |
| N1 ResourceAddress and pure resolver | #1095 | **Closed** |
| N2 navigate by address and palette rows | #1101 | **Closed** |
| N4 goto surface over address-carrying rows | #1118 | **Closed** |
| W1 Rust window containment and WindowId | #1090 | **Closed** |
| C1 portable client provisioning, bind, disposition | #1136 | **Closed** |
| C2a CreateTab scaffolding (gated) | #1149 | **Closed** |
| P3 Runtime provisioning admission and create | #1105 | **Closed** |
| P4 Runtime explicit execution disposition | #1114 | **Closed** |
| M1 provisioning performance/resource/scaling evidence | #1164 | **Closed** |
| Workspace chrome (partial) | #922 / PR #1017 | PR **merged**; Issue **still open** for remaining Done gates — see §5.2 |
| Production startup TOML/theme/font wiring (implementation) | #993 / PR #1006 | PR **merged** (`Refs #993`); Issue **still open** pending product-owner confirmation — see §5.2 |
| Flow live-tail compositor (implementation) | #865 / PR #1058 | PR **merged**; Issue **still open** for remaining acceptance/DoD — see §5.3 |
| Block Component design doc | #1010 / PR #1059 | design-doc PR **merged**; VF-5 product Issue **still open** — see §5.2 |
| Adaptive Depth design doc | #934 / PR #1018 | design-doc PR **merged**; fidelity Issue **still open** — see §5.2 |
| Composer shell-context docs | #1036 / PR #1031 | docs PR **merged**; Issue **still open** |

### 5.2 Handoff-owned remaining chrome / config / VF-5

Do **not** duplicate these Issues. M003 completion is being driven as a campaign. Take a branch/PR only after the assignee or product owner records an explicit ISSUE-PROTOCOL transfer. Do not reassign until they confirm keep vs handoff.

| Slice | Issue | Status (live GitHub, 2026-10-05) |
|---|---|---|
| Workspace chrome remainder | #922 | **Open** — @crdileep82. Partial chrome on `master` via merged PR #1017. Remaining Done gates include e2e create/split (gated on provisioning children) and host keyboard/menu toggles. Handoff requested 2026-10-05. |
| Split-tree host projection | #923 | **Open** — @crdileep82. Blocked only on a consumable remaining-#922 head (not on #922 merge). Abandoned `cursor/issue-923-*` is not production authority. Handoff requested 2026-10-05. |
| Split drag-resize ratios | #928 | **Open** — @crdileep82. Blocked only on a consumable #923 head; may stack before #923 merges. |
| Adaptive Depth chrome fidelity | #934 | **Open** — @crdileep82. Design doc on `master` via PR #1018; presentation-only; stacked after a consumable #922 head. |
| Production startup config DoD | #993 | **Open** — @anulalbs. Implementation on `master` via merged PR #1006 (`Refs #993`). Remaining HUMAN GATE is product-owner confirmation against Issue acceptance. Handoff requested 2026-10-05. |
| VF-5 Block chrome | #1010 | **Open** — @crdileep82. Open PR #1044 (`Refs #1010`, branch `crdileep82/issue/1010`). Related: #1042 / #1043 / #1041 / #1062. Handoff requested 2026-10-05. |
| ADR-009 prompt-anchor | #1041 | **Open** — @crdileep82. Propose PR #1045 **merged**; ADR-009 prompt-anchor section remains **Proposed**, not normative. |

Historical `cursor/issue-922-*` / `cursor/issue-923-*` branches are not current production authority. Do not resume old branches implicitly.

Agent inventory/Agents-center/attention product work (#926/#927/#930) is not part of the M003 critical path; `docs/product/ROADMAP.md` assigns the agent-native local substrate and Attention foundations to M005 (#667/#680). The specific Sessions/Resources **center views** (#929/#931) are likewise not M003 close blockers. This does **not** defer #674's required stable local `Session` identity or hierarchy semantics; it only avoids making those optional center surfaces a prerequisite. #937 is historical orchestration, not an implementation pickup.

### 5.3 Remaining open children (do not treat as M003 Done)

Umbrellas **#674 / #675 / #676** remain **Open** and are never pickups. Live GitHub, 2026-10-05:

**Lane A — provisioning / multi-execution**

| Slice | Issue | Status |
|---|---|---|
| C2b enable `CreateTab` with live second Controller | #1175 | **Open** — @mahboobmonnamd; C1 + C2a already closed |
| C3 enable pane splitting, one execution per leaf | #1217 | **Open** — @mahboobmonnamd |
| Multiple live Metal surfaces per Tab | #936 | **Open** — unassigned; **Blocked** on #923 plus C3 through accepted ADR-017 |

**Lane B — windows/tabs (ADR-018; user close gated on W6)**

| Slice | Issue | Status |
|---|---|---|
| W2a Rust window/tab actions without close | #1092 | **Open** — @mahboobmonnamd |
| W3 versioned multi-window snapshot / FFI | #1108 | **Open** — @mahboobmonnamd |
| W4a thin AppKit multi-window, no user close | #1123 | **Open** — @mahboobmonnamd |
| W6 live-unpresented enumerate/adopt/terminate | #1143 | **Open** — @mahboobmonnamd |
| W2b Rust presentation removal without terminate | #1153 | **Open** — @mahboobmonnamd |
| W4b headed close / last-window / zero-window | #1158 | **Open** — @mahboobmonnamd |
| S1 SPEC-004 delivery-suspend + capacity | #1162 | **Open** — @mahboobmonnamd |
| W5 presentation tiers | #1220 | **Open** — @mahboobmonnamd; needs W3/W4a, #923, S1 |
| W7 adversarial matrix + §8.2 measurements | #1221 | **Open** — @mahboobmonnamd; after W1–W6, PT4/PT5, #936 |

**Lane C — PaneTree (ADR-021)**

| Slice | Issue | Status |
|---|---|---|
| PT1 zoom overlay and close successor | #1122 | **Open** — @mahboobmonnamd |
| PT2 swap / move-beside | #1131 | **Open** — @mahboobmonnamd |
| PT3 directional pane focus | #1142 | **Open** — @mahboobmonnamd |
| PT4 equalize nested split ratios | #1218 | **Open** — @mahboobmonnamd; after #928 |
| PT5 snapshot/FFI + thin host verbs | #1219 | **Open** — @mahboobmonnamd; after #923 + PT1–PT3 |
| PT6 property/adversarial suite | #1167 | **Open** — @mahboobmonnamd |

**Lane D — navigation + remaining keybindings**

| Slice | Issue | Status |
|---|---|---|
| N3 focus history Back/Forward | #1117 | **Open** — @mahboobmonnamd |
| N5 cross-window activation | #1144 | **Open** — @mahboobmonnamd; after W4a / multi-window |
| N6 headed navigation evidence | #1156 | **Open** — @mahboobmonnamd |
| K7 bind zoom/swap/move pane verbs | #1145 | **Open** — @mahboobmonnamd; after matching PT verbs + K3 |
| K7 bind directional pane focus | #1150 | **Open** — @mahboobmonnamd |
| K8 bind focus-history Back/Forward | #1132 | **Open** — @mahboobmonnamd; after N3 |

K9 window/hierarchical-close bindings fold into W4a/W4b PRs per SPEC-024 §5.0. Do not invent a duplicate Issue.

**Lane E — same-execution presentation (#675)**

| Slice | Issue | Status |
|---|---|---|
| Flow compositor live tail | #865 | **Open** — @mahboobmonnamd; implementation PR #1058 **merged**; remaining Issue acceptance/DoD |
| Flow composer IME/focus fence | #866 | **Open** — unassigned; **Blocked on #865** remaining Done |
| Raw/TUI full-Pane takeover | #867 | **Open** — @mahboobmonnamd; **Blocked on #866** |
| Headed Flow/Raw/TUI workload matrix | #868 | **Open** — unassigned; **Blocked on #867** |
| Renderer qualification / regression | #869 | **Open** — unassigned; after #868 |
| ViewportLineIds (SPEC-004 type 35) | #1083 | **Open** — @mahboobmonnamd; only if still required by Flow compositor |
| Prompt-anchor implementation | #1042 | **Open** — unassigned; waits on #1041 acceptance |
| Runtime-measured Block duration | #1043 | **Open** — unassigned |
| Light-theme Metal cell background bug | #1062 | **Open** — unassigned; headed Block evidence if still failing |

**Lane F — config remainder (#676)**

Finish #993 product-owner confirmation after handoff (§5.2). Leftover theme/font/keybinding user-docs only if #993 / K6 did not already close them. #686 is Closed; do not reopen it as a pickup. A #675 semantic-boundary claim may depend on a new Issue only if it still needs contract work beyond the closed spike.

**Executable frontier (2026-10-05):**

```text
Handoff first (do not duplicate; no reassignment until keep vs handoff)
  #922 @crdileep82 (PR #1017 merged, Issue open) → #923 → #928
  #993 @anulalbs (PR #1006 merged, DoD confirmation open)
  #1010 @crdileep82 (open PR #1044)

Provisioning
  C2b #1175 → C3 #1217 → #936 (also needs #923)

Windows (never merge Tab/Window close before W6)
  W2a #1092 → W3 #1108 → W4a #1123
  W2a #1092 → W6 #1143 → W2b #1153 → W4b #1158
  W3 + #923 + S1 #1162 → W5 #1220 → W7 #1221

PaneTree
  PT1 #1122 → PT2 #1131 → PT3 #1142
  #928 → PT4 #1218
  #923 + PT1–PT3 → PT5 #1219 → PT6 #1167

Presentation
  #865 remainder → #866 → #867 → #868 → #869
  VF-5 #1010 / PR #1044; #1041 Proposed; #1042/#1043/#1062 follow-ups

Config
  #993 owner confirmation; L0–L5 and K1–K6 already Closed
```

#936 must consume the accepted ADR-017 seam; it must not invent execution creation inside renderer/AppKit code. None of these children may edit M002 terminal-state/VT/Unicode/reflow authorities.

Contributor ownership follows #997 / merged PR #998: work remains human-owned, with agents as delegated tools/co-authors. #989 / PR #990 were closed as superseded. New branches use the human owner's namespace per the merged `ISSUE-PROTOCOL.md`.

### 5.4 Groomed contributor frontier

Current contributor-ready M003 work is maintained in #999. Repeated references below are summaries of the same Issues, not duplicate pickup authority.

**Start now — unclaimed and unblocked (live GitHub, 2026-10-05)**
- Scarce. Most remaining M003 children are either assigned to the completion campaign (@mahboobmonnamd), assigned to @crdileep82 / @anulalbs (handoff requested; not free pickups), or explicitly blocked. Do not manufacture fake Ready tickets. #1043 / #1062 may be independently startable if their Issue Ready gates still pass; verify before claiming.

**Handoff-owned (not available for pickup)**
- #922 / #923 / #928 / #934 / #1010 / #1041 — @crdileep82
- #993 — @anulalbs

**Campaign-owned remaining children (not available for pickup)**
- Provisioning: #1175, #1217
- Windows: #1092, #1108, #1123, #1143, #1153, #1158, #1162, #1220, #1221
- PaneTree: #1122, #1131, #1142, #1218, #1219, #1167
- Navigation/keybindings: #1117, #1144, #1156, #1145, #1150, #1132
- Presentation: #865, #867, #1083

**Stacked / blocked (not start-now)**
- #923 / #934 after a consumable remaining-#922 head
- #928 after a consumable #923 head
- #936 after #923 + C3
- #866 after #865 Done; #868 after #867; #869 after #868
- #1042 after #1041 acceptance

**Closed contract/foundation Issues (do not reopen as start-now)**
- #686, #994, #1000–#1005, L0–L5, K1–K6, N1/N2/N4, W1, C1/C2a, P3/P4/M1, and the M001.1 foundations in §5.1

**Not M003 close blockers (classify; do not implement as M003)**
- #926/#927/#929/#930/#931 → M005
- Durable restore / signing → M004
- Spike #739 external-editor attach → M003/M004 spike, not §8 freeze
- Cohesion #1069 unless a remaining slice is blocked by it
- M002 #824/#673/#837: parallel only

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

The pane/tab execution provisioning seam is accepted under #994: [`../architecture/ADR-017-EXECUTION-PROVISIONING-AND-DISPOSITION.md`](../architecture/ADR-017-EXECUTION-PROVISIONING-AND-DISPOSITION.md) (**Accepted** on merge of PR #1088) with child drafts in [`M003-674-EXECUTION-PROVISIONING-CHILDREN.md`](M003-674-EXECUTION-PROVISIONING-CHILDREN.md). C1 / C2a / P3 / P4 / M1 are Closed; remaining production consume path is C2b #1175 then C3 #1217, then #936.

The window/tab contract is #1000: [`../architecture/ADR-018-NATIVE-WINDOW-TAB-LIFECYCLE.md`](../architecture/ADR-018-NATIVE-WINDOW-TAB-LIFECYCLE.md) (**Accepted** on merge of PR #1208). W1 is Closed. Remaining children are listed in §5.3. Do not merge user Tab/Window close before W6.

The pane move/reparent/zoom/equalize/directional-focus contract is #1001: [`../architecture/ADR-021-PANE-TREE-OPERATIONS.md`](../architecture/ADR-021-PANE-TREE-OPERATIONS.md) (**Accepted** on merge of PR #1208) with observable behavior in [`../specs/SPEC-025-M003-PANE-TREE-OPERATIONS.md`](../specs/SPEC-025-M003-PANE-TREE-OPERATIONS.md). Focus-history retention and Resource Addressing are #1004 (ADR-019 / SPEC-022 **Accepted**). Remaining PT/N children are listed in §5.3.

### 6.3 #923 — split-tree projection after #922

Project the Tab `PaneTree` into visible Pane regions. Only the focused Pane hosts the live terminal/Metal/composer surface in that slice. Multiple simultaneous live PTY/Metal surfaces stay #936. The historical Cursor branch has been explicitly abandoned/superseded. #923 does **not** require #922 to merge: once #922 publishes a refreshed/current consumable branch or PR with stable action/snapshot fields, #923 may start stacked on that head and rebase after #922 lands.

### 6.4 #686 — shell-metadata spike (Closed)

#686 is **Closed**. The accepted zsh path already exists; Bash/fish remain Unsupported/Raw. The ADR-009 duration amendment is on `master` via merged #1022; SPEC-008 shell-matrix / duration/CWD close-out landed via merged #1201. Do not reopen #686 as a pickup. Do not block unrelated Flow compositor or config work on it.

### 6.5 #675 / #865–#867 — same-execution presentation

Blocks, composer, Raw, and TUI are projections of one `TerminalExecution`. Do not start #675 as a bundle. #865 implementation PR #1058 is **merged**; the Issue remains **open** for remaining acceptance/DoD and is not available for pickup. Closed PR #874 is historical only. #866 follows #865 Done, and #867 follows #866. Their final multi-pane integration is validated later with the hierarchy lane; none may create a second terminal authority.

### 6.6 #676 / #993 — configuration frontier

#740 already owns Rust TOML/theme/config semantics. #993 implementation PR #1006 is **merged** (`Refs #993`); the Issue remains **open** under @anulalbs pending product-owner confirmation. Do not duplicate that wiring. L0–L5, K1–K6, #1002, and #1003 are Closed. Do not implement #676 wholesale.

### 6.7 #994 — execution provisioning before #936

ADR-017 is **Accepted**. C1 / C2a / P3 / P4 / M1 are Closed. Production `CreateTab` / pane splitting remain fail-closed until C2b #1175 and C3 #1217 land. #936 must consume that accepted seam; it must not invent execution creation inside renderer/AppKit code.

### 6.8 Closed M001.1 corrective work

#890 and #904 are closed. They are historical corrective authority/evidence, not remaining M003 work and not implementation pickups.

### 6.9 #676 / #1003 — startup launch policy (Accepted)

Umbrella #676 remains unassigned and is not a pickup. #1003 / ADR-020 / SPEC-023 are **Accepted** (PR #1089). L0–L5 children that implement that contract are Closed. The typed `EffectiveLaunchPolicy` is the input the accepted ADR-017 provisioning seam resolves for profile `0`. Remaining #676 product work is #993 confirmation and any leftover user-docs, not a second launch-policy contract. Do not assign #676 from this section.

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
- #686 is Closed; a #675 semantic-boundary claim may depend on a new Issue only if it still needs contract work beyond that closed spike
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
