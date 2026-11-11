# M003 consolidated campaign status (#1474)

**Date:** 2026-11-11 (verification resume)  
**Branch:** `mahboobmonnamd/issue/1474`  
**Head:** local WIP after Defect B page-key wipe fix (see commits on PR #1478)  
**PR:** https://github.com/seyal-org/seyal/pull/1478 (`Part of #1474`; do not merge without confirmation)  
**Base:** `origin/master`  
**Phase A context:** `/Users/mahboob/.cursor/plans/seyal-phase-a-reality-audit-2026-11-11.md`

## Functional deliverables matrix (this verification pass)

| Deliverable | Verdict | Evidence |
|---|---|---|
| Terminal / PTY→VT automated | **PASS** | Prior tip + campaign `make check` / CI green on `59aab4b9` |
| #1379 Defect A (Block bleed Raw→Flow) | **UNVERIFIED / policy-limited** | Builtin Toggle Raw / View-menu Raw admission removed; `testCmdEnterRawToggleRestoresFlowBlocks` XCTSkipped; no headed Raw entry path on this candidate |
| #1379 Defect B (shell-exit history paint) | **FAIL** on `59aab4b9` headed | Exclusive focused XCUI `testPrimaryShellExitIsReportedAndKeepsFlowHistoryVisible`: AX body text present, Metal body ink=0 / history paint false-green. Units `recovery_tests` 14/14 PASS (insufficient). |
| Defect B fix (page-key wipe) | **CODE+component PASS; headed retest PENDING** | Cleared `historyRenderPageKeys` on disconnect retain wipe; source-order component gates PASS. Exclusive headed retest blocked while `seyal-1476` headed leftovers held the display. |
| Composer / Flow body paint (happy path) | **PASS** headed | `testCompletedFlowBlockKeepsPaintedOutputBody` PASS |
| Alt-screen return / clear→exit | **FAIL / flaky** under sequential focus | `testAlternateScreenReturnRestoresFlowNotRawTerminal`, `testClearThenExitKeepsPaintedBlockBodies` failed early (`composer` not hittable) in same focused batch |
| Python/Node DirectInput | **UNVERIFIED** this pass | Only-testing names did not match suite; not re-run |
| Multipane / tabs / scroll / SSH / reconnect | **UNVERIFIED** this pass | Deferred for exclusive full `make ui-test` after Defect B headed green |
| Quality (`make check` + CI) | **PASS** on `59aab4b9` | CI: repository-policy, rust-and-harness-quality, native-macos-compile SUCCESS |

## Gate 1–4 status

| Gate | Verdict | Notes |
|---|---|---|
| Gate 1 | **FAIL** | Defect B headed FAIL on prior head; Defect A not headed-reproducible via removed Raw admission |
| Gate 2 | **UNVERIFIED** | Multipane/composer matrix not exclusively re-run this pass |
| Gate 3 | **PARTIAL (code)** | Workspace chrome landed; headed not re-proven exclusively |
| Gate 4 | **PARTIAL** | `make check`+CI green; headed matrix + independent review open; resource pack incomplete |

## Landed on this PR (cumulative)

- Chrome toggles (#1365 / #922)
- Blocks-default headed evidence adaptation (#867)
- Block Rerun ABI 87 + Rust-owned action projection (#1010)
- History-backed Block Copy
- Gate 1 P0/P1 multipane/scroll/Metal/prepare fixes
- **NEW:** disconnect retain wipe clears `historyRenderPageKeys` (#1379 Defect B false-green)

## Explicit classifications

- Simultaneous multi-window live Metal: **deferred**
- ADR-025 acceptance: **governance** (separate Architecture PR)
- Defect A Raw admission: **policy-limited** under Blocks-default; restate via TUI round-trip / trusted Raw entry Issue if still required
- Full headed XCUI matrix: **not green** — Defect B FAIL; exclusive retest of fix pending
- #1476 concurrent headed: **do not dual-run XCUI**; wait for exclusive slot

## Remaining before `Closes #1474`

| Severity | Item |
|---|---|
| P0 | Exclusive headed re-proof of Defect B (+ clear→exit) on fix SHA |
| P1 | Exclusive multipane/tabs/scroll/composer/Python/Node/nvim/tmux/SSH/reconnect matrix |
| P1 | Independent non-author review GO/CONDITIONAL/NO-GO |
| P2 | ADR-025 acceptance PR; #993 human config sign-off |

## Recommendation

**M003: NO-GO** until Defect B headed paint is green on an exclusive Runtime and the Gate 1–3 headed matrix is recorded without contention. Do not merge PR #1478.

**Do not merge without explicit product-owner confirmation.**
