# Milestone 002 — Market-Parity Terminal Fundamentals

**Status:** **In progress.** Not Done. This file freezes remaining close-out work. It does **not** claim technical preview, M002 complete, or market-ready.

**GitHub epic:** [#664](https://github.com/seyal-org/seyal/issues/664)
**Compatibility parent:** [#672](https://github.com/seyal-org/seyal/issues/672)
**Performance sibling:** [#673](https://github.com/seyal-org/seyal/issues/673)
**Entry gate:** M001 Done on freeze `c536c5454583f6a036910e145fe1187446319630` (Pass 10 / #727 / #5).

**Authority:** This document is subordinate to the accepted Seyal foundation architecture, accepted ADRs (especially ADR-010, ADR-011, ADR-015), and SPEC-010 / SPEC-011 / SPEC-006 §21. It narrows those authorities into an M002 close-out contract. It does not reopen architecture, invent a second terminal engine, or override GitHub Issue acceptance with prose.

**Why this file exists:** M001 had `MILESTONE-001.md`. M002 previously lived only as epic #664 plus children. That gap let evidence follow-ups and adjacent tracks attach as if they were new M002 product directions. After this freeze, remaining M002 work is only the chain in §5.

**Operational close-out plan:** [`docs/engineering/M002-CLOSEOUT-PLAN.md`](../engineering/M002-CLOSEOUT-PLAN.md) records development-readiness verdicts, review-churn preflight, and Track A/B/C sequencing. It is subordinate to this freeze and to live GitHub ownership; it does not add product scope.

---

## 1. Goal

M002 extends the **same** production path already proven in M001:

```text
PTY → VT/parser → TerminalState/grid → damage → projection → Metal
```

until target shells, TUIs, nested SSH/tmux-as-child, Unicode, scrollback/reflow, selection/search/mouse/keyboard, and measured latency/resource gates are **credible for technical preview**.

M002 is **not** market-ready. That remains M004 / #666. M002 is **not** the agent platform, M003 workspace chrome, or a new VT/renderer.

Roadmap exit ([`docs/product/ROADMAP.md`](../product/ROADMAP.md)):

> Target shells, TUIs, nested mux/SSH workloads, Unicode, scrollback/reflow, terminal search/input/mouse/link behavior and performance/resource gates are credible for technical preview.

---

## 2. Architecture slice

Keep one `TerminalExecution` → one PTY → one authoritative `TerminalState`. ADR-010 and ADR-011 extend that state internally; they do not create a second history, text, or renderer authority. ADR-015: Rust owns portable product/UI; Swift is a thin macOS adapter. Headed oracle is Flow/Blocks unless Raw/TUI takeover is the case under test.

```text
TerminalExecution
  -> authoritative TerminalState
       -> Unicode grapheme / width (ADR-011 / SPEC-011)
       -> canonical retained primary HistoryStore (ADR-010 / SPEC-010)
       -> derived reflow / search / selection indexes
  -> derived display projection (Candidate D, grapheme v2)
  -> Metal / CoreText shaping + cache

AppKit marked/preedit
  -> ephemeral native IME only
  -> committed UTF-8 enters the existing input/Runtime path
```

No synchronous agent, persistence, cloud, licensing, telemetry, Lua, or Block semantic work on the terminal hot path.

---

## 3. In scope

M002 implements and proves, on the permanent path:

- Unicode grapheme/width/fallback and macOS IME **contract** (SPEC-011; language-source breadth is not M002)
- production scrollback, hard/soft lineage, resize reflow, bounded eviction
- selection, copy/paste, search, keyboard copy mode on the terminal path
- VT/xterm/DEC breadth, OSC title/CWD/hyperlinks, queries/modes, terminfo honesty
- application mouse reporting and host-override arbitration
- keyboard compatibility including the bounded Kitty subset (flags 1 and 2 only)
- real-workload matrix: zsh/bash/fish; SSH/nested SSH; Vim/Neovim; tmux-as-child; htop/watch/ncurses; git/Docker/kubectl/terraform; high-volume logs; CLI-agent TUIs as ordinary PTY workloads
- versioned latency/throughput/scaling gates (#673 / SY-017)

---

## 4. Explicit non-goals

Do **not** pull these into M002. They are not “remaining M002 work.”

```text
agent platform / memory / context / M005 production (#838 does not change M002)
M003 windows/tabs/splits/workspace chrome
graphics / image protocols (#689 / M006)
persistence backend / on-disk history format (SPEC-010 non-goal)
complete Kitty keyboard (MARKET-READY: pre-launch optional)
multilingual / input-source IME breadth (#836, post-M004)
Linux/Windows GUI, remote-product attach (SSH-as-child is in M002; M007 is not)
plugins, collaboration, enterprise, cloud
replacing Seyal VT / TerminalState / Metal
reopening closed #815–#823 because issue bodies still say Blocked
```

A FAIL or INCONCLUSIVE **mandatory** criterion on #824, #673, or #837 may create one scoped child. That is defect closure, not a new product slice.

---

## 5. Finding-set freeze and remaining chain

Approved decomposition is **M002.1–M002.10** under #672, plus performance sibling #673. Snapshot **2026-09-18** (live GitHub is authority over stale issue-body prose).

| Slice | Issue | Status |
|---|---|---|
| M002.1 Unicode constants | #815 | **Closed** |
| M002.2 Unicode in `TerminalState` | #816 | **Closed** |
| M002.3 grapheme projection / Metal / IME path | #817 | **Closed**; multilingual IME parked on **#836 (not M002)** |
| M002.4 history budgets | #818 | **Closed** |
| M002.5 HistoryStore + reflow | #819 + #842 | **Closed**; physical five-cohort rows deferred to **#673** |
| M002.6 selection / copy / search | #820 | **Closed** |
| M002.7 VT/OSC/terminfo breadth | #821 | **Closed** |
| M002.8 mouse + host override | #822 | **Closed** |
| M002.9 keyboard protocol | #823 + #834 | **Closed** |
| M002.10 real-workload matrix | #824 | **Open** — remaining #672 child; assignee @anulalbs; PR #964 is `Refs #824` |
| Performance contract | #673 (PR #919) | Contract on `master`; **measured PHYSICAL_ARM64 rows open**; first HistoryStore row on PR #969 is not Done |
| Unicode-heavy ARM64 | #837 | **Open** — after accepted #673 ceilings |

**Epic close recipe (not a start gate):**

```text
#664 closes only after #824/#672, #673, and #837 are Done
  + milestone-validation on one freeze SHA
```

Unresolved tension, written explicitly: #824 issue-local evidence (`performance_claim=false`) is not #673 qualification, and #673 does **not** depend on #672/#824 closure — live #673 and PR #969 may keep measuring. There is no circular dependency. #837 still waits on accepted #673 ceilings.

Do not start #673 or #837 from an #824 PR. Do not steal the #824 or #673 assignee claims. One Issue → one branch → one PR.

After this freeze, **do not create new M002 implementation Issues** except:

1. a production defect that makes a remaining §5/#6 criterion `FAIL` or `INCONCLUSIVE`, or
2. an amended frozen finding from independent review of those remaining Issues.

---

## 6. Remaining issue contracts

### 6.1 #824 — workload matrix and #672 closure

Prove the #672 matrix on the permanent path. Retain automated fixtures **or** an explicit repeatable native/manual case per required workload. Classify `PLATFORM_LIMITED` / `ENVIRONMENT_UNSUPPORTED` honestly; they are not automatic PASS.

Still required before `Closes #824` / `Closes #672`:

- exclusive-Runtime headed Flow/Blocks XCUI (a leftover `control.sock` owner is `INCONCLUSIVE`, not PASS)
- the ten headed/manual steps in the Issue body
- fuzz/security/terminfo honesty
- independent review with no unresolved red/orange blocker
- `performance_claim=false` here; #673 remains release-performance authority
- SPEC-011 IME fixtures 37–41 classified under §7 (do **not** pull #836)

PR [#964](https://github.com/seyal-org/seyal/pull/964) is `Refs #824` only until those gates are green. Head `204d14c` (docs sync on remediation `ad50bd1`) has hosted Foundation Quality / native-macos-smoke / production fuzz green. Independent review **CHANGES_REQUESTED** on `ad50bd1` for stale ledgers; re-review the exact merge candidate after evidence correction. Do not merge as `Closes #824` / `Closes #672` or M002.10 Done.

Keep the live #824 assignee. Do not start #673/#837 from that PR.

### 6.2 #673 — PHYSICAL_ARM64 rows

The versioned contract is already on `master`: [`docs/evidence/M002-PERFORMANCE-CONTRACT-V1.md`](../evidence/M002-PERFORMANCE-CONTRACT-V1.md). Remaining work is measured rows, not a new engine.

Required: five fresh-process cohorts, 20 warmups, 100 samples, nearest-rank p50/p95/p99, on controlled Apple Silicon, with CI vs `PHYSICAL_ARM64` vs `PLATFORM_LIMITED` kept distinct.

Families: HistoryStore reflow vs #818 ceilings (active 2/4/8 ms, sealed 1/2/4 ms); PTY-read → `TerminalState`; damage/projection; Metal/visible-frame proxy; input admission (do not call it key-to-photon unless scanout is measured); high-output while input/resize/scroll are active; startup/idle CPU/RSS/FD/threads/teardown for 1/10/50/100.

PR [#969](https://github.com/seyal-org/seyal/pull/969) is a first HistoryStore reflow row (`Refs #673`), not #673 Done. Its active-gate relative p95/p99 vs the 10% same-SHA rule is not an absolute-ceiling miss; diagnose before treating the row as accepted.

Do not weaken functional tests to hit numbers. #673 may proceed independently of #824/#672 closure.

### 6.3 #837 — Unicode-heavy ARM64

After accepted #673 ceilings: Unicode PTY throughput, combining-storm RSS, 1/10/50/100 scaling, and IME latency only if AppKit IME is already on the headed path. No renderer rewrite.

---

## 7. IME honesty (not a new product Issue)

SPEC-011 still lists headed IME fixtures 37–41 (commit, cancel, replacement, candidate coordinates, detach discards preedit). #817 closed with AppKit IME **unverified**. #836 is multilingual / post-M004 and **must not** be pulled into M002.

#824 PR #964 recorded the close pick as **covered** by existing native `NSTextInputClient` + local ABC XCUI (`c3 a9 78 1b`). Hosted CI skips that XCUI when ABC is not the active layout. Physical / multilingual / mixed-scale IME stays on #836 and is not required for M002 technical preview. This classification does not close #824.

Do not invent a new Unicode/IME architecture Issue for this.

---

## 8. Final milestone validation

M002 may be marked Done only when every mandatory criterion below is `PASS` on **one freeze SHA**, using `.agents/skills/milestone-validation/SKILL.md` and the M001 Pass 10 verdict model (`PASS` / `FAIL` / `INCONCLUSIVE` / `PLATFORM_LIMITED` / `N/A`). `PLATFORM_LIMITED` is not automatic PASS.

### 8.1 Evidence classes

Label every performance/presentation/fuzz claim: `CI` | `controlled-host` / `PHYSICAL_ARM64` | `PLATFORM_LIMITED`. Foundation `make bench` with `SEYAL_REQUIRE_DISPLAY_LINK_BENCHMARK=0` is `CI` only.

### 8.2 Mandatory close-out criteria

**Architecture**

- one authoritative `TerminalState` per `TerminalExecution`; no second VT/grid/renderer
- HistoryStore / Unicode / keyboard / mouse / selection remain extensions of that state
- Swift remains a thin host (ADR-015); headed oracle is Flow/Blocks unless Raw/TUI takeover is the case
- no agent/persistence/cloud/licensing on the terminal hot path
- OSS has no commercial dependency

**Compatibility**

- #824 matrix green or explicitly classified with owning follow-up
- SSH/nested SSH and tmux-as-child preserve one Seyal PTY/VT per execution
- terminfo advertises only implemented/tested capabilities
- M001 detach/reconnect still holds after M002 changes

**Performance / resources**

- #673 PHYSICAL_ARM64 matrix accepted against the versioned contract
- #837 Unicode-heavy ARM64 vs those ceilings
- #818 history byte caps not silently weakened

**Engineering**

- exact-head `make bootstrap`, `make build`, `make test`, `make check`, `make bench`
- applicable fuzz/security suites
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

Plus the #824 exclusive-Runtime headed/manual procedure and the #673/#837 measured-row commands recorded in their evidence files.

---

## 9. What “M002 Done” means

Closing #664 means the §8 ledger is `PASS` on the freeze SHA and #672 / #673 / #837 / #824 are Done. It authorizes **technical preview / alpha** in the roadmap release-channel sense. It does **not** authorize M004 launch claims, M003 workspace completeness, or agent production.

M003 may continue in parallel only behind stable terminal seams. Terminal-state, VT, Unicode, and reflow changes remain the terminal lane until this milestone is closed.
